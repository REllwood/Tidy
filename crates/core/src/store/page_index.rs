//! Written pages, searchable by meaning. Pages are cut into passages and
//! embedded with the same search model as meeting transcripts, so "Ask your
//! meetings" can find a page worded differently from the question.
use crate::{
    db::{new_id, now_ms},
    error::AppResult,
    store::meeting_ai::{text_windows, validate_vector, Passage, SourceKind, EMBEDDING_MODEL},
};
use rusqlite::{params, Connection};
use std::collections::{HashMap, HashSet};

/// A page waiting to be (re)indexed.
#[derive(Debug, Clone)]
pub struct PageSource {
    pub id: String,
    pub title: String,
    pub text: String,
    /// Fingerprint of the title and text the passages were cut from.
    pub hash: String,
}

fn fingerprint(title: &str, text: &str) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(title.as_bytes());
    hasher.update(&[0]);
    hasher.update(text.as_bytes());
    hasher.finalize().to_hex().to_string()
}

/// Pages whose search passages are missing or out of date. Pages edited in
/// the last `quiet_ms` are left until the person stops typing. Meeting pages
/// have their own transcript index, and agent notes wait for their summary.
pub fn pending(conn: &Connection, quiet_ms: i64, limit: usize) -> AppResult<Vec<PageSource>> {
    let mut stmt = conn.prepare(
        "SELECT p.id,p.title,p.body_text FROM page p
         LEFT JOIN page_index i ON i.page_id=p.id
         WHERE (i.page_id IS NULL OR i.embedding_model NOT IN (?1,'failed:'||?1))
         AND p.deleted_at IS NULL AND p.type IN ('doc','record')
         AND coalesce(p.body_text,'')<>'' AND p.updated_at<?2
         AND NOT EXISTS(SELECT 1 FROM meeting m WHERE m.page_id=p.id)
         AND NOT EXISTS(SELECT 1 FROM note_ai n WHERE n.page_id=p.id)
         ORDER BY p.updated_at DESC LIMIT ?3",
    )?;
    let rows = stmt.query_map(
        params![EMBEDDING_MODEL, now_ms() - quiet_ms, limit as i64],
        |r| {
            let (id, title, text): (String, String, String) = (r.get(0)?, r.get(1)?, r.get(2)?);
            Ok(PageSource {
                hash: fingerprint(&title, &text),
                id,
                title,
                text,
            })
        },
    )?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// A page's passages, as they're embedded.
pub fn passages(page: &PageSource) -> Vec<String> {
    text_windows(&page.text, 1400)
}

/// What the search model reads for a passage: the page title travels with it.
pub fn document_text(title: &str, passage: &str) -> String {
    format!("search_document: {title}\n{passage}")
}

/// Vectors already worked out for a page's passages, including ones an edit
/// set aside, by passage text.
pub fn cached_vectors(conn: &Connection, page_id: &str) -> AppResult<HashMap<String, Vec<f32>>> {
    let mut stmt = conn.prepare(
        "SELECT text,embedding FROM page_chunk WHERE page_id=?1 AND embedding IS NOT NULL AND embedding_model IN (?2,'stale:'||?2)",
    )?;
    let rows = stmt.query_map(params![page_id, EMBEDDING_MODEL], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })?;
    let mut found = HashMap::new();
    for row in rows {
        let (text, raw) = row?;
        if let Ok(vector) = serde_json::from_str::<Vec<f32>>(&raw) {
            if validate_vector(&vector).is_ok() {
                found.insert(text, vector);
            }
        }
    }
    Ok(found)
}

