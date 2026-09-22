//! Tauri command wrappers over `tidy_core::store::knowledge`.

use tauri::{AppHandle, Emitter};

use crate::error::AppResult;
use crate::workspace::run_db;
use tidy_core::store::knowledge::{core, Backlink, LinkGraph, LinkInput};

#[tauri::command]
pub async fn set_page_links(
    app: AppHandle,
    page_id: String,
    links: Vec<LinkInput>,
    tags: Vec<String>,
) -> AppResult<()> {
    let emit_id = page_id.clone();
    run_db(&app, move |conn| {
        core::set_page_links(conn, &page_id, &links, &tags)
    })
    .await?;
    let _ = app.emit("links-changed", &emit_id);
    Ok(())
}

#[tauri::command]
pub async fn get_backlinks(app: AppHandle, page_id: String) -> AppResult<Vec<Backlink>> {
    run_db(&app, move |conn| core::get_backlinks(conn, &page_id)).await
}

#[tauri::command]
pub async fn get_page_tags(app: AppHandle, page_id: String) -> AppResult<Vec<String>> {
    run_db(&app, move |conn| core::tags_for_page(conn, &page_id)).await
}

#[tauri::command]
pub async fn list_tags(app: AppHandle) -> AppResult<Vec<String>> {
    run_db(&app, core::all_tags).await
}

#[tauri::command]
pub async fn get_graph(app: AppHandle) -> AppResult<LinkGraph> {
    run_db(&app, core::get_graph).await
}
