//! The flagship ingest pipeline: turn a raw blob (a meeting transcript, a note
//! dumped from an agent) into a filed, summarized `record` page under the right
//! client, plus optional Tasks rows for each action item. Used by BOTH the MCP
//! sidecar (`ingest_note` tool) and the meeting recorder on save.

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::db::{new_id, now_ms, Db};
use crate::error::AppResult;
use crate::store::{databases, documents, pages};

#[derive(Debug, Deserialize)]
pub struct IngestArgs {
    /// The raw text to summarize and file (ignored when `body_json` is given).
    pub raw_text: String,
    /// Optional client/company name — an existing page is reused, else created.
    pub client_hint: Option<String>,
    /// Optional meeting page id to link the note back to (kind = meeting_ref).
    pub meeting_id: Option<String>,
    /// Optional Tasks database id; each action item becomes a row there.
    pub task_db_id: Option<String>,
    /// Optional title for the note page (defaults to a summary-derived title).
    pub title: Option<String>,
    /// Pre-rendered BlockNote JSON body. When present the LLM summarize step is
    /// SKIPPED and this is used verbatim — the meeting recorder passes its rich
    /// diarized transcript here so filing/linking/tasks are unified without
    /// re-summarizing or losing the transcript.
    #[serde(default)]
    pub body_json: Option<String>,
    /// Explicit action items (used for Task rows when `body_json` is supplied).
    #[serde(default)]
    pub action_items: Option<Vec<String>>,
}

#[derive(Debug, Serialize)]
pub struct IngestResult {
    pub page_id: String,
    pub client_page_id: Option<String>,
    pub task_row_ids: Vec<String>,
    pub summary: String,
    /// The note was filed with its original text; Tidy adds the summary (and
    /// any tasks) when its local AI next runs.
    #[serde(default)]
    pub summary_pending: bool,
}

/// The line an agent-filed note carries until its summary is written.
pub const SUMMARY_PENDING: &str =
    "Tidy will add a summary here once local AI has read this note. It runs while Tidy is open.";

/// Cut text at a word so it fits `max` characters, marking the cut.
pub(crate) fn shorten(text: &str, max: usize) -> String {
    let text = text.trim();
    if text.chars().count() <= max {
        return text.to_string();
    }
    let cut: String = text.chars().take(max).collect();
    let at = cut.rfind(char::is_whitespace).filter(|i| *i > max / 2).unwrap_or(cut.len());
    format!("{}…", cut[..at].trim_end_matches([',', ';', ':', ' ']))
}

/// A page title from text: its first sentence, cut at a word near 80
/// characters rather than mid-word.
pub fn title_from(text: &str) -> String {
    let first = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    let sentence = first
        .split_inclusive(['.', '?', '!'])
        .next()
        .unwrap_or(first)
        .trim()
        .trim_end_matches('.');
    if sentence.is_empty() {
        "Note".to_string()
    } else {
        shorten(sentence, 80)
    }
}

/// The pasted or filed text, kept under its own heading so a summary can
/// always be checked against what it came from.
fn original_blocks(raw_text: &str) -> Vec<Value> {
    let lines: Vec<&str> = raw_text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    if lines.is_empty() {
        return vec![];
    }
    let mut blocks = vec![json!({"type":"heading","props":{"level":2},"content":"Original text"})];
    blocks.extend(lines.into_iter().map(|l| json!({"type":"paragraph","content":l})));
    blocks
}

/// A note that is waiting for its summary: the original text and a line
/// saying the summary is coming.
pub fn pending_body(raw_text: &str) -> String {
    let mut blocks = vec![json!({"type":"paragraph","content":SUMMARY_PENDING})];
    blocks.extend(original_blocks(raw_text));
    Value::Array(blocks).to_string()
}

/// Find a live client page by trimmed, case-insensitive title (deterministic: oldest match).
fn find_page_by_title(conn: &rusqlite::Connection, title: &str) -> AppResult<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT id FROM page
             WHERE trim(title) = ?1 COLLATE NOCASE AND type = 'doc' AND deleted_at IS NULL
             ORDER BY created_at LIMIT 1",
            params![title],
            |r| r.get::<_, String>(0),
        )
        .optional()?)
}

/// Build a BlockNote document body from a summary (string content is accepted).
pub fn build_body(summary: &crate::llm::MeetingSummary) -> String {
    Value::Array(summary_blocks(summary)).to_string()
}

