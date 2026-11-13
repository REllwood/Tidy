//! Durable meeting metadata and append-only transcript revisions.
use crate::{
    db::{new_id, now_ms},
    error::{AppError, AppResult},
    store::{documents, pages},
};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub struct SavedMeeting {
    pub retain_audio: bool,
    pub id: String,
    pub page_id: Option<String>,
    pub title: String,
    pub client: Option<String>,
    pub started_at: i64,
    pub duration_ms: i64,
    pub audio_path: Option<String>,
    pub model_used: Option<String>,
    pub transcript_state: String,
    pub transcript_error: Option<String>,
    /// Why Tidy stopped the recording itself, such as the call ending.
    pub stop_reason: Option<String>,
}
#[derive(Serialize)]
pub struct Version {
    pub id: String,
    pub created_at: i64,
    pub model: Option<String>,
    pub language: Option<String>,
    pub body_json: String,
    pub reason: String,
}

pub fn list(c: &Connection) -> AppResult<Vec<SavedMeeting>> {
    let mut q = c.prepare("SELECT m.id,m.page_id,coalesce(p.title,'Recovered recording'),
      (SELECT cp.title FROM link l JOIN page cp ON cp.id=l.target_page_id WHERE l.source_page_id=p.id AND l.kind='task_of' LIMIT 1),
      m.started_at,m.duration,m.audio_path,m.model_used,m.transcript_state,m.transcript_error,m.retain_audio,m.stop_reason
      FROM meeting m LEFT JOIN page p ON p.id=m.page_id ORDER BY m.started_at DESC")?;
    let rows = q.query_map([], |r| {
        Ok(SavedMeeting {
            id: r.get(0)?,
            page_id: r.get(1)?,
            title: r.get(2)?,
            client: r.get(3)?,
            started_at: r.get(4)?,
            duration_ms: r.get(5)?,
            audio_path: r.get(6)?,
            model_used: r.get(7)?,
            transcript_state: r.get(8)?,
            transcript_error: r.get(9)?,
            retain_audio: r.get(10)?,
            stop_reason: r.get(11)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// The recording is registered before the first Whisper pass, including when no model is installed.
pub fn register(
    c: &Connection,
    path: &str,
    duration: i64,
    started: i64,
    client: &str,
) -> AppResult<String> {
    register_with_retention(c, path, duration, started, client, true)
}

pub fn register_with_retention(
    c: &Connection,
    path: &str,
    duration: i64,
    started: i64,
    client: &str,
    retain: bool,
) -> AppResult<String> {
    if let Some(page) = c
        .query_row(
            "SELECT page_id FROM meeting WHERE audio_path=?1 AND page_id IS NOT NULL",
            [path],
            |r| r.get::<_, String>(0),
        )
        .optional()?
    {
        return Ok(page);
    }
    let tx = c.unchecked_transaction()?;
    let parent = if client.trim().is_empty() {
        None
    } else {
        let existing=tx.query_row("SELECT id FROM page WHERE trim(title)=?1 COLLATE NOCASE AND type='doc' AND deleted_at IS NULL ORDER BY created_at LIMIT 1",[client.trim()],|r|r.get::<_,String>(0)).optional()?;
        Some(match existing {
            Some(id) => id,
            None => pages::core::create(&tx, None, client.trim().into(), "doc".into())?.id,
        })
    };
    let page = pages::core::create(
        &tx,
        parent.clone(),
        format!("Meeting {started}"),
        "doc".into(),
    )?;
    documents::core::update(
        &tx,
        &page.id,
        r#"[{"type":"paragraph","content":"Transcription is pending."}]"#,
    )?;
    if let Some(parent) = parent {
        tx.execute("INSERT INTO link(id,source_page_id,target_page_id,kind,created_at) VALUES(?1,?2,?3,'task_of',?4)",params![new_id(),page.id,parent,started])?;
    }
    tx.execute("INSERT INTO meeting(id,page_id,started_at,duration,audio_path,transcript_state,retain_audio) VALUES(?1,?2,?3,?4,?5,'pending',?6)",params![new_id(),page.id,started,duration,path,retain])?;
    tx.commit()?;
    Ok(page.id)
}

pub fn versions(c: &Connection, page: &str) -> AppResult<Vec<Version>> {
    let mut q=c.prepare("SELECT v.id,v.created_at,v.model,v.language,v.body_json,v.reason FROM transcript_version v JOIN meeting m ON m.id=v.meeting_id WHERE m.page_id=?1 ORDER BY v.created_at DESC,v.rowid DESC")?;
    let rows = q.query_map([page], |r| {
        Ok(Version {
            id: r.get(0)?,
            created_at: r.get(1)?,
            model: r.get(2)?,
            language: r.get(3)?,
            body_json: r.get(4)?,
            reason: r.get(5)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub fn save_result(
    c: &Connection,
    page: &str,
    expected: &str,
    body: &str,
    model: &str,
    language: &str,
) -> AppResult<bool> {
    let _: serde_json::Value = serde_json::from_str(body)?;
    let tx = c.unchecked_transaction()?;
    let id: String = tx.query_row(
        "SELECT id FROM meeting WHERE page_id=?1 ORDER BY started_at DESC LIMIT 1",
        [page],
        |r| r.get(0),
    )?;
    let current = documents::core::get(&tx, page)?;
    for (text, reason) in [
        (current.as_str(), "Before re-transcription"),
        (body, "Transcribed from saved audio"),
    ] {
        tx.execute("INSERT INTO transcript_version(id,meeting_id,created_at,model,language,body_json,reason) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![new_id(),id,now_ms(),if reason=="Before re-transcription" {None}else{Some(model)},if reason=="Before re-transcription" {None}else{Some(language)},text,reason])?;
    }
    let applied = documents::core::update_if_unchanged(&tx, page, expected, body)?;
    tx.execute("UPDATE meeting SET transcript_state='saved',transcript_error=NULL,model_used=CASE WHEN ?2 THEN ?3 ELSE model_used END WHERE id=?1",params![id,applied,model])?;
    tx.commit()?;
    Ok(applied)
}

/// The calendar event a meeting was recorded for.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MeetingEvent {
    pub title: String,
    pub start_ms: i64,
    pub end_ms: i64,
    /// The other people invited, by name or email, without the person recording.
    #[serde(default)]
    pub attendees: Vec<String>,
    /// The same id for every meeting in a recurring series.
    #[serde(default)]
    pub series: Option<String>,
}

pub fn set_event(c: &Connection, page: &str, event: &MeetingEvent) -> AppResult<()> {
    c.execute(
        "UPDATE meeting SET event=?2 WHERE page_id=?1",
        params![page, serde_json::to_string(event)?],
    )?;
    Ok(())
}

pub fn event(c: &Connection, page: &str) -> AppResult<Option<MeetingEvent>> {
    let raw: Option<String> = c
        .query_row(
            "SELECT event FROM meeting WHERE page_id=?1 ORDER BY started_at DESC LIMIT 1",
            [page],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    Ok(raw.and_then(|r| serde_json::from_str(&r).ok()))
}

/// Someone the call window showed speaking, from one moment to another, in
/// ms from the start of the recording.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Spoken {
    pub from_ms: i64,
    pub to_ms: i64,
    pub name: String,
}

/// What the Teams or Google Meet window showed while a meeting was recorded.
/// Only names and times are kept, never caption text.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CallRecord {
    /// The app the call ran in, such as "Teams" or "Google Meet".
    pub app: String,
    /// The meeting's title, when the window showed one.
    #[serde(default)]
    pub title: Option<String>,
    /// Everyone else seen in the call, without the person recording.
    #[serde(default)]
    pub people: Vec<String>,
    /// Who was shown speaking when, one person at a time.
    #[serde(default)]
    pub speaking: Vec<Spoken>,
}

impl CallRecord {
    pub fn is_empty(&self) -> bool {
        self.title.is_none() && self.people.is_empty() && self.speaking.is_empty()
    }
}

pub fn set_call(c: &Connection, page: &str, call: &CallRecord) -> AppResult<()> {
    c.execute(
        "UPDATE meeting SET call=?2 WHERE page_id=?1",
        params![page, serde_json::to_string(call)?],
    )?;
    Ok(())
}

pub fn call(c: &Connection, page: &str) -> AppResult<Option<CallRecord>> {
    let raw: Option<String> = c
        .query_row(
            "SELECT call FROM meeting WHERE page_id=?1 ORDER BY started_at DESC LIMIT 1",
            [page],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    Ok(raw.and_then(|r| serde_json::from_str(&r).ok()))
}

/// The people expected in a meeting: those invited to its calendar event,
/// by name, then anyone else the call window showed, without repeats.
pub fn people(c: &Connection, page: &str) -> AppResult<Vec<String>> {
    let mut names: Vec<String> = Vec::new();
    let invited = event(c, page)?.map(|e| e.attendees).unwrap_or_default();
    let seen = call(c, page)?.map(|r| r.people).unwrap_or_default();
    for name in invited.into_iter().chain(seen) {
        // Email addresses are not names anyone is called by.
        if !name.contains('@') && !names.iter().any(|n| n.eq_ignore_ascii_case(&name)) {
            names.push(name);
        }
    }
    Ok(names)
}

/// How many voices could be on the other side of the call: everyone invited
/// or everyone seen in the call, whichever is more, and one spare. Zero when
/// neither is known.
pub fn voices_at_most(c: &Connection, page: &str) -> AppResult<i32> {
    let invited = event(c, page)?.map_or(0, |e| e.attendees.len());
    let seen = call(c, page)?.map_or(0, |r| r.people.len());
    let most = invited.max(seen);
    Ok(if most == 0 { 0 } else { most as i32 + 1 })
}

/// Record how a recording ended and the moments marked during it.
pub fn set_stop_details(
    c: &Connection,
    page: &str,
    reason: Option<&str>,
    marks: &[i64],
) -> AppResult<()> {
    let reason = reason.map(str::trim).filter(|r| !r.is_empty());
    let marks = (!marks.is_empty()).then(|| serde_json::to_string(marks)).transpose()?;
    c.execute(
        "UPDATE meeting SET stop_reason=?2, marks=?3 WHERE page_id=?1",
        params![page, reason, marks],
    )?;
    Ok(())
}

/// The moments marked while a meeting was recorded, in ms from its start.
pub fn marks(c: &Connection, page: &str) -> AppResult<Vec<i64>> {
    let raw: Option<String> = c
        .query_row(
            "SELECT marks FROM meeting WHERE page_id=?1 ORDER BY started_at DESC LIMIT 1",
            [page],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    Ok(raw
        .and_then(|r| serde_json::from_str::<Vec<i64>>(&r).ok())
        .unwrap_or_default())
}

/// The setting holding the person's own list of names and terms for Whisper.
pub const VOCABULARY: &str = "transcription.vocabulary";

/// Split a typed list of terms on commas, semicolons and new lines.
pub fn split_terms(text: &str) -> Vec<String> {
    text.split([',', ';', '\n'])
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect()
}

/// Names and terms Whisper should expect in this meeting: the client it is
/// filed under, the people invited to its calendar event or seen in the
/// call, then the person's own list from Settings.
pub fn vocabulary(c: &Connection, page: &str) -> AppResult<Vec<String>> {
    let mut terms = Vec::new();
    let client = c
        .query_row(
            "SELECT p.title FROM page m JOIN page p ON p.id = m.parent_id WHERE m.id = ?1 AND p.deleted_at IS NULL",
            [page],
            |r| r.get::<_, String>(0),
        )
        .optional()?;
    terms.extend(client.filter(|t| !t.trim().is_empty()));
    terms.extend(people(c, page)?);
    terms.extend(split_terms(&crate::db::get_setting(c, VOCABULARY)?.unwrap_or_default()));
    Ok(terms)
}

pub fn source(c: &Connection, page: &str) -> AppResult<String> {
    c.query_row(
        "SELECT audio_path FROM meeting WHERE page_id=?1 ORDER BY started_at DESC LIMIT 1",
        [page],
        |r| r.get::<_, Option<String>>(0),
    )
    .optional()?
    .flatten()
    .ok_or_else(|| {
        AppError::Invalid(
            "No saved audio is linked to this meeting. The saved text is still available.".into(),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    #[test]
    fn retry_preserves_original_and_respects_edits() {
        let db = Db::open_in_memory().unwrap();
        let c = db.conn.lock().unwrap();
        let page = register(&c, "/recordings/test.wav", 1000, 42, "Acme").unwrap();
        assert_eq!(
            register(&c, "/recordings/test.wav", 1000, 42, "Acme").unwrap(),
            page
        );
        let original = r#"[{"type":"paragraph","content":"Original transcript"}]"#;
        documents::core::update(&c, &page, original).unwrap();
        let revised = r#"[{"type":"paragraph","content":"Better transcript"}]"#;
        assert!(save_result(&c, &page, original, revised, "small", "en").unwrap());
        assert!(versions(&c, &page)
            .unwrap()
            .iter()
            .any(|v| v.body_json == original));
        documents::core::update(&c, &page, original).unwrap();
        assert!(!save_result(&c, &page, revised, "[]", "medium", "en").unwrap());
        assert_eq!(documents::core::get(&c, &page).unwrap(), original);
        assert_eq!(versions(&c, &page).unwrap().len(), 4);
    }
    #[test]
    fn how_a_recording_stopped_and_its_marks_are_kept() {
        let db = Db::open_in_memory().unwrap();
        let c = db.conn.lock().unwrap();
        let page = register(&c, "/recordings/a.wav", 1000, 42, "").unwrap();
        assert!(marks(&c, &page).unwrap().is_empty());
        set_stop_details(&c, &page, Some("Stopped automatically when Zoom ended the call"), &[61_000, 125_500]).unwrap();
        assert_eq!(marks(&c, &page).unwrap(), vec![61_000, 125_500]);
        let saved = list(&c).unwrap();
        assert_eq!(saved[0].stop_reason.as_deref(), Some("Stopped automatically when Zoom ended the call"));
        set_stop_details(&c, &page, Some("  "), &[]).unwrap();
        assert!(list(&c).unwrap()[0].stop_reason.is_none());
        assert!(marks(&c, &page).unwrap().is_empty());
    }
    #[test]
    fn a_meetings_calendar_event_is_kept_and_its_people_are_heard() {
        let db = Db::open_in_memory().unwrap();
        let c = db.conn.lock().unwrap();
        let page = register(&c, "/recordings/a.wav", 1000, 42, "Acme").unwrap();
        assert!(event(&c, &page).unwrap().is_none());
        let invite = MeetingEvent {
            title: "Acme weekly".into(),
            start_ms: 0,
            end_ms: 1_800_000,
            attendees: vec!["Priya Shah".into(), "jo@acme.test".into()],
            series: Some("series-1".into()),
        };
        set_event(&c, &page, &invite).unwrap();
        assert_eq!(event(&c, &page).unwrap(), Some(invite));
        assert_eq!(vocabulary(&c, &page).unwrap(), vec!["Acme", "Priya Shah"]);
    }
    #[test]
    fn people_seen_in_the_call_join_the_invitees() {
        let db = Db::open_in_memory().unwrap();
        let c = db.conn.lock().unwrap();
        let page = register(&c, "/recordings/a.wav", 1000, 42, "").unwrap();
        assert!(call(&c, &page).unwrap().is_none());
        assert_eq!(voices_at_most(&c, &page).unwrap(), 0, "nobody known, no cap");
        let invite = MeetingEvent {
            title: "Acme weekly".into(),
            attendees: vec!["Priya Shah".into(), "jo@acme.test".into()],
            ..Default::default()
        };
        set_event(&c, &page, &invite).unwrap();
        assert_eq!(voices_at_most(&c, &page).unwrap(), 3);
        let seen = CallRecord {
            app: "Teams".into(),
            title: None,
            people: vec!["priya shah".into(), "Tom Baker".into(), "Mia Chen".into()],
            speaking: vec![Spoken { from_ms: 1_000, to_ms: 4_000, name: "Tom Baker".into() }],
        };
        set_call(&c, &page, &seen).unwrap();
        assert_eq!(call(&c, &page).unwrap(), Some(seen));
        // The invitee's spelling wins and the email address is left out.
        assert_eq!(people(&c, &page).unwrap(), vec!["Priya Shah", "Tom Baker", "Mia Chen"]);
        assert_eq!(voices_at_most(&c, &page).unwrap(), 4, "three seen beats two invited");
        assert_eq!(vocabulary(&c, &page).unwrap(), vec!["Priya Shah", "Tom Baker", "Mia Chen"]);
    }
    #[test]
    fn whisper_hears_the_client_and_the_persons_own_terms() {
        let db = Db::open_in_memory().unwrap();
        let c = db.conn.lock().unwrap();
        let page = register(&c, "/recordings/a.wav", 1000, 42, "Harbourside Dental").unwrap();
        assert_eq!(vocabulary(&c, &page).unwrap(), vec!["Harbourside Dental"]);
        crate::db::set_setting(&c, VOCABULARY, "Cliniko, HotDoc;\nDr Chen\n").unwrap();
        assert_eq!(
            vocabulary(&c, &page).unwrap(),
            vec!["Harbourside Dental", "Cliniko", "HotDoc", "Dr Chen"]
        );
        let unfiled = register(&c, "/recordings/b.wav", 1000, 43, "").unwrap();
        assert_eq!(vocabulary(&c, &unfiled).unwrap(), vec!["Cliniko", "HotDoc", "Dr Chen"]);
    }
    #[test]
    fn invalid_result_does_not_change_saved_text_or_source() {
        let db = Db::open_in_memory().unwrap();
        let c = db.conn.lock().unwrap();
        let page = register(&c, "/retained.wav", 2000, 42, "Client").unwrap();
        let original = documents::core::get(&c, &page).unwrap();
        assert!(save_result(&c, &page, &original, "invalid JSON", "small", "en").is_err());
        assert_eq!(documents::core::get(&c, &page).unwrap(), original);
        assert_eq!(source(&c, &page).unwrap(), "/retained.wav");
        assert!(versions(&c, &page).unwrap().is_empty());
    }
    #[test]
    fn recording_exists_without_any_transcript() {
        let db = Db::open_in_memory().unwrap();
        let c = db.conn.lock().unwrap();
        let p = register(&c, "/saved.wav", 2000, 42, "").unwrap();
        assert_eq!(list(&c).unwrap()[0].transcript_state, "pending");
        assert_eq!(source(&c, &p).unwrap(), "/saved.wav");
    }
}
