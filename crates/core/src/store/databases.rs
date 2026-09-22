use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

use crate::db::{new_id, now_ms};
use crate::error::{AppError, AppResult};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Field {
    pub id: String,
    pub database_id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub options: Option<Value>,
    pub position: f64,
}

#[derive(Debug, Serialize, Clone)]
pub struct RowWithCells {
    pub id: String,
    pub database_id: String,
    pub position: f64,
    pub created_at: i64,
    pub cells: HashMap<String, Value>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DbView {
    pub id: String,
    pub database_id: String,
    pub kind: String,
    pub config: Option<Value>,
    pub position: f64,
}

#[derive(Debug, Serialize, Clone)]
pub struct DatabaseBundle {
    pub database_id: String,
    pub fields: Vec<Field>,
    pub rows: Vec<RowWithCells>,
    pub views: Vec<DbView>,
}

/// Lightweight database descriptor for relation/lookup/rollup field authoring:
/// which databases exist and what fields each has (to pick a target).
#[derive(Debug, Serialize, Clone)]
pub struct DatabaseSummary {
    pub database_id: String,
    pub page_id: String,
    pub title: String,
    pub icon: Option<String>,
    pub fields: Vec<Field>,
}

const FIELD_TYPES: [&str; 10] = [
    "text",
    "number",
    "select",
    "date",
    "checkbox",
    "dependencies",
    "relation",
    "lookup",
    "rollup",
    "formula",
];

fn row_to_field(r: &Row) -> rusqlite::Result<Field> {
    let opts: Option<String> = r.get(4)?;
    Ok(Field {
        id: r.get(0)?,
        database_id: r.get(1)?,
        name: r.get(2)?,
        kind: r.get(3)?,
        options: opts.and_then(|s| serde_json::from_str(&s).ok()),
        position: r.get(5)?,
    })
}

fn row_to_view(r: &Row) -> rusqlite::Result<DbView> {
    let cfg: Option<String> = r.get(3)?;
    Ok(DbView {
        id: r.get(0)?,
        database_id: r.get(1)?,
        kind: r.get(2)?,
        config: cfg.and_then(|s| serde_json::from_str(&s).ok()),
        position: r.get(4)?,
    })
}

pub mod core {
    use super::*;

    pub fn database_id_for_page(conn: &Connection, page_id: &str) -> AppResult<String> {
        conn.query_row(
            "SELECT id FROM database WHERE page_id = ?1",
            params![page_id],
            |r| r.get(0),
        )
        .optional()?
        .ok_or_else(|| AppError::NotFound(format!("database for page {page_id}")))
    }

    /// Create a database for a page and seed a sensible starter schema + views
    /// so all four views are immediately usable.
    pub fn create_for_page(conn: &Connection, page_id: &str) -> AppResult<String> {
        let db_id = new_id();
        conn.execute(
            "INSERT INTO database (id, page_id) VALUES (?1, ?2)",
            params![db_id, page_id],
        )?;
        // fields — Name / Status / Start / Due / Depends on (mirrors the mock's
        // blank-database schema so browser-verified behavior matches the real app).
        create_field(conn, &db_id, "Name", "text", None)?;
        let status_opts = serde_json::json!({
            "choices": [
                {"id": new_id(), "name": "To do", "color": "grey"},
                {"id": new_id(), "name": "In progress", "color": "blue"},
                {"id": new_id(), "name": "Done", "color": "green"}
            ]
        });
        let status = create_field(conn, &db_id, "Status", "select", Some(status_opts))?;
        let start = create_field(conn, &db_id, "Start", "date", None)?;
        let due = create_field(conn, &db_id, "Due", "date", None)?;
        let deps = create_field(conn, &db_id, "Depends on", "dependencies", None)?;
        // views — grid / board / calendar / gantt
        create_view(conn, &db_id, "grid", None)?;
        create_view(
            conn,
            &db_id,
            "board",
            Some(serde_json::json!({ "groupByFieldId": status.id })),
        )?;
        create_view(
            conn,
            &db_id,
            "calendar",
            Some(serde_json::json!({ "dateFieldId": start.id })),
        )?;
        create_view(
            conn,
            &db_id,
            "gantt",
            Some(serde_json::json!({
                "startFieldId": start.id,
                "endFieldId": due.id,
                "dependenciesFieldId": deps.id
            })),
        )?;
        Ok(db_id)
    }