/// A summary followed by the text it summarises.
pub fn build_body_with_original(summary: &crate::llm::MeetingSummary, raw_text: &str) -> String {
    let mut blocks = summary_blocks(summary);
    blocks.extend(original_blocks(raw_text));
    Value::Array(blocks).to_string()
}

fn summary_blocks(summary: &crate::llm::MeetingSummary) -> Vec<Value> {
    // A "summary" that only repeats one of the points below adds nothing.
    let mut blocks: Vec<Value> = match crate::store::meeting_ai::overview_of(summary) {
        Some(overview) => vec![
            json!({"type":"heading","props":{"level":2},"content":"Summary"}),
            json!({"type":"paragraph","content": overview}),
        ],
        None => vec![],
    };
    for section in &summary.sections {
        blocks.push(json!({"type":"heading","props":{"level":3},"content":section.heading}));
        for point in &section.points { blocks.push(json!({"type":"bulletListItem","content":point})); }
    }
    if !summary.open_questions.is_empty() {
        blocks.push(json!({"type":"heading","props":{"level":2},"content":"Open questions"}));
        for point in &summary.open_questions { blocks.push(json!({"type":"bulletListItem","content":point})); }
    }
    if !summary.action_items.is_empty() {
        blocks.push(json!({"type":"heading","props":{"level":2},"content":"Action items"}));
        // Tick boxes, so the note doubles as a checklist.
        for a in &summary.action_items {
            blocks.push(json!({"type":"checkListItem","props":{"checked":false},"content": a}));
        }
    }
    if !summary.decisions.is_empty() {
        blocks.push(json!({"type":"heading","props":{"level":2},"content":"Decisions"}));
        for d in &summary.decisions {
            blocks.push(json!({"type":"bulletListItem","content": d}));
        }
    }
    blocks
}

/// Insert a link edge between two pages.
fn add_link(
    conn: &rusqlite::Connection,
    source: &str,
    target: &str,
    kind: &str,
) -> AppResult<()> {
    conn.execute(
        "INSERT INTO link (id, source_page_id, target_page_id, dst_title, kind, context, created_at)
         VALUES (?1, ?2, ?3, NULL, ?4, NULL, ?5)",
        params![new_id(), source, target, kind, now_ms()],
    )?;
    Ok(())
}

/// Today, in the local timezone, as the `YYYY-MM-DD` string the date cells use.
fn today_local() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// Create Tasks rows for each action item in `task_db_id`, returning their ids.
pub(crate) fn create_task_rows(
    conn: &rusqlite::Connection,
    task_db_id: &str,
    items: &[String],
) -> AppResult<Vec<String>> {
    let fields = databases::core::list_fields(conn, task_db_id)?;
    let name_field = fields.iter().find(|f| f.kind == "text").map(|f| f.id.clone());
    let status_field = fields.iter().find(|f| f.kind == "select");
    // Dating the task is what puts it on the planner; an undated row is filed
    // and then never seen again.
    let date_field = fields.iter().find(|f| f.kind == "date").map(|f| f.id.clone());
    let today = today_local();
    // resolve the "To do" choice id from the Status field options, if present
    let todo_choice = status_field.and_then(|f| {
        f.options.as_ref().and_then(|o| {
            o.get("choices")?.as_array()?.iter().find_map(|c| {
                let name = c.get("name")?.as_str()?;
                if name.eq_ignore_ascii_case("to do") {
                    Some(c.get("id")?.as_str()?.to_string())
                } else {
                    None
                }
            })
        })
    });

    let mut ids = Vec::new();
    for item in items {
        let row = databases::core::create_row(conn, task_db_id)?;
        if let Some(nf) = &name_field {
            databases::core::set_cell(conn, &row.id, nf, &json!(item))?;
        }
        if let (Some(sf), Some(choice)) = (status_field, &todo_choice) {
            databases::core::set_cell(conn, &row.id, &sf.id, &json!(choice))?;
        }
        if let Some(df) = &date_field {
            databases::core::set_cell(conn, &row.id, df, &json!(today))?;
        }
        ids.push(row.id);
    }
    Ok(ids)
}

