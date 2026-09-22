//! Tauri command wrapper over `tidy_core::store::search`.

use tauri::AppHandle;

use crate::error::AppResult;
use crate::workspace::run_db;
use tidy_core::store::search::{core, SearchResults};

#[tauri::command]
pub async fn search(app: AppHandle, query: String) -> AppResult<SearchResults> {
    run_db(&app, move |conn| core::search(conn, &query)).await
}
