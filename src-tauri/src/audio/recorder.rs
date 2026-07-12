//! Recording controller. cpal and ScreenCaptureKit streams are `!Send`, so the
//! actual capture runs on a dedicated thread; the shared state holds only
//! `Send` handles (stop flag, sample buffers, join handle).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use super::pipeline::{combine_sources, rms, WHISPER_RATE};
use super::system::SYSTEM_RATE;
use crate::db::now_ms;
use crate::error::{AppError, AppResult};

#[derive(Serialize, Clone)]
struct LevelEvent {
    mic: f32,
    system: f32,
}
#[derive(Serialize, Clone)]
struct TickEvent {
    elapsed_ms: i64,
}

pub struct RecordingSession {
    stop: Arc<AtomicBool>,
    mic_buf: Arc<Mutex<Vec<f32>>>,
    sys_buf: Arc<Mutex<Vec<f32>>>,
    mic_rate: Arc<AtomicU32>,
    started_at: i64,
    handle: Option<JoinHandle<()>>,
    live_handle: Option<JoinHandle<()>>,
}

/// Result of stopping a recording.
#[derive(Serialize)]
pub struct Recording {
    pub audio_path: String,
    pub duration_ms: i64,
}

pub fn start(app: AppHandle, model_path: Option<PathBuf>) -> AppResult<RecordingSession> {
    let stop = Arc::new(AtomicBool::new(false));
    let mic_buf = Arc::new(Mutex::new(Vec::<f32>::new()));
    let sys_buf = Arc::new(Mutex::new(Vec::<f32>::new()));
    let mic_rate = Arc::new(AtomicU32::new(WHISPER_RATE));
    let started_at = now_ms();

    // Optional live-transcription preview: re-transcribe a rolling window while
    // recording. The final saved transcript still comes from the full on-stop
    // pass — this is a real-time preview only.
    let live_handle = model_path.map(|mp| {
        spawn_live_transcription(
            app.clone(),
            stop.clone(),
            mic_buf.clone(),
            sys_buf.clone(),
            mic_rate.clone(),
            mp,
        )
    });

    let t_stop = stop.clone();
    let t_mic = mic_buf.clone();
    let t_sys = sys_buf.clone();
    let t_rate = mic_rate.clone();

    let handle = std::thread::spawn(move || {
        // Start both sources; tolerate either failing (e.g. permission denied).
        let mic = match super::mic::start_mic(t_mic.clone()) {
            Ok(m) => {
                t_rate.store(m.sample_rate, Ordering::Relaxed);
                Some(m)
            }
            Err(e) => {
                log::error!("mic capture unavailable: {e}");
                None
            }
        };
        let system = match super::system::start_system(t_sys.clone()) {
            Ok(s) => Some(s),
            Err(e) => {
                log::error!("system audio capture unavailable: {e}");
                None
            }
        };
        let _ = app.emit(
            "recording-sources",
            serde_json::json!({ "mic": mic.is_some(), "system": system.is_some() }),
        );

        let start = std::time::Instant::now();
        let mut last_mic = 0usize;
        let mut last_sys = 0usize;
        while !t_stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(120));
            let mic_level = tail_rms(&t_mic, &mut last_mic);
            let sys_level = tail_rms(&t_sys, &mut last_sys);
            let _ = app.emit("audio-level", LevelEvent { mic: mic_level, system: sys_level });
            let _ = app.emit(
                "recording-tick",
                TickEvent { elapsed_ms: start.elapsed().as_millis() as i64 },
            );
        }
        // Dropping the streams stops capture.
        drop(mic);
        drop(system);
    });

    Ok(RecordingSession {
        stop,
        mic_buf,
        sys_buf,
        mic_rate,
        started_at,
        handle: Some(handle),
        live_handle,
    })
}

