//! The loop that reads the call window once a second during a recording.

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
