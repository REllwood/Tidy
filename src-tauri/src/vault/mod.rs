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
use tauri::{AppHandle, Emitter, Manager, State};

use crate::db::{self, Db};
use crate::error::{AppError, AppResult};
use appflower_core::vault::export;

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

/// Write one page's Markdown file into the vault, journaling the hash first so
/// the watcher recognizes (and skips) the resulting change event.
fn write_page_file(conn: &rusqlite::Connection, vault: &Path, page_id: &str) -> AppResult<()> {
    use rusqlite::OptionalExtension;
    if let Some(file) = export::render_page_file(conn, page_id)? {
        // Prior path — so a title change (new slug) doesn't orphan the old file.
        let old_path: Option<String> = conn
            .query_row(
                "SELECT vault_path FROM page WHERE id = ?1",
                rusqlite::params![page_id],
                |r| r.get(0),
            )
            .optional()?
            .flatten();

        let hash = blake3_hex(file.content.as_bytes());
        db::journal_sync(conn, &file.rel_path, &hash, "app_write")?;
        let abs = vault.join(&file.rel_path);
        atomic_write(&abs, &file.content).map_err(AppError::Io)?;
        conn.execute(
            "UPDATE page SET vault_path = ?2, body_hash = ?3, dirty = 0 WHERE id = ?1",
            rusqlite::params![page_id, file.rel_path, hash],
        )?;

        if let Some(old) = old_path {
            if old != file.rel_path {
                let _ = fs::remove_file(vault.join(&old));
            }
        }
    }
    Ok(())
}

/// Remove a page's vault file (best-effort) — called when a page is deleted.
pub fn remove_page_file(app: &AppHandle, page_id: &str) {
    let db = app.state::<Db>();
    let conn = db.conn.lock().unwrap();
    let vault = match vault_root(&conn) {
        Ok(Some(v)) => v,
        _ => return,
    };
    use rusqlite::OptionalExtension;
    let rel: Option<String> = conn
        .query_row(
            "SELECT vault_path FROM page WHERE id = ?1",
            rusqlite::params![page_id],
            |r| r.get(0),
        )
        .optional()
        .ok()
        .flatten()
        .flatten();
    if let Some(rel) = rel {
        let _ = db::journal_sync(&conn, &rel, "", "app_delete");
        let _ = fs::remove_file(vault.join(&rel));
    }
}

/// Export every note-type page to the vault (one-way, full).
fn export_all(conn: &rusqlite::Connection, vault: &Path) -> AppResult<usize> {
    let ids = export::exportable_page_ids(conn)?;
    let mut n = 0;
    for id in &ids {
        write_page_file(conn, vault, id)?;
        n += 1;
    }
    Ok(n)
}

/// Handle a batch of debounced filesystem events (file → app reconcile).
fn handle_events(app: &AppHandle, vault: &Path, events: Vec<DebouncedEvent>) {
    let db = app.state::<Db>();
    let mut touched = false;
    for ev in events {
        for path in ev.event.paths.iter() {
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            // ignore the internal index dir + temp files
            let s = path.to_string_lossy();
            if s.contains("/.appflower/") || s.ends_with(".md.tmp") || s.contains(".conflict-") {
                continue;
            }
            let rel = match path.strip_prefix(vault) {
                Ok(r) => r.to_string_lossy().replace('\\', "/"),
                Err(_) => continue,
            };
            // Hold the DB lock across read+hash+echo-check+apply so a concurrent
            // app write can't slip a new journal hash in between (echo race).
            let conn = db.conn.lock().unwrap();
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
                Ok(export::ApplyResult::Updated(_id)) => {
                    // record the new external hash so the next app write is diffable
                    let _ = db::journal_sync(&conn, &rel, &hash, "file_write");
                    touched = true;
                }
                Ok(export::ApplyResult::Conflict(_id)) => {
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
                }
                Ok(export::ApplyResult::Unmatched) | Err(_) => {}
            }
        }
    }
    if touched {
        let _ = app.emit("pages-changed", ());
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
    *state.0.lock().unwrap() = Some(debouncer);
    Ok(())
}

fn vault_root(conn: &rusqlite::Connection) -> AppResult<Option<PathBuf>> {
    Ok(db::get_setting(conn, "vault_dir")?.map(PathBuf::from))
}

// ---- commands --------------------------------------------------------------

/// Choose (or change) the vault directory: persist it, export everything once,
/// and start watching for external edits.
#[tauri::command]
pub fn set_vault_dir(app: AppHandle, db: State<Db>, path: String) -> AppResult<usize> {
    let vault = PathBuf::from(&path);
    if !vault.is_dir() {
        return Err(AppError::Invalid(format!("not a directory: {path}")));
    }
    let n = {
        let conn = db.conn.lock().unwrap();
        db::set_setting(&conn, "vault_dir", &path)?;
        export_all(&conn, &vault)?
    };
    start_watcher(&app, &vault)?;
    let _ = app.emit("vault-changed", &path);
    Ok(n)
}

/// The configured vault directory, if any.
#[tauri::command]
pub fn get_vault_dir(db: State<Db>) -> AppResult<Option<String>> {
    let conn = db.conn.lock().unwrap();
    Ok(db::get_setting(&conn, "vault_dir")?)
}

/// Re-export every page to the vault (manual full sync).
#[tauri::command]
pub fn export_vault(db: State<Db>) -> AppResult<usize> {
    let conn = db.conn.lock().unwrap();
    let vault = vault_root(&conn)?
        .ok_or_else(|| AppError::Invalid("no vault configured".into()))?;
    export_all(&conn, &vault)
}

/// Flush a single page to its vault file (called after edits when a vault is set).
#[tauri::command]
pub fn flush_page(db: State<Db>, page_id: String) -> AppResult<()> {
    let conn = db.conn.lock().unwrap();
    if let Some(vault) = vault_root(&conn)? {
        write_page_file(&conn, &vault, &page_id)?;
    }
    Ok(())
}

/// Auto-flush a page to the vault after an in-app edit — best-effort, no-op when
/// no vault is configured. Also clears the page's `dirty` flag (write_page_file
/// does), keeping the file in sync so the vault is truly two-way in real time.
/// Call while already holding the connection.
pub fn flush_if_configured(conn: &rusqlite::Connection, page_id: &str) {
    if let Ok(Some(vault)) = vault_root(conn) {
        if let Err(e) = write_page_file(conn, &vault, page_id) {
            log::warn!("vault auto-flush failed for {page_id}: {e}");
        }
    }
}

/// Called at startup: if a vault is already configured, resume watching it.
pub fn resume_if_configured(app: &AppHandle) {
    let vault = {
        let db = app.state::<Db>();
        let conn = db.conn.lock().unwrap();
        vault_root(&conn).ok().flatten()
    };
    if let Some(vault) = vault {
        if vault.is_dir() {
            if let Err(e) = start_watcher(app, &vault) {
                log::warn!("vault watcher failed to resume: {e}");
            }
        }
    }
}