/// Raw text is summarised by Tidy's own local AI, so it must be switched on.
/// Refuse before anything is written rather than file a note that will never
/// get its summary.
fn require_local_ai(conn: &rusqlite::Connection) -> AppResult<()> {
    const CONFIG_KEY: &str = "local_ai.config.v1";
    let config: Value = crate::db::get_setting(conn, CONFIG_KEY)?
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or(Value::Null);
    let provider = config.get("provider").and_then(|p| p.as_str()).unwrap_or("disabled");
    if provider == "disabled" {
        return Err(crate::error::AppError::Invalid(
            "Turn on local AI in Tidy's Settings so notes can be summarised, or pass body_json.".into(),
        ));
    }
    Ok(())
}

/// An agent-filed note waiting for its summary.
#[derive(Debug, Clone)]
pub struct NoteJob {
    pub page_id: String,
    pub raw_text: String,
    pub task_db_id: Option<String>,
    pub auto_title: bool,
    pub body_hash: Option<String>,
}

/// The oldest note waiting for its summary.
pub fn next_note(conn: &rusqlite::Connection) -> AppResult<Option<NoteJob>> {
    Ok(conn
        .query_row(
            "SELECT n.page_id,n.raw_text,n.task_db_id,n.auto_title,n.body_hash FROM note_ai n
             JOIN page p ON p.id=n.page_id WHERE n.state='queued' AND p.deleted_at IS NULL
             ORDER BY n.created_at LIMIT 1",
            [],
            |r| {
                Ok(NoteJob {
                    page_id: r.get(0)?,
                    raw_text: r.get(1)?,
                    task_db_id: r.get(2)?,
                    auto_title: r.get(3)?,
                    body_hash: r.get(4)?,
                })
            },
        )
        .optional()?)
}

pub fn set_note_state(
    conn: &rusqlite::Connection,
    page_id: &str,
    state: &str,
    error: Option<&str>,
) -> AppResult<()> {
    conn.execute(
        "UPDATE note_ai SET state=?2,error=?3 WHERE page_id=?1",
        params![page_id, state, error],
    )?;
    Ok(())
}

