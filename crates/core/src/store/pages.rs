use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

use crate::db::{new_id, now_ms};
use crate::error::{AppError, AppResult};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Page {
    pub id: String,
    pub parent_id: Option<String>,
    pub title: String,
    pub icon: Option<String>,
    #[serde(rename = "type")]
    pub kind: String,
    pub position: f64,
    pub is_favorite: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

const PAGE_COLS: &str =
    "id, parent_id, title, icon, type, position, is_favorite, created_at, updated_at";

fn row_to_page(r: &Row) -> rusqlite::Result<Page> {
    Ok(Page {
        id: r.get(0)?,
        parent_id: r.get(1)?,
        title: r.get(2)?,
        icon: r.get(3)?,
        kind: r.get(4)?,
        position: r.get(5)?,
        is_favorite: r.get::<_, i64>(6)? != 0,
        created_at: r.get(7)?,
        updated_at: r.get(8)?,
    })
}

fn next_position(conn: &Connection, parent_id: &Option<String>) -> AppResult<f64> {
    let max: Option<f64> = match parent_id {
        Some(p) => conn
            .query_row(
                "SELECT max(position) FROM page WHERE parent_id = ?1",
                params![p],
                |r| r.get(0),
            )
            .optional()?
            .flatten(),
        None => conn
            .query_row(
                "SELECT max(position) FROM page WHERE parent_id IS NULL",
                [],
                |r| r.get(0),
            )
            .optional()?
            .flatten(),
    };
    Ok(max.unwrap_or(0.0) + 1.0)
}

fn read_page(conn: &Connection, id: &str) -> AppResult<Page> {
    conn.query_row(
        &format!("SELECT {PAGE_COLS} FROM page WHERE id = ?1"),
        params![id],
        row_to_page,
    )
    .optional()?
    .ok_or_else(|| AppError::NotFound(format!("page {id}")))
}

/// Core implementations are split out so they're unit-testable without Tauri,
/// and reusable from the MCP sidecar.
pub mod core {
    use super::*;

    pub fn read(conn: &Connection, id: &str) -> AppResult<Page> {
        read_page(conn, id)
    }

    pub fn create(
        conn: &Connection,
        parent_id: Option<String>,
        title: String,
        kind: String,
    ) -> AppResult<Page> {
        if kind != "doc" && kind != "database" {
            return Err(AppError::Invalid(format!("page type '{kind}'")));
        }
        let id = new_id();
        let now = now_ms();
        let position = next_position(conn, &parent_id)?;
        let content = if kind == "doc" { Some("[]") } else { None };
        // A database page is only useful with its database (schema + views), so
        // both land together or not at all.
        crate::db::in_transaction(conn, |tx| {
            tx.execute(
                "INSERT INTO page (id, parent_id, title, icon, type, position, is_favorite, content, body_text, created_at, updated_at)
                 VALUES (?1, ?2, ?3, NULL, ?4, ?5, 0, ?6, '', ?7, ?7)",
                params![id, parent_id, title, kind, position, content, now],
            )?;
            if kind == "database" {
                crate::store::databases::core::create_for_page(tx, &id)?;
            }
            Ok(())
        })?;
        read_page(conn, &id)
    }

    pub fn list(conn: &Connection) -> AppResult<Vec<Page>> {
        let mut stmt = conn.prepare(&format!(
            "SELECT {PAGE_COLS} FROM page WHERE deleted_at IS NULL ORDER BY position ASC"
        ))?;
        let rows = stmt.query_map([], row_to_page)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn rename(conn: &Connection, id: &str, title: &str) -> AppResult<Page> {
        let n = conn.execute(
            "UPDATE page SET title = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, title, now_ms()],
        )?;
        if n == 0 {
            return Err(AppError::NotFound(format!("page {id}")));
        }
        read_page(conn, id)
    }

    pub fn set_icon(conn: &Connection, id: &str, icon: Option<&str>) -> AppResult<Page> {
        let n = conn.execute(
            "UPDATE page SET icon = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, icon, now_ms()],
        )?;
        if n == 0 {
            return Err(AppError::NotFound(format!("page {id}")));
        }
        read_page(conn, id)
    }

    pub fn set_favorite(conn: &Connection, id: &str, fav: bool) -> AppResult<Page> {
        let n = conn.execute(
            "UPDATE page SET is_favorite = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, fav as i64, now_ms()],
        )?;
        if n == 0 {
            return Err(AppError::NotFound(format!("page {id}")));
        }
        read_page(conn, id)
    }

    pub fn move_page(
        conn: &Connection,
        id: &str,
        parent_id: Option<String>,
        position: f64,
    ) -> AppResult<Page> {
        if let Some(ref p) = parent_id {
            // Walk up from the proposed parent. Meeting the page itself means
            // the new parent is the page or one of its descendants, which would
            // cut the subtree loose into a cycle.
            let cycles: i64 = conn.query_row(
                "WITH RECURSIVE ancestor(id) AS (
                     SELECT ?1
                     UNION
                     SELECT p.parent_id FROM page p JOIN ancestor a ON p.id = a.id
                      WHERE p.parent_id IS NOT NULL
                 )
                 SELECT count(*) FROM ancestor WHERE id = ?2",
                params![p, id],
                |r| r.get(0),
            )?;
            if cycles > 0 {
                return Err(AppError::Invalid("A page cannot be moved inside itself.".into()));
            }
        }
        let n = conn.execute(
            "UPDATE page SET parent_id = ?2, position = ?3, updated_at = ?4 WHERE id = ?1",
            params![id, parent_id, position, now_ms()],
        )?;
        if n == 0 {
            return Err(AppError::NotFound(format!("page {id}")));
        }
        read_page(conn, id)
    }

    /// `id` and every page beneath it, deepest last. Deleting a page cascades
    /// to its whole subtree, so callers that have per-page cleanup to do (vault
    /// files, caches) need the full list *before* the delete.
    pub fn subtree_ids(conn: &Connection, id: &str) -> AppResult<Vec<String>> {
        let mut stmt = conn.prepare(
            "WITH RECURSIVE subtree(id) AS (
                 SELECT id FROM page WHERE id = ?1
                 UNION
                 SELECT p.id FROM page p JOIN subtree s ON p.parent_id = s.id
             )
             SELECT id FROM subtree",
        )?;
        let rows = stmt.query_map(params![id], |r| r.get::<_, String>(0))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn delete(conn: &Connection, id: &str) -> AppResult<()> {
        let n = conn.execute("DELETE FROM page WHERE id = ?1", params![id])?;
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

    #[test]
    fn page_crud_and_cascade() {
        let db = Db::open_in_memory().unwrap();
        let conn = db.conn.lock().unwrap();

        let root = create(&conn, None, "Root".into(), "doc".into()).unwrap();
        let child = create(&conn, Some(root.id.clone()), "Child".into(), "doc".into()).unwrap();
        let _grand =
            create(&conn, Some(child.id.clone()), "Grand".into(), "doc".into()).unwrap();

        let sib = create(&conn, None, "Root2".into(), "doc".into()).unwrap();
        assert!(sib.position > root.position);

        assert_eq!(list(&conn).unwrap().len(), 4);

        let r = rename(&conn, &root.id, "Renamed").unwrap();
        assert_eq!(r.title, "Renamed");
        assert!(set_favorite(&conn, &root.id, true).unwrap().is_favorite);
        assert_eq!(
            set_icon(&conn, &root.id, Some("📁")).unwrap().icon.as_deref(),
            Some("📁")
        );

        let moved = move_page(&conn, &child.id, Some(sib.id.clone()), 1.0).unwrap();
        assert_eq!(moved.parent_id.as_deref(), Some(sib.id.as_str()));

        delete(&conn, &sib.id).unwrap();
        let remaining = list(&conn).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].id, root.id);
    }

    #[test]
    fn database_page_creates_database_row() {
        let db = Db::open_in_memory().unwrap();
        let conn = db.conn.lock().unwrap();
        let p = create(&conn, None, "Tasks".into(), "database".into()).unwrap();
        let n: i64 = conn
            .query_row(
                "SELECT count(*) FROM database WHERE page_id = ?1",
                [&p.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn a_page_cannot_be_moved_into_its_own_subtree() {
        let db = Db::open_in_memory().unwrap();
        let conn = db.conn.lock().unwrap();
        let root = create(&conn, None, "Root".into(), "doc".into()).unwrap();
        let child = create(&conn, Some(root.id.clone()), "Child".into(), "doc".into()).unwrap();
        let grand =
            create(&conn, Some(child.id.clone()), "Grand".into(), "doc".into()).unwrap();
        let other = create(&conn, None, "Other".into(), "doc".into()).unwrap();

        // into itself
        assert!(move_page(&conn, &root.id, Some(root.id.clone()), 1.0).is_err());
        // into a direct child
        assert!(move_page(&conn, &root.id, Some(child.id.clone()), 1.0).is_err());
        // into a deeper descendant
        assert!(move_page(&conn, &root.id, Some(grand.id.clone()), 1.0).is_err());
        // the tree is untouched
        assert_eq!(read(&conn, &root.id).unwrap().parent_id, None);
        // an unrelated parent still works, and so does moving a descendant out
        assert!(move_page(&conn, &root.id, Some(other.id.clone()), 1.0).is_ok());
        assert!(move_page(&conn, &grand.id, None, 1.0).is_ok());
    }

    #[test]
    fn subtree_ids_lists_the_page_and_everything_under_it() {
        let db = Db::open_in_memory().unwrap();
        let conn = db.conn.lock().unwrap();
        let root = create(&conn, None, "Root".into(), "doc".into()).unwrap();
        let child = create(&conn, Some(root.id.clone()), "Child".into(), "doc".into()).unwrap();
        let grand =
            create(&conn, Some(child.id.clone()), "Grand".into(), "doc".into()).unwrap();
        let other = create(&conn, None, "Other".into(), "doc".into()).unwrap();

        let mut ids = subtree_ids(&conn, &root.id).unwrap();
        ids.sort();
        let mut want = vec![root.id.clone(), child.id, grand.id];
        want.sort();
        assert_eq!(ids, want);
        assert!(!ids.contains(&other.id));
        assert_eq!(subtree_ids(&conn, "missing").unwrap(), Vec::<String>::new());
    }

    #[test]
    fn rejects_bad_type() {
        let db = Db::open_in_memory().unwrap();
        let conn = db.conn.lock().unwrap();
        assert!(create(&conn, None, "x".into(), "bogus".into()).is_err());
    }
}