    pub fn list_fields(conn: &Connection, database_id: &str) -> AppResult<Vec<Field>> {
        let mut stmt = conn.prepare(
            "SELECT id, database_id, name, type, options, position FROM field WHERE database_id = ?1 ORDER BY position",
        )?;
        let rows = stmt.query_map(params![database_id], row_to_field)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn create_field(
        conn: &Connection,
        database_id: &str,
        name: &str,
        kind: &str,
        options: Option<Value>,
    ) -> AppResult<Field> {
        if !FIELD_TYPES.contains(&kind) {
            return Err(AppError::Invalid(format!("field type '{kind}'")));
        }
        let id = new_id();
        let max: Option<f64> = conn
            .query_row(
                "SELECT max(position) FROM field WHERE database_id = ?1",
                params![database_id],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        let position = max.unwrap_or(0.0) + 1.0;
        let opts = options.as_ref().map(|v| v.to_string());
        conn.execute(
            "INSERT INTO field (id, database_id, name, type, options, position) VALUES (?1,?2,?3,?4,?5,?6)",
            params![id, database_id, name, kind, opts, position],
        )?;
        Ok(Field {
            id,
            database_id: database_id.to_string(),
            name: name.to_string(),
            kind: kind.to_string(),
            options,
            position,
        })
    }

    pub fn update_field(
        conn: &Connection,
        id: &str,
        name: Option<&str>,
        options: Option<Value>,
    ) -> AppResult<Field> {
        // Name and options are one edit: never leave a renamed field with its
        // old choices (or the reverse).
        crate::db::in_transaction(conn, |tx| {
            if let Some(n) = name {
                tx.execute("UPDATE field SET name = ?2 WHERE id = ?1", params![id, n])?;
            }
            if let Some(o) = options {
                tx.execute(
                    "UPDATE field SET options = ?2 WHERE id = ?1",
                    params![id, o.to_string()],
                )?;
            }
            Ok(())
        })?;
        conn.query_row(
            "SELECT id, database_id, name, type, options, position FROM field WHERE id = ?1",
            params![id],
            row_to_field,
        )
        .optional()?
        .ok_or_else(|| AppError::NotFound(format!("field {id}")))
    }

    /// Remove every reference to `field_id` from one view's config, returning
    /// whether anything changed. A view left pointing at a deleted field renders
    /// as an empty board/calendar/gantt, so the config is cleaned up with it.
    fn strip_field_from_config(config: &mut Value, field_id: &str) -> bool {
        let Some(map) = config.as_object_mut() else {
            return false;
        };
        let mut changed = false;
        for key in [
            "groupByFieldId",
            "dateFieldId",
            "startFieldId",
            "endFieldId",
            "dependenciesFieldId",
        ] {
            if map.get(key).and_then(|v| v.as_str()) == Some(field_id) {
                map.remove(key);
                changed = true;
            }
        }
        for key in ["filters", "sorts"] {
            if let Some(entries) = map.get_mut(key).and_then(|v| v.as_array_mut()) {
                let before = entries.len();
                entries.retain(|e| e.get("fieldId").and_then(|f| f.as_str()) != Some(field_id));
                changed |= entries.len() != before;
            }
        }
        if let Some(widths) = map.get_mut("columnWidths").and_then(|v| v.as_object_mut()) {
            changed |= widths.remove(field_id).is_some();
        }
        changed
    }

    pub fn delete_field(conn: &Connection, id: &str) -> AppResult<()> {
        crate::db::in_transaction(conn, |tx| {
            let database_id: String = tx
                .query_row(
                    "SELECT database_id FROM field WHERE id = ?1",
                    params![id],
                    |r| r.get(0),
                )
                .optional()?
                .ok_or_else(|| AppError::NotFound(format!("field {id}")))?;
            tx.execute("DELETE FROM field WHERE id = ?1", params![id])?;

            let views: Vec<(String, String)> = {
                let mut stmt = tx.prepare(
                    "SELECT id, config FROM db_view WHERE database_id = ?1 AND config IS NOT NULL",
                )?;
                let rows = stmt.query_map(params![database_id], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                })?;
                rows.collect::<rusqlite::Result<Vec<_>>>()?
            };
            for (view_id, raw) in views {
                let Ok(mut config) = serde_json::from_str::<Value>(&raw) else {
                    continue;
                };
                if strip_field_from_config(&mut config, id) {
                    tx.execute(
                        "UPDATE db_view SET config = ?2 WHERE id = ?1",
                        params![view_id, config.to_string()],
                    )?;
                }
            }
            Ok(())
        })
    }

    /// Every cell of a database in one query, bucketed by row id — one round
    /// trip for the whole table instead of one per row.
    fn cells_by_row(
        conn: &Connection,
        database_id: &str,
    ) -> AppResult<HashMap<String, HashMap<String, Value>>> {
        let mut stmt = conn.prepare(
            "SELECT cell.row_id, cell.field_id, cell.value
             FROM cell JOIN db_row ON db_row.id = cell.row_id
             WHERE db_row.database_id = ?1",
        )?;
        let rows = stmt.query_map(params![database_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        })?;
        let mut out: HashMap<String, HashMap<String, Value>> = HashMap::new();
        for row in rows {
            let (row_id, field_id, value) = row?;
            if let Some(raw) = value {
                if let Ok(parsed) = serde_json::from_str::<Value>(&raw) {
                    out.entry(row_id).or_default().insert(field_id, parsed);
                }
            }
        }
        Ok(out)
    }

    pub fn list_rows(conn: &Connection, database_id: &str) -> AppResult<Vec<RowWithCells>> {
        let base = {
            let mut stmt = conn.prepare(
                "SELECT id, database_id, position, created_at FROM db_row WHERE database_id = ?1 ORDER BY position",
            )?;
            let rows = stmt.query_map(params![database_id], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, f64>(2)?,
                    r.get::<_, i64>(3)?,
                ))
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        let mut cells = cells_by_row(conn, database_id)?;
        Ok(base
            .into_iter()
            .map(|(id, db_id, position, created_at)| RowWithCells {
                cells: cells.remove(&id).unwrap_or_default(),
                id,
                database_id: db_id,
                position,
                created_at,
            })
            .collect())
    }

    pub fn create_row(conn: &Connection, database_id: &str) -> AppResult<RowWithCells> {
        let id = new_id();
        let now = now_ms();
        let max: Option<f64> = conn
            .query_row(
                "SELECT max(position) FROM db_row WHERE database_id = ?1",
                params![database_id],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        let position = max.unwrap_or(0.0) + 1.0;
        conn.execute(
            "INSERT INTO db_row (id, database_id, position, created_at) VALUES (?1,?2,?3,?4)",
            params![id, database_id, position, now],
        )?;
        Ok(RowWithCells {
            id,
            database_id: database_id.to_string(),
            position,
            created_at: now,
            cells: HashMap::new(),
        })
    }

    /// Create a row and fill it in one transaction, so a half-populated row can
    /// never reach a view. Null values are skipped (a cleared cell is no cell).
    pub fn create_row_with_cells(
        conn: &Connection,
        database_id: &str,
        cells: HashMap<String, Value>,
    ) -> AppResult<RowWithCells> {
        crate::db::in_transaction(conn, |tx| {
            let mut row = create_row(tx, database_id)?;
            for (field_id, value) in cells {
                if value.is_null() {
                    continue;
                }
                set_cell(tx, &row.id, &field_id, &value)?;
                row.cells.insert(field_id, value);
            }
            Ok(row)
        })
    }

    pub fn delete_row(conn: &Connection, id: &str) -> AppResult<()> {
        if conn.execute("DELETE FROM db_row WHERE id = ?1", params![id])? == 0 {
            return Err(AppError::NotFound(format!("row {id}")));
        }
        Ok(())
    }

    pub fn move_row(conn: &Connection, id: &str, position: f64) -> AppResult<()> {
        let n = conn.execute(
            "UPDATE db_row SET position = ?2 WHERE id = ?1",
            params![id, position],
        )?;
        if n == 0 {
            return Err(AppError::NotFound(format!("row {id}")));
        }
        Ok(())
    }

    pub fn set_cell(
        conn: &Connection,
        row_id: &str,
        field_id: &str,
        value: &Value,
    ) -> AppResult<()> {
        if value.is_null() {
            conn.execute(
                "DELETE FROM cell WHERE row_id = ?1 AND field_id = ?2",
                params![row_id, field_id],
            )?;
        } else {
            conn.execute(
                "INSERT INTO cell (row_id, field_id, value) VALUES (?1,?2,?3)
                 ON CONFLICT(row_id, field_id) DO UPDATE SET value = excluded.value",
                params![row_id, field_id, value.to_string()],
            )?;
        }
        Ok(())
    }

    pub fn list_views(conn: &Connection, database_id: &str) -> AppResult<Vec<DbView>> {
        let mut stmt = conn.prepare(
            "SELECT id, database_id, kind, config, position FROM db_view WHERE database_id = ?1 ORDER BY position",
        )?;
        let rows = stmt.query_map(params![database_id], row_to_view)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn create_view(
        conn: &Connection,
        database_id: &str,
        kind: &str,
        config: Option<Value>,
    ) -> AppResult<DbView> {
        let id = new_id();
        let max: Option<f64> = conn
            .query_row(
                "SELECT max(position) FROM db_view WHERE database_id = ?1",
                params![database_id],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        let position = max.unwrap_or(0.0) + 1.0;
        let cfg = config.as_ref().map(|v| v.to_string());
        conn.execute(
            "INSERT INTO db_view (id, database_id, kind, config, position) VALUES (?1,?2,?3,?4,?5)",
            params![id, database_id, kind, cfg, position],
        )?;
        Ok(DbView {
            id,
            database_id: database_id.to_string(),
            kind: kind.to_string(),
            config,
            position,
        })
    }

    pub fn update_view(conn: &Connection, id: &str, config: Value) -> AppResult<DbView> {
        conn.execute(
            "UPDATE db_view SET config = ?2 WHERE id = ?1",
            params![id, config.to_string()],
        )?;
        conn.query_row(
            "SELECT id, database_id, kind, config, position FROM db_view WHERE id = ?1",
            params![id],
            row_to_view,
        )
        .optional()?
        .ok_or_else(|| AppError::NotFound(format!("view {id}")))
    }

    pub fn bundle(conn: &Connection, page_id: &str) -> AppResult<DatabaseBundle> {
        let database_id = database_id_for_page(conn, page_id)?;
        bundle_by_id(conn, &database_id)
    }

    pub fn bundle_by_id(conn: &Connection, database_id: &str) -> AppResult<DatabaseBundle> {
        Ok(DatabaseBundle {
            fields: list_fields(conn, database_id)?,
            rows: list_rows(conn, database_id)?,
            views: list_views(conn, database_id)?,
            database_id: database_id.to_string(),
        })
    }

    /// Every database with its page title/icon and field list (for field authoring).
    pub fn list_databases(conn: &Connection) -> AppResult<Vec<DatabaseSummary>> {
        let mut stmt = conn.prepare(
            "SELECT d.id, d.page_id, p.title, p.icon
             FROM database d JOIN page p ON p.id = d.page_id
             WHERE p.deleted_at IS NULL
             ORDER BY p.title",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
            ))
        })?;
        let summaries = rows.collect::<rusqlite::Result<Vec<_>>>()?;

        // Every field in one query, bucketed by database, rather than a fresh
        // prepare + query per database.
        let mut by_database: HashMap<String, Vec<Field>> = HashMap::new();
        {
            let mut fstmt = conn.prepare(
                "SELECT id, database_id, name, type, options, position FROM field ORDER BY position",
            )?;
            let fields = fstmt.query_map([], row_to_field)?;
            for field in fields {
                let field = field?;
                by_database
                    .entry(field.database_id.clone())
                    .or_default()
                    .push(field);
            }
        }

        Ok(summaries
            .into_iter()
            .map(|(database_id, page_id, title, icon)| DatabaseSummary {
                fields: by_database.remove(&database_id).unwrap_or_default(),
                database_id,
                page_id,
                title,
                icon,
            })
            .collect())
    }

    /// Promote a row into a full `record` page (the additive "row = page").
    /// Idempotent: returns the existing page if the row was already promoted.
    /// Errors (never orphans a page) when the row does not exist.
    pub fn promote_row(
        conn: &Connection,
        row_id: &str,
        title: &str,
    ) -> AppResult<crate::store::pages::Page> {
        let row_page: Option<Option<String>> = conn
            .query_row(
                "SELECT page_id FROM db_row WHERE id = ?1",
                params![row_id],
                |r| r.get(0),
            )
            .optional()?;
        let existing = match row_page {
            None => return Err(AppError::NotFound(format!("row {row_id}"))),
            Some(inner) => inner,
        };
        if let Some(pid) = existing {
            crate::store::pages::core::read(conn, &pid)
        } else {
            // Atomic: create the page, mark it a record, and link the row all in
            // one transaction so a mid-way failure leaves no orphan page.
            let tx = conn.unchecked_transaction()?;
            let p = crate::store::pages::core::create(&tx, None, title.to_string(), "doc".into())?;
            tx.execute("UPDATE page SET type = 'record' WHERE id = ?1", params![p.id])?;
            tx.execute(
                "UPDATE db_row SET page_id = ?2 WHERE id = ?1",
                params![row_id, p.id],
            )?;
            tx.commit()?;
            crate::store::pages::core::read(conn, &p.id)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::core::*;
    use crate::db::Db;
    use crate::store::pages;
    use serde_json::json;

    fn db_with_database() -> (Db, String) {
        let db = Db::open_in_memory().unwrap();
        let page_id;
        {
            let conn = db.conn.lock().unwrap();
            let p = pages::core::create(&conn, None, "Tasks".into(), "database".into()).unwrap();
            page_id = p.id;
        }
        (db, page_id)
    }

    #[test]
    fn seeded_schema_and_views() {
        let (db, page_id) = db_with_database();
        let conn = db.conn.lock().unwrap();
        let b = bundle(&conn, &page_id).unwrap();
        assert_eq!(b.fields.len(), 5); // Name, Status, Start, Due, Depends on
        assert!(b.fields.iter().any(|f| f.kind == "select"));
        assert!(b.fields.iter().any(|f| f.kind == "dependencies"));
        assert_eq!(b.views.len(), 4); // grid, board, calendar, gantt
        assert!(b.views.iter().any(|v| v.kind == "board"));
        assert!(b.views.iter().any(|v| v.kind == "gantt"));
    }

    #[test]
    fn rows_and_cells_roundtrip() {
        let (db, page_id) = db_with_database();
        let conn = db.conn.lock().unwrap();
        let database_id = database_id_for_page(&conn, &page_id).unwrap();
        let name_field = list_fields(&conn, &database_id)
            .unwrap()
            .into_iter()
            .find(|f| f.kind == "text")
            .unwrap();

        let r = create_row(&conn, &database_id).unwrap();
        set_cell(&conn, &r.id, &name_field.id, &json!("Write tests")).unwrap();

        let rows = list_rows(&conn, &database_id).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].cells.get(&name_field.id).unwrap(), &json!("Write tests"));

        set_cell(&conn, &r.id, &name_field.id, &json!(null)).unwrap();
        assert!(list_rows(&conn, &database_id).unwrap()[0].cells.is_empty());

        delete_row(&conn, &r.id).unwrap();
        assert_eq!(list_rows(&conn, &database_id).unwrap().len(), 0);
    }

    #[test]
    fn field_crud_and_bad_type() {
        let (db, page_id) = db_with_database();
        let conn = db.conn.lock().unwrap();
        let database_id = database_id_for_page(&conn, &page_id).unwrap();
        let f = create_field(&conn, &database_id, "Count", "number", None).unwrap();
        let upd = update_field(&conn, &f.id, Some("Total"), None).unwrap();
        assert_eq!(upd.name, "Total");
        assert!(create_field(&conn, &database_id, "X", "bogus", None).is_err());
        delete_field(&conn, &f.id).unwrap();
        assert!(!list_fields(&conn, &database_id)
            .unwrap()
            .iter()
            .any(|x| x.id == f.id));
    }

    #[test]
    fn deleting_a_field_strips_it_from_every_view_config() {
        let (db, page_id) = db_with_database();
        let conn = db.conn.lock().unwrap();
        let database_id = database_id_for_page(&conn, &page_id).unwrap();
        let fields = list_fields(&conn, &database_id).unwrap();
        let status = fields.iter().find(|f| f.kind == "select").unwrap().clone();
        let start = fields.iter().find(|f| f.name == "Start").unwrap().clone();

        // a grid view that filters, sorts and sizes by the doomed field
        let grid = list_views(&conn, &database_id)
            .unwrap()
            .into_iter()
            .find(|v| v.kind == "grid")
            .unwrap();
        update_view(
            &conn,
            &grid.id,
            json!({
                "filters": [
                    {"fieldId": status.id, "op": "is", "value": "x"},
                    {"fieldId": start.id, "op": "is", "value": "y"}
                ],
                "sorts": [{"fieldId": status.id, "dir": "asc"}],
                "columnWidths": {status.id.clone(): 220, start.id.clone(): 120}
            }),
        )
        .unwrap();

        delete_field(&conn, &status.id).unwrap();

        let views = list_views(&conn, &database_id).unwrap();
        let board = views.iter().find(|v| v.kind == "board").unwrap();
        assert!(
            board.config.as_ref().unwrap().get("groupByFieldId").is_none(),
            "the board must not group by a field that is gone"
        );
        let grid = views.iter().find(|v| v.kind == "grid").unwrap();
        let cfg = grid.config.as_ref().unwrap();
        let filters = cfg.get("filters").unwrap().as_array().unwrap();
        assert_eq!(filters.len(), 1);
        assert_eq!(filters[0].get("fieldId").unwrap(), &json!(start.id));
        assert!(cfg.get("sorts").unwrap().as_array().unwrap().is_empty());
        let widths = cfg.get("columnWidths").unwrap().as_object().unwrap();
        assert!(!widths.contains_key(&status.id));
        assert!(widths.contains_key(&start.id));
        // views that never mentioned the field keep their config
        let calendar = views.iter().find(|v| v.kind == "calendar").unwrap();
        assert_eq!(
            calendar.config.as_ref().unwrap().get("dateFieldId").unwrap(),
            &json!(start.id)
        );

        // a second delete reports the field is gone rather than silently passing
        assert!(delete_field(&conn, &status.id).is_err());
    }

    #[test]
    fn row_writes_report_a_missing_row() {
        let (db, page_id) = db_with_database();
        let conn = db.conn.lock().unwrap();
        let database_id = database_id_for_page(&conn, &page_id).unwrap();
        let row = create_row(&conn, &database_id).unwrap();
        assert!(move_row(&conn, &row.id, 5.0).is_ok());
        assert!(move_row(&conn, "no-such-row", 5.0).is_err());
        assert!(delete_row(&conn, &row.id).is_ok());
        assert!(delete_row(&conn, &row.id).is_err());
    }

    #[test]
    fn create_row_with_cells_fills_the_row_in_one_go() {
        let (db, page_id) = db_with_database();
        let conn = db.conn.lock().unwrap();
        let database_id = database_id_for_page(&conn, &page_id).unwrap();
        let fields = list_fields(&conn, &database_id).unwrap();
        let name = fields.iter().find(|f| f.kind == "text").unwrap();
        let start = fields.iter().find(|f| f.name == "Start").unwrap();

        let mut cells = std::collections::HashMap::new();
        cells.insert(name.id.clone(), json!("Write tests"));
        cells.insert(start.id.clone(), json!(null));
        let row = create_row_with_cells(&conn, &database_id, cells).unwrap();

        assert_eq!(row.cells.get(&name.id), Some(&json!("Write tests")));
        assert!(!row.cells.contains_key(&start.id), "nulls are skipped");
        let stored = list_rows(&conn, &database_id).unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].cells, row.cells);

