//! Recording controller. cpal and ScreenCaptureKit streams are `!Send`, so the
//! actual capture runs on a dedicated thread; the shared state holds only
//! `Send` handles (stop flag, sample buffers, join handle).
//!
//! Audio is streamed to a `.partial` WAV as it arrives rather than held in
//! memory until Stop, so a long meeting costs a bounded amount of RAM and a
//! crash leaves a recoverable file behind.
//!
//! The file is stereo: the microphone on the left channel and system audio on
//! the right, so each side of a call can be transcribed separately.

use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use super::pipeline::{mix_mono, rms, ChunkedPair, WHISPER_RATE};
use super::system::SYSTEM_RATE;
use crate::db::now_ms;
use crate::error::{AppError, AppResult};

/// How often captured audio is mixed and appended to the file on disk.
const CHUNK: Duration = Duration::from_secs(5);
/// The rolling window kept in memory for the live-transcription preview.
const PREVIEW_SAMPLES: usize = WHISPER_RATE as usize * 15;
/// How long `start` waits for both capture sources to be attempted.
const START_TIMEOUT: Duration = Duration::from_secs(30);
/// A level this loud on either side counts as someone speaking (about -42 dBFS).
const VOICE_LEVEL: f32 = 0.008;

/// When the current recording last heard someone, in ms since the epoch.
static LAST_VOICE_MS: AtomicI64 = AtomicI64::new(0);

/// When the current recording last heard someone speak, on either side.
pub fn last_voice_ms() -> i64 {
    LAST_VOICE_MS.load(Ordering::Relaxed)
}

#[derive(Serialize, Clone)]
struct LevelEvent {
    mic: f32,
    system: f32,
}
#[derive(Serialize, Clone)]
struct TickEvent {
    elapsed_ms: i64,
}

struct CaptureOutcome {
    frames: u64,
    error: Option<String>,
}

pub struct RecordingSession {
    keep_audio: bool,
    stop: Arc<AtomicBool>,
    started_at: i64,
    /// When capture began, the zero point for marked moments.
    began: Instant,
    marks: Vec<i64>,
    path: PathBuf,
    partial: PathBuf,
    outcome: Arc<Mutex<Option<CaptureOutcome>>>,
    handle: Option<JoinHandle<()>>,
    live_handle: Option<JoinHandle<()>>,
    finished: Option<Recording>,
}

