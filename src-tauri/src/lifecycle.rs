//! Quitting Tidy safely.
//!
//! A recording in flight has to be finalised before the process goes away, so a
//! close request is intercepted, handed to the front end (which can warn, save,
//! or simply confirm), and only then carried out. The front end is not trusted
//! with the decision: a fallback timer performs the same finalisation and exits
//! anyway, so a hung or crashed web view can never trap the user in the app.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};

use crate::error::AppResult;

/// How long the front end gets to answer `close-requested` before Tidy quits
/// on its own.
const FALLBACK: Duration = Duration::from_secs(5);

#[derive(Default)]
pub struct CloseState {
    /// Set once the quit has been confirmed. From then on close and exit
    /// requests are allowed straight through.
    confirmed: AtomicBool,
    /// Set while a close request is outstanding, so repeated clicks on the red
    /// button do not stack up fallback timers.
    pending: AtomicBool,
}

impl CloseState {
    pub fn is_confirmed(&self) -> bool {
        self.confirmed.load(Ordering::SeqCst)
    }
}

/// Handle a close or exit request. Returns whether the quit should be blocked
/// for now; the caller prevents its own event accordingly.
pub fn intercept(app: &AppHandle) -> bool {
    let state = app.state::<CloseState>();
    if state.is_confirmed() {
        return false;
    }
    // Only the first request starts the clock.
    if !state.pending.swap(true, Ordering::SeqCst) {
        let _ = app.emit("close-requested", ());
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(FALLBACK).await;
            if !app.state::<CloseState>().is_confirmed() {
                log::warn!("no answer to close-requested in {FALLBACK:?}; quitting anyway");
                finalise_and_exit(&app);
            }
        });
    }
    true
}

/// Finish any in-flight work and quit. Errors are logged, never fatal: failing
/// to tidy up must not leave the user unable to close the app.
pub fn finalise_and_exit(app: &AppHandle) {
    app.state::<CloseState>()
        .confirmed
        .store(true, Ordering::SeqCst);
    if let Err(e) = crate::commands::recording::finish_on_exit(app) {
        log::warn!("could not finalise the recording before exit: {e}");
    }
    app.exit(0);
}

/// The front end is done with its save-and-warn pass: quit for real.
#[tauri::command]
pub fn confirm_close(app: AppHandle) -> AppResult<()> {
    finalise_and_exit(&app);
    Ok(())
}