/// Stop the session, mix to 16 kHz mono, write a WAV, and return its path.
pub fn stop(app: &AppHandle, mut session: RecordingSession) -> AppResult<Recording> {
    session.stop.store(true, Ordering::Relaxed);
    if let Some(h) = session.handle.take() {
        let _ = h.join();
    }
    // Join the live-transcription worker too so the Whisper model isn't leaked
    // and a re-start doesn't contend with an orphaned decode.
    if let Some(h) = session.live_handle.take() {
        let _ = h.join();
    }
    let mic = session.mic_buf.lock().unwrap().clone();
    let sys = session.sys_buf.lock().unwrap().clone();
    let mic_rate = session.mic_rate.load(Ordering::Relaxed);

    let mixed = combine_sources(&mic, mic_rate, &sys, SYSTEM_RATE);
    let duration_ms = (mixed.len() as i64 * 1000) / WHISPER_RATE as i64;

    let dir = app_recordings_dir(app)?;
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.wav", session.started_at));
    write_wav(&path, &mixed)?;

    Ok(Recording {
        audio_path: path.to_string_lossy().to_string(),
        duration_ms,
    })
}

/// Background worker: every few seconds, transcribe the last ~15s of mixed
/// audio and emit it as a live preview. Loads the Whisper model once and reuses
/// the state across windows.
fn spawn_live_transcription(
    app: AppHandle,
    stop: Arc<AtomicBool>,
    mic_buf: Arc<Mutex<Vec<f32>>>,
    sys_buf: Arc<Mutex<Vec<f32>>>,
    mic_rate: Arc<AtomicU32>,
    model_path: PathBuf,
) -> JoinHandle<()> {
    use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};
    const STEP: Duration = Duration::from_millis(2500);
    const WINDOW_SAMPLES: usize = (WHISPER_RATE as usize) * 15; // last 15s @16k
    const SILENCE_RMS: f32 = 0.005;

    std::thread::spawn(move || {
        let ctx = match WhisperContext::new_with_params(
            &model_path,
            WhisperContextParameters::default(),
        ) {
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

        while !stop.load(Ordering::Relaxed) {
            std::thread::sleep(STEP);
            if stop.load(Ordering::Relaxed) {
                break;
            }
            let mic = mic_buf.lock().unwrap().clone();
            let sys = sys_buf.lock().unwrap().clone();
            let mixed = super::pipeline::combine_sources(
                &mic,
                mic_rate.load(Ordering::Relaxed),
                &sys,
                super::system::SYSTEM_RATE,
            );
            if mixed.is_empty() {
                continue;
            }
            let window = if mixed.len() > WINDOW_SAMPLES {
                &mixed[mixed.len() - WINDOW_SAMPLES..]
            } else {
                &mixed[..]
            };
            if rms(window) < SILENCE_RMS {
                continue; // skip silent windows (cheap VAD gate)
            }
            let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
            params.set_n_threads(threads);
            params.set_print_special(false);
            params.set_print_progress(false);
            params.set_print_realtime(false);
            params.set_print_timestamps(false);
            if state.full(params, window).is_ok() {
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
        let b = buf.lock().unwrap();
        let from = (*last).min(b.len());
        *last = b.len();
        b[from..].to_vec()
    };
    if tail.is_empty() {
        0.0
    } else {
        rms(&tail)
    }
}

pub fn app_recordings_dir(app: &AppHandle) -> AppResult<PathBuf> {
    use tauri::Manager;
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Other(format!("app_data_dir: {e}")))?;
    Ok(dir.join("recordings"))
}

fn write_wav(path: &PathBuf, samples: &[f32]) -> AppResult<()> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: WHISPER_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(path, spec)
        .map_err(|e| AppError::Other(format!("wav create: {e}")))?;
    for s in samples {
        let v = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
        w.write_sample(v)
            .map_err(|e| AppError::Other(format!("wav write: {e}")))?;
    }
    w.finalize()
        .map_err(|e| AppError::Other(format!("wav finalize: {e}")))?;
    Ok(())
}
