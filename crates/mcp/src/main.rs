//! Tidy MCP sidecar.
//!
//! A stdio MCP server that exposes the local Tidy knowledge base to agents
//! (Claude Code, Codex). It opens the SAME SQLite index as the desktop app (a
//! second WAL writer with a busy_timeout) and calls the exact `tidy_core`
//! store functions the GUI uses — one source of truth, zero duplication.
//!
//! Read tools are always available. Write tools (create/update/ingest) are gated
//! behind a per-install token: they only work when `TIDY_MCP_TOKEN` matches
//! `setting('mcp_token')` minted by the app. The stored token is re-read on
//! every call, so revoking write access in Tidy takes effect immediately rather
//! than at the sidecar's next restart. Everything is soft, additive, and local.

use std::path::PathBuf;
use std::sync::Arc;

use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router, ServerHandler, ServiceExt,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use rusqlite::Connection;
use tidy_core::db::{self, Db};
use tidy_core::store::{databases, documents, ingest, knowledge, pages, search};

// ---- tool argument schemas -------------------------------------------------

#[derive(Debug, Deserialize, JsonSchema)]
struct SearchArgs {
    /// Full-text query (matches page titles, bodies, and text cells).
    query: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct IdArgs {
    /// The page id.
    id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct PageIdArgs {
    /// The page id.
    page_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct CreatePageArgs {
    /// Optional parent page id.
    parent_id: Option<String>,
    /// Page title.
    title: String,
    /// "doc" (default) or "database".
    kind: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct UpdateDocArgs {
    /// The page id.
    id: String,
    /// BlockNote document JSON (an array of blocks).
    content: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct CreateFieldArgs {
    /// The database id.
    database_id: String,
    /// Field name.
    name: String,
    /// Field type: text, number, select, date, checkbox, dependencies, relation, lookup, rollup, formula.
    kind: String,
    /// Optional field options (JSON), e.g. select choices or relation target.
    options: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct AddRowArgs {
    /// The database id.
    database_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct SetCellArgs {
    /// The row id.
    row_id: String,
    /// The field id.
    field_id: String,
    /// The cell value (JSON). `null` clears the cell.
    value: serde_json::Value,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct IngestArgs {
    /// The raw text to file and summarise (a meeting transcript, notes, an email).
    raw_text: String,
    /// Optional client/company name — an existing page is reused, else created.
    client_hint: Option<String>,
    /// Optional meeting page id to link the note back to.
    meeting_id: Option<String>,
    /// Optional Tasks database id; each next step in the summary becomes a row there.
    task_db_id: Option<String>,
    /// Optional title for the note page (otherwise taken from the summary).
    title: Option<String>,
}

// ---- server ----------------------------------------------------------------

#[derive(Clone)]
struct TidyServer {
    db: Arc<Db>,
    /// The token this process was launched with, compared against the token the
    /// app has stored on every write call.
    env_token: Option<String>,
    /// Whether writes were enabled at startup — only used for the banner.
    writes_enabled: bool,
    tool_router: ToolRouter<Self>,
}

fn json<T: Serialize>(v: &T) -> Result<String, String> {
    serde_json::to_string_pretty(v).map_err(|e| e.to_string())
}

/// Does the app still consider this token valid? A missing stored token means
/// the user revoked write access.
fn token_matches(conn: &Connection, env_token: Option<&str>) -> bool {
    let stored = db::get_setting(conn, "mcp_token").ok().flatten();
    match (env_token, stored) {
        (Some(env), Some(stored)) => !env.is_empty() && env == stored,
        _ => false,
    }
}

/// The page a database lives on, as a one-element list for the change signal.
fn page_for_database(conn: &Connection, database_id: &str) -> Vec<String> {
    one(conn.query_row(
        "SELECT page_id FROM database WHERE id = ?1",
        rusqlite::params![database_id],
        |r| r.get::<_, String>(0),
    ))
}

/// The page the row's database lives on.
fn page_for_row(conn: &Connection, row_id: &str) -> Vec<String> {
    one(conn.query_row(
        "SELECT d.page_id FROM db_row r JOIN database d ON d.id = r.database_id WHERE r.id = ?1",
        rusqlite::params![row_id],
        |r| r.get::<_, String>(0),
    ))
}

fn one(found: rusqlite::Result<String>) -> Vec<String> {
    found.ok().into_iter().collect()
}

#[derive(Serialize)]
struct PageWithBody {
    page: pages::Page,
    body: String,
}

#[tool_router(router = tool_router)]
impl TidyServer {
    fn new(db: Arc<Db>, env_token: Option<String>) -> Self {
        let writes_enabled = {
            let conn = db.lock().ok();
            conn.map(|c| token_matches(&c, env_token.as_deref()))
                .unwrap_or(false)
        };
        Self {
            db,
            env_token,
            writes_enabled,
            tool_router: Self::tool_router(),
        }
    }

    /// Take the connection, mapping a poisoned lock into a tool error rather
    /// than bringing the sidecar down.
    fn conn(&self) -> Result<std::sync::MutexGuard<'_, Connection>, String> {
        self.db.lock().map_err(|e| e.to_string())
    }

    /// Re-read the app's token on every write, so revoking access in Tidy's
    /// settings stops an already-running sidecar from writing immediately.
    fn require_write(&self, conn: &Connection) -> Result<(), String> {
        if token_matches(conn, self.env_token.as_deref()) {
            Ok(())
        } else {
            Err(
                "Write tools are disabled. Set TIDY_MCP_TOKEN to the token minted in Tidy \
                 under Settings, Connections to enable them."
                    .into(),
            )
        }
    }

    /// Bump the change sequence the running app polls, naming the pages it
    /// should reload. Best-effort: a write that succeeded must not be reported
    /// as failed because the signal did not land.
    fn signal(conn: &Connection, page_ids: &[String]) {
        if let Err(e) = db::record_change(conn, page_ids) {
            eprintln!("tidy-mcp: could not record the change signal: {e}");
        }
    }

    // ---- read tools (always available) ----

    #[tool(
        description = "Full-text search the knowledge base (page titles, bodies, and text cells)."
    )]
    async fn kb_search(&self, Parameters(a): Parameters<SearchArgs>) -> Result<String, String> {
        let conn = self.conn()?;
        let r = search::core::search(&conn, &a.query).map_err(|e| e.to_string())?;
        json(&r)
    }

    #[tool(description = "List every page in the workspace (id, title, type, parent).")]
    async fn list_pages(&self) -> Result<String, String> {
        let conn = self.conn()?;
        let r = pages::core::list(&conn).map_err(|e| e.to_string())?;
        json(&r)
    }

    #[tool(description = "Get a page's metadata and its document body (BlockNote JSON).")]
    async fn get_page(&self, Parameters(a): Parameters<IdArgs>) -> Result<String, String> {
        let conn = self.conn()?;
        let page = pages::core::read(&conn, &a.id).map_err(|e| e.to_string())?;
        let body = documents::core::get(&conn, &a.id).map_err(|e| e.to_string())?;
        json(&PageWithBody { page, body })
    }

    #[tool(
        description = "List the pages that link to a given page (backlinks). Each entry carries a \
                       `kind`: \"mention\" (a wiki link in the body), \"task_of\" (a note filed under \
                       a client) or \"meeting_ref\" (a note linked to a meeting)."
    )]
    async fn get_backlinks(&self, Parameters(a): Parameters<PageIdArgs>) -> Result<String, String> {
        let conn = self.conn()?;
        let r = knowledge::core::get_backlinks(&conn, &a.page_id).map_err(|e| e.to_string())?;
        json(&r)
    }

    #[tool(description = "List all databases with their fields (for picking a table to write to).")]
    async fn list_databases(&self) -> Result<String, String> {
        let conn = self.conn()?;
        let r = databases::core::list_databases(&conn).map_err(|e| e.to_string())?;
        json(&r)
    }

    #[tool(
        description = "Get a database's full bundle (fields, rows, views) for a database page id."
    )]
    async fn get_database(&self, Parameters(a): Parameters<PageIdArgs>) -> Result<String, String> {
        let conn = self.conn()?;
        let r = databases::core::bundle(&conn, &a.page_id).map_err(|e| e.to_string())?;
        json(&r)
    }

    // ---- write tools (gated) ----

    #[tool(description = "Create a new page ('doc' or 'database'). Requires write access.")]
    async fn create_page(
        &self,
        Parameters(a): Parameters<CreatePageArgs>,
    ) -> Result<String, String> {
        let conn = self.conn()?;
        self.require_write(&conn)?;
        let kind = a.kind.unwrap_or_else(|| "doc".to_string());
        let p =
            pages::core::create(&conn, a.parent_id, a.title, kind).map_err(|e| e.to_string())?;
        Self::signal(&conn, std::slice::from_ref(&p.id));
        json(&p)
    }

    #[tool(
        description = "Replace a page's document body with BlockNote JSON. Requires write access."
    )]
    async fn update_document(
        &self,
        Parameters(a): Parameters<UpdateDocArgs>,
    ) -> Result<String, String> {
        let conn = self.conn()?;
        self.require_write(&conn)?;
        documents::core::update(&conn, &a.id, &a.content).map_err(|e| e.to_string())?;
        // Keep a linked vault in step with an agent's edit, exactly as the app does.
        tidy_core::vault::export::flush_if_configured(&conn, &a.id);
        Self::signal(&conn, std::slice::from_ref(&a.id));
        Ok("ok".to_string())
    }

    #[tool(description = "Add a field to a database. Requires write access.")]
    async fn create_field(
        &self,
        Parameters(a): Parameters<CreateFieldArgs>,
    ) -> Result<String, String> {
        let conn = self.conn()?;
        self.require_write(&conn)?;
        let f = databases::core::create_field(&conn, &a.database_id, &a.name, &a.kind, a.options)
            .map_err(|e| e.to_string())?;
        Self::signal(&conn, &page_for_database(&conn, &a.database_id));
        json(&f)
    }

    #[tool(description = "Add an empty row to a database. Requires write access.")]
    async fn add_row(&self, Parameters(a): Parameters<AddRowArgs>) -> Result<String, String> {
        let conn = self.conn()?;
        self.require_write(&conn)?;
        let r = databases::core::create_row(&conn, &a.database_id).map_err(|e| e.to_string())?;
        Self::signal(&conn, &page_for_database(&conn, &a.database_id));
        json(&r)
    }

    #[tool(description = "Set (or clear, when value is null) a cell. Requires write access.")]
    async fn set_cell(&self, Parameters(a): Parameters<SetCellArgs>) -> Result<String, String> {
        let conn = self.conn()?;
        self.require_write(&conn)?;
        databases::core::set_cell(&conn, &a.row_id, &a.field_id, &a.value)
            .map_err(|e| e.to_string())?;
        Self::signal(&conn, &page_for_row(&conn, &a.row_id));
        Ok("ok".to_string())
    }

    #[tool(
        description = "Flagship: file raw text (meeting notes, an email, a transcript) as a record page \
                       under the matched or created client and link it. The note is saved at once with \
                       its original text (summary_pending is true); Tidy's local AI then adds a checked \
                       summary and, with task_db_id, a task row for each next step, while Tidy is open. \
                       Requires write access and local AI turned on in Tidy's Settings."
    )]
    async fn ingest_note(&self, Parameters(a): Parameters<IngestArgs>) -> Result<String, String> {
        {
            let conn = self.conn()?;
            self.require_write(&conn)?;
        }
        let args = ingest::IngestArgs {
            raw_text: a.raw_text,
            client_hint: a.client_hint,
            meeting_id: a.meeting_id,
            task_db_id: a.task_db_id,
            title: a.title,
            // The MCP surface takes raw text only; the recorder uses body_json.
            body_json: None,
            action_items: None,
        };
        // ingest_note mirrors the new pages to a linked vault itself, so an
        // agent-filed note is not left permanently dirty.
        let r = ingest::ingest_note(&self.db, args)
            .await
            .map_err(|e| e.to_string())?;
        {
            let conn = self.conn()?;
            let mut touched = vec![r.page_id.clone()];
            touched.extend(r.client_page_id.clone());
            Self::signal(&conn, &touched);
        }
        json(&r)
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for TidyServer {
    fn get_info(&self) -> ServerInfo {
        let mode = if self.writes_enabled {
            "read-write"
        } else {
            "read-only"
        };
        let mut info = ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(format!(
                "Tidy local knowledge base ({mode}). Read with kb_search / list_pages / get_page / \
                 get_backlinks / list_databases / get_database. File a note with ingest_note. Write tools \
                 require TIDY_MCP_TOKEN to match the token minted in Tidy Settings."
            ));
        info.server_info.name = "tidy-mcp".to_string();
        info.server_info.version = env!("CARGO_PKG_VERSION").to_string();
        info.server_info.title = Some("Tidy".to_string());
        info
    }
}

// ---- entrypoint ------------------------------------------------------------

/// Resolve the Tidy SQLite index path (env override, else the macOS app dir).
fn db_path() -> tidy_core::error::AppResult<PathBuf> {
    let previous = tidy_core::installation::previous()?;
    if let Some(path) = std::env::var("TIDY_DB")
        .ok()
        .or_else(|| std::env::var(previous.database_environment).ok())
        .filter(|p| !p.is_empty())
    {
        return Ok(PathBuf::from(path));
    }
    let home = std::env::var("HOME")
        .map_err(|_| tidy_core::error::AppError::Other("HOME is unavailable".into()))?;
    let preferred = PathBuf::from(home)
        .join("Library/Application Support")
        .join(tidy_core::installation::DATA_DIRECTORY);
    tidy_core::installation::database_path(&tidy_core::installation::data_directory(&preferred)?)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = db_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let db = Db::open(&path)?;

    // Write gate: enabled ONLY when the env token matches the token minted by
    // the app (setting('mcp_token')). Revoking writes in the app deletes that
    // setting, and the token is re-read on every write, so revoking takes
    // effect at once and a stale env token can never re-enable writes.
    let env_token = std::env::var("TIDY_MCP_TOKEN")
        .ok()
        .or_else(|| {
            tidy_core::installation::previous()
                .ok()
                .and_then(|p| std::env::var(p.token_environment).ok())
        })
        .filter(|s| !s.is_empty());

    let server = TidyServer::new(Arc::new(db), env_token);
    let service = server.serve(rmcp::transport::stdio()).await?;
    service.waiting().await?;
    Ok(())
}
