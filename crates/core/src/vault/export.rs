//! DB-aware bridge between the store and the pure vault serializer: turn a page
//! row into a `notes/<slug>-<id8>.md` file (frontmatter + Markdown body).

use rusqlite::{params, Connection, OptionalExtension};

use crate::error::AppResult;
use crate::store::knowledge;

use super::{blocks_to_markdown, render_page, slug, PageMeta};

/// A rendered file destined for the vault (path is relative to the vault root).
pub struct ExportedFile {
    pub page_id: String,
    pub rel_path: String,
    pub content: String,
}

/// Page types mirrored to `notes/*.md`. Database pages are exported separately.
pub const NOTE_TYPES: [&str; 3] = ["doc", "record", "meeting"];

/// Ids of all non-deleted pages that mirror to a `notes/*.md` file.
pub fn exportable_page_ids(conn: &Connection) -> AppResult<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT id FROM page
         WHERE type IN ('doc','record','meeting') AND deleted_at IS NULL
         ORDER BY created_at",
    )?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Render one page to its Markdown file, or `None` if it isn't a note-type page
/// (or was deleted).
pub fn render_page_file(conn: &Connection, page_id: &str) -> AppResult<Option<ExportedFile>> {
    let row = conn
        .query_row(
            "SELECT id, parent_id, title, icon, type, content, created_at, updated_at
             FROM page WHERE id = ?1 AND deleted_at IS NULL",
            params![page_id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, Option<String>>(5)?,
                    r.get::<_, i64>(6)?,
                    r.get::<_, i64>(7)?,
                ))
            },
        )
        .optional()?;

    let (id, parent_id, title, icon, kind, content, created_at, updated_at) = match row {
        Some(t) => t,
        None => return Ok(None),
    };
    if !NOTE_TYPES.contains(&kind.as_str()) {
        return Ok(None);
    }

    let tags = knowledge::core::tags_for_page(conn, &id)?;
    let body_md = blocks_to_markdown(content.as_deref().unwrap_or("[]"));
    let meta = PageMeta {
        id: &id,
        title: &title,
        icon: icon.as_deref(),
        kind: &kind,
        parent: parent_id.as_deref(),
        created: created_at,
        updated: updated_at,
        tags: &tags,
    };
    let content = render_page(&meta, &body_md);
    let id8 = id.chars().take(8).collect::<String>();
    let rel_path = format!("notes/{}-{}.md", slug(&title), id8);
    Ok(Some(ExportedFile {
        page_id: id,
        rel_path,
        content,
    }))
}

/// Outcome of applying an external Markdown file to the DB.
#[derive(Debug, PartialEq, Eq)]
pub enum ApplyResult {
    /// The page was updated from the file.
    Updated(String),
    /// The app has unflushed changes to this page (`dirty`) — the external file
    /// was NOT applied; the caller should write a `.conflict` sidecar instead so
    /// neither side's edits are lost.
    Conflict(String),
    /// No known page matches the file's frontmatter id.
    Unmatched,
}

