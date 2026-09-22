//! Tauri command wrappers over `tidy_core::store::documents`.
//!
//! These deliberately do NOT emit `document-changed`: that event tells the
//! editor its body was rewritten underneath it (by a vault import or an agent),
//! and the app's own saves are already on screen.

use tauri::AppHandle;

use crate::error::AppResult;
use crate::workspace::run_db;
use tidy_core::store::documents::core;

#[tauri::command]
pub async fn get_document(app: AppHandle, id: String) -> AppResult<String> {
    run_db(&app, move |conn| core::get(conn, &id)).await
}

#[tauri::command]
pub async fn update_document(app: AppHandle, id: String, content: String) -> AppResult<()> {
    run_db(&app, move |conn| {
        core::update(conn, &id, &content)?;
        // Mirror the edit to the vault immediately (clears the `dirty` flag), so a
        // linked Obsidian vault reflects in-app edits in real time.
        crate::vault::flush_if_configured(conn, &id);
        Ok(())
    })
    .await
}

/// Save exported text through the system save panel. The desktop webview
/// cancels browser-style downloads, so the file is written here instead.
/// Returns false when the person closes the panel without saving.
#[tauri::command]
pub async fn save_text_file(
    app: AppHandle,
    file_name: String,
    contents: String,
    extension: String,
) -> AppResult<bool> {
    use tauri_plugin_dialog::DialogExt;
    tauri::async_runtime::spawn_blocking(move || {
        let Some(file) = app
            .dialog()
            .file()
            .set_file_name(&file_name)
            .add_filter(extension.to_uppercase(), &[extension.as_str()])
            .blocking_save_file()
        else {
            return Ok(false);
        };
        let path = file
            .into_path()
            .map_err(|e| crate::error::AppError::Other(e.to_string()))?;
        std::fs::write(path, contents)?;
        Ok(true)
    })
    .await
    .map_err(|e| crate::error::AppError::Other(e.to_string()))?
}

/// Speaker labelling must never overwrite edits made after the transcript was saved.
#[tauri::command]
pub async fn update_document_if_unchanged(
    app: AppHandle,
    id: String,
    expected: String,
    content: String,
) -> AppResult<bool> {
    run_db(&app, move |conn| {
        let changed = core::update_if_unchanged(conn, &id, &expected, &content)?;
        if changed {
            crate::vault::flush_if_configured(conn, &id);
        }
        Ok(changed)
    })
    .await
}