impl Drop for RecordingSession {
    fn drop(&mut self) {
        // A session dropped without `stop` must still release the devices.
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Result of stopping a recording.
#[derive(Serialize, Clone)]
pub struct Recording {
    pub keep_audio: bool,
    pub audio_path: String,
    pub duration_ms: i64,
    pub started_at: i64,
    pub page_id: Option<String>,
    /// Moments marked while recording, in ms from the start.
    pub marks: Vec<i64>,
}

impl RecordingSession {
    /// Mark this moment. Returns how far into the recording it is, in ms.
    pub fn mark(&mut self) -> i64 {
        let at = self.began.elapsed().as_millis() as i64;
        self.marks.push(at);
        at
    }

    pub fn marks(&self) -> &[i64] {
        &self.marks
    }
}

pub fn start(
    app: AppHandle,
    model_path: Option<PathBuf>,
    keep_audio: bool,
) -> AppResult<RecordingSession> {
    let started_at = now_ms();
    LAST_VOICE_MS.store(started_at, Ordering::Relaxed);
    let dir = if keep_audio {
        app_recordings_dir(&app)?
    } else {
        app_recordings_dir(&app)?.join("temporary")
    };
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}-{}.wav", started_at, crate::db::new_id()));
    let partial = path.with_extension("partial");

    let stop = Arc::new(AtomicBool::new(false));
    let preview = Arc::new(Mutex::new(Vec::<f32>::new()));
    let generation = Arc::new(AtomicU64::new(0));
    let outcome = Arc::new(Mutex::new(None));

    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<(bool, bool), String>>();
    let thread = Capture {
        app: app.clone(),
        stop: stop.clone(),
        mic_buf: Arc::new(Mutex::new(Vec::new())),
        sys_buf: Arc::new(Mutex::new(Vec::new())),
        mic_rate: Arc::new(AtomicU32::new(WHISPER_RATE)),
        preview: preview.clone(),
        generation: generation.clone(),
        partial: partial.clone(),
        outcome: outcome.clone(),
    };
    let handle = std::thread::spawn(move || thread.run(ready_tx));

    let ready = match ready_rx.recv_timeout(START_TIMEOUT) {
        Ok(ready) => ready,
        Err(_) => {
            stop.store(true, Ordering::Relaxed);
            return Err(AppError::Other(
                "Audio capture did not start in time. Try again.".into(),
            ));
        }
    };
    if let Err(message) = ready {
        stop.store(true, Ordering::Relaxed);
        let _ = handle.join();
        let _ = std::fs::remove_file(&partial);
        return Err(AppError::Other(message));
    }

    // Optional live-transcription preview: re-transcribe a rolling window while
    // recording. The final saved transcript still comes from the recorded file
    // — this is a real-time preview only.
    let live_handle = model_path.map(|mp| {
        spawn_live_transcription(
            app.clone(),
            stop.clone(),
            preview.clone(),
            generation.clone(),
            mp,
        )
    });

    Ok(RecordingSession {
        keep_audio,
        stop,
        started_at,
        began: Instant::now(),
        marks: Vec::new(),
        path,
        partial,
        outcome,
        handle: Some(handle),
        live_handle,
        finished: None,
    })
}

struct Capture {
    app: AppHandle,
    stop: Arc<AtomicBool>,
    mic_buf: Arc<Mutex<Vec<f32>>>,
    sys_buf: Arc<Mutex<Vec<f32>>>,
    mic_rate: Arc<AtomicU32>,
    preview: Arc<Mutex<Vec<f32>>>,
    generation: Arc<AtomicU64>,
    partial: PathBuf,
    outcome: Arc<Mutex<Option<CaptureOutcome>>>,
}

impl Capture {
    fn run(self, ready: std::sync::mpsc::Sender<Result<(bool, bool), String>>) {
        // Start both sources; tolerate one failing (e.g. permission denied).
        let mic = match super::mic::start_mic(self.mic_buf.clone()) {
            Ok(m) => {
                self.mic_rate.store(m.sample_rate, Ordering::Relaxed);
                Some(m)
            }
            Err(e) => {
                log::error!("mic capture unavailable: {e}");
                None
            }
        };
        let system = match super::system::start_system(self.sys_buf.clone()) {
            Ok(s) => Some(s),
            Err(e) => {
                log::error!("system audio capture unavailable: {e}");
                None
            }
        };
        // Recording silence helps nobody: fail the start instead.
        if mic.is_none() && system.is_none() {
            let _ = ready.send(Err("Tidy could not capture any audio. Allow microphone access under Privacy and Security, and screen recording if you want system audio, then try again.".into()));
            return;
        }
        let mut writer = match open_writer(&self.partial) {
            Ok(w) => w,
            Err(e) => {
                let _ = ready.send(Err(format!("The recording file could not be created: {e}")));
                return;
            }
        };
        if ready.send(Ok((mic.is_some(), system.is_some()))).is_err() {
            // The caller gave up before capture was confirmed.
            self.stop.store(true, Ordering::Relaxed);
        }
        let _ = self.app.emit(
            "recording-sources",
            serde_json::json!({ "mic": mic.is_some(), "system": system.is_some() }),
        );

        let mut pair = ChunkedPair::new(
            self.mic_rate.load(Ordering::Relaxed),
            mic.is_some(),
            SYSTEM_RATE,
            system.is_some(),
        );
        let start = Instant::now();
        let mut last_mic = 0usize;
        let mut last_sys = 0usize;
        let mut next_chunk = Instant::now() + CHUNK;
        let mut frames: u64 = 0;
        let mut error: Option<String> = None;

        while !self.stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(120));
            let mic_level = tail_rms(&self.mic_buf, &mut last_mic);
            let sys_level = tail_rms(&self.sys_buf, &mut last_sys);
            if mic_level >= VOICE_LEVEL || sys_level >= VOICE_LEVEL {
                LAST_VOICE_MS.store(now_ms(), Ordering::Relaxed);
            }
            let _ = self.app.emit(
                "audio-level",
                LevelEvent {
                    mic: mic_level,
                    system: sys_level,
                },
            );
            let _ = self.app.emit(
                "recording-tick",
                TickEvent {
                    elapsed_ms: start.elapsed().as_millis() as i64,
                },
            );
            if Instant::now() < next_chunk {
                continue;
            }
            next_chunk = Instant::now() + CHUNK;
            last_mic = 0;
            last_sys = 0;
            let (mic_raw, sys_raw) = self.drain();
            let (mic16, sys16) = pair.push(&mic_raw, &sys_raw);
            if let Err(e) = append(&mut writer, &mic16, &sys16) {
                error = Some(e.to_string());
                break;
            }
            frames += mic16.len() as u64;
            self.remember(&mic16, &sys16);
            // Checkpoint the header so a crash still leaves a readable file.
            if let Err(e) = writer.flush() {
                log::warn!("recording checkpoint: {e}");
            }
        }

