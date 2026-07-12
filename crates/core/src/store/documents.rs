use rusqlite::{params, Connection, OptionalExtension};

use crate::db::now_ms;
use crate::error::{AppError, AppResult};

pub mod core {
    use super::*;

    pub fn get(conn: &Connection, id: &str) -> AppResult<String> {
        let content: Option<Option<String>> = conn
            .query_row("SELECT content FROM page WHERE id = ?1", params![id], |r| {
                r.get(0)
            })
            .optional()?;
        match content {
            Some(c) => Ok(c.unwrap_or_else(|| "[]".to_string())),
            None => Err(AppError::NotFound(format!("page {id}"))),
        }
    }

    pub fn update(conn: &Connection, id: &str, content: &str) -> AppResult<()> {
        // validate it's JSON before persisting
        serde_json::from_str::<serde_json::Value>(content)?;
        // dirty = 1: content changed, the vault file is now out of sync until
        // the next flush (which clears it). Used for vault conflict detection.
        let n = conn.execute(
            "UPDATE page SET content = ?2, updated_at = ?3, dirty = 1 WHERE id = ?1",
            params![id, content, now_ms()],
        )?;
        if n == 0 {
            return Err(AppError::NotFound(format!("page {id}")));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::core::*;
    use crate::db::Db;
    use crate::store::pages;

    #[test]
    fn document_roundtrip() {
        let db = Db::open_in_memory().unwrap();
        let conn = db.conn.lock().unwrap();
        let p = pages::core::create(&conn, None, "Doc".into(), "doc".into()).unwrap();
        assert_eq!(get(&conn, &p.id).unwrap(), "[]");
        let blocks = r#"[{"type":"paragraph","content":"hi"}]"#;
        update(&conn, &p.id, blocks).unwrap();
        assert_eq!(get(&conn, &p.id).unwrap(), blocks);
    }

    #[test]
    fn rejects_invalid_json() {
        let db = Db::open_in_memory().unwrap();
        let conn = db.conn.lock().unwrap();
        let p = pages::core::create(&conn, None, "Doc".into(), "doc".into()).unwrap();
        assert!(update(&conn, &p.id, "not json").is_err());
    }
}
