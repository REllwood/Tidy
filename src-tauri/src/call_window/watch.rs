//! The loop that reads the call window once a second during a recording.

use std::path::Path;
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
/// After asking an app to share its page, how long before reading it.
const WAKE_SETTLE: Duration = Duration::from_millis(2_500);
/// How often to ask again while an app keeps its page hidden.
const WAKE_EVERY: Duration = Duration::from_secs(10);
/// Each window's read: how long it may take and how many elements it visits.
const WINDOW_BUDGET: Duration = Duration::from_millis(1_500);
const WINDOW_ELEMENTS: usize = 6_000;
/// At most this many windows of one app are read each time.
const MAX_WINDOWS: usize = 4;

/// The Teams app, new or classic, by its bundle identifier.
fn is_teams(bundle: &str) -> bool {
    bundle == "com.microsoft.teams2" || bundle == "com.microsoft.teams"
}

/// Browsers a Meet or Teams call may run in, by bundle identifier, with
/// their test channels. Installed web apps open their windows in the
/// browser itself, so they are covered too.
fn is_browser(bundle: &str) -> bool {
    const BROWSERS: &[&str] = &[
        "com.google.Chrome", "org.chromium.Chromium", "com.microsoft.edgemac", "com.brave.Browser",
        "company.thebrowser.Browser", "company.thebrowser.dia", "com.vivaldi.Vivaldi", "com.operasoftware.Opera",
        "com.apple.Safari", "com.apple.SafariTechnologyPreview", "org.mozilla.firefox",
        "org.mozilla.firefoxdeveloperedition", "org.mozilla.nightly",
    ];
    const CHANNELS: &[&str] = &["", ".beta", ".dev", ".canary", ".Beta", ".Dev", ".Canary", ".nightly", ".Nightly"];
    BROWSERS.iter().any(|b| CHANNELS.iter().any(|c| bundle.strip_prefix(b) == Some(c)))
}

/// Windows that may hold a call: any Teams window, and browser windows
/// whose title is a Meet or Teams page.
fn call_windows(app: &Element, bundle: &str) -> Vec<(Element, Call, String)> {
    app.windows()
        .into_iter()
        .filter_map(|(window, title)| {
            let call = if is_teams(bundle) { Some(Call::Teams) } else { scan::browser_call(&title) };
            call.map(|c| (window, c, title))
        })
        .collect()
}

#[cfg(test)]
pub(super) fn call_windows_for_test(app: &Element, bundle: &str) -> Vec<(Element, Call, String)> {
    if is_teams(bundle) || is_browser(bundle) {
        call_windows(app, bundle)
    } else {
        Vec::new()
    }
}

/// An app with a call window, and how Tidy found it.
struct Watched {
    pid: i32,
    app: Element,
    browser: bool,
    switches: ax::Switches,
    woken: Option<Instant>,
    /// Call windows seen so far. A call that goes full screen or moves to
    /// another Space leaves the app's window list but can still be read.
    windows: Vec<Element>,
}

impl Watched {
    /// The call windows to read now: those the app lists, then earlier ones
    /// that still show a call. Teams meetings come before Teams' own
    /// sections, such as Chat.
    fn windows_now(&mut self, listed: Vec<(Element, Call, String)>, teams: bool) -> Vec<(Element, Call, String)> {
        let mut windows = listed;
        for known in &self.windows {
            if windows.iter().any(|(w, _, _)| w == known) {
                continue;
            }
            let title = known.title();
            let call = if teams { Some(Call::Teams) } else { scan::browser_call(&title) };
            if let Some(call) = call.filter(|_| !title.is_empty()) {
                windows.push((known.clone(), call, title));
            }
        }
        if teams {
            windows.sort_by_key(|(_, _, title)| scan::is_teams_section(title));
        }
        windows.truncate(MAX_WINDOWS);
        for (window, _, _) in &windows {
            if !self.windows.contains(window) {
                self.windows.push(window.clone());
            }
        }
        windows
    }
}

