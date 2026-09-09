//! Tauri command wrappers over `appflower_core::store::documents`.

use tauri::State;

use crate::db::Db;
use crate::error::AppResult;
use appflower_core::store::documents::core;

#[tauri::command]
pub fn get_document(db: State<Db>, id: String) -> AppResult<String> {
    let conn = db.conn.lock().unwrap();
    core::get(&conn, &id)
}

#[tauri::command]
pub fn update_document(db: State<Db>, id: String, content: String) -> AppResult<()> {
    let conn = db.conn.lock().unwrap();
    core::update(&conn, &id, &content)?;
    // Mirror the edit to the vault immediately (clears the `dirty` flag), so a
    // linked Obsidian vault reflects in-app edits in real time.
    crate::vault::flush_if_configured(&conn, &id);
    Ok(())
}

/// Speaker labelling must never overwrite edits made after the transcript was saved.
#[tauri::command]
pub fn update_document_if_unchanged(
    db: State<Db>,
    id: String,
    expected: String,
    content: String,
) -> AppResult<bool> {
    let conn = db
        .conn
        .lock()
        .map_err(|_| crate::error::AppError::Other("Database unavailable".into()))?;
    let changed = core::update_if_unchanged(&conn, &id, &expected, &content)?;
    if changed {
        crate::vault::flush_if_configured(&conn, &id);
    }
    Ok(changed)
}
