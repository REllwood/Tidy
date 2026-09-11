use std::sync::Mutex;

use rusqlite::params;
use tauri::{AppHandle, State};

use crate::audio::recorder::{self, Recording, RecordingSession};
use crate::db::{new_id, now_ms, Db};
use crate::error::{AppError, AppResult};

/// Holds the in-flight recording session (at most one at a time).
#[derive(Default)]
pub struct RecorderState(pub Mutex<Option<RecordingSession>>);

#[tauri::command]
pub async fn start_recording(
    app: AppHandle,
    state: State<'_, RecorderState>,
    db: State<'_, Db>,
) -> AppResult<bool> {
    use std::sync::atomic::Ordering;
    use tauri::Manager;
    let ai = app.state::<crate::local_ai::AiState>();
    ai.pause_for_recording().await?;
    let result = (|| {
        let mut guard = state.0.lock().unwrap();
        if guard.is_some() {
            return Err(AppError::Other("a recording is already in progress".into()));
        }
        // Resolve the selected Whisper model for live-preview transcription (optional).
        let model_path = {
            let conn = db.conn.lock().unwrap();
            crate::whisper::models::core::selected_path(&conn, &app).ok()
        };
        let keep_audio = {
            let conn = db
                .conn
                .lock()
                .map_err(|_| AppError::Other("Database unavailable".into()))?;
            super::recording_preferences::retention_enabled(&conn)?
        };
        if model_path.is_none() && !keep_audio {
            return Err(AppError::Invalid("Download a transcription model in Settings before recording, or turn on Keep meeting audio to transcribe later.".into()));
        }
        *guard = Some(recorder::start(app.clone(), model_path, keep_audio)?);
        Ok(keep_audio)
    })();
    if result.is_err() {
        ai.recording.store(false, Ordering::SeqCst);
    }
    result
}

#[tauri::command]
pub async fn stop_recording(
    app: AppHandle,
    state: State<'_, RecorderState>,
    client: Option<String>,
) -> AppResult<Recording> {
    let session = state
        .0
        .lock()
        .map_err(|_| AppError::Other("Recorder unavailable".into()))?
        .take()
        .ok_or_else(|| AppError::Other("No recording in progress".into()))?;
    let handle = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        use tauri::Manager;
        let mut rec = recorder::stop(&handle, session)?;
        let db = handle.state::<Db>();
        let c = db
            .conn
            .lock()
            .map_err(|_| AppError::Other("Database unavailable".into()))?;
        rec.page_id = Some(tidy_core::store::recordings::register_with_retention(
            &c,
            &rec.audio_path,
            rec.duration_ms,
            now_ms() - rec.duration_ms,
            &client.unwrap_or_default(),
            rec.keep_audio,
        )?);
        Ok(rec)
    })
    .await
    .map_err(|e| AppError::Other(e.to_string()));
    use tauri::Manager;
    app.state::<crate::local_ai::AiState>()
        .recording
        .store(false, std::sync::atomic::Ordering::SeqCst);
    result?
}

#[tauri::command]
pub fn is_recording(state: State<RecorderState>) -> bool {
    state.0.lock().unwrap().is_some()
}

/// Persist a meeting record linking the saved page to its recording metadata.
#[tauri::command]
pub fn record_meeting(
    db: State<'_, Db>,
    page_id: String,
    duration_ms: i64,
    audio_path: Option<String>,
    model_used: Option<String>,
) -> AppResult<()> {
    let conn = db.conn.lock().unwrap();
    let started_at = now_ms() - duration_ms;
    conn.execute(
        "INSERT INTO meeting (id, page_id, started_at, duration, audio_path, model_used)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            new_id(),
            page_id,
            started_at,
            duration_ms,
            audio_path,
            model_used
        ],
    )?;
    Ok(())
}
