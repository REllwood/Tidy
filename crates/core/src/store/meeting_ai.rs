//! Transcript-only retrieval and persisted, retryable meeting processing.
use crate::{
    db::{new_id, now_ms},
    error::{AppError, AppResult},
    llm::MeetingSummary,
};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

/// Bumping this re-indexes every meeting (summaries are kept). v3 embeds each
/// passage with its meeting's name and date.
pub const EMBEDDING_MODEL: &str = "nomic-v1.5-q8-3e243421-context-v3";

/// Where a piece of evidence came from. Transcript passages are what the
/// meeting search indexes; notes are the verified points saved for a meeting;
/// pages are the person's own written notes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    #[default]
    Transcript,
    Notes,
    Page,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Passage {
    pub id: String,
    pub page_id: String,
    pub title: String,
    pub block_id: Option<String>,
    pub timestamp: Option<String>,
    pub text: String,
    pub started_at: i64,
    #[serde(default)]
    pub kind: SourceKind,
}

/// The text a passage is embedded with. The meeting's name and date travel
/// with every passage, so "the budget review" or "in March" can find it.
pub fn document_text(p: &Passage) -> String {
    format!(
        "search_document: {} ({})\n{}",
        p.title,
        meeting_date(p.started_at),
        p.text
    )
}
#[derive(Serialize)]
pub struct MeetingJob {
    pub page_id: String,
    pub title: String,
    pub state: String,
    pub error: Option<String>,
    pub summary: Option<MeetingSummary>,
    pub indexed: bool,
    pub client_id: Option<String>,
    pub client_name: Option<String>,
    pub passage_count: i64,
    pub started_at: i64,
    /// The notes layout these notes use, chosen or picked from the title.
    pub template: String,
}

pub fn list(conn: &Connection) -> AppResult<Vec<MeetingJob>> {
    let mut stmt = conn.prepare(
        "SELECT a.page_id,p.title,a.state,a.error,a.summary,
        EXISTS(SELECT 1 FROM meeting_chunk c WHERE c.page_id=a.page_id AND c.embedding IS NOT NULL AND c.embedding_model=?1),
        client.id,client.title,(SELECT count(*) FROM meeting_chunk c WHERE c.page_id=a.page_id AND c.embedding_model=?1),
        (SELECT min(started_at) FROM meeting WHERE page_id=a.page_id),
        a.template
        FROM meeting_ai a JOIN page p ON p.id=a.page_id
        LEFT JOIN page client ON client.id=(SELECT target_page_id FROM link WHERE source_page_id=p.id AND kind='task_of' ORDER BY target_page_id LIMIT 1) AND client.deleted_at IS NULL
        WHERE p.deleted_at IS NULL
        ORDER BY p.created_at DESC",
    )?;
    let rows = stmt.query_map([EMBEDDING_MODEL], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, Option<String>>(3)?,
            r.get::<_, Option<String>>(4)?,
            r.get::<_, bool>(5)?,
            r.get::<_, Option<String>>(6)?,
            r.get::<_, Option<String>>(7)?,
            r.get::<_, i64>(8)?,
            r.get::<_, i64>(9)?,
            r.get::<_, Option<String>>(10)?,
        ))
    })?;
    rows.map(|row| {
        let (
            page_id,
            title,
            state,
            error,
            summary,
            indexed,
            client_id,
            client_name,
            passage_count,
            started_at,
            template,
        ) = row?;
        let template = template
            .as_deref()
            .and_then(crate::store::note_templates::by_id)
            .unwrap_or_else(|| crate::store::note_templates::choose(&title))
            .id
            .to_string();
        Ok(MeetingJob {
            page_id,
            title,
            state,
            error,
            summary: summary.map(|s| serde_json::from_str(&s)).transpose()?,
            indexed,
            client_id,
            client_name,
            passage_count,
            started_at,
            template,
        })
    })
    .collect()
}

pub fn inline_text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Array(items) => items.iter().map(inline_text).collect::<Vec<_>>().join(""),
        Value::Object(obj) if obj.get("type").and_then(Value::as_str) == Some("wikilink") => {
            crate::vault::wikilink_title(value).to_string()
        }
        Value::Object(obj) => obj
            .get("text")
            .or_else(|| obj.get("content"))
            .map(inline_text)
            .unwrap_or_default(),
        _ => String::new(),
    }
}

/// Only the original transcript is evidence; AI summaries and unrelated notes are excluded.
pub fn passages(
    page_id: &str,
    title: &str,
    content: &str,
    started_at: i64,
) -> AppResult<Vec<Passage>> {
    let blocks: Vec<Value> = serde_json::from_str(content)?;
    let texts: Vec<String> = blocks.iter().map(|b| inline_text(&b["content"])).collect();
    let mut in_transcript = false;
    let mut turns = Vec::new();
    let mut speaker = String::new();
    for (index, block) in blocks.iter().enumerate() {
        let text = texts[index].clone();
        if block["type"] == "heading" {
            in_transcript = text.trim().eq_ignore_ascii_case("transcript");
            continue;
        }
        if !in_transcript || text.trim().is_empty() || text.contains("(No speech detected.)") {
            continue;
        }
        let next = texts[index + 1..].iter().find(|t| !t.trim().is_empty());
        if let Some(label) = speaker_label(&text, next.map(String::as_str)) {
            speaker = label;
            continue;
        }
        let timestamp = text
            .strip_prefix('[')
            .and_then(|s| s.split_once(']'))
            .map(|(t, _)| t.to_string());
        let text = if speaker.is_empty() {
            text
        } else {
            format!("{speaker}: {text}")
        };
        for part in text_windows(&text, 1200) {
            turns.push(Passage {
                id: new_id(),
                page_id: page_id.into(),
                title: title.into(),
                block_id: block["id"].as_str().map(str::to_owned),
                timestamp: timestamp.clone(),
                text: part,
                started_at,
                kind: SourceKind::Transcript,
            });
        }
    }
    // Keep short utterances with the surrounding conversation. Retain the final
    // turn as overlap so a question, correction or pronoun keeps its context.
    let mut out = Vec::new();
    let mut window: Vec<Passage> = Vec::new();
    let mut size = 0;
    for turn in turns {
        let n = turn.text.len();
        if size + n > 1600 && !window.is_empty() {
            out.push(combine_passages(&window));
            let overlap = window.last().filter(|p| p.text.len() <= 400).cloned();
            window.clear();
            if let Some(last) = overlap {
                window.push(last);
            }
            size = window.iter().map(|p| p.text.len() + 1).sum();
        }
        size += n + 1;
        window.push(turn);
    }
    if !window.is_empty() {
        out.push(combine_passages(&window));
    }
    Ok(out)
}
/// A line naming who speaks next: the generated "Speaker 2", or a name the
/// person typed over it ("Priya", "Jordan (Acme)") just before a timed line.
pub(crate) fn speaker_label(text: &str, next: Option<&str>) -> Option<String> {
    let t = text.trim();
    if t.starts_with('[') || t.contains('[') {
        return None;
    }
    let generated = t.starts_with("Speaker ") && t.len() < 40;
    let renamed = t.chars().count() <= 40
        && t.split_whitespace().count() <= 5
        && !t.ends_with(['.', '?', '!', ','])
        && next.is_some_and(|n| n.trim_start().starts_with('['));
    (generated || renamed).then(|| t.trim_end_matches(':').trim().to_string())
}

