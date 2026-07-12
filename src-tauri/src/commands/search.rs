//! Tauri command wrapper over `appflower_core::store::search`.

use tauri::State;

use crate::db::Db;
use crate::error::AppResult;
use appflower_core::store::search::{core, SearchResults};

#[tauri::command]
pub fn search(db: State<Db>, query: String) -> AppResult<SearchResults> {
    let conn = db.conn.lock().unwrap();
    core::search(&conn, &query)
}
