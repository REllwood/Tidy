//! Tauri command wrappers over `appflower_core::store::knowledge`.

use tauri::{AppHandle, Emitter, State};

use crate::db::Db;
use crate::error::AppResult;
use appflower_core::store::knowledge::{core, Backlink, LinkGraph, LinkInput};

#[tauri::command]
pub fn set_page_links(
    app: AppHandle,
    db: State<Db>,
    page_id: String,
    links: Vec<LinkInput>,
    tags: Vec<String>,
) -> AppResult<()> {
    let conn = db.conn.lock().unwrap();
    core::set_page_links(&conn, &page_id, &links, &tags)?;
    drop(conn);
    let _ = app.emit("links-changed", &page_id);
    Ok(())
}

#[tauri::command]
pub fn get_backlinks(db: State<Db>, page_id: String) -> AppResult<Vec<Backlink>> {
    let conn = db.conn.lock().unwrap();
    core::get_backlinks(&conn, &page_id)
}

#[tauri::command]
pub fn get_page_tags(db: State<Db>, page_id: String) -> AppResult<Vec<String>> {
    let conn = db.conn.lock().unwrap();
    core::tags_for_page(&conn, &page_id)
}

#[tauri::command]
pub fn get_graph(db: State<Db>) -> AppResult<LinkGraph> {
    let conn = db.conn.lock().unwrap();
    core::get_graph(&conn)
}
