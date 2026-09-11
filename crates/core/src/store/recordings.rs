//! Durable meeting metadata and append-only transcript revisions.
use crate::{
    db::{new_id, now_ms},
    error::{AppError, AppResult},
    store::{documents, pages},
};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

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
      m.started_at,m.duration,m.audio_path,m.model_used,m.transcript_state,m.transcript_error,m.retain_audio
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
