//! Checks against real call windows on this Mac. They need a process the
//! Accessibility permission covers and a call open, so they only run when
//! asked for, one at a time:
//!
//! - `list_windows`: the browsers and Teams windows Tidy can see.
//! - `dump_call_windows`: each call window as Tidy reads it, saved with its
//!   words to TIDY_LIVE_OUT, which never leaves this Mac.
//! - `watch_a_call`: the whole watch for TIDY_LIVE_SECONDS, and its report to
//!   TIDY_LIVE_REPORT when no one is found speaking.
//!
//! cargo test --lib call_window::live::watch_a_call -- --ignored --nocapture

use std::fmt::Write as _;
use std::time::{Duration, Instant};

use super::ax::{self, Element};
use super::scan::{self, Node};

fn line(node: &Node, depth: usize, out: &mut String) {
    let cut = |s: &str| -> String {
        let s = s.replace('\n', " ");
        if s.chars().count() > 140 {
            format!("{}…", s.chars().take(140).collect::<String>())
        } else {
            s
        }
    };
    let _ = write!(out, "{:indent$}{}", "", node.role, indent = depth * 2);
    if !node.subrole.is_empty() {
        let _ = write!(out, "/{}", node.subrole);
    }
    if !node.title.is_empty() {
        let _ = write!(out, " title=\"{}\"", cut(&node.title));
    }
    if !node.desc.is_empty() {
        let _ = write!(out, " desc=\"{}\"", cut(&node.desc));
    }
    if !node.value.is_empty() {
        let _ = write!(out, " value=\"{}\"", cut(&node.value));
    }
    if !node.classes.is_empty() {
        let _ = write!(out, " .{}", cut(&node.classes).replace(' ', "."));
    }
    out.push('\n');
    for child in &node.children {
        line(child, depth + 1, out);
    }
}

fn count(node: &Node) -> (usize, usize) {
    let mut nodes = 1;
    let mut depth = 0;
    for child in &node.children {
        let (n, d) = count(child);
        nodes += n;
        depth = depth.max(d + 1);
    }
    (nodes, depth)
}

/// Every call window the watch would read, dumped as Tidy reads it, with
/// what the scan finds in it.
#[test]
#[ignore]
fn dump_call_windows() {
    assert!(ax::trusted(), "this process needs the Accessibility permission");
    let out_dir = std::env::var("TIDY_LIVE_OUT").expect("TIDY_LIVE_OUT");
    let only: Option<i32> = std::env::var("TIDY_LIVE_PID").ok().and_then(|p| p.parse().ok());
    for (pid, bundle) in ax::running_apps() {
        if only.is_some_and(|p| p != pid) {
            continue;
        }
        let Some(app) = Element::application(pid) else { continue };
        let windows = super::watch::call_windows_for_test(&app, &bundle);
        if windows.is_empty() {
            continue;
        }
        println!("{bundle} ({pid}): {} call window(s)", windows.len());
        if !windows.iter().all(|(w, _, _)| scan::shows_page(&ax::Reader::new(Duration::from_secs(2), 6_000).tree(w))) {
            ax::wake_web_tree(&app);
            std::thread::sleep(Duration::from_secs(3));
        }
        for (n, (window, call, title)) in windows.into_iter().enumerate() {
            for (label, budget, max) in [("as-watch", 1_500, 6_000), ("full", 20_000, 60_000)] {
                let started = Instant::now();
                let mut reader = ax::Reader::new(Duration::from_millis(budget), max);
                let tree = reader.tree(&window);
                let took = started.elapsed();
                let (nodes, depth) = count(&tree);
                let found = scan::look(call, !bundle.starts_with("com.microsoft.teams"), &tree);
                println!(
                    "  [{label}] {call:?} \"{title}\": page shown {}, {nodes} elements, {depth} deep, {} ms, stopped {}",
                    scan::shows_page(&tree),
                    took.as_millis(),
                    reader.stopped
                );
                println!(
                    "    people {:?}, me {:?}, speaking {:?}, captions {}, tiles to re-read {}",
                    found.people,
                    found.me,
                    found.speaking,
                    found.captions.len(),
                    found.hot.len()
                );
                let mut text = String::new();
                line(&tree, 0, &mut text);
                let path = format!("{out_dir}/{pid}-{n}-{label}.txt");
                std::fs::write(&path, text).expect("write the dump");
                println!("    saved {path}");
            }
        }
    }
}

/// The whole watch, as a recording runs it, for TIDY_LIVE_SECONDS.
#[test]
#[ignore]
fn watch_a_call() {
    assert!(ax::trusted(), "this process needs the Accessibility permission");
    let seconds: u64 = std::env::var("TIDY_LIVE_SECONDS").ok().and_then(|s| s.parse().ok()).unwrap_or(30);
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = stop.clone();
    let began = Instant::now();
    let report = std::env::var("TIDY_LIVE_REPORT").ok().map(std::path::PathBuf::from);
    let handle = std::thread::spawn(move || super::watch::run(began, &flag, report.as_deref()));
    std::thread::sleep(Duration::from_secs(seconds));
    stop.store(true, std::sync::atomic::Ordering::SeqCst);
    let record = handle.join().expect("the watch ends");
    println!("app {:?}, title {:?}", record.app, record.title);
    println!("people {:?}", record.people);
    for s in &record.speaking {
        println!("  {:>6} – {:>6} ms  {}", s.from_ms, s.to_ms, s.name);
    }
}

/// Every running app Tidy would consider, with its windows' titles.
#[test]
#[ignore]
fn list_windows() {
    println!("trusted {}", ax::trusted());
    for (pid, bundle) in ax::running_apps() {
        let lower = bundle.to_lowercase();
        if !["chrome", "teams", "brave", "edge", "safari", "arc"].iter().any(|k| lower.contains(k)) {
            continue;
        }
        let Some(app) = Element::application(pid) else { continue };
        let windows = app.windows();
        println!("{bundle} ({pid}): {} window(s)", windows.len());
        for (_, title) in windows {
            println!("  {title:?} -> {:?}", scan::browser_call(&title));
        }
    }
}
