//! Linked-knowledge layer: wiki-link edges (`link` table) + tags. The frontend
//! extracts links/tags from a page's blocks on save and calls `set_page_links`;
//! backlinks are just a reverse query. Dangling links (target not yet created)
//! resolve by title when the target page appears or is renamed.

use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::db::{new_id, now_ms};
use crate::error::AppResult;

#[derive(Debug, Deserialize)]
pub struct LinkInput {
    pub target_page_id: Option<String>,
    pub dst_title: Option<String>,
    pub context: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Backlink {
    pub source_page_id: String,
    pub source_title: String,
    pub source_icon: Option<String>,
    pub context: Option<String>,
}

fn row_to_backlink(r: &Row) -> rusqlite::Result<Backlink> {
    Ok(Backlink {
        source_page_id: r.get(0)?,
        source_title: r.get(1)?,
        source_icon: r.get(2)?,
        context: r.get(3)?,
    })
}

#[derive(Debug, Serialize)]
pub struct GraphNode {
    pub id: String,
    pub title: String,
    pub icon: Option<String>,
    #[serde(rename = "type")]
    pub kind: String,
    pub degree: i64,
}

#[derive(Debug, Serialize)]
pub struct GraphEdge {
    pub source: String,
    pub target: String,
}

#[derive(Debug, Serialize)]
pub struct LinkGraph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

pub mod core {
    use super::*;

    /// Replace a page's outgoing wiki-link ("mention") edges and its tags.
    pub fn set_page_links(
        conn: &Connection,
        page_id: &str,
        links: &[LinkInput],
        tags: &[String],
    ) -> AppResult<()> {
        conn.execute(
            "DELETE FROM link WHERE source_page_id = ?1 AND kind = 'mention'",
            params![page_id],
        )?;
        let now = now_ms();
        for l in links {
            let target: Option<String> = match &l.target_page_id {
                Some(id) => Some(id.clone()),
                None => match &l.dst_title {
                    Some(t) => conn
                        .query_row(
                            "SELECT id FROM page WHERE title = ?1 LIMIT 1",
                            params![t],
                            |r| r.get::<_, String>(0),
                        )
                        .ok(),
                    None => None,
                },
            };
            conn.execute(
                "INSERT INTO link (id, source_page_id, target_page_id, dst_title, kind, context, created_at)
                 VALUES (?1, ?2, ?3, ?4, 'mention', ?5, ?6)",
                params![new_id(), page_id, target, l.dst_title, l.context, now],
            )?;
        }

        conn.execute("DELETE FROM page_tag WHERE page_id = ?1", params![page_id])?;
        for name in tags {
            let name = name.trim_start_matches('#').trim();
            if name.is_empty() {
                continue;
            }
            conn.execute("INSERT OR IGNORE INTO tag (name) VALUES (?1)", params![name])?;
            let tag_id: i64 =
                conn.query_row("SELECT id FROM tag WHERE name = ?1", params![name], |r| {
                    r.get(0)
                })?;
            conn.execute(
                "INSERT OR IGNORE INTO page_tag (page_id, tag_id) VALUES (?1, ?2)",
                params![page_id, tag_id],
            )?;
        }
        Ok(())
    }

