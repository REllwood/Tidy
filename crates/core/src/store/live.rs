//! A meeting shared live with an assistant. When the person turns this on,
//! the app writes the meeting being recorded and its transcript here as it
//! goes, and the MCP server reads them for Claude or Codex. Assistants can
//! post notes back, which the app shows in its live panel. Everything here
//! is removed when the recording stops.

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use crate::db::{get_setting, now_ms, set_setting};
use crate::error::{AppError, AppResult};

const ENABLED: &str = "live_feed.enabled";

/// The kinds of note an assistant can post, as the live panel groups them.
pub const VIEWS: &[&str] = &["summary", "questions", "decisions", "open", "note"];
/// A posted note may be this long, and a meeting may hold this many.
const NOTE_MAX_CHARS: usize = 2_000;
const NOTES_MAX: i64 = 200;
const AUTHOR_MAX_CHARS: usize = 40;

/// Whether the person shares meetings live with assistants.
pub fn enabled(c: &Connection) -> AppResult<bool> {
    Ok(get_setting(c, ENABLED)?.as_deref() == Some("true"))
}

/// Turn sharing on or off. Turning it off also removes a meeting being
/// shared now.
pub fn set_enabled(c: &Connection, on: bool) -> AppResult<()> {
    set_setting(c, ENABLED, if on { "true" } else { "false" })?;
    if !on {
        end(c)?;
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LiveMeeting {
    /// When recording began, in ms since 1970. Line times count from here.
    pub started_at: i64,
    pub title: Option<String>,
}

/// A recording has begun: anything left from an earlier one goes.
pub fn begin(c: &Connection, started_at: i64, title: Option<&str>) -> AppResult<()> {
    end(c)?;
    c.execute("INSERT INTO live_meeting(id, started_at, title) VALUES (1, ?1, ?2)", params![started_at, title])?;
    Ok(())
}

pub fn set_title(c: &Connection, title: Option<&str>) -> AppResult<()> {
    c.execute("UPDATE live_meeting SET title = ?1 WHERE id = 1", [title])?;
    Ok(())
}

/// The recording stopped, or sharing was turned off: remove all of it.
pub fn end(c: &Connection) -> AppResult<()> {
    c.execute_batch("DELETE FROM live_line; DELETE FROM live_note; DELETE FROM live_meeting;")?;
    Ok(())
}

pub fn meeting(c: &Connection) -> AppResult<Option<LiveMeeting>> {
    Ok(c
        .query_row("SELECT started_at, title FROM live_meeting WHERE id = 1", [], |r| {
            Ok(LiveMeeting { started_at: r.get(0)?, title: r.get(1)? })
        })
        .optional()?)
}

/// One line of the shared transcript.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Line {
    /// "mic" for the person recording, "system" for the call.
    pub source: String,
    /// From the start of the recording.
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
    /// Who said a call line, when the call window showed them.
    pub speaker: Option<String>,
    /// Where this line comes in the order lines were written. Read "since"
    /// the last one seen to get only what is new or newly named.
    pub seq: i64,
}

impl Line {
    /// "You" for the person recording, the name shown in the call, or
    /// "Others".
    pub fn who(&self) -> &str {
        match (self.source.as_str(), &self.speaker) {
            ("mic", _) => "You",
            (_, Some(name)) => name,
            _ => "Others",
        }
    }
}

/// Write a line, or write it again with its speaker's name. Nothing is
/// written when no meeting is being shared. Returns the line's `seq`.
pub fn put_line(
    c: &Connection,
    source: &str,
    start_ms: i64,
    end_ms: i64,
    text: &str,
    speaker: Option<&str>,
) -> AppResult<Option<i64>> {
    if meeting(c)?.is_none() {
        return Ok(None);
    }
    let seq: i64 = c.query_row("SELECT COALESCE(MAX(seq), 0) + 1 FROM live_line", [], |r| r.get(0))?;
    c.execute(
        "INSERT INTO live_line(source, start_ms, end_ms, text, speaker, seq) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(source, start_ms) DO UPDATE SET end_ms = ?3, text = ?4, speaker = ?5, seq = ?6",
        params![source, start_ms, end_ms, text, speaker, seq],
    )?;
    Ok(Some(seq))
}

/// Lines written after `after`, oldest first, at most `limit`.
pub fn lines_since(c: &Connection, after: i64, limit: usize) -> AppResult<Vec<Line>> {
    let mut statement = c.prepare(
        "SELECT source, start_ms, end_ms, text, speaker, seq FROM live_line WHERE seq > ?1 ORDER BY seq LIMIT ?2",
    )?;
    let lines = statement
        .query_map(params![after, limit as i64], |r| {
            Ok(Line {
                source: r.get(0)?,
                start_ms: r.get(1)?,
                end_ms: r.get(2)?,
                text: r.get(3)?,
                speaker: r.get(4)?,
                seq: r.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(lines)
}

/// Lines as text for an assistant: "[mm:ss] Who: words", one a line.
pub fn as_text(lines: &[Line]) -> String {
    lines
        .iter()
        .map(|l| {
            let s = l.start_ms.max(0) / 1000;
            format!("[{:02}:{:02}] {}: {}", s / 60, s % 60, l.who(), l.text)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// A note an assistant posted to the live panel.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Note {
    pub id: i64,
    /// Who wrote it, such as "Claude" or "Codex".
    pub author: String,
    /// One of `VIEWS`.
    pub view: String,
    pub text: String,
    pub created_at: i64,
}

/// Post a note to the live panel of the meeting being shared.
pub fn add_note(c: &Connection, author: &str, view: &str, text: &str) -> AppResult<Note> {
    if meeting(c)?.is_none() {
        return Err(AppError::Invalid("No meeting is being shared right now.".into()));
    }
    if !VIEWS.contains(&view) {
        return Err(AppError::Invalid(format!("The view must be one of: {}.", VIEWS.join(", "))));
    }
    let text = text.trim();
    if text.is_empty() || text.chars().count() > NOTE_MAX_CHARS {
        return Err(AppError::Invalid(format!("A note needs between 1 and {NOTE_MAX_CHARS} characters.")));
    }
    let author: String = author.trim().chars().take(AUTHOR_MAX_CHARS).collect();
    let author = if author.is_empty() { "Assistant".to_string() } else { author };
    let count: i64 = c.query_row("SELECT count(*) FROM live_note", [], |r| r.get(0))?;
    if count >= NOTES_MAX {
        return Err(AppError::Invalid("This meeting already has as many notes as it can hold.".into()));
    }
    let created_at = now_ms();
    c.execute(
        "INSERT INTO live_note(author, view, text, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![author, view, text, created_at],
    )?;
    Ok(Note { id: c.last_insert_rowid(), author, view: view.into(), text: text.into(), created_at })
}

/// Notes posted after the note with id `after`, oldest first.
pub fn notes_since(c: &Connection, after: i64) -> AppResult<Vec<Note>> {
    let mut statement =
        c.prepare("SELECT id, author, view, text, created_at FROM live_note WHERE id > ?1 ORDER BY id")?;
    let notes = statement
        .query_map([after], |r| {
            Ok(Note { id: r.get(0)?, author: r.get(1)?, view: r.get(2)?, text: r.get(3)?, created_at: r.get(4)? })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(notes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn() -> Connection {
        let mut c = Connection::open_in_memory().unwrap();
        crate::db::migrations::migrations().to_latest(&mut c).unwrap();
        c
    }

    #[test]
    fn nothing_is_shared_until_a_meeting_begins_and_all_of_it_goes_when_it_ends() {
        let c = conn();
        assert!(!enabled(&c).unwrap());
        assert_eq!(put_line(&c, "mic", 0, 1_000, "Morning.", None).unwrap(), None);
        assert!(add_note(&c, "Claude", "summary", "Nothing yet.").is_err());

        begin(&c, 1_700_000_000_000, Some("Acme weekly")).unwrap();
        assert_eq!(meeting(&c).unwrap().unwrap().title.as_deref(), Some("Acme weekly"));
        put_line(&c, "mic", 0, 1_000, "Morning.", None).unwrap();
        add_note(&c, "Claude", "summary", "Greetings so far.").unwrap();

        end(&c).unwrap();
        assert_eq!(meeting(&c).unwrap(), None);
        assert!(lines_since(&c, 0, 100).unwrap().is_empty());
        assert!(notes_since(&c, 0).unwrap().is_empty());
    }

    #[test]
    fn a_line_named_later_comes_again_when_read_since_the_last_one_seen() {
        let c = conn();
        begin(&c, 0, None).unwrap();
        put_line(&c, "system", 4_000, 7_000, "The build is ready.", None).unwrap();
        put_line(&c, "mic", 8_000, 9_000, "Great, thanks.", None).unwrap();
        let first = lines_since(&c, 0, 100).unwrap();
        assert_eq!(first.len(), 2);
        let seen = first.last().unwrap().seq;
        assert_eq!(as_text(&first), "[00:04] Others: The build is ready.\n[00:08] You: Great, thanks.");

        // The call window catches up and names the first line.
        put_line(&c, "system", 4_000, 7_000, "The build is ready.", Some("Priya Shah")).unwrap();
        let again = lines_since(&c, seen, 100).unwrap();
        assert_eq!(again.len(), 1);
        assert_eq!(as_text(&again), "[00:04] Priya Shah: The build is ready.");
        assert!(lines_since(&c, again[0].seq, 100).unwrap().is_empty());
        assert_eq!(lines_since(&c, 0, 1).unwrap().len(), 1, "the limit holds");
    }

    #[test]
    fn notes_are_checked_and_read_in_order() {
        let c = conn();
        begin(&c, 0, None).unwrap();
        assert!(add_note(&c, "Claude", "gossip", "Hi").is_err(), "only the known views");
        assert!(add_note(&c, "Claude", "summary", "   ").is_err());
        assert!(add_note(&c, "Claude", "summary", &"a".repeat(NOTE_MAX_CHARS + 1)).is_err());
        let one = add_note(&c, "Claude", "questions", "Who owns the pilot?").unwrap();
        let two = add_note(&c, "", "note", "Watch the budget.").unwrap();
        assert_eq!(two.author, "Assistant");
        assert_eq!(notes_since(&c, 0).unwrap(), vec![one.clone(), two.clone()]);
        assert_eq!(notes_since(&c, one.id).unwrap(), vec![two]);
    }

    #[test]
    fn turning_sharing_off_removes_the_meeting_being_shared() {
        let c = conn();
        set_enabled(&c, true).unwrap();
        assert!(enabled(&c).unwrap());
        begin(&c, 0, None).unwrap();
        put_line(&c, "mic", 0, 1_000, "Morning.", None).unwrap();
        set_enabled(&c, false).unwrap();
        assert!(!enabled(&c).unwrap());
        assert_eq!(meeting(&c).unwrap(), None);
        assert!(lines_since(&c, 0, 10).unwrap().is_empty());
    }
}
