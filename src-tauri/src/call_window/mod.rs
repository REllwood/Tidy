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

use serde::Serialize;
use tauri::{AppHandle, Manager};
use tidy_core::store::recordings::CallRecord;

use crate::error::AppResult;

const ENABLED: &str = "call_names.enabled";

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
        let handle = std::thread::Builder::new()
            .name("call-window".into())
            .spawn(move || watch::run(began, &flag))
            .ok()?;
        Some(Watch { stop, handle: Some(handle) })
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = began;
        None
    }
}

#[cfg(target_os = "macos")]
mod watch {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    use tidy_core::store::recordings::CallRecord;

    use super::ax::{self, Element};
    use super::scan::{self, Call, Look, Tracker};

    /// How often to look at the call window.
    const EVERY: Duration = Duration::from_secs(1);
    /// How often to read the whole window again, to find new tiles and
    /// captions. In between, only the tiles and captions are read.
    const FULL_EVERY: Duration = Duration::from_secs(8);
    /// Sooner when nothing has been found yet.
    const RETRY_EVERY: Duration = Duration::from_secs(3);

    fn is_teams(app: &str) -> bool {
        app.to_lowercase().starts_with("microsoft teams")
    }

    fn is_browser(app: &str) -> bool {
        const BROWSERS: &[&str] = &[
            "google chrome", "google chrome beta", "google chrome canary", "chromium", "microsoft edge",
            "brave browser", "arc", "vivaldi", "opera", "safari", "firefox",
        ];
        BROWSERS.contains(&app.to_lowercase().as_str())
    }

    /// Windows that may hold a call: any Teams window, and browser windows
    /// whose title is a Meet or Teams page.
    fn call_windows(app: &Element, name: &str) -> Vec<(Element, Call, String)> {
        app.windows()
            .into_iter()
            .filter_map(|(window, title)| {
                let call = if is_teams(name) { Some(Call::Teams) } else { scan::browser_call(&title) };
                call.map(|c| (window, c, title))
            })
            .collect()
    }

    struct Shared {
        pid: i32,
        app: Element,
        put_back: Vec<&'static str>,
    }

    fn pause(until: Instant, stop: &AtomicBool) {
        while Instant::now() < until && !stop.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    pub fn run(began: Instant, stop: &AtomicBool) -> CallRecord {
        let mut tracker = Tracker::default();
        let mut hot: Vec<Element> = Vec::new();
        let mut last_full: Option<Instant> = None;
        let mut shared: Vec<Shared> = Vec::new();

        while !stop.load(Ordering::SeqCst) {
            let tick = Instant::now();
            let due = last_full.map_or(true, |at| {
                at.elapsed() >= FULL_EVERY || (hot.is_empty() && at.elapsed() >= RETRY_EVERY)
            });
            let at_ms = began.elapsed().as_millis() as i64;
            let mut look = Look::default();

            if due {
                last_full = Some(Instant::now());
                hot.clear();
                let mut reader = ax::Reader::new(Duration::from_millis(1_500), 6_000);
                let mut waiting = false;
                for (pid, name) in ax::running_apps() {
                    if !is_teams(&name) && !is_browser(&name) {
                        continue;
                    }
                    let Some(app) = Element::application(pid) else { continue };
                    let windows = call_windows(&app, &name);
                    if windows.is_empty() {
                        continue;
                    }
                    if !shared.iter().any(|s| s.pid == pid) {
                        let (changed, put_back) = ax::share_web_tree(&app);
                        shared.push(Shared { pid, app: app.clone(), put_back });
                        if changed {
                            // The app builds its page's elements now: read it next time.
                            waiting = true;
                            continue;
                        }
                    }
                    for (window, call, title) in windows {
                        let tree = reader.tree(&window);
                        let found = scan::look(&tree);
                        let (meeting, named) = scan::window_title(call, &title);
                        // A Teams window counts only once it shows a call.
                        if call == Call::Meet || !found.is_empty() {
                            tracker.saw_window(call, meeting, named);
                        }
                        hot.extend(found.hot.iter().filter_map(|i| reader.elements.get(*i).cloned()));
                        look.add(found);
                        if reader.stopped {
                            break;
                        }
                    }
                }
                if waiting {
                    last_full = None;
                }
            } else {
                let mut reader = ax::Reader::new(Duration::from_millis(600), 1_500);
                for element in &hot {
                    let tree = reader.tree(element);
                    // A tile that has gone means the layout changed: look again.
                    if tree.role.is_empty() {
                        last_full = None;
                    }
                    look.add(scan::look(&tree));
                }
            }
            tracker.add(at_ms, &look);
            pause(tick + EVERY, stop);
        }

        for s in &shared {
            ax::put_back(&s.app, &s.put_back);
        }
        tracker.finish()
    }
}