    pub fn get_backlinks(conn: &Connection, page_id: &str) -> AppResult<Vec<Backlink>> {
        let mut stmt = conn.prepare(
            "SELECT l.source_page_id, p.title, p.icon, l.context
             FROM link l JOIN page p ON p.id = l.source_page_id
             WHERE l.target_page_id = ?1 AND l.kind = 'mention' AND p.deleted_at IS NULL
             ORDER BY p.updated_at DESC",
        )?;
        let rows = stmt.query_map(params![page_id], row_to_backlink)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// When a page is created/renamed, resolve any dangling links that pointed
    /// at its title.
    pub fn resolve_danglers(conn: &Connection, page_id: &str, title: &str) -> AppResult<()> {
        conn.execute(
            "UPDATE link SET target_page_id = ?1
             WHERE dst_title = ?2 AND target_page_id IS NULL",
            params![page_id, title],
        )?;
        Ok(())
    }

    pub fn tags_for_page(conn: &Connection, page_id: &str) -> AppResult<Vec<String>> {
        let mut stmt = conn.prepare(
            "SELECT t.name FROM page_tag pt JOIN tag t ON t.id = pt.tag_id
             WHERE pt.page_id = ?1 ORDER BY t.name",
        )?;
        let rows = stmt.query_map(params![page_id], |r| r.get::<_, String>(0))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// The whole page-link graph: every non-deleted page is a node; every
    /// resolved `mention` edge between two distinct non-deleted pages is an edge.
    pub fn get_graph(conn: &Connection) -> AppResult<LinkGraph> {
        let mut estmt = conn.prepare(
            "SELECT DISTINCT l.source_page_id, l.target_page_id
             FROM link l
             JOIN page sp ON sp.id = l.source_page_id
             JOIN page tp ON tp.id = l.target_page_id
             WHERE l.kind = 'mention' AND l.target_page_id IS NOT NULL
               AND sp.deleted_at IS NULL AND tp.deleted_at IS NULL
               AND l.source_page_id <> l.target_page_id",
        )?;
        let edges: Vec<GraphEdge> = estmt
            .query_map([], |r| {
                Ok(GraphEdge {
                    source: r.get(0)?,
                    target: r.get(1)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let mut degree: HashMap<String, i64> = HashMap::new();
        for e in &edges {
            *degree.entry(e.source.clone()).or_default() += 1;
            *degree.entry(e.target.clone()).or_default() += 1;
        }

        let mut nstmt = conn.prepare(
            "SELECT id, title, icon, type FROM page WHERE deleted_at IS NULL ORDER BY title",
        )?;
        let nodes: Vec<GraphNode> = nstmt
            .query_map([], |r| {
                let id: String = r.get(0)?;
                let deg = *degree.get(&id).unwrap_or(&0);
                Ok(GraphNode {
                    id,
                    title: r.get(1)?,
                    icon: r.get(2)?,
                    kind: r.get(3)?,
                    degree: deg,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        Ok(LinkGraph { nodes, edges })
    }

    /// Extract `[[Title]]` wiki-links and `#tags` from plain text — mirrors the
    /// frontend `extractLinksAndTags`, for backfilling existing content.
    pub fn extract_links_and_tags(text: &str) -> (Vec<String>, Vec<String>) {
        let mut links: Vec<String> = Vec::new();
        let mut i = 0;
        let bytes = text.as_bytes();
        while i + 1 < bytes.len() {
            if bytes[i] == b'[' && bytes[i + 1] == b'[' {
                if let Some(close) = text[i + 2..].find("]]") {
                    let title = text[i + 2..i + 2 + close].trim();
                    if !title.is_empty() && !links.iter().any(|l| l == title) {
                        links.push(title.to_string());
                    }
                    i = i + 2 + close + 2;
                    continue;
                }
            }
            i += 1;
        }
        let mut tags: Vec<String> = Vec::new();
        let chars: Vec<char> = text.chars().collect();
        let mut j = 0;
        while j < chars.len() {
            let boundary = j == 0 || chars[j - 1].is_whitespace();
            if chars[j] == '#' && boundary {
                let mut k = j + 1;
                while k < chars.len()
                    && (chars[k].is_ascii_alphanumeric() || matches!(chars[k], '_' | '/' | '-'))
                {
                    k += 1;
                }
                if k > j + 1 {
                    let tag: String = chars[j + 1..k].iter().collect();
                    if !tags.iter().any(|t| t == &tag) {
                        tags.push(tag);
                    }
                }
                j = k;
                continue;
            }
            j += 1;
        }
        (links, tags)
    }

    /// Parse every existing page body for `[[links]]`/`#tags` and populate the
    /// link/tag tables. Returns how many pages had links/tags.
    pub fn backfill_all(conn: &Connection) -> AppResult<usize> {
        // `content` is NULL for database pages — read it as Option, default "[]".
        let pages: Vec<(String, String)> = {
            let mut stmt =
                conn.prepare("SELECT id, content FROM page WHERE deleted_at IS NULL")?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?.unwrap_or_else(|| "[]".into()),
                ))
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        let mut changed = 0;
        for (id, content) in pages {
            // Use the Markdown projection (not plaintext) so a `#tag` or `[[link]]`
            // at the start of a block isn't stripped as a leading marker.
            let text = crate::vault::blocks_to_markdown(&content);
            let (link_titles, tags) = extract_links_and_tags(&text);
            if link_titles.is_empty() && tags.is_empty() {
                continue;
            }
            let links: Vec<LinkInput> = link_titles
                .into_iter()
                .map(|t| LinkInput {
                    target_page_id: None,
                    dst_title: Some(t),
                    context: None,
                })
                .collect();
            set_page_links(conn, &id, &links, &tags)?;
            changed += 1;
        }
        Ok(changed)
    }

    /// Run `backfill_all` once (guarded by a setting), so an existing DB gets its
    /// graph/backlinks populated on first v3 launch instead of starting empty.
    pub fn maybe_backfill(conn: &Connection) -> AppResult<()> {
        if crate::db::get_setting(conn, "links_backfilled")?.as_deref() == Some("1") {
            return Ok(());
        }
        let n = backfill_all(conn)?;
        crate::db::set_setting(conn, "links_backfilled", "1")?;
        log::info!("backfilled links/tags for {n} page(s)");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::core::*;
    use super::LinkInput;
    use crate::db::Db;
    use crate::store::pages;

    #[test]
    fn links_backlinks_tags_and_dangler_resolution() {
        let db = Db::open_in_memory().unwrap();
        let conn = db.conn.lock().unwrap();
        let a = pages::core::create(&conn, None, "Note A".into(), "doc".into()).unwrap();
        let b = pages::core::create(&conn, None, "Note B".into(), "doc".into()).unwrap();

        set_page_links(
            &conn,
            &a.id,
            &[
                LinkInput { target_page_id: Some(b.id.clone()), dst_title: Some("Note B".into()), context: Some("see B".into()) },
                LinkInput { target_page_id: None, dst_title: Some("Note C".into()), context: None },
            ],
            &["client".into(), "#urgent".into()],
        )
        .unwrap();

        let bl = get_backlinks(&conn, &b.id).unwrap();
        assert_eq!(bl.len(), 1);
        assert_eq!(bl[0].source_title, "Note A");

        let mut tags = tags_for_page(&conn, &a.id).unwrap();
        tags.sort();
        assert_eq!(tags, vec!["client".to_string(), "urgent".to_string()]);

        let c = pages::core::create(&conn, None, "Note C".into(), "doc".into()).unwrap();
        resolve_danglers(&conn, &c.id, "Note C").unwrap();
        assert_eq!(get_backlinks(&conn, &c.id).unwrap().len(), 1);

        set_page_links(&conn, &a.id, &[LinkInput { target_page_id: Some(b.id.clone()), dst_title: None, context: None }], &[]).unwrap();
        assert_eq!(get_backlinks(&conn, &c.id).unwrap().len(), 0);
        assert_eq!(get_backlinks(&conn, &b.id).unwrap().len(), 1);
        assert!(tags_for_page(&conn, &a.id).unwrap().is_empty());
    }

    #[test]
    fn graph_nodes_edges_and_degree() {
        let db = Db::open_in_memory().unwrap();
        let conn = db.conn.lock().unwrap();
        let a = pages::core::create(&conn, None, "A".into(), "doc".into()).unwrap();
        let b = pages::core::create(&conn, None, "B".into(), "doc".into()).unwrap();
        let c = pages::core::create(&conn, None, "C".into(), "doc".into()).unwrap();

        set_page_links(
            &conn,
            &a.id,
            &[
                LinkInput { target_page_id: Some(b.id.clone()), dst_title: None, context: None },
                LinkInput { target_page_id: Some(c.id.clone()), dst_title: None, context: None },
                LinkInput { target_page_id: None, dst_title: Some("Ghost".into()), context: None },
                LinkInput { target_page_id: Some(a.id.clone()), dst_title: None, context: None },
            ],
            &[],
        )
        .unwrap();

        let g = get_graph(&conn).unwrap();
        assert_eq!(g.nodes.len(), 3);
        assert_eq!(g.edges.len(), 2);
        let deg = |id: &str| g.nodes.iter().find(|n| n.id == id).unwrap().degree;
        assert_eq!(deg(&a.id), 2);
        assert_eq!(deg(&b.id), 1);
        assert_eq!(deg(&c.id), 1);

        pages::core::delete(&conn, &c.id).unwrap();
        let g2 = get_graph(&conn).unwrap();
        assert_eq!(g2.nodes.len(), 2);
        assert_eq!(g2.edges.len(), 1);
    }

    #[test]
    fn extracts_links_and_tags_from_text() {
        let (links, tags) =
            extract_links_and_tags("See [[Q3 Kickoff]] and [[Roadmap]] #urgent #client/acme not#atag");
        assert_eq!(links, vec!["Q3 Kickoff".to_string(), "Roadmap".to_string()]);
        assert_eq!(tags, vec!["urgent".to_string(), "client/acme".to_string()]);
    }

    #[test]
    fn backfill_populates_links_and_tags_from_existing_bodies() {
        use crate::store::documents;
        let db = Db::open_in_memory().unwrap();
        let conn = db.conn.lock().unwrap();
        // A database page has NULL content — backfill must NOT crash on it.
        pages::core::create(&conn, None, "Tasks".into(), "database".into()).unwrap();
        let a = pages::core::create(&conn, None, "Note A".into(), "doc".into()).unwrap();
        let b = pages::core::create(&conn, None, "Note B".into(), "doc".into()).unwrap();
        // A references [[Note B]] and a block that STARTS with #urgent (would be
        // stripped by the plaintext projection) — as text, no link rows yet.
        documents::core::update(
            &conn,
            &a.id,
            r##"[{"type":"paragraph","content":"See [[Note B]]"},{"type":"paragraph","content":"#urgent follow-up"}]"##,
        )
        .unwrap();
        assert!(get_backlinks(&conn, &b.id).unwrap().is_empty());

        // maybe_backfill runs once and wires them up (and doesn't error on the DB page)
        maybe_backfill(&conn).unwrap();
        assert_eq!(get_backlinks(&conn, &b.id).unwrap().len(), 1);
        assert_eq!(tags_for_page(&conn, &a.id).unwrap(), vec!["urgent".to_string()]);
        // the guard was set (so it won't re-run)
        assert_eq!(crate::db::get_setting(&conn, "links_backfilled").unwrap().as_deref(), Some("1"));

        // second call is a no-op (guarded)
        assert_eq!(backfill_all(&conn).unwrap(), 1); // direct call still processes A
        maybe_backfill(&conn).unwrap(); // guarded — no error, no change
        assert_eq!(get_backlinks(&conn, &b.id).unwrap().len(), 1);
    }
}