/// Where a note's summary is up to: `None` once it's written (or for notes
/// that never needed one).
pub fn note_status(
    conn: &rusqlite::Connection,
    page_id: &str,
) -> AppResult<Option<(String, Option<String>)>> {
    Ok(conn
        .query_row(
            "SELECT state,error FROM note_ai WHERE page_id=?1",
            [page_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?)
}

/// Write a note's summary. If nobody has touched the note since it was
/// filed, the summary replaces the waiting line; if someone has, it goes on
/// top and their edits stay. Returns the ids of any task rows made.
pub fn finish_note(
    conn: &rusqlite::Connection,
    job: &NoteJob,
    summary: &crate::llm::MeetingSummary,
) -> AppResult<Vec<String>> {
    let tx = conn.unchecked_transaction()?;
    let current = documents::core::get(&tx, &job.page_id)?;
    let untouched = job.body_hash.as_deref()
        == Some(blake3::hash(current.as_bytes()).to_hex().as_str());
    let body = if untouched {
        build_body_with_original(summary, &job.raw_text)
    } else {
        let mut blocks: Vec<Value> = serde_json::from_str(&current).unwrap_or_default();
        blocks.retain(|b| b["content"].as_str() != Some(SUMMARY_PENDING));
        let mut top = summary_blocks(summary);
        top.extend(blocks);
        Value::Array(top).to_string()
    };
    documents::core::update(&tx, &job.page_id, &body)?;
    if job.auto_title {
        pages::core::rename(&tx, &job.page_id, &title_from(&summary.summary))?;
    }
    let tasks = match &job.task_db_id {
        Some(db) if !db.trim().is_empty() => create_task_rows(&tx, db, &summary.action_items)?,
        _ => Vec::new(),
    };
    tx.execute("DELETE FROM note_ai WHERE page_id=?1", [&job.page_id])?;
    tx.commit()?;
    Ok(tasks)
}

/// Everything that has to happen after an ingest commits, wherever it was
/// triggered from: mirror the new pages to the vault so an MCP-created note
/// doesn't sit permanently dirty and invisible to a linked Obsidian vault.
pub fn post_ingest(conn: &rusqlite::Connection, result: &IngestResult) {
    crate::vault::export::flush_if_configured(conn, &result.page_id);
    if let Some(client_page_id) = &result.client_page_id {
        crate::vault::export::flush_if_configured(conn, client_page_id);
    }
}

/// The one-transaction ingest: summarize → file under client → link → tasks.
pub async fn ingest_note(db: &Db, args: IngestArgs) -> AppResult<IngestResult> {
    // 1. Body + action items: either supplied pre-rendered (the recorder passes
    //    its diarized doc) or derived by summarizing the raw text off-lock. We
    //    only ever .await here (before taking the guard), so the future is Send.
    // Raw text is filed at once with its original wording; Tidy's own local
    // AI (whichever model the person chose, checked against the text) adds
    // the summary and tasks when it next runs. An agent never waits on it.
    let pending = args.body_json.is_none();
    let (body, action_items, summary_text) = if let Some(body) = args.body_json.clone() {
        (body, args.action_items.clone().unwrap_or_default(), String::new())
    } else {
        {
            let conn = db.lock()?;
            require_local_ai(&conn)?;
        }
        if args.raw_text.trim().is_empty() {
            return Err(crate::error::AppError::Invalid("There's no text to file.".into()));
        }
        (pending_body(&args.raw_text), Vec::new(), String::new())
    };

    // 2. Everything else is synchronous SQLite work under one lock, wrapped in a
    //    single transaction so a mid-way failure rolls back cleanly (no orphan
    //    note/client/tasks).
    let conn = db.lock()?;
    let tx = conn.unchecked_transaction()?;

    // Client page (reuse by title, else create a doc).
    let client_page_id = match &args.client_hint {
        Some(hint) if !hint.trim().is_empty() => {
            let hint = hint.trim();
            match find_page_by_title(&tx, hint)? {
                Some(id) => Some(id),
                None => Some(pages::core::create(&tx, None, hint.to_string(), "doc".into())?.id),
            }
        }
        _ => None,
    };

    // Note page (a record), parented under the client when we have one.
    let title = args.title.clone().unwrap_or_else(|| {
        if pending {
            title_from(&args.raw_text)
        } else if summary_text.trim().is_empty() {
            "Meeting note".to_string()
        } else {
            title_from(&summary_text)
        }
    });
    let note = pages::core::create(&tx, client_page_id.clone(), title, "doc".into())?;
    tx.execute(
        "UPDATE page SET type = 'record' WHERE id = ?1",
        params![note.id],
    )?;
    documents::core::update(&tx, &note.id, &body)?;

    // Links: note → client (task_of context), note → meeting (meeting_ref).
    if let Some(cid) = &client_page_id {
        add_link(&tx, &note.id, cid, "task_of")?;
    }
    if let Some(mid) = &args.meeting_id {
        add_link(&tx, &note.id, mid, "meeting_ref")?;
    }

    // Tasks for each action item. A pending note makes them with its summary.
    let task_row_ids = match &args.task_db_id {
        Some(db_id) if !db_id.trim().is_empty() && !pending => {
            create_task_rows(&tx, db_id, &action_items)?
        }
        _ => Vec::new(),
    };
    if pending {
        tx.execute(
            "INSERT INTO note_ai(page_id,raw_text,task_db_id,auto_title,body_hash,created_at) VALUES(?1,?2,?3,?4,?5,?6)",
            params![
                note.id,
                args.raw_text,
                args.task_db_id.as_deref().filter(|d| !d.trim().is_empty()),
                args.title.is_none(),
                blake3::hash(body.as_bytes()).to_hex().to_string(),
                now_ms()
            ],
        )?;
    }

    tx.commit()?;

    let result = IngestResult {
        page_id: note.id,
        client_page_id,
        task_row_ids,
        summary: summary_text,
        summary_pending: pending,
    };
    post_ingest(&conn, &result);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    #[tokio::test]
    async fn repeated_meetings_reuse_client_ignoring_case_and_spaces() {
        let db = Db::open_in_memory().unwrap();
        let mut results = Vec::new();
        for hint in ["Acme Corp", "  acme corp  "] {
            results.push(ingest_note(&db, IngestArgs {
                raw_text: String::new(), client_hint: Some(hint.into()), meeting_id: None,
                task_db_id: None, title: Some("Review".into()), body_json: Some("[]".into()), action_items: None,
            }).await.unwrap());
        }
        assert_eq!(results[0].client_page_id, results[1].client_page_id);
        assert_ne!(results[0].page_id, results[1].page_id);
        let c = db.conn.lock().unwrap();
        let count:i64 = c.query_row("SELECT count(*) FROM link WHERE target_page_id=?1 AND kind='task_of'", [&results[0].client_page_id], |r| r.get(0)).unwrap();
        assert_eq!(count,2);
    }

    /// The recorder path: pre-rendered body + explicit action items, no LLM.
    #[tokio::test]
    async fn ingest_with_body_json_files_under_client_and_makes_tasks() {
        let db = Db::open_in_memory().unwrap();
        // a Tasks database to receive action-item rows
        let task_db_id = {
            let conn = db.conn.lock().unwrap();
            let p = pages::core::create(&conn, None, "Tasks".into(), "database".into()).unwrap();
            databases::core::database_id_for_page(&conn, &p.id).unwrap()
        };

        let body = r#"[{"type":"paragraph","content":"Rich diarized transcript"}]"#;
        let args = IngestArgs {
            raw_text: String::new(),
            client_hint: Some("Acme Corp".into()),
            meeting_id: None,
            task_db_id: Some(task_db_id.clone()),
            title: Some("Kickoff".into()),
            body_json: Some(body.to_string()),
            action_items: Some(vec!["Send proposal".into(), "Book follow-up".into()]),
        };
        let r = ingest_note(&db, args).await.unwrap();

        let conn = db.conn.lock().unwrap();
        // note is a record page, parented under the (new) client, with our body
        let note = pages::core::read(&conn, &r.page_id).unwrap();
        assert_eq!(note.title, "Kickoff");
        assert_eq!(note.kind, "record");
        assert_eq!(note.parent_id.as_deref(), r.client_page_id.as_deref());
        assert_eq!(documents::core::get(&conn, &r.page_id).unwrap(), body);
        // client reused on a second ingest with the same hint
        assert!(r.client_page_id.is_some());
        // two action items → two task rows
        assert_eq!(r.task_row_ids.len(), 2);
        let rows = databases::core::list_rows(&conn, &task_db_id).unwrap();
        assert_eq!(rows.len(), 2);
        // note → client link exists
        let backlinks = crate::store::knowledge::core::get_backlinks(&conn, r.client_page_id.as_ref().unwrap()).unwrap();
        assert_eq!(backlinks.len(), 1);
        assert_eq!(backlinks[0].kind, "task_of");
        assert_eq!(backlinks[0].source_page_id, r.page_id);
    }

    #[tokio::test]
    async fn task_rows_are_dated_today_so_they_reach_the_planner() {
        let db = Db::open_in_memory().unwrap();
        let (task_db_id, date_field, name_field) = {
            let conn = db.conn.lock().unwrap();
            let p = pages::core::create(&conn, None, "Tasks".into(), "database".into()).unwrap();
            let id = databases::core::database_id_for_page(&conn, &p.id).unwrap();
            let fields = databases::core::list_fields(&conn, &id).unwrap();
            let date = fields.iter().find(|f| f.kind == "date").unwrap().id.clone();
            let name = fields.iter().find(|f| f.kind == "text").unwrap().id.clone();
            (id, date, name)
        };

        let r = ingest_note(
            &db,
            IngestArgs {
                raw_text: String::new(),
                client_hint: None,
                meeting_id: None,
                task_db_id: Some(task_db_id.clone()),
                title: Some("Kickoff".into()),
                body_json: Some("[]".into()),
                action_items: Some(vec!["Send proposal".into()]),
            },
        )
        .await
        .unwrap();
        assert_eq!(r.task_row_ids.len(), 1);

        let conn = db.conn.lock().unwrap();
        let rows = databases::core::list_rows(&conn, &task_db_id).unwrap();
        let today = super::today_local();
        assert_eq!(rows[0].cells.get(&date_field), Some(&json!(today)));
        assert_eq!(rows[0].cells.get(&name_field), Some(&json!("Send proposal")));
    }

    #[tokio::test]
    async fn raw_text_with_local_ai_off_is_refused_before_any_write() {
        let db = Db::open_in_memory().unwrap();
        let err = ingest_note(
            &db,
            IngestArgs {
                raw_text: "some meeting notes".into(),
                client_hint: Some("Acme".into()),
                meeting_id: None,
                task_db_id: None,
                title: None,
                body_json: None,
                action_items: None,
            },
        )
        .await
        .unwrap_err();
        assert!(
            matches!(err, crate::error::AppError::Invalid(ref m) if m.contains("Turn on local AI")),
            "unexpected error: {err}"
        );
        // nothing was filed
        let conn = db.conn.lock().unwrap();
        let n: i64 = conn
            .query_row("SELECT count(*) FROM page", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
    }

    fn with_local_ai(db: &Db) {
        let c = db.conn.lock().unwrap();
        crate::db::set_setting(&c, "local_ai.config.v1", r#"{"provider":"builtin","model_id":"qwen3-4b","ollama_model":""}"#).unwrap();
    }

    #[tokio::test]
    async fn agent_notes_are_filed_at_once_and_summarised_later() {
        let db = Db::open_in_memory().unwrap();
        with_local_ai(&db);
        let task_db = {
            let conn = db.conn.lock().unwrap();
            let p = pages::core::create(&conn, None, "Tasks".into(), "database".into()).unwrap();
            databases::core::database_id_for_page(&conn, &p.id).unwrap()
        };
        let raw = "Call with Acme about the renewal. They want the quote by Friday.\n\nMia will send it.";
        let filed = ingest_note(&db, IngestArgs {
            raw_text: raw.into(), client_hint: Some("Acme".into()), meeting_id: None,
            task_db_id: Some(task_db.clone()), title: None, body_json: None, action_items: None,
        }).await.unwrap();
        assert!(filed.summary_pending);
        assert!(filed.task_row_ids.is_empty());
        let conn = db.conn.lock().unwrap();
        let page = pages::core::read(&conn, &filed.page_id).unwrap();
        assert_eq!(page.title, "Call with Acme about the renewal");
        let body = documents::core::get(&conn, &filed.page_id).unwrap();
        assert!(body.contains(SUMMARY_PENDING) && body.contains("Mia will send it."));
        let job = next_note(&conn).unwrap().unwrap();
        let summary = crate::llm::MeetingSummary {
            summary: "Acme asked for the renewal quote by Friday, and Mia will send it.".into(),
            action_items: vec!["Mia will send the renewal quote by Friday.".into()],
            ..Default::default()
        };
        let tasks = finish_note(&conn, &job, &summary).unwrap();
        assert_eq!(tasks.len(), 1);
        let body = documents::core::get(&conn, &filed.page_id).unwrap();
        assert!(!body.contains(SUMMARY_PENDING));
        assert!(body.contains("checkListItem") && body.contains("Original text") && body.contains("Mia will send it."));
        assert_eq!(pages::core::read(&conn, &filed.page_id).unwrap().title, "Acme asked for the renewal quote by Friday, and Mia will send it");
        assert!(next_note(&conn).unwrap().is_none());
        assert!(note_status(&conn, &filed.page_id).unwrap().is_none());
    }

    #[tokio::test]
    async fn edits_made_while_waiting_are_kept_under_the_summary() {
        let db = Db::open_in_memory().unwrap();
        with_local_ai(&db);
        let filed = ingest_note(&db, IngestArgs {
            raw_text: "Raw notes".into(), client_hint: None, meeting_id: None,
            task_db_id: None, title: Some("Kept title".into()), body_json: None, action_items: None,
        }).await.unwrap();
        let conn = db.conn.lock().unwrap();
        documents::core::update(&conn, &filed.page_id, r#"[{"type":"paragraph","content":"My own line"}]"#).unwrap();
        let job = next_note(&conn).unwrap().unwrap();
        finish_note(&conn, &job, &crate::llm::MeetingSummary { summary: "A summary.".into(), ..Default::default() }).unwrap();
        let body = documents::core::get(&conn, &filed.page_id).unwrap();
        assert!(body.find("A summary.").unwrap() < body.find("My own line").unwrap());
        assert_eq!(pages::core::read(&conn, &filed.page_id).unwrap().title, "Kept title");
    }

    #[test]
    fn titles_end_at_a_word() {
        assert_eq!(title_from("Short note."), "Short note");
        let long = "The meeting focused on website migration planning, content creation, and technical decisions for the relaunch.";
        let title = title_from(long);
        assert!(title.chars().count() <= 81 && title.ends_with('…'), "{title}");
        assert!(!title.contains("techn…"));
        assert_eq!(title_from("   "), "Note");
    }
}
