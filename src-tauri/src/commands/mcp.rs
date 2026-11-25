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
    /// The same for Codex.
    pub codex_command: String,
    /// Starts Claude Code with the meeting being recorded pushed to it, on a
    /// cheaper model. Channels are a research preview, so Claude Code asks
    /// to confirm the development flag each time.
    pub claude_live_command: String,
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

/// Claude Code following the meeting being recorded: a cheaper model, none
/// of its own tools and no MCP server but Tidy's, so nothing said in the
/// meeting can get it to run commands on this Mac, with Tidy loaded as a
/// channel so new lines are pushed to it.
fn claude_live_command(sidecar: &str, token: &str) -> String {
    let config = serde_json::json!({
        "mcpServers": { "tidy": { "type": "stdio", "command": sidecar, "env": { "TIDY_MCP_TOKEN": token } } }
    });
    // Quoted for the shell: a single quote ends the quoted text, so it is
    // written as '\''.
    let quoted = format!("'{}'", config.to_string().replace('\'', "'\\''"));
    format!(
        "claude --model haiku --tools \"\" --strict-mcp-config --mcp-config {quoted} --dangerously-load-development-channels server:tidy"
    )
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
    let codex_command = format!("codex mcp add tidy --env TIDY_MCP_TOKEN={token} -- \"{sidecar}\"");
    let claude_live_command = claude_live_command(&sidecar, &token);
    Ok(McpInfo {
        token,
        sidecar_path: sidecar,
        claude_command,
        codex_command,
        claude_live_command,
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

#[cfg(test)]
mod tests {
    #[test]
    fn claude_follows_the_meeting_with_only_tidys_tools() {
        let command = super::claude_live_command("/Applications/Tidy.app/Contents/MacOS/tidy-mcp", "tok");
        assert!(command.starts_with("claude --model haiku --tools \"\" --strict-mcp-config --mcp-config '"));
        assert!(command.ends_with("--dangerously-load-development-channels server:tidy"));
        let config = command.split('\'').nth(1).unwrap();
        let config: serde_json::Value = serde_json::from_str(config).unwrap();
        assert_eq!(config["mcpServers"]["tidy"]["env"]["TIDY_MCP_TOKEN"], "tok");
        // A quote in the path cannot end the shell quoting early.
        let odd = super::claude_live_command("/Users/o'neil/tidy-mcp", "tok");
        assert!(odd.contains("o'\\''neil"));
    }
}
