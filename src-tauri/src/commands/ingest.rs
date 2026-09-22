//! Tauri command over the shared `ingest_note` pipeline — the same function the
//! MCP sidecar exposes and the meeting recorder folds into on save.

use tauri::{AppHandle, Emitter, Manager};

use crate::db::Db;
use crate::error::AppResult;
use tidy_core::store::ingest::{self, IngestArgs, IngestResult};

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn ingest_note(
    app: AppHandle,
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
    // Keep what was pasted under the summary, so the summary can always be
    // checked against it.
    let body_json = body_json.or_else(|| {
        generated
            .as_ref()
            .map(|s| ingest::build_body_with_original(s, &raw_text))
    });
    let action_items = action_items.or_else(|| generated.as_ref().map(|s| s.action_items.clone()));
    let title = title.or_else(|| generated.as_ref().map(|s| ingest::title_from(&s.summary)));
    let args = IngestArgs {
        raw_text,
        client_hint,
        meeting_id,
        task_db_id,
        title,
        body_json,
        action_items,
    };
    // ingest_note mirrors the new note (and any created client page) to the
    // vault itself, so recorder and sidecar notes reach a linked vault the same
    // way and never sit dirty (and later false-conflict).
    let mut result = ingest::ingest_note(&app.state::<Db>(), args).await?;
    if let Some(summary) = generated {
        result.summary = summary.summary;
    }
    // Refresh the sidebar/tree and any open database view.
    let _ = app.emit("pages-changed", ());
    let _ = app.emit("database-changed", ());
    Ok(result)
}

/// Where the summary of a note an agent filed is up to: `null` once it's
/// written, otherwise its state and any error.
#[tauri::command]
pub async fn note_summary_status(
    app: AppHandle,
    page_id: String,
) -> AppResult<Option<NoteSummaryStatus>> {
    crate::workspace::run_db(&app, move |c| {
        Ok(ingest::note_status(c, &page_id)?.map(|(state, error)| NoteSummaryStatus { state, error }))
    })
    .await
}

#[derive(serde::Serialize)]
pub struct NoteSummaryStatus {
    pub state: String,
    pub error: Option<String>,
}

#[tauri::command]
pub async fn note_summary_retry(app: AppHandle, page_id: String) -> AppResult<()> {
    crate::workspace::run_db(&app, move |c| ingest::set_note_state(c, &page_id, "queued", None)).await
}