/// Apply an external Markdown file to the DB: match by frontmatter `id`, then —
/// unless the app has unflushed changes to that page — round-trip the external
/// body into the canonical `content` (and the `body_text` projection) plus the
/// title. Returns what happened so the watcher can handle conflicts.
pub fn apply_external_markdown(conn: &Connection, file_text: &str) -> AppResult<ApplyResult> {
    let (front, body) = super::parse_frontmatter(file_text);
    let id = match front.get("id") {
        Some(id) if !id.is_empty() => id.clone(),
        _ => return Ok(ApplyResult::Unmatched),
    };
    // Only touch a page we already know about (identity by id). Pull `dirty` too.
    let dirty: Option<i64> = conn
        .query_row(
            "SELECT dirty FROM page WHERE id = ?1 AND deleted_at IS NULL",
            params![id],
            |r| r.get(0),
        )
        .optional()?;
    let dirty = match dirty {
        Some(d) => d,
        None => return Ok(ApplyResult::Unmatched),
    };
    // The app changed this page since the last flush — don't clobber it.
    if dirty != 0 {
        return Ok(ApplyResult::Conflict(id));
    }

    // Round-trip the external body into the CANONICAL content (BlockNote JSON)
    // as well as the plain body_text projection — otherwise the editor would
    // keep showing the old body and the next flush would clobber the edit.
    // The DB now matches the file, so dirty stays 0.
    let now = crate::db::now_ms();
    let content = super::markdown_to_blocks(&body);
    if let Some(title) = front.get("title") {
        conn.execute(
            "UPDATE page SET title = ?2, content = ?3, body_text = ?4, updated_at = ?5, dirty = 0 WHERE id = ?1",
            params![id, title, content, body, now],
        )?;
    } else {
        conn.execute(
            "UPDATE page SET content = ?2, body_text = ?3, updated_at = ?4, dirty = 0 WHERE id = ?1",
            params![id, content, body, now],
        )?;
    }
    Ok(ApplyResult::Updated(id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::store::pages;

    #[test]
    fn renders_a_doc_page_file() {
        let db = Db::open_in_memory().unwrap();
        let conn = db.conn.lock().unwrap();
        let p = pages::core::create(&conn, None, "Project Apollo".into(), "doc".into()).unwrap();
        crate::store::documents::core::update(
            &conn,
            &p.id,
            r#"[{"type":"paragraph","content":"launch sequence"}]"#,
        )
        .unwrap();

        let f = render_page_file(&conn, &p.id).unwrap().unwrap();
        assert!(f.rel_path.starts_with("notes/project-apollo-"));
        assert!(f.rel_path.ends_with(".md"));
        assert!(f.content.contains(&format!("id: {}", p.id)));
        assert!(f.content.contains("title: Project Apollo"));
        assert!(f.content.contains("launch sequence"));
    }

    #[test]
    fn database_pages_are_not_note_files() {
        let db = Db::open_in_memory().unwrap();
        let conn = db.conn.lock().unwrap();
        let p = pages::core::create(&conn, None, "Tasks".into(), "database".into()).unwrap();
        assert!(render_page_file(&conn, &p.id).unwrap().is_none());
    }

    #[test]
    fn external_markdown_updates_title_by_id() {
        let db = Db::open_in_memory().unwrap();
        let conn = db.conn.lock().unwrap();
        let p = pages::core::create(&conn, None, "Old Title".into(), "doc".into()).unwrap();
        let file = format!(
            "---\nid: {}\ntitle: New Title\ntype: doc\ncreated: 1\nupdated: 2\n---\n\nEdited in Obsidian\n",
            p.id
        );
        let matched = apply_external_markdown(&conn, &file).unwrap();
        assert_eq!(matched, ApplyResult::Updated(p.id.clone()));
        let updated = pages::core::read(&conn, &p.id).unwrap();
        assert_eq!(updated.title, "New Title");
        // the external body is now in the CANONICAL content, so the editor shows
        // it and the next flush won't clobber it
        let body = crate::store::documents::core::get(&conn, &p.id).unwrap();
        assert!(body.contains("Edited in Obsidian"), "content should reflect the external edit");

        // an unknown id is ignored
        let unknown = "---\nid: nope\ntitle: X\n---\nbody";
        assert_eq!(apply_external_markdown(&conn, unknown).unwrap(), ApplyResult::Unmatched);
    }

    #[test]
    fn external_edit_on_dirty_page_is_a_conflict() {
        let db = Db::open_in_memory().unwrap();
        let conn = db.conn.lock().unwrap();
        let p = pages::core::create(&conn, None, "Note".into(), "doc".into()).unwrap();
        // an in-app edit marks the page dirty (unflushed)
        crate::store::documents::core::update(
            &conn,
            &p.id,
            r#"[{"type":"paragraph","content":"app version"}]"#,
        )
        .unwrap();
        let file = format!(
            "---\nid: {}\ntitle: Note\n---\n\nobsidian version\n",
            p.id
        );
        // external change arrives while dirty → conflict, DB NOT overwritten
        assert_eq!(
            apply_external_markdown(&conn, &file).unwrap(),
            ApplyResult::Conflict(p.id.clone())
        );
        let body = crate::store::documents::core::get(&conn, &p.id).unwrap();
        assert!(body.contains("app version"), "app edit must survive the conflict");
    }
}