fn pause(until: Instant, stop: &AtomicBool) {
    while Instant::now() < until && !stop.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// What the watch did, for a report when a recording finds no one speaking.
#[derive(Default)]
struct Report {
    apps: Vec<String>,
    full_reads: usize,
    hidden_reads: usize,
    wakes: usize,
    people: usize,
    captions: usize,
    tiles: usize,
    /// The latest read of each call window.
    windows: Vec<(Element, String, scan::Node)>,
}

impl Report {
    fn read(&mut self, window: &Element, bundle: &str, call: Call, title: &str, tree: &scan::Node, found: Option<&Look>) {
        self.full_reads += 1;
        if let Some(found) = found {
            self.people = self.people.max(found.people.len());
            self.captions = self.captions.max(found.captions.len());
            self.tiles = self.tiles.max(found.hot.len());
        } else {
            self.hidden_reads += 1;
        }
        let header = format!("{bundle}, read as {}, title \"{}\", page shown: {}", call.app(), scan::shape(title), found.is_some());
        if let Some(entry) = self.windows.iter_mut().find(|(w, _, _)| w == window) {
            entry.1 = header;
            entry.2 = tree.clone();
        } else if self.windows.len() < 6 {
            self.windows.push((window.clone(), header, tree.clone()));
        }
    }

    /// Written only on this Mac, over the last one, and only when the
    /// recording found no one speaking. Every word in it is replaced.
    fn write(&self, path: &Path, record: &CallRecord, seconds: u64) {
        let mut text = format!(
            "Tidy call window report\n\nRecording length: {seconds} seconds\nApps with a call window: {}\nWindow reads: {}, of which the page was hidden: {}\nTimes an app was asked to share its page: {}\nMost seen at once: {} people, {} caption lines, {} tiles\nKept: {} people, {} stretches of speaking\n",
            if self.apps.is_empty() { "none".to_string() } else { self.apps.join(", ") },
            self.full_reads,
            self.hidden_reads,
            self.wakes,
            self.people,
            self.captions,
            self.tiles,
            record.people.len(),
            record.speaking.len(),
        );
        for (_, header, tree) in &self.windows {
            text.push_str(&format!("\n--- {header}\n{}\n", scan::outline(tree, 4_000)));
        }
        if let Err(e) = std::fs::write(path, text) {
            log::warn!("call window report not written: {e}");
        }
    }
}

pub fn run(began: Instant, stop: &AtomicBool, report_to: Option<&Path>) -> CallRecord {
    let mut report = Report::default();
    let mut tracker = Tracker::default();
    let mut hot: Vec<(Element, Call)> = Vec::new();
    let mut next_full = Instant::now();
    let mut watched: Vec<Watched> = Vec::new();

    while !stop.load(Ordering::SeqCst) {
        let tick = Instant::now();
        let at_ms = began.elapsed().as_millis() as i64;
        let mut look = Look::default();

        if tick >= next_full {
            hot.clear();
            let mut woke = false;
            for (pid, bundle) in ax::running_apps() {
                let teams = is_teams(&bundle);
                if !teams && !is_browser(&bundle) {
                    continue;
                }
                let Some(app) = Element::application(pid) else { continue };
                let listed = call_windows(&app, &bundle);
                let at = match watched.iter().position(|w| w.pid == pid) {
                    Some(at) => at,
                    None if listed.is_empty() => continue,
                    None => {
                        report.apps.push(bundle.clone());
                        let switches = ax::switches(&app);
                        watched.push(Watched { pid, app, browser: !teams, switches, woken: None, windows: Vec::new() });
                        watched.len() - 1
                    }
                };
                let entry = &mut watched[at];
                let mut hidden = false;
                for (window, call, title) in entry.windows_now(listed, teams) {
                    let mut reader = ax::Reader::new(WINDOW_BUDGET, WINDOW_ELEMENTS);
                    let tree = reader.tree(&window);
                    if tree.role.is_empty() {
                        // Closed.
                        entry.windows.retain(|w| w != &window);
                        continue;
                    }
                    if !scan::shows_page(&tree) {
                        report.read(&window, &bundle, call, &title, &tree, None);
                        hidden = true;
                        continue;
                    }
                    let found = scan::look(call, entry.browser, &tree);
                    report.read(&window, &bundle, call, &title, &tree, Some(&found));
                    let (meeting, named) = scan::window_title(call, &title);
                    // A Teams window counts only once it shows a call.
                    if call == Call::Meet || !found.is_empty() {
                        tracker.saw_window(call, meeting, named);
                    }
                    hot.extend(found.hot.iter().filter_map(|i| reader.elements.get(*i).cloned()).map(|e| (e, call)));
                    look.add(found);
                }
                if hidden && entry.woken.map_or(true, |at| at.elapsed() >= WAKE_EVERY) {
                    ax::wake_web_tree(&entry.app);
                    entry.woken = Some(Instant::now());
                    report.wakes += 1;
                    woke = true;
                }
            }
            next_full = Instant::now()
                + if woke {
                    WAKE_SETTLE
                } else if hot.is_empty() {
                    RETRY_EVERY
                } else {
                    FULL_EVERY
                };
        } else {
            for (element, call) in &hot {
                let mut reader = ax::Reader::new(Duration::from_millis(600), 1_500);
                let tree = reader.tree(element);
                // A tile that has gone means the layout changed: look again.
                if tree.role.is_empty() {
                    next_full = Instant::now();
                }
                look.add(scan::look_part(*call, &tree));
            }
        }
        tracker.add(at_ms, &look);
        pause(tick + EVERY, stop);
    }

    for w in &watched {
        ax::put_back(&w.app, &w.switches);
    }
    let record = tracker.finish();
    if let Some(path) = report_to.filter(|_| record.speaking.is_empty()) {
        report.write(path, &record, began.elapsed().as_secs());
    }
    record
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn call_apps_are_known_by_bundle_identifier() {
        assert!(is_teams("com.microsoft.teams2"));
        assert!(is_teams("com.microsoft.teams"));
        assert!(!is_teams("com.microsoft.teams2.helper"));
        for browser in ["com.google.Chrome", "com.google.Chrome.canary", "com.microsoft.edgemac.Beta", "com.apple.Safari", "company.thebrowser.Browser"] {
            assert!(is_browser(browser), "{browser}");
        }
        // Helpers and installed web apps' small launchers are not the browser.
        for other in ["com.google.Chrome.helper", "com.google.Chrome.app.kjgfgldnnfoeklkmfkjfagphfepbbdan", "com.apple.mail", ""] {
            assert!(!is_browser(other), "{other}");
        }
    }
}
