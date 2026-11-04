//! On-device transcription via whisper-rs. Runs off-thread (whisper is a long
//! blocking call) and emits progress events.
//!
//! A recording's microphone and system audio are transcribed separately, so
//! the person recording is labelled "You" without guessing and speaker
//! detection only has to tell the other side apart.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::audio::pipeline::{downmix_to_mono, resample_mono, WHISPER_RATE};
use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::whisper::assemble::{self, Labelled, Source};
use crate::whisper::models;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct TranscriptSegment {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

fn read_samples(path: &Path) -> AppResult<(hound::WavSpec, Vec<f32>)> {
    let reader =
        hound::WavReader::open(path).map_err(|e| AppError::Other(format!("open wav: {e}")))?;
    let spec = reader.spec();
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int => reader
            .into_samples::<i16>()
            .map(|s| s.map(|v| v as f32 / 32768.0))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Other(format!("Read recording: {e}")))?,
        hound::SampleFormat::Float => reader
            .into_samples::<f32>()
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Other(format!("Read recording: {e}")))?,
    };
    Ok((spec, samples))
}

/// Load a WAV file as 16 kHz mono f32 (resampling/downmixing if needed).
pub(crate) fn load_pcm_16k_mono(path: &Path) -> AppResult<Vec<f32>> {
    let (spec, samples) = read_samples(path)?;
    let mono = downmix_to_mono(&samples, spec.channels as usize);
    Ok(resample_mono(&mono, spec.sample_rate, WHISPER_RATE))
}

/// Load each channel of a WAV file separately at 16 kHz. New recordings hold
/// the microphone on the first channel and system audio on the second.
///
/// Samples go straight into their channel, and a 16 kHz recording is not
/// copied again, so a long meeting needs memory for one copy of its audio.
pub(crate) fn load_channels_16k(path: &Path) -> AppResult<Vec<Vec<f32>>> {
    let reader =
        hound::WavReader::open(path).map_err(|e| AppError::Other(format!("open wav: {e}")))?;
    let spec = reader.spec();
    let count = (spec.channels as usize).max(1);
    let frames = reader.duration() as usize;
    let mut channels: Vec<Vec<f32>> = (0..count).map(|_| Vec::with_capacity(frames)).collect();
    let read = |e: hound::Error| AppError::Other(format!("Read recording: {e}"));
    match spec.sample_format {
        hound::SampleFormat::Int => {
            for (i, sample) in reader.into_samples::<i16>().enumerate() {
                channels[i % count].push(sample.map_err(read)? as f32 / 32768.0);
            }
        }
        hound::SampleFormat::Float => {
            for (i, sample) in reader.into_samples::<f32>().enumerate() {
                channels[i % count].push(sample.map_err(read)?);
            }
        }
    }
    Ok(channels
        .into_iter()
        .map(|c| if spec.sample_rate == WHISPER_RATE { c } else { resample_mono(&c, spec.sample_rate, WHISPER_RATE) })
        .collect())
}

/// How to transcribe: the spoken language, the names and terms Whisper should
/// expect, and the voice activity model when it is available.
#[derive(Debug, Clone, Default)]
pub struct Options {
    pub language: String,
    pub vocabulary: String,
    pub vad_model: Option<PathBuf>,
}

/// A short glossary for Whisper's prompt, so names such as a client or a
/// colleague are spelt the way the person spells them. Whisper reads only the
/// last couple of hundred tokens of a prompt, so the list is kept short.
pub fn vocabulary_prompt(terms: &[String]) -> String {
    const LIMIT: usize = 500;
    let mut seen = std::collections::HashSet::new();
    let mut kept: Vec<String> = Vec::new();
    let mut length = 0;
    for term in terms {
        let term: String = term.replace('\0', "").split_whitespace().collect::<Vec<_>>().join(" ");
        let term = term.trim_matches(|c: char| c == ',' || c == ';' || c == '.').to_string();
        if term.is_empty() || term.chars().count() > 60 || !seen.insert(term.to_lowercase()) {
            continue;
        }
        if length + term.len() + 2 > LIMIT {
            break;
        }
        length += term.len() + 2;
        kept.push(term);
    }
    if kept.is_empty() {
        String::new()
    } else {
        format!("Glossary: {}.", kept.join(", "))
    }
}

