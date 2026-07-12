//! MCP connection management: mint the per-install write token and resolve the
//! bundled sidecar path, so Settings can show a one-click registration command.

use serde::Serialize;
use tauri::State;

use crate::db::{self, Db};
use crate::error::AppResult;

#[derive(Serialize)]
pub struct McpInfo {
    /// The write-access token (minted on demand).
    pub token: String,
    /// Absolute path to the bundled sidecar binary (for `claude mcp add`).
    pub sidecar_path: String,
    /// A ready-to-paste `claude mcp add` command.
    pub claude_command: String,
}

/// Best-effort path to the sidecar next to the running executable (in a bundled
/// .app it sits in `Contents/MacOS/appflower-mcp`).
fn sidecar_path() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|p| p.join("appflower-mcp")))
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| "appflower-mcp".to_string())
}

/// The existing token, or `None` if the user hasn't enabled writes yet.
#[tauri::command]
pub fn mcp_get_token(db: State<Db>) -> AppResult<Option<String>> {
    let conn = db.conn.lock().unwrap();
    db::get_setting(&conn, "mcp_token")
}

/// Mint (or return the existing) write token and hand back registration details.
#[tauri::command]
pub fn mcp_enable(db: State<Db>) -> AppResult<McpInfo> {
    let conn = db.conn.lock().unwrap();
    let token = match db::get_setting(&conn, "mcp_token")? {
        Some(t) => t,
        None => {
            let t = db::new_id();
            db::set_setting(&conn, "mcp_token", &t)?;
            t
        }
    };
    let sidecar = sidecar_path();
    let claude_command = format!(
        "claude mcp add --scope user --transport stdio --env APPFLOWER_MCP_TOKEN={token} appflower -- \"{sidecar}\""
    );
    Ok(McpInfo {
        token,
        sidecar_path: sidecar,
        claude_command,
    })
}

/// Revoke write access (removes the token; read tools keep working).
#[tauri::command]
pub fn mcp_disable(db: State<Db>) -> AppResult<()> {
    let conn = db.conn.lock().unwrap();
    conn.execute("DELETE FROM setting WHERE key = 'mcp_token'", [])?;
    Ok(())
}
