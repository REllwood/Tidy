//! Sharing the meeting being recorded with Claude or Codex. It is off until
//! the person turns it on, because the meeting then leaves this Mac: the
//! MCP server hands the live transcript to whichever assistant is connected,
//! and live notes can be written by Claude or Codex instead of the local
//! model. While it is on, the transcript is kept in the database as it
//! grows, and all of it is removed when the recording stops.

use std::sync::mpsc::Sender;

use serde::Serialize;
use tauri::{AppHandle, Manager};
use tidy_core::store::live;

use crate::audio::live::LiveLine;
use crate::db::Db;
use crate::error::AppResult;
use crate::workspace::run_db;

fn with_db<T>(app: &AppHandle, f: impl FnOnce(&rusqlite::Connection) -> AppResult<T>) -> Option<T> {
    let db = app.state::<Db>();
    let conn = db.lock().ok()?;
    match f(&conn) {
        Ok(value) => Some(value),
        Err(e) => {
            log::warn!("live sharing: {e}");
            None
        }
    }
}

pub fn enabled(app: &AppHandle) -> bool {
    with_db(app, live::enabled).unwrap_or(false)
}

/// A recording has started: share it, with any lines already said, when
/// the person turned sharing on.
pub fn begin(app: &AppHandle, started_at: i64, title: Option<&str>, lines: &[LiveLine]) {
    with_db(app, |c| {
        if !live::enabled(c)? {
            return Ok(());
        }
        live::begin(c, started_at, title)?;
        for l in lines {
            live::put_line(c, l.source, l.start_ms, l.end_ms, &l.text, l.speaker.as_deref())?;
        }
        Ok(())
    });
}

/// The recording has stopped, or Tidy has just opened: nothing stays shared.
pub fn end(app: &AppHandle) {
    with_db(app, live::end);
}

/// Where the live transcript sends each new or newly named line. Lines are
/// written on their own thread, so a busy database never holds up the
/// transcript. Nothing is written unless a meeting is being shared.
pub fn writer(app: AppHandle) -> Sender<LiveLine> {
    let (tx, rx) = std::sync::mpsc::channel::<LiveLine>();
    std::thread::Builder::new()
        .name("live-share".into())
        .spawn(move || {
            for line in rx {
                with_db(&app, |c| {
                    live::put_line(c, line.source, line.start_ms, line.end_ms, &line.text, line.speaker.as_deref())
                });
            }
        })
        .ok();
    tx
}

#[derive(Debug, Serialize)]
pub struct LiveFeedStatus {
    /// The person shares meetings live with assistants.
    pub enabled: bool,
    /// The Claude Code and Codex command-line tools Tidy found, which can
    /// write live notes.
    pub claude: bool,
    pub codex: bool,
}

#[tauri::command]
pub async fn live_feed_status(app: AppHandle) -> AppResult<LiveFeedStatus> {
    let enabled = run_db(&app, live::enabled).await?;
    let (claude, codex) = tauri::async_runtime::spawn_blocking(|| {
        (
            crate::assistant_cli::find(crate::assistant_cli::Assistant::Claude).is_some(),
            crate::assistant_cli::find(crate::assistant_cli::Assistant::Codex).is_some(),
        )
    })
    .await
    .unwrap_or((false, false));
    Ok(LiveFeedStatus { enabled, claude, codex })
}

/// Turn sharing on. A recording already running is shared from now, with
/// what has been said so far.
#[tauri::command]
pub async fn live_feed_enable(app: AppHandle) -> AppResult<LiveFeedStatus> {
    run_db(&app, |c| live::set_enabled(c, true)).await?;
    let running = {
        let state = app.state::<crate::commands::recording::RecorderState>();
        let guard = state.lock();
        guard.as_ref().map(|s| (s.started_at(), s.event.as_ref().map(|e| e.title.clone()), s.live_lines()))
    };
    if let Some((started_at, title, lines)) = running {
        let handle = app.clone();
        tauri::async_runtime::spawn_blocking(move || begin(&handle, started_at, title.as_deref(), &lines))
            .await
            .map_err(|e| crate::error::AppError::Other(e.to_string()))?;
    }
    live_feed_status(app).await
}

/// Turn sharing off. A meeting being shared now stops being shared, and
/// what was shared of it is removed.
#[tauri::command]
pub async fn live_feed_disable(app: AppHandle) -> AppResult<()> {
    run_db(&app, |c| live::set_enabled(c, false)).await
}

/// Notes Claude or Codex posted to the live panel after the note `after`.
#[tauri::command]
pub async fn live_feed_notes(app: AppHandle, after: Option<i64>) -> AppResult<Vec<live::Note>> {
    run_db(&app, move |c| live::notes_since(c, after.unwrap_or(0))).await
}