pub(crate) fn run(
    app: &AppHandle,
    model_path: &Path,
    audio_path: &Path,
    language: &str,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> AppResult<Vec<TranscriptSegment>> {
    let callback_app = app.clone();
    decode(model_path, audio_path, language, cancel, move |p| {
        let _ = callback_app.emit("transcribe-progress", p);
    })
}

// Own callback data for the duration of synchronous `full`. The 0.16 wrapper's
// safe abort helper casts a boxed trait object to the concrete closure type.
// Use the low-level API with one stable, correctly typed allocation instead.
struct DecodeCallbacks {
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    progress: std::sync::Mutex<Box<dyn FnMut(i32) + Send>>,
}
unsafe extern "C" fn abort_decode(data: *mut std::ffi::c_void) -> bool {
    // SAFETY: installed only with a live DecodeCallbacks allocation below.
    let callbacks = unsafe { &*(data as *const DecodeCallbacks) };
    callbacks.cancel.load(std::sync::atomic::Ordering::SeqCst)
}
unsafe extern "C" fn report_progress(
    _: *mut whisper_rs::WhisperSysContext,
    _: *mut whisper_rs::WhisperSysState,
    value: i32,
    data: *mut std::ffi::c_void,
) {
    // SAFETY: `full` completes all callbacks before the allocation is dropped.
    let callbacks = unsafe { &*(data as *const DecodeCallbacks) };
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if let Ok(mut progress) = callbacks.progress.lock() {
            progress(value);
        }
    }));
    if result.is_err() {
        log::error!("Transcription progress callback failed");
        callbacks
            .cancel
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

fn check_audio(pcm: &[f32]) -> AppResult<()> {
    if pcm.is_empty()
        || !pcm.iter().all(|v| v.is_finite())
        || crate::audio::pipeline::rms(pcm) < 0.000001
    {
        return Err(AppError::Invalid(
            "The saved recording contains no audio.".into(),
        ));
    }
    Ok(())
}

fn cancelled() -> AppError {
    AppError::Other("Transcription cancelled. Audio and earlier text have been kept.".into())
}

fn load_model(model_path: &Path) -> AppResult<WhisperContext> {
    WhisperContext::new_with_params(model_path, WhisperContextParameters::default())
        .map_err(|e| AppError::Other(format!("load model: {e}")))
}

/// Transcribe one 16 kHz channel with a model that is already loaded.
fn decode_pcm(
    ctx: &WhisperContext,
    pcm: &[f32],
    options: &Options,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    progress: impl FnMut(i32) + Send + 'static,
) -> AppResult<Vec<TranscriptSegment>> {
    let mut state = ctx
        .create_state()
        .map_err(|e| AppError::Other(format!("whisper state: {e}")))?;

    let mut params = FullParams::new(SamplingStrategy::BeamSearch {
        beam_size: 5,
        patience: -1.0,
    });
    let threads = std::thread::available_parallelism()
        .map(|n| n.get() as i32)
        .unwrap_or(4);
    params.set_n_threads(threads.min(6));
    params.set_translate(false);
    let language = if options.language.is_empty() { "en" } else { options.language.as_str() };
    params.set_language(Some(language));
    if !options.vocabulary.is_empty() {
        params.set_initial_prompt(&options.vocabulary);
    }
    if let Some(vad) = options.vad_model.as_deref().and_then(|p| p.to_str()) {
        params.set_vad_model_path(Some(vad));
        let mut vad_params = whisper_rs::WhisperVadParams::new();
        // Keep a little audio either side of each turn, and split very long
        // turns so Whisper's timestamps stay close to what was said.
        vad_params.set_speech_pad(200);
        vad_params.set_min_silence_duration(500);
        vad_params.set_max_speech_duration(25.0);
        params.set_vad_params(vad_params);
        params.enable_vad(true);
    }
    let mut callbacks = Box::new(DecodeCallbacks {
        cancel: cancel.clone(),
        progress: std::sync::Mutex::new(Box::new(progress)),
    });
    // SAFETY: the allocation stays alive and stationary until `full` returns;
    // callbacks access only the atomic flag and mutex, never Whisper state.
    unsafe {
        let data = (&mut *callbacks as *mut DecodeCallbacks).cast();
        params.set_abort_callback(Some(abort_decode));
        params.set_abort_callback_user_data(data);
        params.set_progress_callback(Some(report_progress));
        params.set_progress_callback_user_data(data);
    }
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);

    let decoded = state.full(params, pcm);
    if cancel.load(std::sync::atomic::Ordering::SeqCst) {
        return Err(cancelled());
    }
    decoded.map_err(|e| AppError::Other(format!("transcribe: {e}")))?;

    let mut out = Vec::new();
    for seg in state.as_iter() {
        let text = seg
            .to_str_lossy()
            .map(|c| c.to_string())
            .unwrap_or_default();
        out.push(TranscriptSegment {
            // whisper timestamps are centiseconds → milliseconds
            start_ms: seg.start_timestamp() * 10,
            end_ms: seg.end_timestamp() * 10,
            text: text.trim().to_string(),
        });
    }
    if cancel.load(std::sync::atomic::Ordering::SeqCst) {
        return Err(cancelled());
    }
    Ok(out)
}

