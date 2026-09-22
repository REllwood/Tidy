use std::path::PathBuf;
use tauri::{AppHandle, Manager};

use crate::db::Db;
use crate::error::{AppError, AppResult};

pub struct WorkspaceDirectory(pub PathBuf);
pub fn directory(app: &AppHandle) -> PathBuf {
    app.state::<WorkspaceDirectory>().0.clone()
}

/// Run a piece of database work off the UI thread.
///
/// Every command that touches SQLite goes through here. Tauri runs commands on
/// the main thread, so doing the query inline blocks the window (and the whole
/// app) behind whatever else holds the connection — a long vault export, a
/// transcript write. `spawn_blocking` moves the work to the blocking pool and
/// the command awaits it instead.
pub async fn run_db<T: Send + 'static>(
    app: &AppHandle,
    f: impl FnOnce(&rusqlite::Connection) -> AppResult<T> + Send + 'static,
) -> AppResult<T> {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let db = app.state::<Db>();
        let conn = db.lock()?;
        f(&conn)
    })
    .await
    .map_err(|e| AppError::Other(e.to_string()))?
}
