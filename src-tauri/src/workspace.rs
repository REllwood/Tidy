use std::path::PathBuf;
use tauri::{AppHandle, Manager};

pub struct WorkspaceDirectory(pub PathBuf);
pub fn directory(app: &AppHandle) -> PathBuf {
    app.state::<WorkspaceDirectory>().0.clone()
}
