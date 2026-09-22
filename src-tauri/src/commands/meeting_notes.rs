//! Tauri command wrappers over `tidy_core::store::meeting_notes`.
//!
//! Writes to a meeting page emit `document-changed`, so an open editor loads
//! the new body instead of saving over it.

use tauri::{AppHandle, Emitter};

use crate::error::AppResult;
use crate::workspace::run_db;
use tidy_core::store::meeting_notes::{self, Speaker, SpeakerName, TasksAdded};

fn page_changed(app: &AppHandle, page_id: &str) {
    let _ = app.emit("document-changed", serde_json::json!({ "page_id": page_id }));
}

#[tauri::command]
pub async fn meeting_speakers(app: AppHandle, page_id: String) -> AppResult<Vec<Speaker>> {
    run_db(&app, move |conn| meeting_notes::speakers(conn, &page_id)).await
}

#[tauri::command]
pub async fn rename_meeting_speakers(
    app: AppHandle,
    page_id: String,
    names: Vec<SpeakerName>,
) -> AppResult<bool> {
    let id = page_id.clone();
    let changed = run_db(&app, move |conn| {
        let changed = meeting_notes::rename_speakers(conn, &id, &names)?;
        if changed {
            crate::vault::flush_if_configured(conn, &id);
        }
        Ok(changed)
    })
    .await?;
    if changed {
        page_changed(&app, &page_id);
    }
    Ok(changed)
}

#[tauri::command]
pub async fn add_meeting_notes_to_page(app: AppHandle, page_id: String) -> AppResult<bool> {
    let id = page_id.clone();
    let replaced = run_db(&app, move |conn| {
        let replaced = meeting_notes::add_notes_to_page(conn, &id)?;
        crate::vault::flush_if_configured(conn, &id);
        Ok(replaced)
    })
    .await?;
    page_changed(&app, &page_id);
    Ok(replaced)
}

#[tauri::command]
pub async fn meeting_next_steps_to_tasks(
    app: AppHandle,
    page_id: String,
    database_id: String,
) -> AppResult<TasksAdded> {
    let added = run_db(&app, move |conn| {
        meeting_notes::next_steps_to_tasks(conn, &page_id, &database_id)
    })
    .await?;
    let _ = app.emit("database-changed", ());
    Ok(added)
}
