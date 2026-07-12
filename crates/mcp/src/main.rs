//! AppFlower MCP sidecar.
//!
//! A stdio MCP server that exposes the local AppFlower knowledge base to agents
//! (Claude Code, Codex). It opens the SAME SQLite index as the desktop app (a
//! second WAL writer with a busy_timeout) and calls the exact `appflower_core`
//! store functions the GUI uses — one source of truth, zero duplication.
//!
//! Read tools are always available. Write tools (create/update/ingest) are gated
//! behind a per-install token: they only work when `APPFLOWER_MCP_TOKEN` matches
//! `setting('mcp_token')` minted by the app (or when no token has been minted yet
//! and the env var is set). Everything is soft, additive, and local.

use std::path::PathBuf;
use std::sync::Arc;

use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router, ServerHandler, ServiceExt,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use appflower_core::db::{self, Db};
use appflower_core::store::{databases, documents, ingest, knowledge, pages, search};

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
    /// The raw text to summarize and file (a meeting transcript, notes, etc.).
    raw_text: String,
    /// Optional client/company name — an existing page is reused, else created.
    client_hint: Option<String>,
    /// Optional meeting page id to link the note back to.
    meeting_id: Option<String>,
    /// Optional Tasks database id; each action item becomes a row there.
    task_db_id: Option<String>,
    /// Optional title for the note page.
    title: Option<String>,
}

// ---- server ----------------------------------------------------------------

#[derive(Clone)]
struct AppFlowerServer {
    db: Arc<Db>,
    writes_enabled: bool,
    tool_router: ToolRouter<Self>,
}

fn json<T: Serialize>(v: &T) -> Result<String, String> {
    serde_json::to_string_pretty(v).map_err(|e| e.to_string())
}

#[derive(Serialize)]
struct PageWithBody {
    page: pages::Page,
    body: String,
}

#[tool_router(router = tool_router)]
impl AppFlowerServer {
    fn new(db: Arc<Db>, writes_enabled: bool) -> Self {
        Self {
            db,
            writes_enabled,
            tool_router: Self::tool_router(),
        }
    }

    fn require_write(&self) -> Result<(), String> {
        if self.writes_enabled {
            Ok(())
        } else {
            Err("write tools are disabled — set APPFLOWER_MCP_TOKEN to the token \
                 minted in AppFlower → Settings → Connections to enable them"
                .into())
        }
    }

    // ---- read tools (always available) ----

    #[tool(description = "Full-text search the knowledge base (page titles, bodies, and text cells).")]
    async fn kb_search(&self, Parameters(a): Parameters<SearchArgs>) -> Result<String, String> {
        let conn = self.db.conn.lock().unwrap();
        let r = search::core::search(&conn, &a.query).map_err(|e| e.to_string())?;
        json(&r)
    }

    #[tool(description = "List every page in the workspace (id, title, type, parent).")]
    async fn list_pages(&self) -> Result<String, String> {
        let conn = self.db.conn.lock().unwrap();
        let r = pages::core::list(&conn).map_err(|e| e.to_string())?;
        json(&r)
    }

    #[tool(description = "Get a page's metadata and its document body (BlockNote JSON).")]
    async fn get_page(&self, Parameters(a): Parameters<IdArgs>) -> Result<String, String> {
        let conn = self.db.conn.lock().unwrap();
        let page = pages::core::read(&conn, &a.id).map_err(|e| e.to_string())?;
        let body = documents::core::get(&conn, &a.id).map_err(|e| e.to_string())?;
        json(&PageWithBody { page, body })
    }

    #[tool(description = "List the pages that link to a given page (backlinks).")]
    async fn get_backlinks(&self, Parameters(a): Parameters<PageIdArgs>) -> Result<String, String> {
        let conn = self.db.conn.lock().unwrap();
        let r = knowledge::core::get_backlinks(&conn, &a.page_id).map_err(|e| e.to_string())?;
        json(&r)
    }

    #[tool(description = "List all databases with their fields (for picking a table to write to).")]
    async fn list_databases(&self) -> Result<String, String> {
        let conn = self.db.conn.lock().unwrap();
        let r = databases::core::list_databases(&conn).map_err(|e| e.to_string())?;
        json(&r)
    }

    #[tool(description = "Get a database's full bundle (fields, rows, views) for a database page id.")]
    async fn get_database(&self, Parameters(a): Parameters<PageIdArgs>) -> Result<String, String> {
        let conn = self.db.conn.lock().unwrap();
        let r = databases::core::bundle(&conn, &a.page_id).map_err(|e| e.to_string())?;
        json(&r)
    }

