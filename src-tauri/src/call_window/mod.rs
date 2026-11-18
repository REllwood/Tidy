//! Names from the call window. When the person turns this on and macOS
//! allows it, Tidy reads the Teams or Google Meet window once a second while
//! it records: who is in the call, who is shown speaking, and the speaker
//! names on captions. Only names and times are kept, never caption text, so
//! the transcript's voices can be named after the people shown speaking.
//!
//! Reading another app's window needs the Accessibility permission, which is
//! a large one to give, so this is off until the person turns it on. Nothing
//! is read from other apps or from browser tabs that are not a call.

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
mod scan;

#[cfg(target_os = "macos")]
mod ax;

#[cfg(target_os = "macos")]
mod watch;

#[cfg(all(test, target_os = "macos"))]
mod live;

use serde::Serialize;
use tauri::{AppHandle, Manager};
use tidy_core::store::recordings::CallRecord;

use crate::error::AppResult;

const ENABLED: &str = "call_names.enabled";

/// Where a recording that found no one speaking leaves a report of what the
/// call window looked like, with every word in it replaced.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const REPORT_FILE: &str = "call-window-report.txt";

#[derive(Debug, Serialize)]
pub struct CallNamesStatus {
    /// The person turned the option on.
    pub enabled: bool,
    /// macOS allows Tidy to read other apps' windows.
    pub allowed: bool,
    /// This Mac can do it at all.
    pub supported: bool,
}

pub fn enabled(app: &AppHandle) -> bool {
    let db = app.state::<crate::db::Db>();
    let Ok(conn) = db.lock() else {
        return false;
    };
    crate::db::get_setting(&conn, ENABLED).ok().flatten().as_deref() == Some("true")
}

fn status(app: &AppHandle) -> CallNamesStatus {
    CallNamesStatus { enabled: enabled(app), allowed: allowed(), supported: cfg!(target_os = "macos") }
}

#[cfg(target_os = "macos")]
fn allowed() -> bool {
    ax::trusted()
}

#[cfg(not(target_os = "macos"))]
fn allowed() -> bool {
    false
}

#[tauri::command]
pub async fn call_names_status(app: AppHandle) -> CallNamesStatus {
    status(&app)
}

/// Turn the option on, asking macOS for the Accessibility permission. macOS
/// shows its own prompt, which leads to System Settings.
#[tauri::command]
pub async fn call_names_enable(app: AppHandle) -> AppResult<CallNamesStatus> {
    crate::local_ai::db(&app, |c| crate::db::set_setting(c, ENABLED, "true")).await?;
    #[cfg(target_os = "macos")]
    ax::ask();
    Ok(status(&app))
}

#[tauri::command]
pub async fn call_names_disable(app: AppHandle) -> AppResult<()> {
    crate::local_ai::db(&app, |c| crate::db::set_setting(c, ENABLED, "false")).await
}

/// Open the Accessibility list in System Settings.
#[tauri::command]
pub async fn call_names_open_settings() -> AppResult<()> {
    std::process::Command::new("/usr/bin/open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
        .spawn()
        .map_err(|e| crate::error::AppError::Other(format!("System Settings did not open: {e}")))?;
    Ok(())
}

/// Reading the call window during one recording.
pub struct Watch {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    handle: Option<std::thread::JoinHandle<CallRecord>>,
}

impl Watch {
    /// Stop reading and return what was seen.
    pub fn finish(mut self) -> CallRecord {
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        self.handle.take().and_then(|h| h.join().ok()).unwrap_or_default()
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Start reading the call window for a recording that began at `began`,
/// when the option is on and macOS allows it.
pub fn start(app: &AppHandle, began: std::time::Instant) -> Option<Watch> {
    if !enabled(app) || !allowed() {
        return None;
    }
    #[cfg(target_os = "macos")]
    {
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = stop.clone();
        let report = crate::workspace::directory(app).join(REPORT_FILE);
        let handle = std::thread::Builder::new()
            .name("call-window".into())
            .spawn(move || watch::run(began, &flag, Some(&report)))
            .ok()?;
        Some(Watch { stop, handle: Some(handle) })
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = began;
        None
    }
}
