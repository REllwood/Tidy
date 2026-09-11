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
    keep_audio: bool,
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
    pub keep_audio: bool,
    pub audio_path: String,
    pub duration_ms: i64,
    pub page_id: Option<String>,
}

pub fn start(
    app: AppHandle,
    model_path: Option<PathBuf>,
    keep_audio: bool,
) -> AppResult<RecordingSession> {
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
            let _ = app.emit(
                "audio-level",
                LevelEvent {
                    mic: mic_level,
                    system: sys_level,
                },
            );
            let _ = app.emit(
                "recording-tick",
                TickEvent {
                    elapsed_ms: start.elapsed().as_millis() as i64,
                },
            );
        }
        // Dropping the streams stops capture.
        drop(mic);
        drop(system);
    });

    Ok(RecordingSession {
        keep_audio,
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
    let mic = std::mem::take(&mut *session.mic_buf.lock().unwrap());
    let sys = std::mem::take(&mut *session.sys_buf.lock().unwrap());
    let mic_rate = session.mic_rate.load(Ordering::Relaxed);

    let mixed = combine_sources(&mic, mic_rate, &sys, SYSTEM_RATE);
    let duration_ms = (mixed.len() as i64 * 1000) / WHISPER_RATE as i64;

    let dir = if session.keep_audio {
        app_recordings_dir(app)?
    } else {
        app_recordings_dir(app)?.join("temporary")
    };
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!(
        "{}-{}.wav",
        session.started_at,
        crate::db::new_id()
    ));
    let partial = path.with_extension("partial");
    write_wav(&partial, &mixed)?;
    std::fs::File::open(&partial)?.sync_all()?;
    std::fs::rename(&partial, &path)?;

    Ok(Recording {
        keep_audio: session.keep_audio,
        audio_path: path.to_string_lossy().to_string(),
        duration_ms,
        page_id: None,
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

        while !stop.load(Ordering::Relaxed) {
            std::thread::sleep(STEP);
            if stop.load(Ordering::Relaxed) {
                break;
            }
            // Copy only the preview window, not the entire meeting every 2.5 seconds.
            let mic = {
                let samples = mic_buf.lock().unwrap();
                samples[samples
                    .len()
                    .saturating_sub(mic_rate.load(Ordering::Relaxed) as usize * 15)..]
                    .to_vec()
            };
            let sys = {
                let samples = sys_buf.lock().unwrap();
                samples[samples
                    .len()
                    .saturating_sub(super::system::SYSTEM_RATE as usize * 15)..]
                    .to_vec()
            };
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
            params.set_n_threads(threads.min(4));
            params.set_translate(false);
            params.set_language(Some("en"));
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

#[cfg(test)]
mod meter_tests {
    use super::*;
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
}