        // an unknown field aborts the whole row rather than leaving a blank one
        let mut bad = std::collections::HashMap::new();
        bad.insert("no-such-field".to_string(), json!("x"));
        assert!(create_row_with_cells(&conn, &database_id, bad).is_err());
        assert_eq!(list_rows(&conn, &database_id).unwrap().len(), 1);
    }

    #[test]
    fn relational_field_types_accepted() {
        let (db, page_id) = db_with_database();
        let conn = db.conn.lock().unwrap();
        let database_id = database_id_for_page(&conn, &page_id).unwrap();
        for kind in ["relation", "lookup", "rollup", "formula"] {
            assert!(
                create_field(&conn, &database_id, kind, kind, None).is_ok(),
                "field type '{kind}' should be accepted"
            );
        }
    }

    #[test]
    fn list_databases_reports_each_db_with_fields() {
        let (db, page_id) = db_with_database();
        let conn = db.conn.lock().unwrap();
        let p2 = pages::core::create(&conn, None, "Clients".into(), "database".into()).unwrap();

        let summaries = list_databases(&conn).unwrap();
        assert_eq!(summaries.len(), 2);
        assert_eq!(summaries[0].title, "Clients");
        assert_eq!(summaries[0].page_id, p2.id);
        assert_eq!(summaries[1].title, "Tasks");
        assert_eq!(summaries[1].page_id, page_id);
        assert!(summaries.iter().all(|s| s.fields.len() == 5));
    }

    #[test]
    fn promote_row_creates_record_and_is_idempotent() {
        let (db, page_id) = db_with_database();
        let conn = db.conn.lock().unwrap();
        let database_id = database_id_for_page(&conn, &page_id).unwrap();
        let row = create_row(&conn, &database_id).unwrap();

        let p = promote_row(&conn, &row.id, "My Record").unwrap();
        assert_eq!(p.kind, "record");
        assert_eq!(p.title, "My Record");
        // idempotent — second call returns the same page, no new page
        let p2 = promote_row(&conn, &row.id, "Ignored").unwrap();
        assert_eq!(p2.id, p.id);
        // missing row errors instead of orphaning a page
        assert!(promote_row(&conn, "no-such-row", "x").is_err());
    }
}
