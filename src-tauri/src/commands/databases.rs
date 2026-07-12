//! Tauri command wrappers over `appflower_core::store::databases`.

use serde_json::Value;
use tauri::{AppHandle, Emitter, State};

use crate::db::Db;
use crate::error::AppResult;
use appflower_core::store::databases::{
    core, DatabaseBundle, DatabaseSummary, DbView, Field, RowWithCells,
};
use appflower_core::store::pages::Page;

fn changed(app: &AppHandle) {
    let _ = app.emit("database-changed", ());
}

#[tauri::command]
pub fn get_database(db: State<Db>, page_id: String) -> AppResult<DatabaseBundle> {
    let conn = db.conn.lock().unwrap();
    core::bundle(&conn, &page_id)
}

#[tauri::command]
pub fn get_database_by_id(db: State<Db>, database_id: String) -> AppResult<DatabaseBundle> {
    let conn = db.conn.lock().unwrap();
    core::bundle_by_id(&conn, &database_id)
}

#[tauri::command]
pub fn list_databases(db: State<Db>) -> AppResult<Vec<DatabaseSummary>> {
    let conn = db.conn.lock().unwrap();
    core::list_databases(&conn)
}

/// Promote a row into a full page (the additive "row = page"). Returns the page.
#[tauri::command]
pub fn promote_row(
    app: AppHandle,
    db: State<Db>,
    row_id: String,
    title: String,
) -> AppResult<Page> {
    let conn = db.conn.lock().unwrap();
    let page = core::promote_row(&conn, &row_id, &title)?;
    drop(conn);
    changed(&app);
    // A new record page appeared in the tree — refresh the sidebar too.
    let _ = app.emit("pages-changed", ());
    Ok(page)
}

#[tauri::command]
pub fn create_field(
    app: AppHandle,
    db: State<Db>,
    database_id: String,
    name: String,
    kind: String,
    options: Option<Value>,
) -> AppResult<Field> {
    let conn = db.conn.lock().unwrap();
    let f = core::create_field(&conn, &database_id, &name, &kind, options)?;
    drop(conn);
    changed(&app);
    Ok(f)
}

#[tauri::command]
pub fn update_field(
    app: AppHandle,
    db: State<Db>,
    id: String,
    name: Option<String>,
    options: Option<Value>,
) -> AppResult<Field> {
    let conn = db.conn.lock().unwrap();
    let f = core::update_field(&conn, &id, name.as_deref(), options)?;
    drop(conn);
    changed(&app);
    Ok(f)
}

#[tauri::command]
pub fn delete_field(app: AppHandle, db: State<Db>, id: String) -> AppResult<()> {
    let conn = db.conn.lock().unwrap();
    core::delete_field(&conn, &id)?;
    drop(conn);
    changed(&app);
    Ok(())
}

#[tauri::command]
pub fn create_row(app: AppHandle, db: State<Db>, database_id: String) -> AppResult<RowWithCells> {
    let conn = db.conn.lock().unwrap();
    let r = core::create_row(&conn, &database_id)?;
    drop(conn);
    changed(&app);
    Ok(r)
}

#[tauri::command]
pub fn delete_row(app: AppHandle, db: State<Db>, id: String) -> AppResult<()> {
    let conn = db.conn.lock().unwrap();
    core::delete_row(&conn, &id)?;
    drop(conn);
    changed(&app);
    Ok(())
}

#[tauri::command]
pub fn move_row(app: AppHandle, db: State<Db>, id: String, position: f64) -> AppResult<()> {
    let conn = db.conn.lock().unwrap();
    core::move_row(&conn, &id, position)?;
    drop(conn);
    changed(&app);
    Ok(())
}

#[tauri::command]
pub fn set_cell(
    app: AppHandle,
    db: State<Db>,
    row_id: String,
    field_id: String,
    value: Value,
) -> AppResult<()> {
    let conn = db.conn.lock().unwrap();
    core::set_cell(&conn, &row_id, &field_id, &value)?;
    drop(conn);
    changed(&app);
    Ok(())
}

#[tauri::command]
pub fn update_view(app: AppHandle, db: State<Db>, id: String, config: Value) -> AppResult<DbView> {
    let conn = db.conn.lock().unwrap();
    let v = core::update_view(&conn, &id, config)?;
    drop(conn);
    changed(&app);
    Ok(v)
}