        // Dropping the streams stops capture; their last callbacks have landed.
        drop(mic);
        drop(system);
        if error.is_none() {
            let (mic_raw, sys_raw) = self.drain();
            let (mut mic16, mut sys16) = pair.push(&mic_raw, &sys_raw);
            let (mic_rest, sys_rest) = pair.flush();
            mic16.extend(mic_rest);
            sys16.extend(sys_rest);
            match append(&mut writer, &mic16, &sys16) {
                Ok(()) => {
                    frames += mic16.len() as u64;
                    self.remember(&mic16, &sys16);
                }
                Err(e) => error = Some(e.to_string()),
            }
        }
        if let Err(e) = writer.finalize() {
            error.get_or_insert_with(|| format!("wav finalize: {e}"));
        }
        if let Ok(mut slot) = self.outcome.lock() {
            *slot = Some(CaptureOutcome { frames, error });
        }
    }

    fn drain(&self) -> (Vec<f32>, Vec<f32>) {
        let mic = self
            .mic_buf
            .lock()
            .map(|mut b| std::mem::take(&mut *b))
            .unwrap_or_default();
        let system = self
            .sys_buf
            .lock()
            .map(|mut b| std::mem::take(&mut *b))
            .unwrap_or_default();
        (mic, system)
    }

    fn remember(&self, mic: &[f32], system: &[f32]) {
        if mic.is_empty() {
            return;
        }
        // The live preview listens to both sides at once.
        let mixed = mix_mono(mic, system);
        if let Ok(mut preview) = self.preview.lock() {
            preview.extend_from_slice(&mixed);
            let len = preview.len();
            if len > PREVIEW_SAMPLES {
                preview.drain(..len - PREVIEW_SAMPLES);
            }
        }
        self.generation.fetch_add(1, Ordering::Relaxed);
    }
}

/// Stop the session, finish the WAV, and describe the recording. Safe to call
/// again if a later step (such as registering it) failed: the work already done
/// is not repeated and the file is not written twice.
pub fn stop(session: &mut RecordingSession) -> AppResult<Recording> {
    if let Some(done) = &session.finished {
        return Ok(done.clone());
    }
    session.stop.store(true, Ordering::Relaxed);
    if let Some(h) = session.handle.take() {
        let _ = h.join();
    }
    // Join the live-transcription worker too so the Whisper model isn't leaked
    // and a re-start doesn't contend with an orphaned decode.
    if let Some(h) = session.live_handle.take() {
        let _ = h.join();
    }
    let (frames, error) = {
        let guard = session
            .outcome
            .lock()
            .map_err(|_| AppError::Other("Recorder state is unavailable.".into()))?;
        match guard.as_ref() {
            Some(outcome) => (outcome.frames, outcome.error.clone()),
            None => {
                return Err(AppError::Other(
                    "The recording thread stopped before the audio was written.".into(),
                ))
            }
        }
    };
    if let Some(error) = error {
        return Err(AppError::Other(format!(
            "The recording could not be saved: {error}"
        )));
    }
    if session.partial.exists() {
        std::fs::File::open(&session.partial)?.sync_all()?;
        std::fs::rename(&session.partial, &session.path)?;
    }
    if !session.path.exists() {
        return Err(AppError::Other(
            "The recording file is missing. Nothing was saved.".into(),
        ));
    }
    let recording = Recording {
        keep_audio: session.keep_audio,
        audio_path: session.path.to_string_lossy().to_string(),
        duration_ms: (frames as i64 * 1000) / WHISPER_RATE as i64,
        started_at: session.started_at,
        page_id: None,
        marks: session.marks.clone(),
    };
    session.finished = Some(recording.clone());
    Ok(recording)
}

