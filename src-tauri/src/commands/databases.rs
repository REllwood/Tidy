//! Tauri command wrappers over `tidy_core::store::databases`.

use std::collections::HashMap;

use serde_json::Value;
use tauri::{AppHandle, Emitter};

use crate::error::AppResult;
use crate::workspace::run_db;
use tidy_core::store::databases::{
    core, DatabaseBundle, DatabaseSummary, DbView, Field, RowWithCells,
};
use tidy_core::store::pages::Page;

fn changed(app: &AppHandle) {
    let _ = app.emit("database-changed", ());
}

#[tauri::command]
pub async fn get_database(app: AppHandle, page_id: String) -> AppResult<DatabaseBundle> {
    run_db(&app, move |conn| core::bundle(conn, &page_id)).await
}

#[tauri::command]
pub async fn get_database_by_id(
    app: AppHandle,
    database_id: String,
) -> AppResult<DatabaseBundle> {
    run_db(&app, move |conn| core::bundle_by_id(conn, &database_id)).await
}

#[tauri::command]
pub async fn list_databases(app: AppHandle) -> AppResult<Vec<DatabaseSummary>> {
    run_db(&app, core::list_databases).await
}

/// Promote a row into a full page (the additive "row = page"). Returns the page.
#[tauri::command]
pub async fn promote_row(app: AppHandle, row_id: String, title: String) -> AppResult<Page> {
    let page = run_db(&app, move |conn| core::promote_row(conn, &row_id, &title)).await?;
    changed(&app);
    // A new record page appeared in the tree — refresh the sidebar too.
    let _ = app.emit("pages-changed", ());
    Ok(page)
}

#[tauri::command]
pub async fn create_field(
    app: AppHandle,
    database_id: String,
    name: String,
    kind: String,
    options: Option<Value>,
) -> AppResult<Field> {
    let f = run_db(&app, move |conn| {
        core::create_field(conn, &database_id, &name, &kind, options)
    })
    .await?;
    changed(&app);
    Ok(f)
}

#[tauri::command]
pub async fn update_field(
    app: AppHandle,
    id: String,
    name: Option<String>,
    options: Option<Value>,
) -> AppResult<Field> {
    let f = run_db(&app, move |conn| {
        core::update_field(conn, &id, name.as_deref(), options)
    })
    .await?;
    changed(&app);
    Ok(f)
}

#[tauri::command]
pub async fn delete_field(app: AppHandle, id: String) -> AppResult<()> {
    run_db(&app, move |conn| core::delete_field(conn, &id)).await?;
    changed(&app);
    Ok(())
}

#[tauri::command]
pub async fn create_row(app: AppHandle, database_id: String) -> AppResult<RowWithCells> {
    let r = run_db(&app, move |conn| core::create_row(conn, &database_id)).await?;
    changed(&app);
    Ok(r)
}

/// Create a row with its cells already filled, in one transaction — so a row
/// added from a template or a form never shows up blank first.
#[tauri::command]
pub async fn create_row_with_cells(
    app: AppHandle,
    database_id: String,
    cells: HashMap<String, Value>,
) -> AppResult<RowWithCells> {
    let r = run_db(&app, move |conn| {
        core::create_row_with_cells(conn, &database_id, cells)
    })
    .await?;
    changed(&app);
    Ok(r)
}

#[tauri::command]
pub async fn delete_row(app: AppHandle, id: String) -> AppResult<()> {
    run_db(&app, move |conn| core::delete_row(conn, &id)).await?;
    changed(&app);
    Ok(())
}

#[tauri::command]
pub async fn move_row(app: AppHandle, id: String, position: f64) -> AppResult<()> {
    run_db(&app, move |conn| core::move_row(conn, &id, position)).await?;
    changed(&app);
    Ok(())
}

#[tauri::command]
pub async fn set_cell(
    app: AppHandle,
    row_id: String,
    field_id: String,
    value: Value,
) -> AppResult<()> {
    run_db(&app, move |conn| {
        core::set_cell(conn, &row_id, &field_id, &value)
    })
    .await?;
    changed(&app);
    Ok(())
}

#[tauri::command]
pub async fn update_view(app: AppHandle, id: String, config: Value) -> AppResult<DbView> {
    let v = run_db(&app, move |conn| core::update_view(conn, &id, config)).await?;
    changed(&app);
    Ok(v)
}
