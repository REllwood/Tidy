//! On-device transcription via whisper-rs. Runs off-thread (whisper is a long
//! blocking call) and emits progress events.

use std::path::Path;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::audio::pipeline::{downmix_to_mono, resample_mono, WHISPER_RATE};
use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::whisper::models;

#[derive(Serialize, Clone)]
pub struct TranscriptSegment {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

/// Load a WAV file as 16 kHz mono f32 (resampling/downmixing if needed).
pub(crate) fn load_pcm_16k_mono(path: &Path) -> AppResult<Vec<f32>> {
    let reader = hound::WavReader::open(path)
        .map_err(|e| AppError::Other(format!("open wav: {e}")))?;
    let spec = reader.spec();
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int => reader
            .into_samples::<i16>()
            .map(|s| s.unwrap_or(0) as f32 / 32768.0)
            .collect(),
        hound::SampleFormat::Float => reader
            .into_samples::<f32>()
            .map(|s| s.unwrap_or(0.0))
            .collect(),
    };
    let mono = downmix_to_mono(&samples, spec.channels as usize);
    Ok(resample_mono(&mono, spec.sample_rate, WHISPER_RATE))
}

fn run(app: &AppHandle, model_path: &Path, audio_path: &Path) -> AppResult<Vec<TranscriptSegment>> {
    let pcm = load_pcm_16k_mono(audio_path)?;
    if pcm.is_empty() {
        return Ok(vec![]);
    }

    let ctx = WhisperContext::new_with_params(
        model_path,
        WhisperContextParameters::default(),
    )
    .map_err(|e| AppError::Other(format!("load model: {e}")))?;
    let mut state = ctx
        .create_state()
        .map_err(|e| AppError::Other(format!("whisper state: {e}")))?;

    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    let threads = std::thread::available_parallelism()
        .map(|n| n.get() as i32)
        .unwrap_or(4);
    params.set_n_threads(threads);
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);

    let app_cb = app.clone();
    params.set_progress_callback_safe(move |p: i32| {
        let _ = app_cb.emit("transcribe-progress", p);
    });

    state
        .full(params, &pcm)
        .map_err(|e| AppError::Other(format!("transcribe: {e}")))?;

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
    let _ = app.emit("transcribe-progress", 100);
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
    let cleanup_path = audio.clone();
    let result = tauri::async_runtime::spawn_blocking(move || run(&app, &model_path, &audio))
        .await
        .map_err(|e| AppError::Other(format!("transcribe task: {e}")))?;
    // The recording WAV is temporary — remove it once transcribed successfully.
    if result.is_ok() {
        let _ = std::fs::remove_file(&cleanup_path);
    }
    result
}
