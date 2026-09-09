//! Tauri command over the shared `ingest_note` pipeline — the same function the
//! MCP sidecar exposes and the meeting recorder folds into on save.

use tauri::{AppHandle, Emitter, State};

use crate::db::Db;
use crate::error::AppResult;
use tidy_core::store::ingest::{self, IngestArgs, IngestResult};

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn ingest_note(
    app: AppHandle,
    db: State<'_, Db>,
    raw_text: String,
    client_hint: Option<String>,
    meeting_id: Option<String>,
    task_db_id: Option<String>,
    title: Option<String>,
    body_json: Option<String>,
    action_items: Option<Vec<String>>,
) -> AppResult<IngestResult> {
    // Route desktop note assistance through the same managed AI provider.
    let generated = if body_json.is_none() {
        Some(crate::local_ai::summarise(&app, &raw_text).await?)
    } else {
        None
    };
    let body_json = body_json.or_else(|| generated.as_ref().map(ingest::build_body));
    let action_items = action_items.or_else(|| generated.as_ref().map(|s| s.action_items.clone()));
    let title = title.or_else(|| {
        generated
            .as_ref()
            .map(|s| s.summary.chars().take(80).collect())
    });
    let args = IngestArgs {
        raw_text,
        client_hint,
        meeting_id,
        task_db_id,
        title,
        body_json,
        action_items,
    };
    let mut result = ingest::ingest_note(&db, args).await?;
    if let Some(summary) = generated {
        result.summary = summary.summary;
    }
    // Mirror the new note (and any created client page) to the vault immediately,
    // clearing their dirty flags — otherwise recorder/MCP notes would never reach
    // a linked vault and would stay dirty (and later false-conflict).
    {
        let conn = db.conn.lock().unwrap();
        crate::vault::flush_if_configured(&conn, &result.page_id);
        if let Some(cid) = &result.client_page_id {
            crate::vault::flush_if_configured(&conn, cid);
        }
    }
    // Refresh the sidebar/tree and any open database view.
    let _ = app.emit("pages-changed", ());
    let _ = app.emit("database-changed", ());
    Ok(result)
}
