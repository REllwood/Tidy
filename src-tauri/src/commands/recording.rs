use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager, State};

use crate::audio::recorder::{self, Recording, RecordingSession};
use crate::db::Db;
use crate::error::{AppError, AppResult};

/// Holds the in-flight recording session (at most one at a time).
#[derive(Default)]
pub struct RecorderState(pub Mutex<Option<RecordingSession>>);

impl RecorderState {
    fn lock(&self) -> std::sync::MutexGuard<'_, Option<RecordingSession>> {
        // A panic elsewhere must not make Stop impossible to press.
        self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

const NO_MODEL_SELECTED: &str = "No transcription model is selected. Choose one in Settings, or turn on Keep meeting audio to transcribe later.";
const NO_MODEL_DOWNLOADED: &str = "Download a transcription model in Settings before recording, or turn on Keep meeting audio to transcribe later.";

/// True while a session is held, including one whose Stop failed part-way.
pub fn recording_in_flight(app: &AppHandle) -> bool {
    app.state::<RecorderState>().lock().is_some()
}

/// Control-Option-M marks a moment while recording, from any app.
fn mark_shortcut() -> tauri_plugin_global_shortcut::Shortcut {
    use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut};
    Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyM)
}

fn listen_for_marks(app: &AppHandle, on: bool) {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;
    let shortcuts = app.global_shortcut();
    let result = if on {
        shortcuts.register(mark_shortcut())
    } else {
        shortcuts.unregister(mark_shortcut())
    };
    if let Err(error) = result {
        // Another app may own the shortcut; the Mark button still works.
        log::warn!("The mark shortcut could not be {}: {error}", if on { "set" } else { "released" });
    }
}

#[derive(serde::Serialize, Clone)]
struct Marked {
    at_ms: i64,
    count: usize,
}

/// Mark this moment in the recording, if one is running.
pub fn mark_now(app: &AppHandle) -> Option<i64> {
    let marked = app.state::<RecorderState>().lock().as_mut().map(|session| {
        let at_ms = session.mark();
        Marked { at_ms, count: session.marks().len() }
    })?;
    let _ = app.emit("moment-marked", marked.clone());
    Some(marked.at_ms)
}

#[tauri::command]
pub fn mark_moment(app: AppHandle) -> AppResult<i64> {
    mark_now(&app).ok_or_else(|| AppError::Invalid("There is no recording to mark.".into()))
}

#[tauri::command]
pub async fn start_recording(
    app: AppHandle,
    state: State<'_, RecorderState>,
    db: State<'_, Db>,
) -> AppResult<bool> {
    use std::sync::atomic::Ordering;
    let ai = app.state::<crate::local_ai::AiState>();
    ai.pause_for_recording().await?;
    let result = (|| {
        let mut guard = state.lock();
        if guard.is_some() {
            return Err(AppError::Other("a recording is already in progress".into()));
        }
        let conn = db.lock()?;
        let keep_audio = super::recording_preferences::retention_enabled(&conn)?;
        // Resolve the selected Whisper model for live-preview transcription.
        // Without one we can still record, but only if the audio is kept.
        let model_path = crate::whisper::models::core::selected_path(&conn, &app).ok();
        if model_path.is_none() && !keep_audio {
            let downloaded = crate::whisper::models::core::any_downloaded(&app)?;
            return Err(AppError::Invalid(if downloaded {
                NO_MODEL_SELECTED.into()
            } else {
                NO_MODEL_DOWNLOADED.into()
            }));
        }
        drop(conn);
        *guard = Some(recorder::start(app.clone(), model_path, keep_audio)?);
        Ok(keep_audio)
    })();
    if result.is_ok() {
        listen_for_marks(&app, true);
    }
    if result.is_err() {
        ai.recording.store(false, Ordering::SeqCst);
    }
    result
}

/// Finish a session and write it to the database. On failure the session is
/// handed back so the caller can keep it: the audio is already on disk and
/// pressing Stop again retries the part that failed.
fn finish(
    app: &AppHandle,
    mut session: RecordingSession,
    client: &str,
    stop_reason: Option<&str>,
) -> (Option<RecordingSession>, AppResult<Recording>) {
    let mut recording = match recorder::stop(&mut session) {
        Ok(recording) => recording,
        Err(error) => return (Some(session), Err(error)),
    };
    let registered = (|| {
        let db = app.state::<Db>();
        let conn = db.lock()?;
        let page = tidy_core::store::recordings::register_with_retention(
            &conn,
            &recording.audio_path,
            recording.duration_ms,
            recording.started_at,
            client,
            recording.keep_audio,
        )?;
        tidy_core::store::recordings::set_stop_details(&conn, &page, stop_reason, &recording.marks)?;
        Ok(page)
    })();
    match registered {
        Ok(page_id) => {
            recording.page_id = Some(page_id);
            (None, Ok(recording))
        }
        Err(error) => (Some(session), Err(error)),
    }
}

#[tauri::command]
pub async fn stop_recording(
    app: AppHandle,
    state: State<'_, RecorderState>,
    client: Option<String>,
    stop_reason: Option<String>,
) -> AppResult<Recording> {
    let session = state
        .lock()
        .take()
        .ok_or_else(|| AppError::Other("No recording in progress".into()))?;
    listen_for_marks(&app, false);
    let handle = app.clone();
    let client = client.unwrap_or_default();
    let (kept, result) = tauri::async_runtime::spawn_blocking(move || {
        finish(&handle, session, &client, stop_reason.as_deref())
    })
    .await
    .map_err(|e| AppError::Other(e.to_string()))?;
    if let Some(session) = kept {
        // Keep `is_recording` true so the user can press Stop again.
        *state.lock() = Some(session);
    }
    app.state::<crate::local_ai::AiState>()
        .recording
        .store(false, std::sync::atomic::Ordering::SeqCst);
    if result.is_ok() {
        let handle = app.clone();
        if let Err(error) =
            tauri::async_runtime::spawn_blocking(move || super::recording_history::register_orphans(&handle))
                .await
                .map_err(|e| AppError::Other(e.to_string()))?
        {
            log::warn!("Orphaned recordings could not be registered: {error}");
        }
    }
    // Work the AI queue dropped when this meeting started can run again.
    if let Err(error) = crate::local_ai::requeue_cancelled(&app).await {
        log::warn!("AI queue could not be resumed: {error}");
    }
    result
}

/// Close down an in-flight recording on app exit: write the audio, register it
/// so nothing is lost, and release the AI pause. Does nothing when idle.
pub fn finish_on_exit(app: &AppHandle) -> AppResult<()> {
    let session = app.state::<RecorderState>().lock().take();
    let Some(session) = session else {
        return Ok(());
    };
    listen_for_marks(app, false);
    let (kept, result) = finish(app, session, "", Some("Saved when Tidy quit"));
    if let Some(session) = kept {
        *app.state::<RecorderState>().lock() = Some(session);
    }
    app.state::<crate::local_ai::AiState>()
        .recording
        .store(false, std::sync::atomic::Ordering::SeqCst);
    result.map(|_| ())
}

#[tauri::command]
pub fn is_recording(app: AppHandle) -> bool {
    recording_in_flight(&app)
}