/// Store a page's passages. Returns false, storing nothing, when the page
/// changed while they were being embedded; the edit queues it again.
pub fn replace(
    conn: &mut Connection,
    page: &PageSource,
    passages: &[String],
    vectors: &[Vec<f32>],
) -> AppResult<bool> {
    if passages.len() != vectors.len() {
        return Err(crate::error::AppError::Invalid("Embedding count mismatch".into()));
    }
    let tx = conn.transaction()?;
    let current: Option<(String, String)> = tx
        .query_row(
            "SELECT title,coalesce(body_text,'') FROM page WHERE id=?1 AND deleted_at IS NULL",
            [&page.id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .ok();
    if current.map(|(t, b)| fingerprint(&t, &b)) != Some(page.hash.clone()) {
        return Ok(false);
    }
    tx.execute("DELETE FROM page_chunk WHERE page_id=?1", [&page.id])?;
    for (position, (text, vector)) in passages.iter().zip(vectors).enumerate() {
        validate_vector(vector)?;
        tx.execute(
            "INSERT INTO page_chunk(id,page_id,position,text,embedding,embedding_model) VALUES(?1,?2,?3,?4,?5,?6)",
            params![new_id(), page.id, position as i64, text, serde_json::to_string(vector)?, EMBEDDING_MODEL],
        )?;
    }
    tx.execute(
        "INSERT INTO page_index(page_id,embedding_model,indexed_at) VALUES(?1,?2,?3)
         ON CONFLICT(page_id) DO UPDATE SET embedding_model=excluded.embedding_model, indexed_at=excluded.indexed_at",
        params![page.id, EMBEDDING_MODEL, now_ms()],
    )?;
    tx.commit()?;
    Ok(true)
}

/// Set aside a page that couldn't be embedded, so it isn't retried every few
/// seconds. Editing the page clears this and queues it again.
pub fn mark_failed(conn: &Connection, page_id: &str) -> AppResult<()> {
    conn.execute(
        "INSERT INTO page_index(page_id,embedding_model,indexed_at) VALUES(?1,'failed:'||?2,?3)
         ON CONFLICT(page_id) DO UPDATE SET embedding_model=excluded.embedding_model, indexed_at=excluded.indexed_at",
        params![page_id, EMBEDDING_MODEL, now_ms()],
    )?;
    Ok(())
}

/// Pages filed under a client: the client page, its subpages, and pages
/// linked to it.
pub fn client_pages(conn: &Connection, client: &str) -> AppResult<HashSet<String>> {
    let mut stmt = conn.prepare(
        "WITH RECURSIVE tree(id) AS (SELECT ?1 UNION SELECT p.id FROM page p JOIN tree ON p.parent_id=tree.id)
         SELECT id FROM tree UNION SELECT source_page_id FROM link WHERE kind='task_of' AND target_page_id=?1",
    )?;
    let ids = stmt
        .query_map([client], |r| r.get::<_, String>(0))?
        .collect::<Result<HashSet<_>, _>>()?;
    Ok(ids)
}

/// Every indexed page passage a question may see, with its stored vector,
/// ready for `meeting_ai::score_candidates`.
pub fn retrieval_rows(conn: &Connection, client_id: Option<&str>) -> AppResult<Vec<(Passage, String)>> {
    let allowed = client_id.map(|c| client_pages(conn, c)).transpose()?;
    let mut stmt = conn.prepare(
        "SELECT c.id,c.page_id,p.title,c.text,p.updated_at,c.embedding FROM page_chunk c JOIN page p ON p.id=c.page_id
         WHERE p.deleted_at IS NULL AND c.embedding_model=?1 ORDER BY c.page_id,c.position LIMIT 50000",
    )?;
    let rows = stmt.query_map([EMBEDDING_MODEL], |r| {
        Ok((
            Passage {
                id: format!("pagechunk:{}", r.get::<_, String>(0)?),
                page_id: r.get(1)?,
                title: r.get(2)?,
                block_id: None,
                timestamp: None,
                text: r.get(3)?,
                started_at: r.get(4)?,
                kind: SourceKind::Page,
            },
            r.get::<_, String>(5)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let row = row?;
        if allowed.as_ref().map_or(true, |ids| ids.contains(&row.0.page_id)) {
            out.push(row);
        }
    }
    Ok(out)
}

/// The best page passages for a question: close matches by meaning, one per
/// page, topped up with keyword matches for pages not indexed yet.
pub fn best_pages(
    scored: Vec<(Passage, f32)>,
    keyword: Vec<Passage>,
    limit: usize,
) -> Vec<Passage> {
    let mut scored = scored;
    scored.sort_by(|a, b| b.1.total_cmp(&a.1));
    let floor = scored.first().map_or(1.0, |(_, s)| (s - 0.1).max(0.5));
    let mut chosen: Vec<Passage> = Vec::new();
    for (passage, score) in scored {
        if score < floor || chosen.len() == limit {
            break;
        }
        if !chosen.iter().any(|p| p.page_id == passage.page_id) {
            chosen.push(passage);
        }
    }
    for passage in keyword {
        if chosen.len() == limit {
            break;
        }
        if !chosen.iter().any(|p| p.page_id == passage.page_id) {
            chosen.push(passage);
        }
    }
    chosen
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        db::Db,
        store::{documents, pages},
    };

    fn page(c: &Connection, parent: Option<String>, title: &str, body: &str) -> String {
        let p = pages::core::create(c, parent, title.into(), "doc".into()).unwrap();
        documents::core::update(c, &p.id, &format!(r#"[{{"type":"paragraph","content":"{body}"}}]"#)).unwrap();
        // Old enough to index.
        c.execute("UPDATE page SET updated_at=0 WHERE id=?1", [&p.id]).unwrap();
        p.id
    }

    #[test]
    fn pages_are_indexed_once_and_again_after_an_edit() {
        let db = Db::open_in_memory().unwrap();
        let mut c = db.conn.lock().unwrap();
        let id = page(&c, None, "Acme contract", "The renewal date is 1 March.");
        let todo = pending(&c, 30_000, 10).unwrap();
        assert_eq!(todo.len(), 1);
        let chunks = passages(&todo[0]);
        let vector = vec![1.0; 768];
        assert!(replace(&mut c, &todo[0], &chunks, &vec![vector.clone(); chunks.len()]).unwrap());
        assert!(pending(&c, 30_000, 10).unwrap().is_empty());
        assert_eq!(retrieval_rows(&c, None).unwrap().len(), 1);
        // An edit takes it out of search, keeps the vector, and queues it.
        documents::core::update(&c, &id, r#"[{"type":"paragraph","content":"The renewal date is 1 April."}]"#).unwrap();
        c.execute("UPDATE page SET updated_at=0 WHERE id=?1", [&id]).unwrap();
        assert!(retrieval_rows(&c, None).unwrap().is_empty());
        assert_eq!(cached_vectors(&c, &id).unwrap().len(), 1);
        let again = pending(&c, 30_000, 10).unwrap();
        assert_eq!(again.len(), 1);
        // A page that can't be embedded waits for an edit.
        mark_failed(&c, &again[0].id).unwrap();
        assert!(pending(&c, 30_000, 10).unwrap().is_empty());
        // A change while embedding is noticed.
        documents::core::update(&c, &id, r#"[{"type":"paragraph","content":"Changed again."}]"#).unwrap();
        assert!(!replace(&mut c, &again[0], &passages(&again[0]), &[vector]).unwrap());
    }

    #[test]
    fn recent_edits_meeting_pages_and_agent_notes_wait() {
        let db = Db::open_in_memory().unwrap();
        let c = db.conn.lock().unwrap();
        let fresh = pages::core::create(&c, None, "Typing".into(), "doc".into()).unwrap();
        documents::core::update(&c, &fresh.id, r#"[{"type":"paragraph","content":"still typing"}]"#).unwrap();
        let meeting = page(&c, None, "Meeting", "transcript");
        c.execute("INSERT INTO meeting(id,page_id,started_at) VALUES('m',?1,0)", [&meeting]).unwrap();
        let note = page(&c, None, "Agent note", "raw text");
        c.execute("INSERT INTO note_ai(page_id,raw_text,created_at) VALUES(?1,'raw',0)", [&note]).unwrap();
        assert!(pending(&c, 30_000, 10).unwrap().is_empty());
    }

    #[test]
    fn client_scope_and_best_matches() {
        let db = Db::open_in_memory().unwrap();
        let c = db.conn.lock().unwrap();
        let client = page(&c, None, "Acme", "Client notes");
        let filed = page(&c, Some(client.clone()), "Acme contract", "Renewal in March");
        let other = page(&c, None, "Globex", "Renewal in June");
        let allowed = client_pages(&c, &client).unwrap();
        assert!(allowed.contains(&filed) && !allowed.contains(&other));
        let passage = |page_id: &str| Passage {
            id: format!("x{page_id}"),
            page_id: page_id.into(),
            title: String::new(),
            block_id: None,
            timestamp: None,
            text: String::new(),
            started_at: 0,
            kind: SourceKind::Page,
        };
        let chosen = best_pages(
            vec![(passage("a"), 0.8), (passage("a"), 0.79), (passage("b"), 0.75), (passage("c"), 0.4)],
            vec![passage("d"), passage("b")],
            3,
        );
        let ids: Vec<&str> = chosen.iter().map(|p| p.page_id.as_str()).collect();
        assert_eq!(ids, ["a", "b", "d"]);
    }
}
