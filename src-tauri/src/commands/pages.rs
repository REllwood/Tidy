//! Tauri command wrappers over `appflower_core::store::pages`. All domain logic
//! lives in the shared core crate; these just marshal state and emit events.

use tauri::{AppHandle, Emitter, State};

use crate::db::Db;
use crate::error::AppResult;
use appflower_core::store::knowledge;
use appflower_core::store::pages::{core, Page};

fn emit_changed(app: &AppHandle) {
    let _ = app.emit("pages-changed", ());
}

#[tauri::command]
pub fn create_page(
    app: AppHandle,
    db: State<Db>,
    parent_id: Option<String>,
    title: String,
    kind: String,
) -> AppResult<Page> {
    let conn = db.conn.lock().unwrap();
    let page = core::create(&conn, parent_id, title, kind)?;
    let _ = knowledge::core::resolve_danglers(&conn, &page.id, &page.title);
    drop(conn);
    emit_changed(&app);
    Ok(page)
}

#[tauri::command]
pub fn get_page(db: State<Db>, id: String) -> AppResult<Page> {
    let conn = db.conn.lock().unwrap();
    core::read(&conn, &id)
}

#[tauri::command]
pub fn list_pages(db: State<Db>) -> AppResult<Vec<Page>> {
    let conn = db.conn.lock().unwrap();
    core::list(&conn)
}

#[tauri::command]
pub fn rename_page(app: AppHandle, db: State<Db>, id: String, title: String) -> AppResult<Page> {
    let conn = db.conn.lock().unwrap();
    let page = core::rename(&conn, &id, &title)?;
    let _ = knowledge::core::resolve_danglers(&conn, &page.id, &page.title);
    // Title drives the frontmatter + filename; re-flush so the vault renames too.
    crate::vault::flush_if_configured(&conn, &id);
    drop(conn);
    emit_changed(&app);
    Ok(page)
}

#[tauri::command]
pub fn set_page_icon(
    app: AppHandle,
    db: State<Db>,
    id: String,
    icon: Option<String>,
) -> AppResult<Page> {
    let conn = db.conn.lock().unwrap();
    let page = core::set_icon(&conn, &id, icon.as_deref())?;
    drop(conn);
    emit_changed(&app);
    Ok(page)
}

#[tauri::command]
pub fn set_page_favorite(
    app: AppHandle,
    db: State<Db>,
    id: String,
    is_favorite: bool,
) -> AppResult<Page> {
    let conn = db.conn.lock().unwrap();
    let page = core::set_favorite(&conn, &id, is_favorite)?;
    drop(conn);
    emit_changed(&app);
    Ok(page)
}

#[tauri::command]
pub fn move_page(
    app: AppHandle,
    db: State<Db>,
    id: String,
    parent_id: Option<String>,
    position: f64,
) -> AppResult<Page> {
    let conn = db.conn.lock().unwrap();
    let page = core::move_page(&conn, &id, parent_id, position)?;
    drop(conn);
    emit_changed(&app);
    Ok(page)
}

#[tauri::command]
pub fn delete_page(app: AppHandle, db: State<Db>, id: String) -> AppResult<()> {
    // Remove the page's vault file first (needs its vault_path, gone after delete).
    crate::vault::remove_page_file(&app, &id);
    let conn = db.conn.lock().unwrap();
    core::delete(&conn, &id)?;
    drop(conn);
    emit_changed(&app);
    Ok(())
}
