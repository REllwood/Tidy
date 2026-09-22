pub mod migrations;

use std::path::Path;
use std::sync::Mutex;

use rusqlite::Connection;

use crate::error::AppResult;

/// Owns the single SQLite connection for the app. The Rust core is the sole
/// writer (audio/whisper + UI both go through here), so one guarded connection
/// in WAL mode is sufficient and avoids lock contention.
pub struct Db {
    pub conn: Mutex<Connection>,
}

impl Db {
    /// Lock the connection without panicking on a poisoned mutex. A panic on
    /// one thread must not take the whole app down with it, so a poisoned lock
    /// is recovered (SQLite state is transactional and stays consistent) and the
    /// caller gets a usable guard.
    pub fn lock(&self) -> AppResult<std::sync::MutexGuard<'_, Connection>> {
        Ok(self.conn.lock().unwrap_or_else(|poisoned| poisoned.into_inner()))
    }

    /// Open (creating if needed) the database at `path`, set pragmas, and run
    /// migrations.
    pub fn open(path: &Path) -> AppResult<Self> {
        let conn = Connection::open(path)?;
        Self::init(conn)
    }

    /// In-memory database — used by tests.
    #[cfg(test)]
    pub fn open_in_memory() -> AppResult<Self> {
        let conn = Connection::open_in_memory()?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> AppResult<Self> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        // Second-writer friendliness: the MCP sidecar opens the same file while
        // the GUI is running, so wait rather than fail on a momentary lock.
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        let mut conn = conn;
        migrations::migrations().to_latest(&mut conn)?;
        Ok(Db {
            conn: Mutex::new(conn),
        })
    }
}

/// Run `f` inside a transaction, committing on success and rolling back on
/// error. When the connection is already inside a transaction (a caller higher
/// up opened one) `f` runs directly — SQLite has no nested transactions, so
/// re-opening one here would fail and the outer transaction already gives the
/// same all-or-nothing guarantee.
pub fn in_transaction<T>(
    conn: &Connection,
    f: impl FnOnce(&Connection) -> AppResult<T>,
) -> AppResult<T> {
    if !conn.is_autocommit() {
        return f(conn);
    }
    let tx = conn.unchecked_transaction()?;
    let out = f(&tx)?;
    tx.commit()?;
    Ok(out)
}

/// Read a value from the `setting` key/value table.
pub fn get_setting(conn: &Connection, key: &str) -> AppResult<Option<String>> {
    use rusqlite::OptionalExtension;
    Ok(conn
        .query_row(
            "SELECT value FROM setting WHERE key = ?1",
            rusqlite::params![key],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten())
}

/// Upsert a value into the `setting` key/value table.
pub fn set_setting(conn: &Connection, key: &str, value: &str) -> AppResult<()> {
    conn.execute(
        "INSERT INTO setting (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![key, value],
    )?;
    Ok(())
}

/// Setting keys the MCP sidecar bumps after a write so the running app can
/// notice and refresh. The app polls `CHANGE_SEQ` and drains `CHANGED_PAGES`.
pub const CHANGE_SEQ_KEY: &str = "mcp_change_seq";
pub const CHANGED_PAGES_KEY: &str = "mcp_changed_pages";

/// Signal that a sidecar write touched `page_ids` (ids may be empty for writes
/// that only changed database rows). Bumps the change sequence so a running app
/// refreshes, and appends the ids (deduped) for per-document refreshes.
pub fn record_change(conn: &Connection, page_ids: &[String]) -> AppResult<()> {
    let seq = change_seq(conn)?;
    set_setting(conn, CHANGE_SEQ_KEY, &(seq + 1).to_string())?;
    if page_ids.is_empty() {
        return Ok(());
    }
    let mut pages = changed_pages(conn)?;
    for id in page_ids {
        if !id.is_empty() && !pages.iter().any(|p| p == id) {
            pages.push(id.clone());
        }
    }
    set_setting(conn, CHANGED_PAGES_KEY, &serde_json::to_string(&pages)?)?;
    Ok(())
}

/// The current sidecar change sequence (0 when nothing has been written yet).
pub fn change_seq(conn: &Connection) -> AppResult<i64> {
    Ok(get_setting(conn, CHANGE_SEQ_KEY)?
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0))
}

fn changed_pages(conn: &Connection) -> AppResult<Vec<String>> {
    Ok(get_setting(conn, CHANGED_PAGES_KEY)?
        .and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
        .unwrap_or_default())
}

/// Read and clear the list of page ids the sidecar has touched.
pub fn take_changed_pages(conn: &Connection) -> AppResult<Vec<String>> {
    let pages = changed_pages(conn)?;
    if !pages.is_empty() {
        set_setting(conn, CHANGED_PAGES_KEY, "[]")?;
    }
    Ok(pages)
}

/// Record that the app just wrote `path` with content hash `expected_hash`.
/// The vault watcher uses this to skip its own echo events (loop-free sync).
pub fn journal_sync(conn: &Connection, path: &str, expected_hash: &str, op: &str) -> AppResult<()> {
    conn.execute(
        "INSERT INTO sync_op (path, expected_hash, op, at) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(path) DO UPDATE SET expected_hash = excluded.expected_hash, op = excluded.op, at = excluded.at",
        rusqlite::params![path, expected_hash, op, now_ms()],
    )?;
    Ok(())
}

/// The last app-written hash for `path`, if any.
pub fn sync_expected(conn: &Connection, path: &str) -> AppResult<Option<String>> {
    use rusqlite::OptionalExtension;
    Ok(conn
        .query_row(
            "SELECT expected_hash FROM sync_op WHERE path = ?1",
            rusqlite::params![path],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten())
}

/// Current unix time in milliseconds.
pub fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// New random id.
pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_and_migrates_in_memory() {
        let db = Db::open_in_memory().expect("open");
        let conn = db.conn.lock().unwrap();
        let v: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert!(v >= 1, "user_version should advance after migration");
        // all v1 tables exist
        for t in [
            "page", "database", "field", "db_row", "cell", "db_view", "model",
            "meeting", "setting", "link", "tag", "page_tag", "sync_op",
        ] {
            let n: i64 = conn
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1",
                    [t],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(n, 1, "table {t} should exist");
        }
    }
}
