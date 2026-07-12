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
pub fn start_recording(
    app: AppHandle,
    state: State<RecorderState>,
    db: State<Db>,
) -> AppResult<()> {
    let mut guard = state.0.lock().unwrap();
    if guard.is_some() {
        return Err(AppError::Other("a recording is already in progress".into()));
    }
    // Resolve the selected Whisper model for live-preview transcription (optional).
    let model_path = {
        let conn = db.conn.lock().unwrap();
        crate::whisper::models::core::selected_path(&conn, &app).ok()
    };
    *guard = Some(recorder::start(app, model_path)?);
    Ok(())
}

#[tauri::command]
pub fn stop_recording(app: AppHandle, state: State<RecorderState>) -> AppResult<Recording> {
    let session = state
        .0
        .lock()
        .unwrap()
        .take()
        .ok_or_else(|| AppError::Other("no recording in progress".into()))?;
    recorder::stop(&app, session)
}

#[tauri::command]
pub fn is_recording(state: State<RecorderState>) -> bool {
    state.0.lock().unwrap().is_some()
}

/// Persist a meeting record linking the saved page to its recording metadata.
#[tauri::command]
pub fn record_meeting(
    db: State<Db>,
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
        params![new_id(), page_id, started_at, duration_ms, audio_path, model_used],
    )?;
    Ok(())
}