    // ---- write tools (gated) ----

    #[tool(description = "Create a new page ('doc' or 'database'). Requires write access.")]
    async fn create_page(&self, Parameters(a): Parameters<CreatePageArgs>) -> Result<String, String> {
        self.require_write()?;
        let conn = self.db.conn.lock().unwrap();
        let kind = a.kind.unwrap_or_else(|| "doc".to_string());
        let p = pages::core::create(&conn, a.parent_id, a.title, kind).map_err(|e| e.to_string())?;
        json(&p)
    }

    #[tool(description = "Replace a page's document body with BlockNote JSON. Requires write access.")]
    async fn update_document(&self, Parameters(a): Parameters<UpdateDocArgs>) -> Result<String, String> {
        self.require_write()?;
        let conn = self.db.conn.lock().unwrap();
        documents::core::update(&conn, &a.id, &a.content).map_err(|e| e.to_string())?;
        Ok("ok".to_string())
    }

    #[tool(description = "Add a field to a database. Requires write access.")]
    async fn create_field(&self, Parameters(a): Parameters<CreateFieldArgs>) -> Result<String, String> {
        self.require_write()?;
        let conn = self.db.conn.lock().unwrap();
        let f = databases::core::create_field(&conn, &a.database_id, &a.name, &a.kind, a.options)
            .map_err(|e| e.to_string())?;
        json(&f)
    }

    #[tool(description = "Add an empty row to a database. Requires write access.")]
    async fn add_row(&self, Parameters(a): Parameters<AddRowArgs>) -> Result<String, String> {
        self.require_write()?;
        let conn = self.db.conn.lock().unwrap();
        let r = databases::core::create_row(&conn, &a.database_id).map_err(|e| e.to_string())?;
        json(&r)
    }

    #[tool(description = "Set (or clear, when value is null) a cell. Requires write access.")]
    async fn set_cell(&self, Parameters(a): Parameters<SetCellArgs>) -> Result<String, String> {
        self.require_write()?;
        let conn = self.db.conn.lock().unwrap();
        databases::core::set_cell(&conn, &a.row_id, &a.field_id, &a.value).map_err(|e| e.to_string())?;
        Ok("ok".to_string())
    }

    #[tool(
        description = "Flagship: summarize a raw blob (meeting/notes), file it as a record page under \
                       the matched/created client, link it, and create Tasks rows for each action item. \
                       Requires write access and a running local Ollama."
    )]
    async fn ingest_note(&self, Parameters(a): Parameters<IngestArgs>) -> Result<String, String> {
        self.require_write()?;
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
        let r = ingest::ingest_note(&self.db, args)
            .await
            .map_err(|e| e.to_string())?;
        json(&r)
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for AppFlowerServer {
    fn get_info(&self) -> ServerInfo {
        let mode = if self.writes_enabled { "read-write" } else { "read-only" };
        let mut info = ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(format!(
                "AppFlower local knowledge base ({mode}). Read with kb_search / list_pages / get_page / \
                 get_backlinks / list_databases / get_database. File a note with ingest_note. Write tools \
                 require APPFLOWER_MCP_TOKEN to match the token minted in AppFlower Settings."
            ));
        info.server_info.name = "appflower-mcp".to_string();
        info.server_info.version = env!("CARGO_PKG_VERSION").to_string();
        info.server_info.title = Some("AppFlower".to_string());
        info
    }
}

// ---- entrypoint ------------------------------------------------------------

/// Resolve the AppFlower SQLite index path (env override, else the macOS app dir).
fn db_path() -> PathBuf {
    if let Ok(p) = std::env::var("APPFLOWER_DB") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home).join("Library/Application Support/com.appflower.app/appflower.db")
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = db_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let db = Db::open(&path)?;

    // Write gate: enabled ONLY when the env token matches the token minted by
    // the app (setting('mcp_token')). Revoking writes in the app deletes that
    // setting, so a stale env token can never re-enable writes.
    let writes_enabled = {
        let conn = db.conn.lock().unwrap();
        let stored = db::get_setting(&conn, "mcp_token").ok().flatten();
        let env = std::env::var("APPFLOWER_MCP_TOKEN")
            .ok()
            .filter(|s| !s.is_empty());
        match (env, stored) {
            (Some(e), Some(s)) => e == s,
            _ => false,
        }
    };

    let server = AppFlowerServer::new(Arc::new(db), writes_enabled);
    let service = server.serve(rmcp::transport::stdio()).await?;
    service.waiting().await?;
    Ok(())
}