fn combine_passages(turns: &[Passage]) -> Passage {
    let mut passage = turns[0].clone();
    passage.id = new_id();
    passage.text = turns
        .iter()
        .map(|p| p.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    passage
}

/// Split on sentence/word boundaries, never in the middle of a UTF-8 character.
pub fn text_windows(text: &str, limit: usize) -> Vec<String> {
    // A byte limit also bounds token count for multilingual transcript windows.
    let limit = limit.max(4);
    let mut result = Vec::new();
    let mut rest = text.trim();
    while rest.len() > limit {
        let end = rest
            .char_indices()
            .map(|(i, _)| i)
            .take_while(|i| *i <= limit)
            .last()
            .unwrap_or(rest.len());
        let start = rest
            .char_indices()
            .map(|(i, _)| i)
            .find(|i| *i >= limit / 2)
            .unwrap_or(0);
        let split = rest[start..end]
            .rfind(['\n', '.', '!', '?'])
            .map(|i| start + i + 1)
            .or_else(|| rest[..end].rfind(char::is_whitespace))
            .filter(|i| *i > 0)
            .unwrap_or(end);
        result.push(rest[..split].trim().to_string());
        rest = rest[split..].trim();
    }
    if !rest.is_empty() {
        result.push(rest.to_string());
    }
    result
}

/// Mark old indexes for rebuilding without modifying their source transcripts.
pub fn queue_outdated(conn: &Connection) -> AppResult<usize> {
    Ok(conn.execute("UPDATE meeting_ai SET state='queued',error=NULL WHERE state='ready' AND EXISTS(SELECT 1 FROM meeting_chunk c WHERE c.page_id=meeting_ai.page_id AND c.embedding_model<>?1)", [EMBEDDING_MODEL])?)
}

/// The meeting's transcript passages and a fingerprint of the transcript
/// alone. Notes typed elsewhere on the page do not change the fingerprint, so
/// they neither discard work in progress nor force a new summary.
pub fn snapshot(conn: &Connection, id: &str) -> AppResult<(String, Vec<Passage>)> {
    let (title, content, started_at) = meeting_source(conn, id)?;
    let passages = passages(id, &title, &content, started_at)?;
    Ok((transcript_hash(&passages), passages))
}

fn meeting_source(conn: &Connection, id: &str) -> AppResult<(String, String, i64)> {
    Ok(conn.query_row(
        "SELECT p.title,coalesce(p.content,'[]'),min(m.started_at) FROM page p JOIN meeting m ON m.page_id=p.id WHERE p.id=?1 AND p.deleted_at IS NULL GROUP BY p.id",[id],
        |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?)
}

pub fn transcript_hash(passages: &[Passage]) -> String {
    let mut hasher = blake3::Hasher::new();
    for p in passages {
        for part in [
            p.block_id.as_deref().unwrap_or(""),
            p.timestamp.as_deref().unwrap_or(""),
            p.text.as_str(),
        ] {
            hasher.update(part.as_bytes());
            hasher.update(&[0]);
        }
    }
    hasher.finalize().to_hex().to_string()
}

/// What the worker needs to decide whether a queued meeting only needs its
/// search index rebuilt, or new notes as well.
pub struct Stored {
    pub has_summary: bool,
    pub regenerate: bool,
    /// The saved notes still describe this transcript.
    pub summary_current: bool,
}
pub fn stored(conn: &Connection, id: &str, hash: &str) -> AppResult<Stored> {
    let (summary, source_hash, regenerate): (Option<String>, Option<String>, bool) = conn
        .query_row(
            "SELECT summary,source_hash,regenerate FROM meeting_ai WHERE page_id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
    // Notes saved before the transcript fingerprint existed recorded a hash
    // of the whole page. They are still current if the page is unchanged.
    let legacy = || -> AppResult<String> {
        let (_, content, _) = meeting_source(conn, id)?;
        Ok(blake3::hash(content.as_bytes()).to_hex().to_string())
    };
    let summary_current = match source_hash.as_deref() {
        Some(saved) if saved == hash => true,
        Some(saved) => saved == legacy()?,
        None => false,
    };
    Ok(Stored {
        has_summary: summary.is_some(),
        regenerate,
        summary_current,
    })
}

pub fn save_summary(
    conn: &Connection,
    id: &str,
    hash: &str,
    summary: &MeetingSummary,
) -> AppResult<bool> {
    let tx = conn.unchecked_transaction()?;
    if snapshot(&tx, id)?.0 != hash {
        return Ok(false);
    }
    // Marked moments come straight from the transcript, not the model.
    let mut summary = summary.clone();
    summary.moments = crate::store::meeting_notes::marked_moments(&tx, id)?;
    tx.execute("UPDATE meeting_ai SET summary=?2,source_hash=?3,state='indexing',error=NULL,regenerate=0,updated_at=?4 WHERE page_id=?1",
        params![id,serde_json::to_string(&summary)?,hash,now_ms()])?;
    tx.commit()?;
    Ok(true)
}

/// Keep the saved notes and move straight on to rebuilding the search index.
pub fn keep_summary(conn: &Connection, id: &str, hash: &str) -> AppResult<()> {
    conn.execute(
        "UPDATE meeting_ai SET source_hash=?2,state='indexing',error=NULL WHERE page_id=?1",
        params![id, hash],
    )?;
    Ok(())
}
pub fn replace_index(
    conn: &mut Connection,
    id: &str,
    hash: &str,
    chunks: &[Passage],
    embeddings: &[Vec<f32>],
) -> AppResult<bool> {
    if chunks.len() != embeddings.len() {
        return Err(AppError::Invalid("Embedding count mismatch".into()));
    }
    let tx = conn.transaction()?;
    if snapshot(&tx, id)?.0 != hash {
        return Ok(false);
    }
    tx.execute("DELETE FROM meeting_chunk WHERE page_id=?1", [id])?;
    for (position, (chunk, vector)) in chunks.iter().zip(embeddings).enumerate() {
        validate_vector(vector)?;
        tx.execute("INSERT INTO meeting_chunk(id,page_id,block_id,position,text,timestamp,embedding,embedding_model) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![chunk.id,id,chunk.block_id,position as i64,chunk.text,chunk.timestamp,serde_json::to_string(vector)?,EMBEDDING_MODEL])?;
    }
    tx.execute("UPDATE meeting_ai SET state='ready',error=NULL,source_hash=?2,updated_at=?3 WHERE page_id=?1",params![id,hash,now_ms()])?;
    tx.commit()?;
    Ok(true)
}
/// Vectors already worked out for this meeting's passages, by passage text,
/// including ones set aside by an edit. An edit that leaves most of the
/// transcript alone then needs only the changed passages embedded again.
pub fn cached_vectors(conn: &Connection, page_id: &str) -> AppResult<HashMap<String, Vec<f32>>> {
    let mut stmt = conn.prepare(
        "SELECT text,embedding FROM meeting_chunk WHERE page_id=?1 AND embedding IS NOT NULL AND embedding_model IN (?2,'stale:'||?2)",
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

pub fn validate_vector(v: &[f32]) -> AppResult<()> {
    if v.len() != 768 || v.iter().any(|n| !n.is_finite()) || v.iter().all(|n| *n == 0.0) {
        return Err(AppError::Invalid(
            "Search model returned an invalid embedding".into(),
        ));
    }
    Ok(())
}

pub fn is_overview_question(question: &str) -> bool {
    super::meeting_question::intent(question) == super::meeting_question::Intent::Overview
}

/// Overview queries need coverage, not a semantic search for generic question words.
fn overview_passages(passages: Vec<Passage>) -> Vec<Passage> {
    let mut groups: std::collections::BTreeMap<(i64, String), Vec<Passage>> =
        std::collections::BTreeMap::new();
    for p in passages {
        groups
            .entry((p.started_at, p.page_id.clone()))
            .or_default()
            .push(p);
    }
    let groups: Vec<_> = groups.into_values().collect();
    let mut selected = Vec::new();
    let mut seen = HashSet::new();
    let mut size = 0;
    // Opening establishes purpose; middle/end capture detail and final decisions.
    for fraction in [0.0, 1.0, 0.5, 0.25, 0.75, 0.125, 0.375, 0.625, 0.875] {
        for group in &groups {
            let index = ((group.len() - 1) as f64 * fraction).round() as usize;
            let p = &group[index];
            if seen.contains(&p.id) {
                continue;
            }
            if size + p.text.chars().count() > 12000 {
                continue;
            }
            size += p.text.chars().count();
            seen.insert(p.id.clone());
            selected.push(p.clone());
        }
    }
    // Include all remaining passages when the scope fits the context budget.
    for group in &groups {
        for p in group {
            if !seen.contains(&p.id) && size + p.text.chars().count() <= 12000 {
                size += p.text.chars().count();
                seen.insert(p.id.clone());
                selected.push(p.clone());
            }
        }
    }
    let order: HashMap<_, _> = groups
        .iter()
        .flatten()
        .enumerate()
        .map(|(i, p)| (p.id.clone(), i))
        .collect();
    selected.sort_by_key(|p| order[&p.id]);
    selected
}

#[derive(Serialize)]
pub struct Coverage {
    pub passages_used: usize,
    pub total_passages: usize,
    pub meetings_used: usize,
    pub total_meetings: usize,
}
pub fn coverage(
    conn: &Connection,
    sources: &[Passage],
    from: Option<i64>,
    until: Option<i64>,
    client_id: Option<&str>,
) -> AppResult<Coverage> {
    let (total_passages,total_meetings) = conn.query_row("SELECT count(DISTINCT c.id),count(DISTINCT p.id) FROM meeting_chunk c JOIN page p ON p.id=c.page_id JOIN meeting m ON m.page_id=p.id WHERE p.deleted_at IS NULL AND c.embedding_model=?1 AND (?2 IS NULL OR m.started_at>=?2) AND (?3 IS NULL OR m.started_at<?3) AND (?4 IS NULL OR EXISTS(SELECT 1 FROM link l JOIN page client ON client.id=l.target_page_id WHERE l.source_page_id=p.id AND l.kind='task_of' AND client.id=?4 AND client.deleted_at IS NULL))", params![EMBEDDING_MODEL,from,until,client_id],|r|Ok((r.get(0)?,r.get(1)?)))?;
    Ok(Coverage {
        passages_used: sources.len(),
        total_passages,
        meetings_used: sources
            .iter()
            .map(|s| &s.page_id)
            .collect::<HashSet<_>>()
            .len(),
        total_meetings,
    })
}

/// Reciprocal-rank fusion combines exact wording and semantic matches. The
/// three steps are separate so a caller can score the embeddings — the only
/// expensive part — without holding the database open across it.
pub fn retrieve(
    conn: &Connection,
    question: &str,
    vector: &[f32],
    from: Option<i64>,
    until: Option<i64>,
    client_id: Option<&str>,
) -> AppResult<Vec<Passage>> {
    let rows = retrieval_rows(conn, from, until, client_id)?;
    let candidates = score_candidates(vector, rows)?;
    let mut found = rank(conn, question, candidates)?;
    if !is_overview_question(question) {
        found.sort_by_key(|p| p.started_at);
    }
    Ok(found)
}

/// Every passage the question is allowed to see, with its stored embedding.
pub fn retrieval_rows(
    conn: &Connection,
    from: Option<i64>,
    until: Option<i64>,
    client_id: Option<&str>,
) -> AppResult<Vec<(Passage, String)>> {
    let mut stmt=conn.prepare("SELECT c.id,c.page_id,p.title,c.block_id,c.timestamp,c.text,min(m.started_at),c.embedding
        FROM meeting_chunk c JOIN page p ON p.id=c.page_id JOIN meeting m ON m.page_id=p.id
        WHERE p.deleted_at IS NULL AND c.embedding_model=?1 AND (?2 IS NULL OR m.started_at>=?2) AND (?3 IS NULL OR m.started_at<?3)
        AND (?4 IS NULL OR EXISTS(SELECT 1 FROM link l JOIN page client ON client.id=l.target_page_id WHERE l.source_page_id=p.id AND l.kind='task_of' AND client.id=?4 AND client.deleted_at IS NULL))
        GROUP BY c.id ORDER BY c.page_id,c.position LIMIT 50001")?;
    let rows = stmt.query_map(params![EMBEDDING_MODEL, from, until, client_id], |r| {
        Ok((
            Passage {
                id: r.get(0)?,
                page_id: r.get(1)?,
                title: r.get(2)?,
                block_id: r.get(3)?,
                timestamp: r.get(4)?,
                text: r.get(5)?,
                started_at: r.get(6)?,
                kind: SourceKind::Transcript,
            },
            r.get::<_, String>(7)?,
        ))
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Compare every passage with the question. This parses and multiplies out tens
/// of thousands of vectors, so it runs away from the database connection.
pub fn score_candidates(
    vector: &[f32],
    rows: Vec<(Passage, String)>,
) -> AppResult<Vec<(Passage, f32)>> {
    validate_vector(vector)?;
    if rows.len() > 50000 {
        return Err(AppError::Invalid(
            "Narrow the date range to search this many transcript passages".into(),
        ));
    }
    let mut candidates = Vec::with_capacity(rows.len());
    let norm = vector.iter().map(|v| v * v).sum::<f32>().sqrt();
    for (p, raw) in rows {
        let v: Vec<f32> = serde_json::from_str(&raw)?;
        validate_vector(&v)?;
        let similarity = vector.iter().zip(&v).map(|(a, b)| a * b).sum::<f32>()
            / (norm * v.iter().map(|v| v * v).sum::<f32>().sqrt());
        candidates.push((p, similarity));
    }
    Ok(candidates)
}

/// Pick the passages to answer from, fusing the semantic ranking above with an
/// exact-wording search.
pub fn rank(
    conn: &Connection,
    question: &str,
    candidates: Vec<(Passage, f32)>,
) -> AppResult<Vec<Passage>> {
    rank_for(conn, question, candidates, is_overview_question(question))
}

/// As `rank`, with the caller deciding whether the question wants an
/// overview (coverage of whole meetings) or a search.
pub fn rank_for(
    conn: &Connection,
    question: &str,
    mut candidates: Vec<(Passage, f32)>,
    overview: bool,
) -> AppResult<Vec<Passage>> {
    if question.trim().is_empty() {
        return Ok(vec![]);
    }
    if overview {
        return Ok(overview_passages(
            candidates.into_iter().map(|(p, _)| p).collect(),
        ));
    }
    candidates.sort_by(|a, b| b.1.total_cmp(&a.1));
    let eligible: HashSet<_> = candidates.iter().map(|(p, _)| p.id.clone()).collect();

    let terms = super::meeting_question::subject_terms(question)
        .into_iter()
        .take(32)
        .map(|t| format!("\"{t}\""))
        .collect::<Vec<_>>()
        .join(" OR ");
    let mut scores: HashMap<String, f32> = HashMap::new();
    let mut semantic_meetings = HashSet::new();
    let threshold = candidates
        .first()
        .map(|(_, s)| (s - 0.15).max(0.38))
        .unwrap_or(1.0);
    for (rank, (p, score)) in candidates.iter().enumerate() {
        let first_for_meeting =
            semantic_meetings.len() < 30 && semantic_meetings.insert(&p.page_id);
        if *score >= threshold && (rank < 30 || first_for_meeting) {
            scores.insert(p.id.clone(), 1.0 / (60.0 + rank as f32));
        }
    }
    if !terms.is_empty() {
        let mut fts = conn.prepare(
            "SELECT id FROM meeting_chunk_fts WHERE meeting_chunk_fts MATCH ?1 ORDER BY rank",
        )?;
        let mut rank = 0;
        for id in fts.query_map([terms], |r| r.get::<_, String>(0))? {
            let id = id?;
            if eligible.contains(&id) {
                *scores.entry(id).or_default() += 1.0 / (60.0 + rank as f32);
                rank += 1;
            }
            if rank >= 30 {
                break;
            }
        }
    }
    let mut found: Vec<_> = candidates
        .into_iter()
        .filter_map(|(p, _)| scores.get(&p.id).map(|s| (p, *s)))
        .collect();
    found.sort_by(|a, b| b.1.total_cmp(&a.1));
    // Reserve evidence for distinct meetings before filling remaining places.
    // This stops one long transcript crowding out a later change or decision.
    let mut meetings = HashSet::new();
    let mut selected = Vec::new();
    let mut remaining = Vec::new();
    for (passage, _) in found {
        if selected.len() < 8 && meetings.insert(passage.page_id.clone()) {
            selected.push(passage);
        } else {
            remaining.push(passage);
        }
    }
    selected.extend(remaining.into_iter().take(10 - selected.len()));
    let mut used = 0;
    selected.retain(|p| {
        used += p.text.chars().count();
        used <= 12000
    });
    // Most relevant first, so a caller short of room can drop from the end.
    Ok(selected)
}

/// The person's own written pages that mention the question's words, as
/// evidence alongside the transcripts. Meeting pages are left out (their
/// transcripts are already searched), and with a client chosen only pages
/// filed under that client count.
pub fn page_passages(
    conn: &Connection,
    terms: &[String],
    client_id: Option<&str>,
) -> AppResult<Vec<Passage>> {
    if terms.is_empty() {
        return Ok(vec![]);
    }
    let allowed: Option<HashSet<String>> = match client_id {
        Some(client) => {
            let mut stmt = conn.prepare(
                "WITH RECURSIVE tree(id) AS (SELECT ?1 UNION SELECT p.id FROM page p JOIN tree ON p.parent_id=tree.id)
                 SELECT id FROM tree UNION SELECT source_page_id FROM link WHERE kind='task_of' AND target_page_id=?1",
            )?;
            let ids = stmt
                .query_map([client], |r| r.get::<_, String>(0))?
                .collect::<Result<HashSet<_>, _>>()?;
            Some(ids)
        }
        None => None,
    };
    let query = terms
        .iter()
        .take(16)
        .map(|t| format!("\"{}\"*", t.replace('"', "")))
        .collect::<Vec<_>>()
        .join(" OR ");
    let mut stmt = conn.prepare(
        "SELECT p.id,p.title,coalesce(p.body_text,''),p.updated_at FROM page_fts f JOIN page p ON p.id=f.page_id
         WHERE page_fts MATCH ?1 AND p.deleted_at IS NULL AND p.type='doc'
         AND NOT EXISTS(SELECT 1 FROM meeting m WHERE m.page_id=p.id)
         ORDER BY bm25(page_fts,0.0,5.0,1.0) LIMIT 24",
    )?;
    let hits = stmt
        .query_map([query], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let lower: Vec<String> = terms.iter().map(|t| t.to_lowercase()).collect();
    let mut found = Vec::new();
    for (id, title, body, updated_at) in hits {
        if allowed.as_ref().is_some_and(|ids| !ids.contains(&id)) || body.trim().is_empty() {
            continue;
        }
        // The part of the page that mentions the most of the question's words.
        let best = text_windows(&body, 1400)
            .into_iter()
            .max_by_key(|window| {
                let text = window.to_lowercase();
                lower.iter().filter(|t| text.contains(t.as_str())).count()
            })
            .unwrap_or_default();
        found.push(Passage {
            id: format!("page:{id}"),
            page_id: id,
            title,
            block_id: None,
            timestamp: None,
            text: best,
            started_at: updated_at,
            kind: SourceKind::Page,
        });
        if found.len() == 3 {
            break;
        }
    }
    Ok(found)
}

/// The notes' overview, when it says more than a repeat of one point (notes
/// from earlier versions used their first point as the overview).
pub fn overview_of(summary: &MeetingSummary) -> Option<&str> {
    let overview = summary.summary.trim();
    let repeated = summary
        .sections
        .iter()
        .flat_map(|s| &s.points)
        .chain(&summary.decisions)
        .chain(&summary.action_items)
        .chain(&summary.open_questions)
        .any(|p| p.trim() == overview);
    (!overview.is_empty() && !repeated).then_some(overview)
}

fn notes_source(job: &MeetingJob) -> Passage {
    let summary = job.summary.clone().unwrap_or_default();
    let text = overview_of(&summary)
        .map(str::to_owned)
        .or_else(|| {
            summary
                .sections
                .first()
                .map(|s| s.points.iter().take(2).cloned().collect::<Vec<_>>().join(" "))
        })
        .unwrap_or_default();
    Passage {
        id: format!("notes:{}", job.page_id),
        page_id: job.page_id.clone(),
        title: job.title.clone(),
        block_id: None,
        timestamp: None,
        text,
        started_at: job.started_at,
        kind: SourceKind::Notes,
    }
}

fn heading(job: &MeetingJob, source: usize) -> String {
    let date = chrono::DateTime::from_timestamp_millis(job.started_at)
        .map(|d| d.with_timezone(&chrono::Local).format("%-d %b %Y").to_string())
        .unwrap_or_default();
    format!("## {} · {date} [{source}]", job.title)
}

/// Describe meetings from their saved notes: the full notes for one meeting,
/// the overview and outcomes for several. Newest first.
pub fn recap(jobs: &[&MeetingJob]) -> (String, Vec<Passage>) {
    let mut lines = Vec::new();
    let mut sources = Vec::new();
    let single = jobs.len() == 1;
    for job in jobs {
        let Some(summary) = &job.summary else {
            continue;
        };
        sources.push(notes_source(job));
        lines.push(heading(job, sources.len()));
        if let Some(overview) = overview_of(summary) {
            lines.push(overview.to_string());
        }
        let limit = if single { usize::MAX } else { 4 };
        if single {
            for section in &summary.sections {
                lines.push(format!("### {}", section.heading));
                lines.extend(section.points.iter().map(|p| format!("- {p}")));
            }
        } else if overview_of(summary).is_none() {
            let points: Vec<_> = summary.sections.iter().flat_map(|s| &s.points).take(3).collect();
            lines.extend(points.iter().map(|p| format!("- {p}")));
        }
        for (title, prefix, items) in [
            ("Decisions", "Decided: ", &summary.decisions),
            ("Next steps", "Next step: ", &summary.action_items),
            ("Open questions", "Still open: ", &summary.open_questions),
        ] {
            if items.is_empty() {
                continue;
            }
            if single {
                lines.push(format!("### {title}"));
            }
            for item in items.iter().take(limit) {
                lines.push(if single {
                    format!("- {item}")
                } else {
                    format!("- {prefix}{item}")
                });
            }
            if items.len() > limit {
                lines.push(format!(
                    "- …and {} more in the meeting notes",
                    items.len() - limit
                ));
            }
        }
    }
    (lines.join("\n"), sources)
}

/// One saved note item (an action, decision or open question) and the
/// meeting it came from.
pub struct NoteItem<'a> {
    pub job: &'a MeetingJob,
    pub text: &'a str,
}

pub fn note_items<'a>(
    jobs: &[&'a MeetingJob],
    intent: super::meeting_question::Intent,
) -> Vec<NoteItem<'a>> {
    use super::meeting_question::Intent;
    let mut items = Vec::new();
    for job in jobs {
        let Some(summary) = &job.summary else {
            continue;
        };
        let list = match intent {
            Intent::Actions => &summary.action_items,
            Intent::Decisions => &summary.decisions,
            Intent::OpenQuestions => &summary.open_questions,
            _ => continue,
        };
        items.extend(list.iter().map(|text| NoteItem { job, text }));
    }
    items
}

/// List the chosen note items under their meetings, newest meeting first.
pub fn items_answer(items: &[&NoteItem]) -> (String, Vec<Passage>) {
    let mut lines = Vec::new();
    let mut sources: Vec<Passage> = Vec::new();
    let mut order: Vec<&MeetingJob> = Vec::new();
    for item in items {
        if !order.iter().any(|j| j.page_id == item.job.page_id) {
            order.push(item.job);
        }
    }
    order.sort_by_key(|j| std::cmp::Reverse(j.started_at));
    for job in order {
        sources.push(notes_source(job));
        lines.push(heading(job, sources.len()));
        for item in items.iter().filter(|i| i.job.page_id == job.page_id) {
            lines.push(format!("- {}", item.text));
        }
    }
    (lines.join("\n"), sources)
}

pub fn meeting_date(started_at: i64) -> String {
    chrono::DateTime::from_timestamp_millis(started_at)
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_default()
}

/// A conservative guard against invented amounts/dates or incomplete source lists.
/// This is not a proof of semantic correctness; users still see the source passages.
pub fn check_numeric_evidence(answer: &str, sources: &[Passage]) -> AppResult<()> {
    let evidence = sources
        .iter()
        .map(|s| format!("{} {}", s.text, meeting_date(s.started_at)))
        .collect::<Vec<_>>()
        .join("\n");
    check_numeric_text(answer, &evidence)
}

pub fn check_numeric_text(answer: &str, evidence: &str) -> AppResult<()> {
    let mut supported = numbers(evidence, false);
    // Transcripts often spell numbers out ("the twenty eighth", "two thousand
    // four hundred dollars") where a note writes 28 or 2,400.
    supported.extend(word_numbers(evidence));
    if !numbers(answer, true).is_subset(&supported) {
        return Err(AppError::Invalid("The answer included a number that its cited transcript passages do not support. Try a more focused question.".into()));
    }
    Ok(())
}

/// Every number in a text, as written digits ("09" and "9" are the same).
/// In a claim, the number in "Speaker 2" names a voice rather than stating an
/// amount, so it is left out when `skip_speakers` is set.
fn numbers(text: &str, skip_speakers: bool) -> HashSet<String> {
    let text = text.replace(',', "");
    let mut found = HashSet::new();
    let mut previous = String::new();
    for word in text.split(|c: char| !(c.is_alphanumeric() || c == '.')) {
        let word = word.trim_matches('.');
        if word.is_empty() {
            continue;
        }
        let speaker = skip_speakers && previous == "speaker";
        previous = word.to_lowercase();
        if speaker {
            continue;
        }
        let mut run = String::new();
        for c in word.chars().chain(std::iter::once(' ')) {
            if c.is_ascii_digit() || (c == '.' && !run.is_empty()) {
                run.push(c);
            } else if !run.is_empty() {
                let number = run.trim_end_matches('.');
                let (whole, fraction) = number.split_once('.').unwrap_or((number, ""));
                let whole = whole.trim_start_matches('0');
                let whole = if whole.is_empty() { "0" } else { whole };
                found.insert(if fraction.is_empty() {
                    whole.to_string()
                } else {
                    format!("{whole}.{fraction}")
                });
                run.clear();
            }
        }
    }
    found
}

/// Names in a claim must sit near its quote. A name is a word the evidence
/// always capitalises and uses mid-sentence at least once (Priya, Balmain,
/// October), so a note can't pin what Mia said on Priya just because Priya
/// spoke elsewhere in the meeting.
pub fn names_supported(claim: &str, near: &str, evidence: &str) -> bool {
    #[derive(Default)]
    struct Seen {
        lower: bool,
        capital_mid_sentence: bool,
    }
    let mut seen: HashMap<String, Seen> = HashMap::new();
    let mut sentence_start = true;
    let mut word = String::new();
    // Title case only: Priya, Balmain, October. Acronyms such as SEO or URL
    // are terms, not people or places, and are left alone.
    let title_case = |w: &str| {
        let mut chars = w.chars();
        chars.next().is_some_and(char::is_uppercase) && chars.all(char::is_lowercase)
    };
    let mut flush = |word: &mut String, sentence_start: bool| {
        if word.chars().count() >= 2 {
            let entry = seen.entry(word.to_lowercase()).or_default();
            if title_case(word) {
                entry.capital_mid_sentence |= !sentence_start;
            } else if word.chars().all(char::is_lowercase) {
                entry.lower = true;
            }
        }
        word.clear();
    };
    for c in evidence.chars() {
        if c.is_alphabetic() {
            word.push(c);
            continue;
        }
        if !word.is_empty() {
            flush(&mut word, sentence_start);
            sentence_start = false;
        }
        if matches!(c, '.' | '!' | '?' | ':' | ']' | '\n') {
            sentence_start = true;
        }
    }
    flush(&mut word, sentence_start);
    let near = near.to_lowercase();
    let near_words: HashSet<&str> = near.split(|c: char| !c.is_alphabetic()).collect();
    // A name can also come from elsewhere in the meeting ("Sam does redirects
    // on staging" in a recap), as long as one clause pairs that name with at
    // least two of the claim's own words. A clause, not a sentence: a recap
    // listing everyone's tasks mustn't lend one person's name to another's task.
    let claim_words = key_words(claim);
    let clauses: Vec<(HashSet<String>, HashSet<String>)> = clauses(evidence)
        .into_iter()
        .map(|clause| {
            let lower = clause.to_lowercase();
            let words: HashSet<String> = lower
                .split(|c: char| !c.is_alphabetic())
                .filter(|w| !w.is_empty())
                .map(str::to_owned)
                .collect();
            (words, key_words(clause))
        })
        .collect();
    claim
        .split(|c: char| !c.is_alphabetic())
        .filter(|w| w.chars().count() >= 2)
        .map(str::to_lowercase)
        .filter(|w| {
            seen.get(w)
                .is_some_and(|s| s.capital_mid_sentence && !s.lower)
        })
        .all(|name| {
            near_words.contains(name.as_str())
                || clauses.iter().any(|(words, keys)| {
                    words.contains(&name) && keys.intersection(&claim_words).count() >= 2
                })
        })
}

/// Split text into clauses at sentence punctuation, commas and semicolons
/// followed by a space, and line breaks, so "$2,400" and "1.5" stay whole.
fn clauses(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        let next_is_space = chars.peek().is_some_and(|(_, n)| n.is_whitespace());
        if c == '\n' || (matches!(c, '.' | '!' | '?' | ';' | ',') && next_is_space) {
            out.push(&text[start..i]);
            start = i + c.len_utf8();
        }
    }
    out.push(&text[start..]);
    out.into_iter().filter(|c| !c.trim().is_empty()).collect()
}

/// Numbers in a claim must be near its quote, be the meeting's own date, or
/// sit in one clause of the evidence with at least two of the claim's own
/// words ("The launch is the twenty eighth of October" backs "Launch moved
/// to 28 October" even when the quote chosen was about the delay).
pub fn numbers_supported(claim: &str, near: &str, evidence: &str) -> bool {
    let wanted = numbers(claim, true);
    if wanted.is_empty() {
        return true;
    }
    let mut found = numbers(near, false);
    found.extend(word_numbers(near));
    let claim_words = key_words(claim);
    for clause in clauses(evidence) {
        if key_words(clause).intersection(&claim_words).count() >= 2 {
            found.extend(numbers(clause, false));
            found.extend(word_numbers(clause));
        }
    }
    wanted.is_subset(&found)
}

/// The words that carry a claim's meaning, roughly stemmed ("redirects" and
/// "redirect" match), for pairing a name with what it's said about.
fn key_words(text: &str) -> HashSet<String> {
    const COMMON: &[&str] = &[
        "will", "with", "that", "this", "have", "from", "they", "their", "them", "about",
        "when", "what", "which", "there", "were", "been", "does", "doing", "also", "just",
        "into", "your", "yeah", "okay", "right", "speaker", "sure", "some", "then", "than",
    ];
    text.to_lowercase()
        .split(|c: char| !c.is_alphabetic())
        .filter(|w| w.chars().count() >= 4 && !COMMON.contains(w))
        .map(|w| {
            ["ing", "ed", "es", "s"]
                .iter()
                .find_map(|end| w.strip_suffix(end).filter(|stem| stem.len() >= 4))
                .unwrap_or(w)
                .to_owned()
        })
        .collect()
}

/// Numbers written as words, cardinal or ordinal: "six" 6, "twenty eighth"
/// 28, "two thousand four hundred" 2400, "a hundred and fifty" 150.
fn word_numbers(text: &str) -> HashSet<String> {
    fn value(word: &str) -> Option<(u64, bool)> {
        const UNITS: [&str; 20] = [
            "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten",
            "eleven", "twelve", "thirteen", "fourteen", "fifteen", "sixteen", "seventeen",
            "eighteen", "nineteen",
        ];
        const ORDINALS: [&str; 20] = [
            "zeroth", "first", "second", "third", "fourth", "fifth", "sixth", "seventh", "eighth",
            "ninth", "tenth", "eleventh", "twelfth", "thirteenth", "fourteenth", "fifteenth",
            "sixteenth", "seventeenth", "eighteenth", "nineteenth",
        ];
        const TENS: [&str; 8] = [
            "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety",
        ];
        const TENTHS: [&str; 8] = [
            "twentieth", "thirtieth", "fortieth", "fiftieth", "sixtieth", "seventieth",
            "eightieth", "ninetieth",
        ];
        // (value, is a scale word such as hundred)
        if let Some(i) = UNITS.iter().position(|w| *w == word) {
            return Some((i as u64, false));
        }
        if let Some(i) = ORDINALS.iter().position(|w| *w == word) {
            return Some((i as u64, false));
        }
        if let Some(i) = TENS.iter().position(|w| *w == word) {
            return Some((20 + 10 * i as u64, false));
        }
        if let Some(i) = TENTHS.iter().position(|w| *w == word) {
            return Some((20 + 10 * i as u64, false));
        }
        match word {
            "hundred" | "hundredth" => Some((100, true)),
            "thousand" | "thousandth" => Some((1_000, true)),
            "million" | "millionth" => Some((1_000_000, true)),
            _ => None,
        }
    }
    let mut found = HashSet::new();
    let (mut total, mut current, mut any) = (0u64, 0u64, false);
    let mut flush = |total: &mut u64, current: &mut u64, any: &mut bool| {
        if *any {
            found.insert((*total + *current).to_string());
        }
        (*total, *current, *any) = (0, 0, false);
    };
    let words: Vec<String> = text
        .to_lowercase()
        .split(|c: char| !c.is_alphabetic())
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect();
    for (i, word) in words.iter().enumerate() {
        let next_is_number = words.get(i + 1).and_then(|w| value(w)).is_some();
        match value(word) {
            Some((scale, true)) => {
                let base = if current == 0 { 1 } else { current };
                if scale >= 1_000 {
                    total += base * scale;
                    current = 0;
                } else {
                    current = base * scale;
                }
                any = true;
            }
            Some((n, false)) => {
                // "twenty eighth" adds up; "six seven" are two numbers.
                let joins = (current % 10 == 0 && current % 100 != 0 && n < 10)
                    || (current % 100 == 0 && n < 100);
                if any && joins {
                    current += n;
                } else {
                    flush(&mut total, &mut current, &mut any);
                    current = n;
                }
                any = true;
            }
            // "a hundred", "two hundred and fifty"
            None if (word == "and" && any && next_is_number)
                || (word == "a" && !any && words.get(i + 1).is_some_and(|w| value(w).is_some_and(|(_, scale)| scale))) => {}
            None => flush(&mut total, &mut current, &mut any),
        }
    }
    flush(&mut total, &mut current, &mut any);
    found
}

/// A word of a transcript or quote, compared without case, punctuation or
/// apostrophes, with where it sits in the original text.
struct Word {
    norm: String,
    start: usize,
    end: usize,
}

/// Split text into comparable words. Timestamps such as [01:02] are skipped:
/// models routinely drop them when quoting.
fn words(text: &str) -> Vec<Word> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < text.len() {
        let c = text[i..].chars().next().unwrap_or(' ');
        if c == '[' {
            if let Some(close) = text[i..].find(']').filter(|close| *close <= 12) {
                let inner = &text[i + 1..i + close];
                if !inner.is_empty() && inner.chars().all(|c| c.is_ascii_digit() || c == ':') {
                    i += close + 1;
                    continue;
                }
            }
        }
        if c.is_alphanumeric() {
            let start = i;
            let mut norm = String::new();
            while i < text.len() {
                let c = text[i..].chars().next().unwrap_or(' ');
                let apostrophe = matches!(c, '\'' | '’' | '‘')
                    && text[i + c.len_utf8()..]
                        .chars()
                        .next()
                        .is_some_and(char::is_alphanumeric);
                if c.is_alphanumeric() {
                    norm.extend(c.to_lowercase());
                } else if !apostrophe {
                    break;
                }
                i += c.len_utf8();
            }
            out.push(Word {
                norm,
                start,
                end: i,
            });
            continue;
        }
        i += c.len_utf8();
    }
    out
}

/// Find where a model's quote sits in the evidence and return the evidence's
/// own wording for it. Small models tidy quotes: they change case and
/// punctuation, drop timestamps and fillers ("um", a repeated word), or leave
/// out a short aside. Those still count, as long as at least 80% of the
/// quote's words appear in order within a span barely longer than the quote.
/// A paraphrase does not, and neither does a quote with a name or number
/// the transcript doesn't have.
/// The last "[hh:mm:ss]" or "[mm:ss]" stamp in some text.
fn last_stamp(text: &str) -> Option<String> {
    let mut rest = text;
    let mut found = None;
    while let Some(open) = rest.find('[') {
        let after = &rest[open + 1..];
        match after.find(']') {
            Some(close)
                if close >= 4
                    && after[..close].contains(':')
                    && after[..close].chars().all(|c| c.is_ascii_digit() || c == ':') =>
            {
                found = Some(after[..close].to_string());
                rest = &after[close + 1..];
            }
            _ => rest = after,
        }
    }
    found
}

/// When a quote was said: the stamp of the transcript line it is found in.
pub fn time_of_quote(transcript: &str, quote: &str) -> Option<String> {
    let found = locate_quote(transcript, quote)?;
    let start = found.as_ptr() as usize - transcript.as_ptr() as usize;
    // The line's own stamp comes before the quote on the same line.
    last_stamp(&transcript[..start])
}

/// When each point of the notes was said, from the quotes that support them.
/// `quotes` pairs each checked point's text with its quote; a next step split
/// into several points shares its quote.
pub fn point_times(
    summary: &MeetingSummary,
    quotes: &[(String, String)],
    transcript: &str,
) -> std::collections::BTreeMap<String, String> {
    let points = summary
        .sections
        .iter()
        .flat_map(|s| s.points.iter())
        .chain(&summary.decisions)
        .chain(&summary.action_items)
        .chain(&summary.open_questions);
    let mut times = std::collections::BTreeMap::new();
    for point in points {
        let quote = quotes
            .iter()
            .find(|(text, _)| text == point)
            .or_else(|| {
                let bare = point.trim_end_matches('.');
                quotes.iter().find(|(text, _)| text.contains(bare))
            })
            .map(|(_, quote)| quote);
        if let Some(time) = quote.and_then(|q| time_of_quote(transcript, q)) {
            times.insert(point.clone(), time);
        }
    }
    times
}

pub fn locate_quote<'a>(evidence: &'a str, quote: &str) -> Option<&'a str> {
    let quote = quote.replace("...", " ").replace('…', " ");
    let quote_words = words(&quote);
    let q: Vec<String> = quote_words.iter().map(|w| w.norm.clone()).collect();
    if q.len() < 3 || q.iter().map(String::len).sum::<usize>() < 10 {
        return None;
    }
    let e = words(evidence);
    if e.is_empty() {
        return None;
    }
    let span = |from: usize, to: usize| Some(&evidence[e[from].start..e[to].end]);
    // The usual case: the same words in the same order.
    if let Some(at) = e
        .windows(q.len())
        .position(|w| w.iter().zip(&q).all(|(a, b)| &a.norm == b))
    {
        return span(at, at + q.len() - 1);
    }
    let slack = (q.len() / 4).max(2);
    let openers: HashSet<&str> = q.iter().take(3).map(String::as_str).collect();
    let mut best: Option<(usize, usize, usize)> = None;
    for start in 0..e.len() {
        if !openers.contains(e[start].norm.as_str()) {
            continue;
        }
        let window = &e[start..e.len().min(start + q.len() + slack)];
        // Longest common subsequence, tracking where the match ends.
        let mut row = vec![0usize; window.len() + 1];
        for qw in &q {
            let mut diagonal = 0;
            for (j, w) in window.iter().enumerate() {
                let above = row[j + 1];
                row[j + 1] = if &w.norm == qw {
                    diagonal + 1
                } else {
                    above.max(row[j])
                };
                diagonal = above;
            }
        }
        let matched = row[window.len()];
        let end = (1..=window.len())
            .find(|j| row[*j] == matched)
            .unwrap_or(window.len());
        if best.map_or(true, |(m, _, _)| matched > m) {
            best = Some((matched, start, start + end - 1));
        }
    }
    let (matched, from, to) = best?;
    if matched * 5 < q.len() * 4 {
        return None;
    }
    // Leaving out a filler word is tidying; swapping a name or an amount is not.
    let found: HashSet<&str> = e[from..=to].iter().map(|w| w.norm.as_str()).collect();
    let significant = |w: &Word| {
        let original = &quote[w.start..w.end];
        let pronoun = original.starts_with('I') && original.len() <= 4;
        original.chars().any(|c| c.is_ascii_digit())
            || (w.start > 0 && original.starts_with(char::is_uppercase) && !pronoun)
    };
    quote_words
        .iter()
        .filter(|w| significant(w))
        .all(|w| found.contains(w.norm.as_str()))
        .then(|| span(from, to))
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        db::Db,
        store::{documents, pages},
    };
    #[test]
    fn keeps_questions_answers_and_speakers_together() {
        let blocks = serde_json::json!([
            {"type":"heading","content":"Transcript"},
            {"id":"speaker","type":"paragraph","content":"Speaker 1"},
            {"id":"question","type":"paragraph","content":"[00:10] Is the handover on Friday?"},
            {"type":"paragraph","content":"Speaker 2"},
            {"id":"answer","type":"paragraph","content":"[00:15] No, Monday. Priya owns it."}
        ]);
        let chunks = passages("m", "Handover", &blocks.to_string(), 0).unwrap();
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].text.contains("Speaker 1: [00:10]"));
        assert!(chunks[0].text.contains("No, Monday. Priya owns it."));
        assert_eq!(chunks[0].block_id.as_deref(), Some("question"));
    }
    #[test]
    fn overview_includes_the_conversation_not_only_a_high_scoring_aside() {
        let (db, id) = fixture();
        let mut c = db.conn.lock().unwrap();
        let (hash, mut chunks) = snapshot(&c, &id).unwrap();
        chunks[0].text = "We are planning client handovers and ownership.".into();
        let mut aside = chunks[0].clone();
        aside.id = new_id();
        aside.text = "A fly-by-night remark.".into();
        let mut ending = chunks[0].clone();
        ending.id = new_id();
        ending.text = "Priya will prepare the handover checklist.".into();
        chunks.extend([aside, ending]);
        let mut query = vec![0.0; 768];
        query[0] = 1.0;
        let mut unrelated = vec![0.0; 768];
        unrelated[1] = 1.0;
        replace_index(
            &mut c,
            &id,
            &hash,
            &chunks,
            &[unrelated.clone(), query.clone(), unrelated],
        )
        .unwrap();
        let found = retrieve(&c, "What was spoken about?", &query, None, None, None).unwrap();
        assert_eq!(found.len(), 3);
        assert!(found[0].text.contains("client handovers"));
        assert!(found[2].text.contains("checklist"));
        let coverage = coverage(&c, &found, None, None, None).unwrap();
        assert_eq!(coverage.total_passages, 3);
    }
    #[test]
    fn old_indexes_are_queued_and_excluded_until_rebuilt() {
        let (db, id) = fixture();
        let mut c = db.conn.lock().unwrap();
        let (hash, chunks) = snapshot(&c, &id).unwrap();
        let vector = vec![1.0; 768];
        replace_index(&mut c, &id, &hash, &chunks, std::slice::from_ref(&vector)).unwrap();
        c.execute(
            "UPDATE meeting_chunk SET embedding_model='previous-version'",
            [],
        )
        .unwrap();
        assert_eq!(queue_outdated(&c).unwrap(), 1);
        assert!(!list(&c).unwrap()[0].indexed);
        assert_eq!(list(&c).unwrap()[0].passage_count, 0);
        assert_eq!(list(&c).unwrap()[0].state, "queued");
        assert!(retrieve(&c, "budget", &vector, None, None, None)
            .unwrap()
            .is_empty());
        assert!(snapshot(&c, &id).unwrap().1[0].text.contains("$500"));
    }
    #[test]
    fn windows_preserve_words_and_unicode() {
        let text = "handover café 客户 ".repeat(1000);
        let windows = text_windows(&text, 1200);
        assert!(windows.iter().all(|w| w.len() <= 1200));
        assert_eq!(
            windows.join(" ").split_whitespace().collect::<Vec<_>>(),
            text.split_whitespace().collect::<Vec<_>>()
        );
    }
    fn fixture() -> (Db, String) {
        let db = Db::open_in_memory().unwrap();
        let id = {
            let c = db.conn.lock().unwrap();
            let p = pages::core::create(&c, None, "Budget review".into(), "doc".into()).unwrap();
            documents::core::update(&c,&p.id,r#"[{"type":"paragraph","content":"invented summary"},{"type":"heading","content":[{"text":"Transcript"}]},{"id":"block-1","type":"paragraph","content":[{"text":"[00:12] We agreed the budget is $500."}]}]"#).unwrap();
            c.execute(
                "INSERT INTO meeting(id,page_id,started_at) VALUES('m',?1,1000)",
                [&p.id],
            )
            .unwrap();
            p.id
        };
        (db, id)
    }
    #[test]
    fn transcript_only_and_stale_results_rejected() {
        let (db, id) = fixture();
        let mut c = db.conn.lock().unwrap();
        let (hash, chunks) = snapshot(&c, &id).unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].timestamp.as_deref(), Some("00:12"));
        assert_eq!(chunks[0].block_id.as_deref(), Some("block-1"));
        let vector = vec![1.0; 768];
        replace_index(&mut c, &id, &hash, &chunks, std::slice::from_ref(&vector)).unwrap();
        assert_eq!(
            retrieve(&c, "budget", &vector, None, None, None)
                .unwrap()
                .len(),
            1
        );
        assert!(retrieve(&c, "budget", &vector, Some(2000), None, None)
            .unwrap()
            .is_empty());
        documents::core::update(&c, &id, "[]").unwrap();
        assert!(retrieve(&c, "budget", &vector, None, None, None)
            .unwrap()
            .is_empty());
        assert!(!save_summary(&c, &id, &hash, &MeetingSummary::default()).unwrap());
        assert!(!replace_index(&mut c, &id, &hash, &chunks, &[vector]).unwrap());
    }
    #[test]
    fn client_search_spans_meetings_without_leaking_other_clients() {
        let (db, first) = fixture();
        let mut c = db.conn.lock().unwrap();
        let client = pages::core::create(&c, None, "Acme".into(), "doc".into()).unwrap();
        let other = pages::core::create(&c, None, "Other client".into(), "doc".into()).unwrap();
        let later = pages::core::create(
            &c,
            Some(client.id.clone()),
            "Later review".into(),
            "doc".into(),
        )
        .unwrap();
        let private = pages::core::create(
            &c,
            Some(other.id.clone()),
            "Private review".into(),
            "doc".into(),
        )
        .unwrap();
        let vector = vec![1.0; 768];
        for (id, cid, date, amount, count) in [
            (&first, &client.id, 1000, 500, 40),
            (&later.id, &client.id, 2000, 700, 1),
            (&private.id, &other.id, 3000, 900, 1),
        ] {
            let mut blocks = vec![serde_json::json!({"type":"heading","content":"Transcript"})];
            for _ in 0..count {
                blocks.push(serde_json::json!({"type":"paragraph","content":format!("The budget is ${amount}.")}));
            }
            documents::core::update(&c, id, &serde_json::to_string(&blocks).unwrap()).unwrap();
            if id != &first {
                c.execute(
                    "INSERT INTO meeting(id,page_id,started_at) VALUES(?1,?2,?3)",
                    params![new_id(), id, date],
                )
                .unwrap();
            }
            c.execute("INSERT INTO link(source_page_id,target_page_id,kind,created_at) VALUES(?1,?2,'task_of',0)",params![id,cid]).unwrap();
            let (hash, chunks) = snapshot(&c, id).unwrap();
            replace_index(
                &mut c,
                id,
                &hash,
                &chunks,
                &vec![vector.clone(); chunks.len()],
            )
            .unwrap();
        }
        let found = retrieve(&c, "budget", &vector, None, None, Some(&client.id)).unwrap();
        assert!(found.iter().any(|p| p.page_id == first));
        assert!(found.iter().any(|p| p.page_id == later.id));
        assert!(found.iter().all(|p| p.page_id != private.id));
        assert!(found.windows(2).all(|p| p[0].started_at <= p[1].started_at));
        let dated = retrieve(&c, "budget", &vector, Some(1500), None, Some(&client.id)).unwrap();
        assert_eq!(dated.len(), 1);
        assert_eq!(dated[0].page_id, later.id);
        assert!(
            retrieve(&c, "budget", &vector, None, None, Some("missing-client"))
                .unwrap()
                .is_empty()
        );
        let jobs = list(&c).unwrap();
        assert_eq!(
            jobs.iter()
                .filter(|j| j.client_id.as_deref() == Some(&client.id))
                .count(),
            2
        );
        assert_eq!(
            jobs.iter()
                .find(|j| j.page_id == first)
                .unwrap()
                .passage_count,
            1
        );
        c.execute("UPDATE page SET deleted_at=1 WHERE id=?1", [&client.id])
            .unwrap();
        assert!(
            retrieve(&c, "budget", &vector, None, None, Some(&client.id))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn quotes_tidied_by_a_model_still_count_but_paraphrases_do_not() {
        let evidence = "Speaker 2: [03:00] Um, I'll I'll book a joint introduction with Acme for Monday. The account manager will stay Jordan until that introduction is complete.";
        // Same words, different case and punctuation, timestamp and filler dropped.
        let found = locate_quote(evidence, "i'll book a joint introduction with Acme for Monday").unwrap();
        assert!(found.starts_with("I'll book"));
        assert!(found.ends_with("Monday"));
        assert!(locate_quote(evidence, "Speaker 2: I'll book a joint introduction with Acme for Monday.").is_some());
        assert!(locate_quote(evidence, "Um I'll book a joint introduction with Acme for Monday").is_some());
        // An aside left out with an ellipsis.
        assert!(locate_quote(evidence, "The account manager will stay Jordan … until that introduction is complete").is_some());
        // Curly apostrophes.
        assert!(locate_quote(evidence, "I’ll book a joint introduction").is_some());
        // Paraphrases, different people and too-short quotes do not.
        assert!(locate_quote(evidence, "Jordan will set up a meeting with the client next week").is_none());
        assert!(locate_quote(evidence, "I'll book a joint introduction with Globex for Monday").is_none());
        assert!(locate_quote(evidence, "I'll book a joint introduction with Acme for Tuesday").is_none());
        assert!(locate_quote(evidence, "for Monday").is_none());
        assert!(locate_quote("", "anything at all here").is_none());
    }

    #[test]
    fn speaker_numbers_are_not_amounts_but_other_numbers_must_match() {
        assert!(check_numeric_text("Speaker 2 will send the report.", "I'll send the report.").is_ok());
        assert!(check_numeric_text("Speaker 2 will send 3 reports.", "I'll send the reports.").is_err());
        assert!(check_numeric_text("Due on 9 September.", "due 2026-09-09").is_ok());
        assert!(check_numeric_text("The budget is $1,500.", "a budget of 1500 dollars").is_ok());
        assert!(check_numeric_text("The budget is $1,600.", "a budget of 1500 dollars").is_err());
    }

    #[test]
    fn names_in_a_claim_must_be_near_its_quote() {
        let meeting = "Speaker 1: [01:24] I think Mia should send them the timeline.\nSpeaker 1: [01:30] Mia, can you send the client the revised timeline by Friday?\nSpeaker 4: [01:34] Yep, I'll send the revised timeline to Dr Chen by Friday.\nSpeaker 1: [03:20] We haven't decided. Priya, can you do a comparison?\nSpeaker 4: [03:34] Oh and Priya, you're chasing the photos, yeah?";
        let near = "Mia, can you send the client the revised timeline by Friday? Speaker 4: [01:34] Yep, I'll send the revised timeline to Dr Chen by Friday.";
        assert!(names_supported("Mia will send the revised timeline to Dr Chen by Friday.", near, meeting));
        assert!(!names_supported("Priya will send the revised timeline to Dr Chen by Friday.", near, meeting));
        // Ordinary capitalised words and names the meeting never uses are left alone.
        assert!(names_supported("Revised timeline goes out Friday.", near, meeting));
        // A name worked out from elsewhere in the meeting counts when one clause
        // pairs it with the claim's words; a recap listing everyone's tasks
        // doesn't lend one person's name to another's.
        let recap = format!("{meeting}\nSpeaker 1: [04:09] Recap: Mia sends the timeline Friday, Sam does redirects on staging by Wednesday, Priya does the booking comparison and chases photos.\nSpeaker 3: [02:33] So I'll set up the redirect map on staging by Wednesday.");
        assert!(names_supported("Sam will set up the redirect map on staging by Wednesday.", "So I'll set up the redirect map on staging by Wednesday.", &recap));
        assert!(!names_supported("Priya will send the revised timeline to Dr Chen by Friday.", near, &recap));
        // So are acronyms.
        let seo = "Speaker 3: [02:20] On the technical side, the SEO migration is risky. Every old URL needs a home.\nSpeaker 3: [02:50] Yes, we keep that exact URL.";
        assert!(names_supported("The emergency dentist URL stays the same for SEO.", "Yes, we keep that exact URL.", seo));
    }

    #[test]
    fn a_number_can_come_from_a_clause_about_the_same_thing() {
        let meeting = "Speaker 1: [00:26] The content from the client is late, like really late.\nSpeaker 1: [01:15] Okay, let's go with that. The launch is the twenty eighth of October.\nSpeaker 4: [01:52] I quoted two thousand four hundred dollars for that.";
        let near = "The content from the client is late, like really late.";
        assert!(numbers_supported("Launch moved to 28 October after content delays.", near, meeting));
        // A number from a clause about something else doesn't count.
        assert!(!numbers_supported("The content was 2,400 words late.", near, meeting));
        assert!(!numbers_supported("Launch moved to 21 October.", near, meeting));
        assert_eq!(clauses("It costs $2,400, or 1.5 days. Done").len(), 3);
    }

    #[test]
    fn numbers_spelt_out_in_a_transcript_support_digits_in_a_note() {
        let spoken = "Let's go with the twenty eighth of October. I quoted two thousand four hundred dollars for six service pages, about a hundred and fifty words each, and ninety pages redirect.";
        for claim in ["Launch moved to 28 October.", "The quote is $2,400.", "Six pages, so 6.", "About 150 words.", "90 pages redirect."] {
            assert!(check_numeric_text(claim, spoken).is_ok(), "{claim}");
        }
        assert!(check_numeric_text("Launch moved to 21 October.", spoken).is_err());
        assert!(check_numeric_text("The quote is $2,500.", spoken).is_err());
        assert_eq!(word_numbers("one two three").len(), 3);
        assert!(word_numbers("twenty one").contains("21"));
        assert!(word_numbers("three hundred and twelve thousand").contains("312000"));
    }

    #[test]
    fn an_edit_takes_passages_out_of_search_but_keeps_their_vectors() {
        let (db, id) = fixture();
        let mut c = db.conn.lock().unwrap();
        let (hash, chunks) = snapshot(&c, &id).unwrap();
        let vector = vec![1.0; 768];
        replace_index(&mut c, &id, &hash, &chunks, std::slice::from_ref(&vector)).unwrap();
        documents::core::update(&c,&id,r#"[{"type":"paragraph","content":"A note typed above"},{"type":"heading","content":[{"text":"Transcript"}]},{"id":"block-1","type":"paragraph","content":[{"text":"[00:12] We agreed the budget is $500."}]}]"#).unwrap();
        assert!(retrieve(&c, "budget", &vector, None, None, None).unwrap().is_empty());
        let cached = cached_vectors(&c, &id).unwrap();
        let (_, again) = snapshot(&c, &id).unwrap();
        assert!(again.iter().all(|p| cached.contains_key(&p.text)));
        assert_eq!(list(&c).unwrap()[0].state, "queued");
    }

    #[test]
    fn renamed_speakers_are_kept_with_what_they_said() {
        let blocks = serde_json::json!([
            {"type":"heading","content":"Transcript"},
            {"type":"paragraph","content":"Priya"},
            {"id":"a","type":"paragraph","content":"[00:10] I'll take the Acme account."},
            {"type":"paragraph","content":"Jordan (Acme)"},
            {"id":"b","type":"paragraph","content":"[00:15] Thanks, that works."}
        ]);
        let chunks = passages("m", "Handover", &blocks.to_string(), 0).unwrap();
        assert!(chunks[0].text.contains("Priya: [00:10] I'll take the Acme account."));
        assert!(chunks[0].text.contains("Jordan (Acme): [00:15]"));
    }

    #[test]
    fn notes_elsewhere_on_the_page_do_not_change_the_transcript_fingerprint() {
        let (db, id) = fixture();
        let c = db.conn.lock().unwrap();
        let (before, _) = snapshot(&c, &id).unwrap();
        documents::core::update(&c,&id,r#"[{"type":"paragraph","content":"My own note"},{"type":"heading","content":[{"text":"Transcript"}]},{"id":"block-1","type":"paragraph","content":[{"text":"[00:12] We agreed the budget is $500."}]}]"#).unwrap();
        assert_eq!(snapshot(&c, &id).unwrap().0, before);
        c.execute("UPDATE meeting_ai SET summary='{}',source_hash=?2 WHERE page_id=?1", params![id, before]).unwrap();
        let kept = stored(&c, &id, &before).unwrap();
        assert!(kept.has_summary && kept.summary_current && !kept.regenerate);
        documents::core::update(&c,&id,r#"[{"type":"heading","content":"Transcript"},{"id":"block-1","type":"paragraph","content":"[00:12] We agreed the budget is $700."}]"#).unwrap();
        let (after, _) = snapshot(&c, &id).unwrap();
        assert_ne!(after, before);
        assert!(!stored(&c, &id, &after).unwrap().summary_current);
    }

    fn job(id: &str, title: &str, started_at: i64, summary: MeetingSummary) -> MeetingJob {
        MeetingJob {
            page_id: id.into(),
            title: title.into(),
            state: "ready".into(),
            error: None,
            summary: Some(summary),
            indexed: true,
            client_id: None,
            client_name: None,
            passage_count: 1,
            started_at,
            template: "general".into(),
        }
    }

    #[test]
    fn saved_notes_answer_list_questions_across_meetings() {
        use crate::llm::SummarySection;
        use crate::store::meeting_question::Intent;
        let older = job("a", "Budget review", 1_000, MeetingSummary {
            summary: "The team reviewed the quarterly budget.".into(),
            action_items: vec!["Alex will send the revised budget.".into()],
            decisions: vec!["The budget stays at $500.".into()],
            sections: vec![SummarySection { heading: "Budget".into(), points: vec!["Costs rose in March.".into()] }],
            ..Default::default()
        });
        let newer = job("b", "Client handovers", 2_000, MeetingSummary {
            // Earlier notes repeated their first point as the overview.
            summary: "Priya takes over Acme.".into(),
            action_items: vec!["Jordan will update the account notes by Thursday.".into()],
            decisions: vec![],
            sections: vec![SummarySection { heading: "Handover".into(), points: vec!["Priya takes over Acme.".into()] }],
            open_questions: vec!["Who owns billing?".into()],
            ..Default::default()
        });
        let jobs = [&newer, &older];
        let items = note_items(&jobs, Intent::Actions);
        assert_eq!(items.len(), 2);
        let (answer, sources) = items_answer(&items.iter().collect::<Vec<_>>());
        assert!(answer.find("Client handovers").unwrap() < answer.find("Budget review").unwrap());
        assert!(answer.contains("- Jordan will update the account notes by Thursday."));
        assert!(answer.contains("[2]"));
        assert_eq!(sources.len(), 2);
        assert!(sources.iter().all(|s| s.kind == SourceKind::Notes));

        let (single, _) = recap(&[&older]);
        assert!(single.contains("The team reviewed the quarterly budget."));
        assert!(single.contains("### Budget\n- Costs rose in March."));
        assert!(single.contains("### Decisions\n- The budget stays at $500."));
        let (both, sources) = recap(&jobs);
        assert_eq!(sources.len(), 2);
        assert!(both.contains("- Still open: Who owns billing?"));
        assert!(both.contains("- Decided: The budget stays at $500."));
        assert_eq!(overview_of(newer.summary.as_ref().unwrap()), None);
    }

    #[test]
    fn written_pages_are_found_by_their_words_and_scoped_to_the_client() {
        let (db, meeting) = fixture();
        let c = db.conn.lock().unwrap();
        let client = pages::core::create(&c, None, "Acme".into(), "doc".into()).unwrap();
        let filed = pages::core::create(&c, Some(client.id.clone()), "Acme contract".into(), "doc".into()).unwrap();
        let other = pages::core::create(&c, None, "Globex contract".into(), "doc".into()).unwrap();
        documents::core::update(&c, &filed.id, r#"[{"type":"paragraph","content":"The renewal date is 1 March and the fee is fixed."}]"#).unwrap();
        documents::core::update(&c, &other.id, r#"[{"type":"paragraph","content":"Globex renewal is in June."}]"#).unwrap();
        let terms = vec!["renewal".to_string()];
        let all = page_passages(&c, &terms, None).unwrap();
        assert_eq!(all.len(), 2);
        assert!(all.iter().all(|p| p.kind == SourceKind::Page && p.page_id != meeting));
        let scoped = page_passages(&c, &terms, Some(&client.id)).unwrap();
        assert_eq!(scoped.len(), 1);
        assert!(scoped[0].text.contains("1 March"));
    }

    #[test]
    fn rejects_amounts_missing_from_cited_evidence() {
        let (_, id) = fixture();
        let sources=passages(&id,"Budget",r#"[{"type":"heading","content":"Transcript"},{"type":"paragraph","content":"The budget changed to $700."}]"#,0).unwrap();
        assert!(check_numeric_evidence("The budget rose from $500 to $700.", &sources).is_err());
        assert!(check_numeric_evidence("The new budget is $700.", &sources).is_ok());
    }
    #[test]
    fn deletion_excludes_evidence_and_invalid_vectors_fail() {
        let (db, id) = fixture();
        let mut c = db.conn.lock().unwrap();
        let (hash, chunks) = snapshot(&c, &id).unwrap();
        let v = vec![1.0; 768];
        replace_index(&mut c, &id, &hash, &chunks, std::slice::from_ref(&v)).unwrap();
        c.execute("UPDATE page SET deleted_at=1 WHERE id=?1", [id])
            .unwrap();
        assert!(retrieve(&c, "budget", &v, None, None, None)
            .unwrap()
            .is_empty());
        assert!(validate_vector(&[1.0]).is_err());
        assert!(validate_vector(&vec![f32::NAN; 768]).is_err());
    }

    #[test]
    fn each_point_is_timed_by_the_line_its_quote_came_from() {
        let transcript = "Speaker 1: [00:00:04] Welcome everyone, let's go through the relaunch.\nSpeaker 2: [00:01:12] We moved the launch date to the twenty eighth of October.\nYou: [00:02:30] I will send the revised timeline to Dr Chen by Friday. Also the photos.";
        let summary = MeetingSummary {
            decisions: vec!["The launch moved to 28 October.".into()],
            action_items: vec!["You will send the revised timeline by Friday.".into(), "You will send the photos.".into()],
            ..Default::default()
        };
        let quotes = vec![
            ("The launch moved to 28 October.".to_string(), "We moved the launch date to the twenty eighth of October".to_string()),
            (
                "You will send the revised timeline by Friday. You will send the photos.".to_string(),
                "I will send the revised timeline to Dr Chen by Friday".to_string(),
            ),
        ];
        let times = point_times(&summary, &quotes, transcript);
        assert_eq!(times.get("The launch moved to 28 October.").map(String::as_str), Some("00:01:12"));
        assert_eq!(times.get("You will send the revised timeline by Friday.").map(String::as_str), Some("00:02:30"));
        assert_eq!(times.get("You will send the photos.").map(String::as_str), Some("00:02:30"));
        assert!(time_of_quote(transcript, "nothing like this was ever said here").is_none());
        assert_eq!(last_stamp("[12:05] a [x] b [3:04:59] c"), Some("3:04:59".into()));
    }
}
