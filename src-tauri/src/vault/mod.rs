//! The hybrid-vault runtime: mirror pages to portable Markdown on disk and pull
//! external edits back in. The SQLite index stays canonical; the `.md` files are
//! a best-effort, Obsidian-friendly mirror.
//!
//! App → file: atomic writes (tmp → rename) journaled by content hash.
//! File → app: a debounced recursive watcher that reconciles by frontmatter id,
//! skipping the app's own writes via the hash journal (so there's no loop).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use notify::{RecursiveMode, Watcher};
use notify_debouncer_full::{new_debouncer, DebouncedEvent, Debouncer, FileIdMap};
use tauri::{AppHandle, Emitter, Manager};

use crate::db::{self, Db};
use crate::error::{AppError, AppResult};
use crate::workspace::run_db;
use tidy_core::vault::export;

/// Holds the live filesystem watcher (dropped/replaced when the vault changes).
#[derive(Default)]
pub struct VaultState(pub Mutex<Option<Debouncer<notify::RecommendedWatcher, FileIdMap>>>);

fn blake3_hex(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

/// Atomic write: write a temp sibling, then rename over the target.
fn atomic_write(path: &Path, content: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("md.tmp");
    fs::write(&tmp, content)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

/// Remove a page's vault file (best-effort) — called when a page is deleted,
/// while its `vault_path` is still readable.
pub fn remove_page_file(conn: &rusqlite::Connection, page_id: &str) {
    export::remove_page_file(conn, page_id);
}

/// Export every note-type page to the vault (one-way, full).
fn export_all(conn: &rusqlite::Connection, vault: &Path) -> AppResult<usize> {
    let ids = export::exportable_page_ids(conn)?;
    let mut n = 0;
    for id in &ids {
        export::write_page_file(conn, vault, id)?;
        n += 1;
    }
    Ok(n)
}

/// Handle a batch of debounced filesystem events (file → app reconcile).
fn handle_events(app: &AppHandle, vault: &Path, events: Vec<DebouncedEvent>) {
    let db = app.state::<Db>();
    let mut touched = false;
    // Pages whose body this batch rewrote underneath the editor. The open
    // document has to reload or the next in-app save would clobber the file.
    let mut rewritten: Vec<String> = Vec::new();
    for ev in events {
        for path in ev.event.paths.iter() {
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            // ignore the internal index dir + temp files
            let s = path.to_string_lossy();
            if s.contains("/.tidy/") || s.ends_with(".md.tmp") || s.contains(".conflict-") {
                continue;
            }
            let rel = match path.strip_prefix(vault) {
                Ok(r) => r.to_string_lossy().replace('\\', "/"),
                Err(_) => continue,
            };
            // Hold the DB lock across read+hash+echo-check+apply so a concurrent
            // app write can't slip a new journal hash in between (echo race).
            let conn = match db.lock() {
                Ok(conn) => conn,
                Err(e) => {
                    // Skip this file, but still report whatever the batch has
                    // already applied.
                    log::warn!("vault import skipped, database unavailable: {e}");
                    continue;
                }
            };
            let text = match fs::read_to_string(path) {
                Ok(t) => t,
                Err(_) => continue, // deleted/unreadable — soft-ignore for now
            };
            let hash = blake3_hex(text.as_bytes());
            // Echo check: if this is exactly what the app last wrote, skip.
            if let Ok(Some(expected)) = db::sync_expected(&conn, &rel) {
                if expected == hash {
                    continue;
                }
            }
            match export::apply_external_markdown(&conn, &text) {
                Ok(export::ApplyResult::Updated(id)) => {
                    // record the new external hash so the next app write is diffable
                    let _ = db::journal_sync(&conn, &rel, &hash, "file_write");
                    touched = true;
                    rewritten.push(id);
                }
                Ok(export::ApplyResult::Conflict(id)) => {
                    // The app has unflushed changes to this page. Write the
                    // external version to a sidecar (never lose either side) and
                    // leave the DB untouched.
                    let ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    let base = rel.strip_suffix(".md").unwrap_or(&rel);
                    let conflict_rel = format!("{base}.conflict-{ts}.md");
                    let abs = vault.join(&conflict_rel);
                    if atomic_write(&abs, &text).is_ok() {
                        // journal as an app write so we don't re-import the sidecar
                        let _ = db::journal_sync(
                            &conn,
                            &conflict_rel,
                            &blake3_hex(text.as_bytes()),
                            "app_write",
                        );
                        log::warn!("vault conflict on {rel}: wrote {conflict_rel}");
                    }
                    // The editor still needs to know the file diverged, so it
                    // can offer the sidecar rather than silently winning.
                    rewritten.push(id);
                }
                Ok(export::ApplyResult::Unmatched) | Err(_) => {}
            }
        }
    }
    if touched {
        let _ = app.emit("pages-changed", ());
    }
    for page_id in rewritten {
        let _ = app.emit("document-changed", serde_json::json!({ "page_id": page_id }));
    }
}

/// Start (or restart) the recursive watcher over `<vault>/notes`.
fn start_watcher(app: &AppHandle, vault: &Path) -> AppResult<()> {
    // Canonicalize so `strip_prefix` matches the real (symlink-resolved) paths
    // that `notify` reports; otherwise every event would be dropped.
    let vault = std::fs::canonicalize(vault).unwrap_or_else(|_| vault.to_path_buf());
    let notes = vault.join("notes");
    fs::create_dir_all(&notes).map_err(AppError::Io)?;

    let app_for_cb = app.clone();
    let vault_owned = vault.clone();
    let mut debouncer = new_debouncer(
        Duration::from_millis(400),
        None,
        move |res: notify_debouncer_full::DebounceEventResult| {
            if let Ok(events) = res {
                handle_events(&app_for_cb, &vault_owned, events);
            }
        },
    )
    .map_err(|e| AppError::Other(format!("watcher init: {e}")))?;

    debouncer
        .watcher()
        .watch(&notes, RecursiveMode::Recursive)
        .map_err(|e| AppError::Other(format!("watch: {e}")))?;
    debouncer.cache().add_root(&notes, RecursiveMode::Recursive);

    let state = app.state::<VaultState>();
    match state.0.lock() {
        Ok(mut slot) => *slot = Some(debouncer),
        Err(poisoned) => *poisoned.into_inner() = Some(debouncer),
    }
    Ok(())
}

fn vault_root(conn: &rusqlite::Connection) -> AppResult<Option<PathBuf>> {
    Ok(db::get_setting(conn, "vault_dir")?.map(PathBuf::from))
}

// ---- commands --------------------------------------------------------------

/// Choose (or change) the vault directory: persist it, export everything once,
/// and start watching for external edits.
#[tauri::command]
pub async fn set_vault_dir(app: AppHandle, path: String) -> AppResult<usize> {
    let vault = PathBuf::from(&path);
    if !vault.is_dir() {
        return Err(AppError::Invalid(format!("not a directory: {path}")));
    }
    // Exporting a whole workspace writes a file per page, so it has to happen
    // off the UI thread.
    let exported = {
        let path = path.clone();
        let vault = vault.clone();
        run_db(&app, move |conn| {
            db::set_setting(conn, "vault_dir", &path)?;
            export_all(conn, &vault)
        })
        .await?
    };
    start_watcher(&app, &vault)?;
    let _ = app.emit("vault-changed", &path);
    Ok(exported)
}

/// The configured vault directory, if any.
#[tauri::command]
pub async fn get_vault_dir(app: AppHandle) -> AppResult<Option<String>> {
    run_db(&app, |conn| db::get_setting(conn, "vault_dir")).await
}

/// Re-export every page to the vault (manual full sync).
#[tauri::command]
pub async fn export_vault(app: AppHandle) -> AppResult<usize> {
    run_db(&app, |conn| {
        let vault =
            vault_root(conn)?.ok_or_else(|| AppError::Invalid("no vault configured".into()))?;
        export_all(conn, &vault)
    })
    .await
}

/// Flush a single page to its vault file (called after edits when a vault is set).
#[tauri::command]
pub async fn flush_page(app: AppHandle, page_id: String) -> AppResult<()> {
    run_db(&app, move |conn| {
        if let Some(vault) = vault_root(conn)? {
            export::write_page_file(conn, &vault, &page_id)?;
        }
        Ok(())
    })
    .await
}

/// Auto-flush a page to the vault after an in-app edit — best-effort, no-op when
/// no vault is configured. Also clears the page's `dirty` flag, keeping the file
/// in sync so the vault is truly two-way in real time. Call while already
/// holding the connection. Shared with the MCP sidecar through the core crate.
pub fn flush_if_configured(conn: &rusqlite::Connection, page_id: &str) {
    export::flush_if_configured(conn, page_id);
}

/// Called at startup: if a vault is already configured, resume watching it.
pub fn resume_if_configured(app: &AppHandle) {
    let vault = {
        let db = app.state::<Db>();
        let root = match db.lock() {
            Ok(conn) => vault_root(&conn).ok().flatten(),
            Err(e) => {
                log::warn!("vault watcher could not resume: {e}");
                None
            }
        };
        root
    };
    if let Some(vault) = vault {
        if vault.is_dir() {
            if let Err(e) = start_watcher(app, &vault) {
                log::warn!("vault watcher failed to resume: {e}");
            }
        }
    }
}