const NO_SPEECH: &str =
    "No speech was recognised. The audio has been kept; try another model or language.";

fn decode(
    model_path: &Path,
    audio_path: &Path,
    language: &str,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    progress: impl FnMut(i32) + Send + 'static,
) -> AppResult<Vec<TranscriptSegment>> {
    if cancel.load(std::sync::atomic::Ordering::SeqCst) {
        return Err(AppError::Other("Transcription cancelled".into()));
    }
    let pcm = load_pcm_16k_mono(audio_path)?;
    check_audio(&pcm)?;
    let ctx = load_model(model_path)?;
    let options = Options { language: language.into(), ..Options::default() };
    let out = decode_pcm(&ctx, &pcm, &options, cancel, progress)?;
    if out.iter().all(|s| s.text.trim().is_empty()) {
        return Err(AppError::Other(NO_SPEECH.into()));
    }
    Ok(out)
}

/// Diarisation over one channel, returning speaker turns.
pub type Diarizer<'a> = &'a dyn Fn(&[f32]) -> AppResult<Vec<assemble::Turn>>;

/// Which channels to transcribe, and what each one holds.
fn plan(channels: Vec<Vec<f32>>) -> Vec<(Source, Vec<f32>)> {
    let mut channels = channels.into_iter();
    let first = channels.next().unwrap_or_default();
    let Some(second) = channels.next() else {
        return vec![(Source::Mixed, first)];
    };
    match (assemble::has_speech(&first), assemble::has_speech(&second)) {
        (true, true) => vec![(Source::Mic, first), (Source::System, second)],
        // A meeting in a room: everyone is on the microphone.
        (true, false) => vec![(Source::Mixed, first)],
        // Only listening, for example a webinar.
        (false, true) => vec![(Source::Mixed, second)],
        // Very quiet audio: let Whisper decide from both sides together.
        (false, false) => vec![(
            Source::Mixed,
            first.iter().zip(&second).map(|(a, b)| a + b).collect(),
        )],
    }
}

/// Transcribe a recording into labelled lines, one side of the call at a time.
pub fn transcribe_recording(
    model_path: &Path,
    audio_path: &Path,
    options: &Options,
    diarize: Option<Diarizer<'_>>,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    progress: impl Fn(i32) + Send + Sync + 'static,
) -> AppResult<Vec<Labelled>> {
    if cancel.load(std::sync::atomic::Ordering::SeqCst) {
        return Err(cancelled());
    }
    let plan = plan(load_channels_16k(audio_path)?);
    check_audio(&plan.iter().flat_map(|(_, pcm)| pcm.iter().copied()).collect::<Vec<_>>())?;
    let ctx = load_model(model_path)?;
    let progress = std::sync::Arc::new(progress);
    let parts = plan.len() as i32;

    let mut decoded: Vec<(Source, Vec<TranscriptSegment>, Vec<f32>)> = Vec::new();
    for (index, (source, pcm)) in plan.into_iter().enumerate() {
        let report = progress.clone();
        let base = index as i32 * 100 / parts;
        let segments = decode_pcm(&ctx, &pcm, options, cancel.clone(), move |p| {
            report(base + p / parts);
        })?;
        let segments = assemble::fit_to_speech(assemble::keep_spoken(segments, &pcm), &pcm);
        decoded.push((source, segments, pcm));
    }
    // The other side's voice coming back through the speakers is not the
    // person recording speaking.
    if let [(Source::Mic, mic, _), (Source::System, system, _)] = decoded.as_mut_slice() {
        *mic = assemble::remove_echo(std::mem::take(mic), system);
    }

    let mut labelled = Vec::new();
    for (source, segments, pcm) in decoded {
        let turns = match (source, diarize) {
            (Source::Mic, _) | (_, None) => None,
            (_, Some(diarize)) => match diarize(&pcm) {
                Ok(turns) if !turns.is_empty() => Some(turns),
                Ok(_) => None,
                Err(error) => {
                    log::warn!("Speakers could not be told apart: {error}");
                    None
                }
            },
        };
        let labels = match (source, turns) {
            (Source::Mic, _) => vec![Some(assemble::YOU.to_string()); segments.len()],
            (Source::System, Some(turns)) => assemble::assign_speakers(&segments, &turns)
                .into_iter()
                .map(|l| l.or_else(|| Some(assemble::OTHERS.to_string())))
                .collect(),
            (Source::System, None) => vec![Some(assemble::OTHERS.to_string()); segments.len()],
            (Source::Mixed, Some(turns)) => assemble::assign_speakers(&segments, &turns),
            (Source::Mixed, None) => vec![None; segments.len()],
        };
        labelled.push((segments, labels));
    }
    let lines = assemble::merge(labelled);
    if lines.iter().all(|l| l.text.trim().is_empty()) {
        return Err(AppError::Other(NO_SPEECH.into()));
    }
    progress(100);
    Ok(lines)
}

