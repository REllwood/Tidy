use rusqlite::{params, Connection};
use serde::Serialize;

use crate::error::AppResult;

#[derive(Debug, Serialize)]
pub struct PageHit {
    pub id: String,
    pub title: String,
    pub icon: Option<String>,
    #[serde(rename = "type")]
    pub kind: String,
}

#[derive(Debug, Serialize)]
pub struct RowHit {
    pub row_id: String,
    pub page_id: String,
    pub text: String,
}

#[derive(Debug, Serialize)]
pub struct SearchResults {
    pub pages: Vec<PageHit>,
    pub rows: Vec<RowHit>,
}

pub mod core {
    use super::*;

    pub fn search(conn: &Connection, query: &str) -> AppResult<SearchResults> {
        let q = query.trim();
        if q.is_empty() {
            return Ok(SearchResults {
                pages: vec![],
                rows: vec![],
            });
        }
        let escaped = q.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
        let like = format!("%{escaped}%");

        let fts_query = q
            .split_whitespace()
            .map(|t| format!("\"{}\"*", t.replace('"', "")))
            .collect::<Vec<_>>()
            .join(" ");
        let mut pages: Vec<PageHit> = Vec::new();
        if !fts_query.is_empty() {
            let mut ps = conn.prepare(
                "SELECT p.id, p.title, p.icon, p.type
                 FROM page_fts f JOIN page p ON p.id = f.page_id
                 WHERE page_fts MATCH ?1
                 ORDER BY rank LIMIT 25",
            )?;
            let it = ps.query_map(params![fts_query], |r| {
                Ok(PageHit {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    icon: r.get(2)?,
                    kind: r.get(3)?,
                })
            })?;
            for row in it {
                pages.push(row?);
            }
        }

        let mut rs = conn.prepare(
            "SELECT db_row.id, page.id, cell.value
             FROM cell
             JOIN field ON field.id = cell.field_id
             JOIN db_row ON db_row.id = cell.row_id
             JOIN database ON database.id = db_row.database_id
             JOIN page ON page.id = database.page_id
             WHERE field.type = 'text' AND cell.value LIKE ?1 ESCAPE '\\'
             LIMIT 25",
        )?;
        let rows = rs
            .query_map(params![like], |r| {
                let raw: Option<String> = r.get(2)?;
                let text = raw
                    .map(|s| s.trim_matches('"').to_string())
                    .unwrap_or_default();
                Ok(RowHit {
                    row_id: r.get(0)?,
                    page_id: r.get(1)?,
                    text,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        Ok(SearchResults { pages, rows })
    }
}

#[cfg(test)]
mod tests {
    use super::core::*;
    use crate::db::Db;
    use crate::store::{databases, documents, pages};
    use serde_json::json;

    #[test]
    fn finds_pages_and_rows() {
        let db = Db::open_in_memory().unwrap();
        let conn = db.conn.lock().unwrap();
        let p = pages::core::create(&conn, None, "Project Apollo".into(), "doc".into()).unwrap();
        documents::core::update(
            &conn,
            &p.id,
            r#"[{"type":"paragraph","content":"launch sequence"}]"#,
        )
        .unwrap();

        let dbp = pages::core::create(&conn, None, "Tasks".into(), "database".into()).unwrap();
        let did = databases::core::database_id_for_page(&conn, &dbp.id).unwrap();
        let name = databases::core::list_fields(&conn, &did)
            .unwrap()
            .into_iter()
            .find(|f| f.kind == "text")
            .unwrap();
        let row = databases::core::create_row(&conn, &did).unwrap();
        databases::core::set_cell(&conn, &row.id, &name.id, &json!("Apollo retro")).unwrap();

        let r = search(&conn, "Apollo").unwrap();
        assert!(r.pages.iter().any(|p| p.title.contains("Apollo")));
        assert!(r.rows.iter().any(|x| x.text.contains("Apollo")));

        let r2 = search(&conn, "launch").unwrap();
        assert!(r2.pages.iter().any(|p| p.title == "Project Apollo"));

        assert!(search(&conn, "  ").unwrap().pages.is_empty());

        let status = databases::core::create_field(
            &conn,
            &did,
            "Status",
            "select",
            Some(json!({"choices":[{"id":"opt-abc123","name":"Done","color":"green"}]})),
        )
        .unwrap();
        databases::core::set_cell(&conn, &row.id, &status.id, &json!("opt-abc123")).unwrap();
        let r3 = search(&conn, "opt-abc123").unwrap();
        assert!(r3.rows.is_empty(), "select-option UUIDs should not be searchable");
    }
}
