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

pub const EMBEDDING_MODEL: &str = "nomic-v1.5-q8-3e243421-context-v2";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Passage {
    pub id: String,
    pub page_id: String,
    pub title: String,
    pub block_id: Option<String>,
    pub timestamp: Option<String>,
    pub text: String,
    pub started_at: i64,
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
}

pub fn list(conn: &Connection) -> AppResult<Vec<MeetingJob>> {
    let mut stmt = conn.prepare(
        "SELECT a.page_id,p.title,a.state,a.error,a.summary,
        EXISTS(SELECT 1 FROM meeting_chunk c WHERE c.page_id=a.page_id AND c.embedding IS NOT NULL AND c.embedding_model=?1),
        client.id,client.title,(SELECT count(*) FROM meeting_chunk c WHERE c.page_id=a.page_id AND c.embedding_model=?1),
        (SELECT min(started_at) FROM meeting WHERE page_id=a.page_id)
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
        ) = row?;
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
        })
    })
    .collect()
}

pub fn inline_text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Array(items) => items.iter().map(inline_text).collect::<Vec<_>>().join(""),
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
    let mut in_transcript = false;
    let mut turns = Vec::new();
    let mut speaker = String::new();
    for block in blocks {
        let text = inline_text(&block["content"]);
        if block["type"] == "heading" {
            in_transcript = text.trim().eq_ignore_ascii_case("transcript");
            continue;
        }
        if !in_transcript || text.trim().is_empty() || text.contains("(No speech detected.)") {
            continue;
        }
        if text.starts_with("Speaker ") && text.len() < 40 && !text.contains('[') {
            speaker = text;
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

pub fn snapshot(conn: &Connection, id: &str) -> AppResult<(String, Vec<Passage>)> {
    let (title,content,started_at):(String,String,i64)=conn.query_row(
        "SELECT p.title,coalesce(p.content,'[]'),min(m.started_at) FROM page p JOIN meeting m ON m.page_id=p.id WHERE p.id=?1 AND p.deleted_at IS NULL GROUP BY p.id",[id],
        |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
    let hash = blake3::hash(content.as_bytes()).to_hex().to_string();
    Ok((hash, passages(id, &title, &content, started_at)?))
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
    tx.execute("UPDATE meeting_ai SET summary=?2,source_hash=?3,state='indexing',error=NULL,updated_at=?4 WHERE page_id=?1",
        params![id,serde_json::to_string(summary)?,hash,now_ms()])?;
    tx.commit()?;
    Ok(true)
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
pub fn validate_vector(v: &[f32]) -> AppResult<()> {
    if v.len() != 768 || v.iter().any(|n| !n.is_finite()) || v.iter().all(|n| *n == 0.0) {
        return Err(AppError::Invalid(
            "Search model returned an invalid embedding".into(),
        ));
    }
    Ok(())
}

pub fn is_overview_question(question: &str) -> bool {
    let q = question.to_lowercase();
    [
        "what was spoken about",
        "what was discussed",
        "what did we discuss",
        "what did they discuss",
        "what did we talk about",
        "what was the meeting about",
        "what was this meeting about",
        "meeting summary",
        "summarise the meeting",
        "summarize the meeting",
        "main topics",
        "meeting overview",
        "what were the meetings about",
    ]
    .iter()
    .any(|p| q.contains(p))
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

/// Reciprocal-rank fusion combines exact wording and semantic matches.
pub fn retrieve(
    conn: &Connection,
    question: &str,
    vector: &[f32],
    from: Option<i64>,
    until: Option<i64>,
    client_id: Option<&str>,
) -> AppResult<Vec<Passage>> {
    validate_vector(vector)?;
    if question.trim().is_empty() {
        return Ok(vec![]);
    }
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
            },
            r.get::<_, String>(7)?,
        ))
    })?;
    let mut candidates = Vec::new();
    let norm = vector.iter().map(|v| v * v).sum::<f32>().sqrt();
    for row in rows {
        let (p, raw) = row?;
        let v: Vec<f32> = serde_json::from_str(&raw)?;
        validate_vector(&v)?;
        let similarity = vector.iter().zip(&v).map(|(a, b)| a * b).sum::<f32>()
            / (norm * v.iter().map(|v| v * v).sum::<f32>().sqrt());
        candidates.push((p, similarity));
    }
    if candidates.len() > 50000 {
        return Err(AppError::Invalid(
            "Narrow the date range to search this many transcript passages".into(),
        ));
    }
    if is_overview_question(question) {
        return Ok(overview_passages(
            candidates.into_iter().map(|(p, _)| p).collect(),
        ));
    }
    candidates.sort_by(|a, b| b.1.total_cmp(&a.1));
    let eligible: HashSet<_> = candidates.iter().map(|(p, _)| p.id.clone()).collect();
    let terms = question
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| {
            t.len() > 2
                && ![
                    "what", "when", "where", "which", "who", "how", "the", "was", "were", "did",
                    "does", "have", "has", "had", "for", "and", "about", "this", "that", "with",
                    "our", "their", "they", "can", "you", "tell", "please", "meeting", "meetings",
                ]
                .contains(&t.to_lowercase().as_str())
        })
        .take(32)
        .map(|t| format!("\"{}\"", t))
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
    selected.sort_by_key(|p| p.started_at);
    Ok(selected)
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
    fn numbers(text: &str) -> HashSet<String> {
        text.replace(',', "")
            .split(|c: char| !c.is_ascii_digit() && c != '.')
            .map(|s| s.trim_matches('.'))
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect()
    }
    if !numbers(answer).is_subset(&numbers(evidence)) {
        return Err(AppError::Invalid("The answer included a number that its cited transcript passages do not support. Try a more focused question.".into()));
    }
    Ok(())
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
        replace_index(&mut c, &id, &hash, &chunks, &[vector.clone()]).unwrap();
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
        replace_index(&mut c, &id, &hash, &chunks, &[vector.clone()]).unwrap();
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
        replace_index(&mut c, &id, &hash, &chunks, &[v.clone()]).unwrap();
        c.execute("UPDATE page SET deleted_at=1 WHERE id=?1", [id])
            .unwrap();
        assert!(retrieve(&c, "budget", &v, None, None, None)
            .unwrap()
            .is_empty());
        assert!(validate_vector(&[1.0]).is_err());
        assert!(validate_vector(&vec![f32::NAN; 768]).is_err());
    }
}
