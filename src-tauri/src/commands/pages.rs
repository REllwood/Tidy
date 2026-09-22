//! Tauri command wrappers over `tidy_core::store::pages`. All domain logic
//! lives in the shared core crate; these just marshal state and emit events.
//!
//! Every command runs its database work on the blocking pool (`run_db`) so the
//! UI thread is never parked behind SQLite, and emits its change event once the
//! write has actually landed.

use tauri::{AppHandle, Emitter};

use crate::error::AppResult;
use crate::workspace::run_db;
use tidy_core::store::knowledge;
use tidy_core::store::pages::{core, Page};

fn emit_changed(app: &AppHandle) {
    let _ = app.emit("pages-changed", ());
}

#[tauri::command]
pub async fn create_page(
    app: AppHandle,
    parent_id: Option<String>,
    title: String,
    kind: String,
) -> AppResult<Page> {
    let page = run_db(&app, move |conn| {
        let page = core::create(conn, parent_id, title, kind)?;
        let _ = knowledge::core::resolve_danglers(conn, &page.id, &page.title);
        Ok(page)
    })
    .await?;
    emit_changed(&app);
    Ok(page)
}

#[tauri::command]
pub async fn get_page(app: AppHandle, id: String) -> AppResult<Page> {
    run_db(&app, move |conn| core::read(conn, &id)).await
}

#[tauri::command]
pub async fn list_pages(app: AppHandle) -> AppResult<Vec<Page>> {
    run_db(&app, core::list).await
}

#[tauri::command]
pub async fn rename_page(app: AppHandle, id: String, title: String) -> AppResult<Page> {
    let page = run_db(&app, move |conn| {
        let page = core::rename(conn, &id, &title)?;
        let _ = knowledge::core::resolve_danglers(conn, &page.id, &page.title);
        // Title drives the frontmatter + filename; re-flush so the vault renames too.
        crate::vault::flush_if_configured(conn, &id);
        Ok(page)
    })
    .await?;
    emit_changed(&app);
    Ok(page)
}

#[tauri::command]
pub async fn set_page_icon(
    app: AppHandle,
    id: String,
    icon: Option<String>,
) -> AppResult<Page> {
    let page = run_db(&app, move |conn| core::set_icon(conn, &id, icon.as_deref())).await?;
    emit_changed(&app);
    Ok(page)
}

#[tauri::command]
pub async fn set_page_favorite(
    app: AppHandle,
    id: String,
    is_favorite: bool,
) -> AppResult<Page> {
    let page = run_db(&app, move |conn| core::set_favorite(conn, &id, is_favorite)).await?;
    emit_changed(&app);
    Ok(page)
}

#[tauri::command]
pub async fn move_page(
    app: AppHandle,
    id: String,
    parent_id: Option<String>,
    position: f64,
) -> AppResult<Page> {
    let page = run_db(&app, move |conn| core::move_page(conn, &id, parent_id, position)).await?;
    emit_changed(&app);
    Ok(page)
}

#[tauri::command]
pub async fn delete_page(app: AppHandle, id: String) -> AppResult<()> {
    let deleted = id.clone();
    run_db(&app, move |conn| {
        // Deleting a page cascades to its whole subtree, so every descendant's
        // vault file has to go too — and their `vault_path` is only readable
        // while the rows still exist.
        for descendant in core::subtree_ids(conn, &id)? {
            crate::vault::remove_page_file(conn, &descendant);
        }
        core::delete(conn, &id)
    })
    .await?;
    emit_changed(&app);
    // The cascade also took the subtree's links and any database on it.
    let _ = app.emit("links-changed", &deleted);
    let _ = app.emit("database-changed", ());
    Ok(())
}
