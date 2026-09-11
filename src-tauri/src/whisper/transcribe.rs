//! On-device transcription via whisper-rs. Runs off-thread (whisper is a long
//! blocking call) and emits progress events.

use std::path::Path;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::audio::pipeline::{downmix_to_mono, resample_mono, WHISPER_RATE};
use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::whisper::models;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TranscriptSegment {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

/// Load a WAV file as 16 kHz mono f32 (resampling/downmixing if needed).
pub(crate) fn load_pcm_16k_mono(path: &Path) -> AppResult<Vec<f32>> {
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
    let mono = downmix_to_mono(&samples, spec.channels as usize);
    Ok(resample_mono(&mono, spec.sample_rate, WHISPER_RATE))
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
    if pcm.is_empty()
        || !pcm.iter().all(|v| v.is_finite())
        || crate::audio::pipeline::rms(&pcm) < 0.000001
    {
        return Err(AppError::Invalid(
            "The saved recording contains no audio.".into(),
        ));
    }

    let ctx = WhisperContext::new_with_params(model_path, WhisperContextParameters::default())
        .map_err(|e| AppError::Other(format!("load model: {e}")))?;
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
    params.set_language(Some(language));
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

    let decoded = state.full(params, &pcm);
    if cancel.load(std::sync::atomic::Ordering::SeqCst) {
        return Err(AppError::Other(
            "Transcription cancelled. Audio and earlier text have been kept.".into(),
        ));
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
        return Err(AppError::Other(
            "Transcription cancelled. Audio and earlier text have been kept.".into(),
        ));
    }
    if out.iter().all(|s| s.text.trim().is_empty()) {
        return Err(AppError::Other(
            "No speech was recognised. The audio has been kept; try another model or language."
                .into(),
        ));
    }
    Ok(out)
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
