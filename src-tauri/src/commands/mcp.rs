//! MCP connection management: mint the per-install write token and resolve the
//! bundled sidecar path, so Settings can show a one-click registration command.

use serde::Serialize;
use tauri::AppHandle;

use crate::db;
use crate::error::AppResult;
use crate::workspace::run_db;

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
/// .app it sits in `Contents/MacOS/tidy-mcp`).
fn sidecar_path() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|p| p.join("tidy-mcp")))
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| "tidy-mcp".to_string())
}

/// The existing token, or `None` if the user hasn't enabled writes yet.
#[tauri::command]
pub async fn mcp_get_token(app: AppHandle) -> AppResult<Option<String>> {
    run_db(&app, |conn| db::get_setting(conn, "mcp_token")).await
}

/// Mint (or return the existing) write token and hand back registration details.
#[tauri::command]
pub async fn mcp_enable(app: AppHandle) -> AppResult<McpInfo> {
    let token = run_db(&app, |conn| match db::get_setting(conn, "mcp_token")? {
        Some(t) => Ok(t),
        None => {
            let t = db::new_id();
            db::set_setting(conn, "mcp_token", &t)?;
            Ok(t)
        }
    })
    .await?;
    let sidecar = sidecar_path();
    let claude_command = format!(
        "claude mcp add --scope user --transport stdio --env TIDY_MCP_TOKEN={token} tidy -- \"{sidecar}\""
    );
    Ok(McpInfo {
        token,
        sidecar_path: sidecar,
        claude_command,
    })
}

/// Revoke write access (removes the token; read tools keep working). A sidecar
/// that is already running re-reads the token on every write, so this takes
/// effect straight away.
#[tauri::command]
pub async fn mcp_disable(app: AppHandle) -> AppResult<()> {
    run_db(&app, |conn| {
        conn.execute("DELETE FROM setting WHERE key = 'mcp_token'", [])?;
        Ok(())
    })
    .await
}
