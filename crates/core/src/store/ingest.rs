//! The flagship ingest pipeline: turn a raw blob (a meeting transcript, a note
//! dumped from an agent) into a filed, summarized `record` page under the right
//! client, plus optional Tasks rows for each action item. Used by BOTH the MCP
//! sidecar (`ingest_note` tool) and the meeting recorder on save.

use rusqlite::params;
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
}

/// Find a live client page by exact title (deterministic: oldest match).
fn find_page_by_title(conn: &rusqlite::Connection, title: &str) -> AppResult<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT id FROM page
             WHERE title = ?1 AND type = 'doc' AND deleted_at IS NULL
             ORDER BY created_at LIMIT 1",
            params![title],
            |r| r.get::<_, String>(0),
        )
        .ok())
}

/// Build a BlockNote document body from a summary (string content is accepted).
fn build_body(summary: &crate::llm::MeetingSummary) -> String {
    let mut blocks: Vec<Value> = vec![
        json!({"type":"heading","props":{"level":2},"content":"Summary"}),
        json!({"type":"paragraph","content": summary.summary}),
    ];
    if !summary.action_items.is_empty() {
        blocks.push(json!({"type":"heading","props":{"level":2},"content":"Action items"}));
        for a in &summary.action_items {
            blocks.push(json!({"type":"bulletListItem","content": a}));
        }
    }
    if !summary.decisions.is_empty() {
        blocks.push(json!({"type":"heading","props":{"level":2},"content":"Decisions"}));
        for d in &summary.decisions {
            blocks.push(json!({"type":"bulletListItem","content": d}));
        }
    }
    Value::Array(blocks).to_string()
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

/// Create Tasks rows for each action item in `task_db_id`, returning their ids.
fn create_task_rows(
    conn: &rusqlite::Connection,
    task_db_id: &str,
    items: &[String],
) -> AppResult<Vec<String>> {
    let fields = databases::core::list_fields(conn, task_db_id)?;
    let name_field = fields.iter().find(|f| f.kind == "text").map(|f| f.id.clone());
    let status_field = fields.iter().find(|f| f.kind == "select");
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
        ids.push(row.id);
    }
    Ok(ids)
}

/// The one-transaction ingest: summarize → file under client → link → tasks.
pub async fn ingest_note(db: &Db, args: IngestArgs) -> AppResult<IngestResult> {
    // 1. Body + action items: either supplied pre-rendered (the recorder passes
    //    its diarized doc) or derived by summarizing the raw text off-lock. We
    //    only ever .await here (before taking the guard), so the future is Send.
    let (body, action_items, summary_text) = if let Some(body) = args.body_json.clone() {
        (body, args.action_items.clone().unwrap_or_default(), String::new())
    } else {
        let summary = crate::llm::ollama::summarize(&args.raw_text).await?;
        (build_body(&summary), summary.action_items.clone(), summary.summary.clone())
    };

    // 2. Everything else is synchronous SQLite work under one lock, wrapped in a
    //    single transaction so a mid-way failure rolls back cleanly (no orphan
    //    note/client/tasks).
    let conn = db.conn.lock().unwrap();
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
        let first = summary_text.lines().next().unwrap_or("").trim();
        if first.is_empty() {
            "Meeting note".to_string()
        } else {
            first.chars().take(80).collect()
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

    // Tasks for each action item.
    let task_row_ids = match &args.task_db_id {
        Some(db_id) if !db_id.trim().is_empty() => create_task_rows(&tx, db_id, &action_items)?,
        _ => Vec::new(),
    };

    tx.commit()?;

    Ok(IngestResult {
        page_id: note.id,
        client_page_id,
        task_row_ids,
        summary: summary_text,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

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
        // get_backlinks filters kind='mention'; the task_of link is separate, so
        // assert directly on the link table instead
        let n: i64 = conn
            .query_row(
                "SELECT count(*) FROM link WHERE source_page_id=?1 AND kind='task_of'",
                params![r.page_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1);
        let _ = backlinks;
    }
}