/// Background worker: every few seconds, transcribe the last ~15s of mixed
/// audio and emit it as a live preview. Loads the Whisper model once and reuses
/// the state across windows.
fn spawn_live_transcription(
    app: AppHandle,
    stop: Arc<AtomicBool>,
    preview: Arc<Mutex<Vec<f32>>>,
    generation: Arc<AtomicU64>,
    model_path: PathBuf,
) -> JoinHandle<()> {
    use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};
    const STEP: Duration = Duration::from_millis(500);
    const SILENCE_RMS: f32 = 0.005;

    std::thread::spawn(move || {
        let ctx =
            match WhisperContext::new_with_params(&model_path, WhisperContextParameters::default())
            {
                Ok(c) => c,
                Err(e) => {
                    log::warn!("live transcription disabled (model load failed): {e}");
                    return;
                }
            };
        let mut state = match ctx.create_state() {
            Ok(s) => s,
            Err(e) => {
                log::warn!("live transcription disabled (state failed): {e}");
                return;
            }
        };
        let threads = std::thread::available_parallelism()
            .map(|n| n.get() as i32)
            .unwrap_or(4);
        let mut seen = generation.load(Ordering::Relaxed);

        while !stop.load(Ordering::Relaxed) {
            std::thread::sleep(STEP);
            if stop.load(Ordering::Relaxed) {
                break;
            }
            // Only re-decode once the capture thread has mixed more audio.
            let current = generation.load(Ordering::Relaxed);
            if current == seen {
                continue;
            }
            seen = current;
            let window = match preview.lock() {
                Ok(samples) => samples.clone(),
                Err(_) => break,
            };
            if window.is_empty() || rms(&window) < SILENCE_RMS {
                continue; // skip silent windows (cheap VAD gate)
            }
            let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
            params.set_n_threads(threads.min(4));
            params.set_translate(false);
            params.set_language(Some("en"));
            params.set_print_special(false);
            params.set_print_progress(false);
            params.set_print_realtime(false);
            params.set_print_timestamps(false);
            if state.full(params, &window).is_ok() {
                let text = state
                    .as_iter()
                    .filter_map(|s| s.to_str_lossy().ok().map(|c| c.to_string()))
                    .collect::<Vec<_>>()
                    .join("")
                    .trim()
                    .to_string();
                if !text.is_empty() {
                    let _ = app.emit("live-transcript", text);
                }
            }
        }
    })
}

fn tail_rms(buf: &Arc<Mutex<Vec<f32>>>, last: &mut usize) -> f32 {
    // Copy the new tail under the lock, then compute RMS after unlocking so we
    // don't stall the realtime audio callback that wants the same mutex.
    let tail: Vec<f32> = {
        let Ok(b) = buf.lock() else {
            return 0.0;
        };
        let from = (*last).min(b.len());
        *last = b.len();
        // Bound each meter window even if the capture thread was delayed.
        b[from.max(b.len().saturating_sub(4800))..].to_vec()
    };
    if tail.is_empty() {
        0.0
    } else {
        rms(&tail)
    }
}

pub fn app_recordings_dir(app: &AppHandle) -> AppResult<PathBuf> {
    let dir = crate::workspace::directory(app);
    Ok(dir.join("recordings"))
}

/// Left: microphone. Right: system audio.
fn wav_spec() -> hound::WavSpec {
    hound::WavSpec {
        channels: 2,
        sample_rate: WHISPER_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    }
}

type Writer = hound::WavWriter<std::io::BufWriter<std::fs::File>>;

fn open_writer(path: &Path) -> AppResult<Writer> {
    hound::WavWriter::create(path, wav_spec())
        .map_err(|e| AppError::Other(format!("wav create: {e}")))
}