#[tauri::command]
pub async fn transcribe(
    app: AppHandle,
    db: State<'_, Db>,
    audio_path: String,
) -> AppResult<Vec<TranscriptSegment>> {
    let model_path = {
        let conn = db.conn.lock().unwrap();
        models::core::selected_path(&conn, &app)?
    };
    // Security: only transcribe files inside our own recordings dir. The
    // legitimate caller always passes a path returned by `stop_recording`;
    // reject anything that escapes the sandbox (path traversal / arbitrary read).
    let audio = std::path::PathBuf::from(&audio_path);
    let recordings = crate::audio::recorder::app_recordings_dir(&app)?;
    let canon_audio = audio
        .canonicalize()
        .map_err(|_| AppError::Invalid("recording not found".into()))?;
    let canon_dir = recordings
        .canonicalize()
        .map_err(|_| AppError::Invalid("recording not found".into()))?;
    if !canon_audio.starts_with(&canon_dir) {
        return Err(AppError::Invalid("recording not found".into()));
    }
    let audio = canon_audio;
    // whisper's `full` is blocking — run it off the async runtime.
    let result = tauri::async_runtime::spawn_blocking(move || {
        run(
            &app,
            &model_path,
            &audio,
            "en",
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        )
    })
    .await
    .map_err(|e| AppError::Other(format!("transcribe task: {e}")))?;
    // Saved recordings are source material, retained for speaker labelling and retries.
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{atomic::AtomicBool, Arc};
    fn fixture(name: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("tidy-transcription-{}", crate::db::new_id()));
        std::fs::create_dir_all(&p).unwrap();
        p.join(name)
    }
    #[test]
    fn rejects_silent_audio_without_removing_it() {
        let path = fixture("silence.wav");
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 16000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(&path, spec).unwrap();
        for _ in 0..16000 {
            w.write_sample(0i16).unwrap();
        }
        w.finalize().unwrap();
        assert!(decode(
            Path::new("no-model"),
            &path,
            "en",
            Arc::new(AtomicBool::new(false)),
            |_| {}
        )
        .is_err());
        assert!(path.exists());
        assert_eq!(load_pcm_16k_mono(&path).unwrap().len(), 16000);
    }
    fn stereo(path: &Path, left: &[f32], right: &[f32]) {
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 16000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(path, spec).unwrap();
        for (l, r) in left.iter().zip(right) {
            w.write_sample((l * 32767.0) as i16).unwrap();
            w.write_sample((r * 32767.0) as i16).unwrap();
        }
        w.finalize().unwrap();
    }

    fn talk(seconds: usize) -> Vec<f32> {
        (0..16000 * seconds).map(|i| (i as f32 / 9.0).sin() * 0.2).collect()
    }

    #[test]
    fn stereo_recordings_load_each_side_separately() {
        let path = fixture("call.wav");
        let left = talk(1);
        let right = vec![0.0; 16000];
        stereo(&path, &left, &right);
        let channels = load_channels_16k(&path).unwrap();
        assert_eq!(channels.len(), 2);
        assert!(channels[0].iter().any(|s| s.abs() > 0.1));
        assert!(channels[1].iter().all(|s| *s == 0.0));
        assert_eq!(load_pcm_16k_mono(&path).unwrap().len(), 16000);
    }

    #[test]
    fn each_side_of_a_call_is_transcribed_and_a_room_is_one_track() {
        let quiet = vec![0.0; 16000 * 5];
        let sources = |channels: Vec<Vec<f32>>| plan(channels).into_iter().map(|(s, _)| s).collect::<Vec<_>>();
        assert_eq!(sources(vec![talk(5), talk(5)]), vec![Source::Mic, Source::System]);
        assert_eq!(sources(vec![talk(5), quiet.clone()]), vec![Source::Mixed]);
        assert_eq!(sources(vec![quiet.clone(), talk(5)]), vec![Source::Mixed]);
        assert_eq!(sources(vec![talk(5)]), vec![Source::Mixed]);
    }

    #[test]
    fn the_vocabulary_prompt_is_short_and_has_no_repeats() {
        let terms: Vec<String> = ["Harbourside Dental", "cliniko", "Cliniko", "  Priya ", "", "Dr\0 Chen"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(vocabulary_prompt(&terms), "Glossary: Harbourside Dental, cliniko, Priya, Dr Chen.");
        assert_eq!(vocabulary_prompt(&[]), "");
        let many: Vec<String> = (0..500).map(|i| format!("Term number {i}")).collect();
        assert!(vocabulary_prompt(&many).len() <= 520);
    }

    #[test]
    fn cancelled_transcription_keeps_source_bytes() {
        let path = fixture("original.wav");
        std::fs::write(&path, b"unchanged").unwrap();
        assert!(decode(
            Path::new("no-model"),
            &path,
            "en",
            Arc::new(AtomicBool::new(true)),
            |_| {}
        )
        .is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"unchanged");
    }
    #[test]
    #[ignore = "requires TIDY_WHISPER_MODEL and TIDY_TRANSCRIPTION_FIXTURE"]
    fn real_calls_label_each_side_and_drop_echo() {
        let model = std::path::PathBuf::from(std::env::var("TIDY_WHISPER_MODEL").unwrap());
        let speech = load_pcm_16k_mono(std::path::Path::new(&std::env::var("TIDY_TRANSCRIPTION_FIXTURE").unwrap())).unwrap();
        let half = speech.len() / 2;
        let silence = |n: usize| vec![0.0f32; n];
        let quieter = |pcm: &[f32]| pcm.iter().map(|s| s * 0.3).collect::<Vec<_>>();
        let run = |left: Vec<f32>, right: Vec<f32>| {
            let path = fixture("call.wav");
            stereo(&path, &left, &right);
            let options = Options { language: "en".into(), ..Options::default() };
            let lines = transcribe_recording(&model, &path, &options, None, Arc::new(AtomicBool::new(false)), |_| {}).unwrap();
            for l in &lines {
                eprintln!("{:>6} {:?}: {}", l.start_ms, l.speaker, l.text);
            }
            eprintln!("--");
            lines
        };

        // A call: the person recording speaks first, then the other side.
        let mut left = speech[..half].to_vec();
        left.extend(silence(speech.len() - half));
        let mut right = silence(half);
        right.extend_from_slice(&speech[half..]);
        let call = run(left, right);
        assert!(call.iter().any(|l| l.speaker.as_deref() == Some("You")), "{call:?}");
        assert!(call.iter().any(|l| l.speaker.as_deref() == Some("Others")), "{call:?}");
        // Their first line starts where they start speaking, after the person recording.
        let first_other = call.iter().position(|l| l.speaker.as_deref() == Some("Others")).unwrap();
        assert!(call[..first_other].iter().all(|l| l.speaker.as_deref() == Some("You")));
        assert!(call[first_other].start_ms >= (half as i64 * 1000 / 16_000) - 300, "{call:?}");

        // A room: only the microphone heard anything, so nobody is "You".
        let room = run(speech.clone(), silence(speech.len()));
        assert!(!room.is_empty());
        assert!(room.iter().all(|l| l.speaker.is_none()), "{room:?}");

        // Their voice leaking quietly into the microphone is not the person speaking.
        let echo = run(quieter(&speech), speech.clone());
        assert!(!echo.is_empty());
        assert!(echo.iter().all(|l| l.speaker.as_deref() == Some("Others")), "{echo:?}");
    }

    #[test]
    #[ignore = "requires TIDY_WHISPER_MODEL and TIDY_TRANSCRIPTION_FIXTURE"]
    fn real_saved_audio_can_be_transcribed_twice() {
        let model = std::path::PathBuf::from(std::env::var("TIDY_WHISPER_MODEL").unwrap());
        let audio = std::path::PathBuf::from(std::env::var("TIDY_TRANSCRIPTION_FIXTURE").unwrap());
        let before = std::fs::read(&audio).unwrap();
        for _ in 0..2 {
            let result = decode(
                &model,
                &audio,
                "en",
                Arc::new(AtomicBool::new(false)),
                |_| {},
            )
            .unwrap();
            let text = result
                .iter()
                .map(|s| s.text.to_lowercase())
                .collect::<Vec<_>>()
                .join(" ");
            assert!(text.contains("client"), "{text}");
            assert!(text.contains("handover"), "{text}");
            assert_eq!(std::fs::read(&audio).unwrap(), before);
        }
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        let error = decode(&model, &audio, "en", cancel, move |_| {
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
        })
        .unwrap_err();
        assert!(error.to_string().contains("cancelled"), "{error}");
        assert_eq!(std::fs::read(&audio).unwrap(), before);
    }
}
