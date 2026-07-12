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