/// Write the two channels as interleaved frames. They always have the same
/// length: the pair pads a missing source with silence.
fn append(writer: &mut Writer, mic: &[f32], system: &[f32]) -> AppResult<()> {
    for (m, s) in mic.iter().zip(system) {
        for sample in [m, s] {
            let v = (sample.clamp(-1.0, 1.0) * 32767.0) as i16;
            writer
                .write_sample(v)
                .map_err(|e| AppError::Other(format!("wav write: {e}")))?;
        }
    }
    Ok(())
}

/// Recover a `.partial` left behind by a crash. Returns the number of audio
/// frames it holds, repairing a header that was never updated before the
/// process died. Zero means there is nothing worth keeping.
pub fn recover_partial(path: &Path) -> AppResult<u32> {
    let length = std::fs::metadata(path)?.len();
    if length < 44 {
        return Ok(0);
    }
    let mut file = std::fs::OpenOptions::new().read(true).write(true).open(path)?;
    let mut header = [0u8; 12];
    file.read_exact(&mut header)?;
    if &header[..4] != b"RIFF" || &header[8..12] != b"WAVE" {
        return Ok(0);
    }
    let riff_size = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);

    // Walk the chunk list to find the format and where the samples start.
    let mut offset: u64 = 12;
    let mut data_offset: Option<u64> = None;
    let mut declared: u32 = 0;
    // Older recordings are mono; newer ones are stereo. Both are 16-bit.
    let mut block_align = 2u64;
    while offset + 8 <= length {
        file.seek(SeekFrom::Start(offset))?;
        let mut chunk = [0u8; 8];
        file.read_exact(&mut chunk)?;
        let size = u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]);
        if &chunk[..4] == b"fmt " && size >= 16 {
            let mut format = [0u8; 16];
            file.read_exact(&mut format)?;
            let align = u16::from_le_bytes([format[12], format[13]]) as u64;
            if align > 0 {
                block_align = align;
            }
        }
        if &chunk[..4] == b"data" {
            data_offset = Some(offset + 8);
            declared = size;
            break;
        }
        offset = offset + 8 + size as u64 + (size as u64 & 1);
    }
    let Some(data_offset) = data_offset else {
        return Ok(0);
    };
    let actual = ((length - data_offset) / block_align) * block_align;
    if actual == 0 || actual > u32::MAX as u64 {
        return Ok(0);
    }
    if declared as u64 != actual || riff_size as u64 != length - 8 {
        file.seek(SeekFrom::Start(4))?;
        file.write_all(&((length - 8) as u32).to_le_bytes())?;
        file.seek(SeekFrom::Start(data_offset - 4))?;
        file.write_all(&(actual as u32).to_le_bytes())?;
        file.sync_all()?;
    }
    drop(file);
    let reader = hound::WavReader::open(path)
        .map_err(|e| AppError::Other(format!("An interrupted recording could not be read: {e}")))?;
    Ok(reader.duration())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::pipeline::resample_mono;

    #[test]
    fn meter_tracks_new_audio_and_returns_to_silence() {
        let samples = Arc::new(Mutex::new(vec![0.01; 100]));
        let mut last = 0;
        assert!((tail_rms(&samples, &mut last) - 0.01).abs() < 0.0001);
        assert_eq!(tail_rms(&samples, &mut last), 0.0);
        samples.lock().unwrap().extend(vec![0.1; 100]);
        assert!((tail_rms(&samples, &mut last) - 0.1).abs() < 0.0001);
        samples.lock().unwrap().extend(vec![0.0; 100]);
        assert_eq!(tail_rms(&samples, &mut last), 0.0);
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tidy-recorder-{}", crate::db::new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    fn sine(samples: usize, rate: u32, hz: f32, amplitude: f32) -> Vec<f32> {
        (0..samples)
            .map(|i| (i as f32 / rate as f32 * hz * std::f32::consts::TAU).sin() * amplitude)
            .collect()
    }

    fn channels(path: &Path) -> (Vec<i16>, Vec<i16>) {
        let reader = hound::WavReader::open(path).unwrap();
        assert_eq!(reader.spec().channels, 2);
        let samples: Vec<i16> = reader.into_samples::<i16>().collect::<Result<_, _>>().unwrap();
        let left = samples.iter().step_by(2).copied().collect();
        let right = samples.iter().skip(1).step_by(2).copied().collect();
        (left, right)
    }

    fn quantise(samples: &[f32]) -> Vec<i16> {
        samples
            .iter()
            .map(|s| (s.clamp(-1.0, 1.0) * 32767.0) as i16)
            .collect()
    }

    #[test]
    fn streamed_chunks_keep_each_source_on_its_own_channel() {
        let mic = sine(44_100 * 2, 44_100, 440.0, 0.9);
        let system = sine(48_000 * 2, 48_000, 261.0, 0.9);
        let expected_mic = resample_mono(&mic, 44_100, WHISPER_RATE);
        let expected_system = resample_mono(&system, 48_000, WHISPER_RATE);

        let path = scratch("streamed.partial");
        let mut writer = open_writer(&path).unwrap();
        let mut pair = ChunkedPair::new(44_100, true, 48_000, true);
        let mut frames = 0u64;
        for (m, s) in mic.chunks(44_100 * 5).zip(system.chunks(48_000 * 5)) {
            let (a, b) = pair.push(m, s);
            append(&mut writer, &a, &b).unwrap();
            frames += a.len() as u64;
            writer.flush().unwrap();
        }
        let (a, b) = pair.flush();
        append(&mut writer, &a, &b).unwrap();
        frames += a.len() as u64;
        writer.finalize().unwrap();

        assert_eq!(frames, expected_mic.len() as u64);
        let (left, right) = channels(&path);
        assert_eq!(left, quantise(&expected_mic), "the microphone is on the left");
        assert_eq!(right, quantise(&expected_system), "system audio is on the right");
    }

    #[test]
    fn a_truncated_partial_is_repaired_and_keeps_its_audio() {
        let path = scratch("crashed.partial");
        let mut writer = open_writer(&path).unwrap();
        let samples = sine(WHISPER_RATE as usize, WHISPER_RATE, 300.0, 0.5);
        append(&mut writer, &samples, &samples).unwrap();
        writer.finalize().unwrap();
        // Simulate a crash: the samples reached the disk but the header still
        // claims an empty data chunk and the RIFF size was never updated.
        let mut file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        file.seek(SeekFrom::Start(4)).unwrap();
        file.write_all(&0u32.to_le_bytes()).unwrap();
        file.seek(SeekFrom::Start(40)).unwrap();
        file.write_all(&0u32.to_le_bytes()).unwrap();
        file.sync_all().unwrap();
        drop(file);

        assert_eq!(
            hound::WavReader::open(&path)
                .map(|r| r.duration())
                .unwrap_or(0),
            0,
            "the damaged header hides the audio"
        );
        assert_eq!(recover_partial(&path).unwrap(), samples.len() as u32);
        let (left, right) = channels(&path);
        assert_eq!(left.len(), samples.len());
        assert_eq!(right.len(), samples.len());
    }

    #[test]
    fn an_older_mono_partial_is_still_repaired() {
        let path = scratch("mono.partial");
        let spec = hound::WavSpec { channels: 1, ..wav_spec() };
        let mut writer = hound::WavWriter::create(&path, spec).unwrap();
        for s in sine(WHISPER_RATE as usize, WHISPER_RATE, 300.0, 0.5) {
            writer.write_sample((s * 32767.0) as i16).unwrap();
        }
        writer.finalize().unwrap();
        let mut file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        file.seek(SeekFrom::Start(40)).unwrap();
        file.write_all(&0u32.to_le_bytes()).unwrap();
        drop(file);
        assert_eq!(recover_partial(&path).unwrap(), WHISPER_RATE);
    }

    #[test]
    fn an_empty_or_unreadable_partial_reports_no_audio() {
        let empty = scratch("empty.partial");
        let writer = open_writer(&empty).unwrap();
        writer.finalize().unwrap();
        assert_eq!(recover_partial(&empty).unwrap(), 0);

        let junk = scratch("junk.partial");
        std::fs::write(&junk, vec![0u8; 200]).unwrap();
        assert_eq!(recover_partial(&junk).unwrap(), 0);

        let tiny = scratch("tiny.partial");
        std::fs::write(&tiny, b"RIFF").unwrap();
        assert_eq!(recover_partial(&tiny).unwrap(), 0);
    }
}
