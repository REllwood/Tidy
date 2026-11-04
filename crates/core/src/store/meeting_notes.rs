//! Working with a meeting's saved notes: naming its speakers, writing the
//! notes into the page, and sending next steps to a task table.
use crate::{
    db::new_id,
    error::{AppError, AppResult},
    llm::MeetingSummary,
    store::{documents, ingest, meeting_ai, recordings},
};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// The heading the notes sit under once written into a page. Writing them
/// again replaces this section rather than adding a second copy.
pub const NOTES_HEADING: &str = "Meeting notes";
/// The line a new meeting page carries until notes are written into it.
const PLACEHOLDER: &str =
    "Transcript saved. Local AI summaries and search are managed in the meeting library.";

fn blocks(content: &str) -> AppResult<Vec<Value>> {
    Ok(serde_json::from_str(content)?)
}
fn text(block: &Value) -> String {
    meeting_ai::inline_text(&block["content"])
}
fn is_heading(block: &Value, name: &str) -> bool {
    block["type"] == "heading" && text(block).trim().eq_ignore_ascii_case(name)
}
fn heading_level(block: &Value) -> Option<u64> {
    (block["type"] == "heading").then(|| block["props"]["level"].as_u64().unwrap_or(1))
}

/// Where each speaker label sits in the transcript, in page order.
fn label_blocks(blocks: &[Value]) -> Vec<(usize, String)> {
    let texts: Vec<String> = blocks.iter().map(text).collect();
    let mut in_transcript = false;
    let mut found = Vec::new();
    for (i, block) in blocks.iter().enumerate() {
        if block["type"] == "heading" {
            in_transcript = texts[i].trim().eq_ignore_ascii_case("transcript");
            continue;
        }
        if !in_transcript || texts[i].trim().is_empty() {
            continue;
        }
        let next = texts[i + 1..].iter().find(|t| !t.trim().is_empty());
        if let Some(label) = meeting_ai::speaker_label(&texts[i], next.map(String::as_str)) {
            found.push((i, label));
        }
    }
    found
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Speaker {
    pub label: String,
    /// The first thing they said, to help tell who is who.
    pub sample: String,
    pub turns: usize,
}

/// The speakers in a meeting's transcript, in the order they first speak.
pub fn speakers(conn: &Connection, page_id: &str) -> AppResult<Vec<Speaker>> {
    let blocks = blocks(&documents::core::get(conn, page_id)?)?;
    let labels = label_blocks(&blocks);
    let mut speakers: Vec<Speaker> = Vec::new();
    for (n, (at, label)) in labels.iter().enumerate() {
        let end = labels.get(n + 1).map_or(blocks.len(), |(next, _)| *next);
        let said: Vec<String> = blocks[at + 1..end]
            .iter()
            .take_while(|b| b["type"] != "heading")
            .map(text)
            .filter(|t| t.trim_start().starts_with('['))
            .collect();
        let speaker = match speakers.iter_mut().find(|s| &s.label == label) {
            Some(s) => s,
            None => {
                speakers.push(Speaker {
                    label: label.clone(),
                    sample: String::new(),
                    turns: 0,
                });
                speakers.last_mut().expect("just pushed")
            }
        };
        speaker.turns += 1;
        if speaker.sample.is_empty() {
            if let Some(first) = said.first() {
                let line = first
                    .split_once(']')
                    .map_or(first.as_str(), |(_, rest)| rest)
                    .trim();
                speaker.sample = line.chars().take(140).collect();
            }
        }
    }
    Ok(speakers)
}

#[derive(Debug, Deserialize)]
pub struct SpeakerName {
    pub from: String,
    pub to: String,
}

/// A name that still reads as a speaker label once it is in the transcript.
fn valid_name(name: &str) -> bool {
    let name = name.trim();
    !name.is_empty()
        && name.chars().count() <= 40
        && name.split_whitespace().count() <= 5
        && !name.contains(['[', ']', ':'])
        && !name.ends_with(['.', '?', '!', ','])
}

/// Replace a speaker label in saved notes ("Speaker 2 will send…"), as a
/// whole word and never inside "Speaker 21".
fn replace_label(text: &str, from: &str, to: &str) -> String {
    let lower = text.to_lowercase();
    let needle = from.to_lowercase();
    if needle.is_empty() || lower.len() != text.len() {
        // Case folding changed byte offsets (rare scripts): leave it alone
        // unless the label appears exactly.
        return text.replace(from, to);
    }
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while let Some(found) = lower[i..].find(&needle) {
        let at = i + found;
        let end = at + needle.len();
        let before = text[..at].chars().next_back();
        let after = text[end..].chars().next();
        let whole = !before.is_some_and(char::is_alphanumeric)
            && !after.is_some_and(char::is_alphanumeric);
        // Renaming "Priya" to "Priya Shah" must not turn "Priya Shah" into
        // "Priya Shah Shah".
        let already = text[at..].starts_with(to);
        out.push_str(&text[i..at]);
        out.push_str(if whole && !already { to } else { &text[at..end] });
        i = end;
    }
    out.push_str(&text[i..]);
    out
}

/// Rename labels inside a block's inline content, keeping its formatting.
fn rename_inline(content: &mut Value, names: &[&SpeakerName]) {
    match content {
        Value::String(s) => {
            for n in names {
                *s = replace_label(s, &n.from, n.to.trim());
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|i| rename_inline(i, names)),
        Value::Object(node) => {
            if let Some(text) = node.get_mut("text") {
                rename_inline(text, names);
            }
            if let Some(inner) = node.get_mut("content") {
                rename_inline(inner, names);
            }
        }
        _ => {}
    }
}

fn rename_in_summary(summary: &mut MeetingSummary, names: &[SpeakerName]) {
    let apply = |text: &mut String| {
        for n in names {
            *text = replace_label(text, &n.from, n.to.trim());
        }
    };
    apply(&mut summary.summary);
    for list in [
        &mut summary.action_items,
        &mut summary.decisions,
        &mut summary.open_questions,
    ] {
        list.iter_mut().for_each(apply);
    }
    for section in &mut summary.sections {
        apply(&mut section.heading);
        section.points.iter_mut().for_each(apply);
    }
}

/// Give speakers real names throughout the transcript. Saved notes that
/// still match the transcript are renamed too and kept, so the meeting only
/// needs its search index rebuilt, not new notes.
pub fn rename_speakers(conn: &Connection, page_id: &str, names: &[SpeakerName]) -> AppResult<bool> {
    let names: Vec<&SpeakerName> = names
        .iter()
        .filter(|n| n.to.trim() != n.from.trim())
        .collect();
    if names.is_empty() {
        return Ok(false);
    }
    if let Some(bad) = names.iter().find(|n| !valid_name(&n.to)) {
        return Err(AppError::Invalid(format!(
            "“{}” can't be used as a speaker name. Use up to 40 characters, without brackets, colons or a full stop at the end.",
            bad.to.trim()
        )));
    }
    let tx = conn.unchecked_transaction()?;
    let (before, _) = meeting_ai::snapshot(&tx, page_id)?;
    let current = meeting_ai::stored(&tx, page_id, &before)?.summary_current;
    let mut content = blocks(&documents::core::get(&tx, page_id)?)?;
    let mut changed = false;
    for (at, label) in label_blocks(&content) {
        if let Some(name) = names.iter().find(|n| n.from == label) {
            content[at]["content"] = json!(name.to.trim());
            changed = true;
        }
    }
    if !changed {
        return Ok(false);
    }
    // Notes already written into the page are renamed in place, so ticked
    // next steps stay ticked.
    if let Some(start) = content.iter().position(|b| is_heading(b, NOTES_HEADING)) {
        for block in content[start + 1..]
            .iter_mut()
            .take_while(|b| !heading_level(b).is_some_and(|l| l <= 2))
        {
            rename_inline(&mut block["content"], &names);
        }
    }
    documents::core::update(&tx, page_id, &serde_json::to_string(&content)?)?;
    if current {
        let saved: Option<String> = tx
            .query_row("SELECT summary FROM meeting_ai WHERE page_id=?1", [page_id], |r| r.get(0))
            .optional()?
            .flatten();
        if let Some(saved) = saved {
            let mut summary: MeetingSummary = serde_json::from_str(&saved)?;
            let owned: Vec<SpeakerName> = names
                .iter()
                .map(|n| SpeakerName { from: n.from.clone(), to: n.to.clone() })
                .collect();
            rename_in_summary(&mut summary, &owned);
            let (after, _) = meeting_ai::snapshot(&tx, page_id)?;
            tx.execute(
                "UPDATE meeting_ai SET summary=?2,source_hash=?3 WHERE page_id=?1",
                params![page_id, serde_json::to_string(&summary)?, after],
            )?;
        }
    }
    tx.commit()?;
    Ok(true)
}

fn saved_summary(conn: &Connection, page_id: &str) -> AppResult<MeetingSummary> {
    let saved: Option<String> = conn
        .query_row("SELECT summary FROM meeting_ai WHERE page_id=?1", [page_id], |r| r.get(0))
        .optional()?
        .flatten();
    let saved = saved.ok_or_else(|| {
        AppError::Invalid("This meeting's notes aren't ready yet.".into())
    })?;
    Ok(serde_json::from_str(&saved)?)
}

/// "[hh:mm:ss] text" or "[mm:ss] text" as a time in ms and the words said.
fn stamped(line: &str) -> Option<(i64, String)> {
    let rest = line.trim_start().strip_prefix('[')?;
    let (clock, said) = rest.split_once(']')?;
    let mut ms = 0i64;
    for part in clock.split(':') {
        ms = ms * 60 + part.trim().parse::<i64>().ok()?;
    }
    Some((ms * 1000, said.trim().to_string()))
}

fn clock(ms: i64) -> String {
    let s = ms.max(0) / 1000;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
    } else {
        format!("{:02}:{:02}", s / 60, s % 60)
    }
}

/// The moments marked while recording, each with the transcript line being
/// said at that point and who said it. Nothing is written by the model here,
/// so these lines are exactly what the transcript holds.
pub fn marked_moments(conn: &Connection, page_id: &str) -> AppResult<Vec<String>> {
    let marks = recordings::marks(conn, page_id)?;
    if marks.is_empty() {
        return Ok(Vec::new());
    }
    let blocks = blocks(&documents::core::get(conn, page_id)?)?;
    let labels = label_blocks(&blocks);
    let mut lines: Vec<(i64, Option<String>, String)> = Vec::new();
    let mut in_transcript = false;
    let mut speaker: Option<String> = None;
    for (i, block) in blocks.iter().enumerate() {
        if block["type"] == "heading" {
            in_transcript = text(block).trim().eq_ignore_ascii_case("transcript");
            continue;
        }
        if !in_transcript {
            continue;
        }
        if let Some((_, label)) = labels.iter().find(|(at, _)| *at == i) {
            speaker = Some(label.clone());
            continue;
        }
        if let Some((ms, said)) = stamped(&text(block)) {
            lines.push((ms, speaker.clone(), said));
        }
    }
    Ok(marks
        .iter()
        .filter_map(|mark| {
            // People mark a moment just after hearing it, so take the line
            // that had started by then.
            let (_, who, said) = lines
                .iter()
                .rev()
                .find(|(ms, _, _)| *ms <= mark + 1_500)
                .or_else(|| lines.first())?;
            let said = ingest::shorten(said, 200);
            Some(match who {
                Some(who) => format!("[{}] {who}: {said}", clock(*mark)),
                None => format!("[{}] {said}", clock(*mark)),
            })
        })
        .collect())
}

/// The notes as page blocks: overview, topics, decisions, next steps (as
/// tick boxes) and open questions.
pub fn notes_blocks(summary: &MeetingSummary) -> Vec<Value> {
    let mut out = vec![json!({"type":"heading","props":{"level":2},"content":NOTES_HEADING})];
    let heading = |title: &str| json!({"type":"heading","props":{"level":3},"content":title});
    let bullet = |point: &str| json!({"type":"bulletListItem","content":point});
    if let Some(overview) = meeting_ai::overview_of(summary) {
        out.push(json!({"type":"paragraph","content":overview}));
    }
    for section in &summary.sections {
        out.push(heading(&section.heading));
        out.extend(section.points.iter().map(|p| bullet(p)));
    }
    if !summary.decisions.is_empty() {
        out.push(heading("Decisions"));
        out.extend(summary.decisions.iter().map(|p| bullet(p)));
    }
    if !summary.action_items.is_empty() {
        out.push(heading("Next steps"));
        out.extend(summary.action_items.iter().map(
            |p| json!({"type":"checkListItem","props":{"checked":false},"content":p}),
        ));
    }
    if !summary.open_questions.is_empty() {
        out.push(heading("Open questions"));
        out.extend(summary.open_questions.iter().map(|p| bullet(p)));
    }
    if !summary.moments.is_empty() {
        out.push(heading("Marked moments"));
        out.extend(summary.moments.iter().map(|p| bullet(p)));
    }
    for block in &mut out {
        block["id"] = json!(new_id());
    }
    out
}

/// Write the saved notes into the meeting page, above the transcript, so
/// they travel with Markdown exports, the vault and search. Writing them
/// again replaces the earlier copy. Returns true when an earlier copy was
/// replaced.
pub fn add_notes_to_page(conn: &Connection, page_id: &str) -> AppResult<bool> {
    let summary = saved_summary(conn, page_id)?;
    let mut content = blocks(&documents::core::get(conn, page_id)?)?;
    content.retain(|b| !(b["type"] == "paragraph" && text(b).trim() == PLACEHOLDER));
    let notes = notes_blocks(&summary);
    let existing = content.iter().position(|b| {
        is_heading(b, NOTES_HEADING) && heading_level(b).is_some_and(|l| l <= 2)
    });
    let replaced = existing.is_some();
    match existing {
        Some(start) => {
            let end = content[start + 1..]
                .iter()
                .position(|b| heading_level(b).is_some_and(|l| l <= 2))
                .map_or(content.len(), |i| start + 1 + i);
            content.splice(start..end, notes);
        }
        None => {
            let at = content
                .iter()
                .position(|b| is_heading(b, "transcript"))
                .unwrap_or(content.len());
            content.splice(at..at, notes);
        }
    }
    documents::core::update(conn, page_id, &serde_json::to_string(&content)?)?;
    Ok(replaced)
}

#[derive(Debug, Serialize, PartialEq)]
pub struct TasksAdded {
    pub added: usize,
    /// Next steps already in the table, left as they are.
    pub skipped: usize,
}

/// Add a meeting's next steps to a task table, dated today so they reach the
/// planner. Steps already in the table are skipped, so sending twice is safe.
pub fn next_steps_to_tasks(
    conn: &Connection,
    page_id: &str,
    database_id: &str,
) -> AppResult<TasksAdded> {
    let summary = saved_summary(conn, page_id)?;
    if summary.action_items.is_empty() {
        return Err(AppError::Invalid(
            "These notes don't list any next steps.".into(),
        ));
    }
    let fields = crate::store::databases::core::list_fields(conn, database_id)?;
    let name = fields
        .iter()
        .find(|f| f.kind == "text")
        .ok_or_else(|| AppError::Invalid("That table has no text column to hold the task.".into()))?;
    let tx = conn.unchecked_transaction()?;
    let mut fresh = Vec::new();
    let mut skipped = 0;
    for item in &summary.action_items {
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM cell c JOIN db_row r ON r.id=c.row_id WHERE r.database_id=?1 AND c.field_id=?2 AND c.value=?3)",
            params![database_id, name.id, json!(item).to_string()],
            |r| r.get(0),
        )?;
        if exists || fresh.contains(item) {
            skipped += 1;
        } else {
            fresh.push(item.clone());
        }
    }
    ingest::create_task_rows(&tx, database_id, &fresh)?;
    tx.commit()?;
    Ok(TasksAdded {
        added: fresh.len(),
        skipped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{db::Db, llm::SummarySection, store::pages};

    const TRANSCRIPT: &str = r#"[
        {"id":"note","type":"paragraph","content":"Transcript saved. Local AI summaries and search are managed in the meeting library."},
        {"type":"heading","props":{"level":2},"content":"Transcript"},
        {"id":"s1","type":"paragraph","content":"Speaker 1"},
        {"id":"a","type":"paragraph","content":"[00:01] Welcome, let's plan the handover."},
        {"id":"s2","type":"paragraph","content":"Speaker 2"},
        {"id":"b","type":"paragraph","content":"[00:05] I'll send the report on Friday."},
        {"id":"s1b","type":"paragraph","content":"Speaker 1"},
        {"id":"c","type":"paragraph","content":"[00:09] Thanks."}
    ]"#;

    fn meeting(c: &Connection) -> String {
        let page = pages::core::create(c, None, "Handover".into(), "doc".into()).unwrap();
        documents::core::update(c, &page.id, TRANSCRIPT).unwrap();
        c.execute("INSERT INTO meeting(id,page_id,started_at) VALUES('m',?1,1000)", [&page.id]).unwrap();
        page.id
    }
    fn save(c: &Connection, id: &str, summary: &MeetingSummary) {
        let (hash, _) = meeting_ai::snapshot(c, id).unwrap();
        assert!(meeting_ai::save_summary(c, id, &hash, summary).unwrap());
    }
    fn notes() -> MeetingSummary {
        MeetingSummary {
            summary: "Speaker 1 and Speaker 2 planned the handover.".into(),
            action_items: vec!["Speaker 2 will send the report on Friday.".into()],
            decisions: vec![],
            sections: vec![SummarySection { heading: "Handover".into(), points: vec!["Speaker 1 opened the planning.".into()] }],
            ..Default::default()
        }
    }

    #[test]
    fn marked_moments_quote_the_line_being_said_and_who_said_it() {
        let db = Db::open_in_memory().unwrap();
        let c = db.conn.lock().unwrap();
        let id = meeting(&c);
        // Marked just after the report was promised, and again near the end.
        recordings::set_stop_details(&c, &id, None, &[6_000, 9_500]).unwrap();
        save(&c, &id, &notes());
        let summary = saved_summary(&c, &id).unwrap();
        assert_eq!(
            summary.moments,
            vec!["[00:06] Speaker 2: I'll send the report on Friday.", "[00:09] Speaker 1: Thanks."]
        );
        let blocks = notes_blocks(&summary);
        let texts: Vec<String> = blocks.iter().map(text).collect();
        let at = texts.iter().position(|t| t == "Marked moments").expect("a Marked moments section");
        assert_eq!(texts[at + 1], "[00:06] Speaker 2: I'll send the report on Friday.");
    }

    #[test]
    fn meetings_without_marks_have_no_moments_section() {
        let db = Db::open_in_memory().unwrap();
        let c = db.conn.lock().unwrap();
        let id = meeting(&c);
        save(&c, &id, &notes());
        let summary = saved_summary(&c, &id).unwrap();
        assert!(summary.moments.is_empty());
        assert!(!notes_blocks(&summary).iter().any(|b| text(b) == "Marked moments"));
    }

    #[test]
    fn lists_speakers_with_what_they_first_said() {
        let db = Db::open_in_memory().unwrap();
        let c = db.conn.lock().unwrap();
        let id = meeting(&c);
        let found = speakers(&c, &id).unwrap();
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].label, "Speaker 1");
        assert_eq!(found[0].turns, 2);
        assert_eq!(found[0].sample, "Welcome, let's plan the handover.");
        assert_eq!(found[1].sample, "I'll send the report on Friday.");
    }

    #[test]
    fn naming_speakers_renames_the_transcript_and_keeps_the_notes() {
        let db = Db::open_in_memory().unwrap();
        let c = db.conn.lock().unwrap();
        let id = meeting(&c);
        save(&c, &id, &notes());
        let names = [
            SpeakerName { from: "Speaker 1".into(), to: "Jordan".into() },
            SpeakerName { from: "Speaker 2".into(), to: " Priya ".into() },
        ];
        assert!(rename_speakers(&c, &id, &names).unwrap());
        let found = speakers(&c, &id).unwrap();
        assert_eq!(found.iter().map(|s| s.label.as_str()).collect::<Vec<_>>(), ["Jordan", "Priya"]);
        let summary = saved_summary(&c, &id).unwrap();
        assert_eq!(summary.summary, "Jordan and Priya planned the handover.");
        assert_eq!(summary.action_items, ["Priya will send the report on Friday."]);
        // The notes still match, so the worker only rebuilds the index.
        let (hash, passages) = meeting_ai::snapshot(&c, &id).unwrap();
        assert!(passages[0].text.contains("Priya: [00:05]"));
        let stored = meeting_ai::stored(&c, &id, &hash).unwrap();
        assert!(stored.summary_current && stored.has_summary);
        assert!(rename_speakers(&c, &id, &[SpeakerName { from: "Jordan".into(), to: "Bad: name".into() }]).is_err());
    }

    #[test]
    fn notes_written_into_the_page_follow_a_rename() {
        let db = Db::open_in_memory().unwrap();
        let c = db.conn.lock().unwrap();
        let id = meeting(&c);
        save(&c, &id, &notes());
        add_notes_to_page(&c, &id).unwrap();
        let mut body: Vec<Value> = serde_json::from_str(&documents::core::get(&c, &id).unwrap()).unwrap();
        let task = body.iter().position(|b| b["type"] == "checkListItem").unwrap();
        body[task]["props"]["checked"] = json!(true);
        documents::core::update(&c, &id, &serde_json::to_string(&body).unwrap()).unwrap();
        rename_speakers(&c, &id, &[SpeakerName { from: "Speaker 2".into(), to: "Priya".into() }]).unwrap();
        let body: Vec<Value> = serde_json::from_str(&documents::core::get(&c, &id).unwrap()).unwrap();
        assert_eq!(text(&body[task]), "Priya will send the report on Friday.");
        assert_eq!(body[task]["props"]["checked"], json!(true));
    }

    #[test]
    fn labels_are_replaced_as_whole_words() {
        assert_eq!(replace_label("Speaker 2 and Speaker 21", "Speaker 2", "Priya"), "Priya and Speaker 21");
        assert_eq!(replace_label("speaker 2 agreed", "Speaker 2", "Priya"), "Priya agreed");
        assert_eq!(replace_label("Priya Shah and Priya", "Priya", "Priya Shah"), "Priya Shah and Priya Shah");
    }

    #[test]
    fn notes_go_above_the_transcript_and_replace_an_earlier_copy() {
        let db = Db::open_in_memory().unwrap();
        let c = db.conn.lock().unwrap();
        let id = meeting(&c);
        assert!(add_notes_to_page(&c, &id).is_err(), "no notes yet");
        save(&c, &id, &notes());
        let (before, _) = meeting_ai::snapshot(&c, &id).unwrap();
        assert!(!add_notes_to_page(&c, &id).unwrap());
        let body: Vec<Value> = serde_json::from_str(&documents::core::get(&c, &id).unwrap()).unwrap();
        assert_eq!(text(&body[0]), NOTES_HEADING);
        assert!(body.iter().all(|b| text(b) != PLACEHOLDER));
        assert!(body.iter().any(|b| b["type"] == "checkListItem"));
        let transcript = body.iter().position(|b| is_heading(b, "transcript")).unwrap();
        assert!(body[..transcript].iter().any(|b| text(b) == "Speaker 1 opened the planning."));
        // Writing again replaces the copy, and the transcript is untouched.
        assert!(add_notes_to_page(&c, &id).unwrap());
        let again: Vec<Value> = serde_json::from_str(&documents::core::get(&c, &id).unwrap()).unwrap();
        assert_eq!(again.iter().filter(|b| is_heading(b, NOTES_HEADING)).count(), 1);
        assert_eq!(again.len(), body.len());
        assert_eq!(meeting_ai::snapshot(&c, &id).unwrap().0, before);
    }

    #[test]
    fn next_steps_become_tasks_once() {
        let db = Db::open_in_memory().unwrap();
        let c = db.conn.lock().unwrap();
        let id = meeting(&c);
        save(&c, &id, &notes());
        let tasks = pages::core::create(&c, None, "Tasks".into(), "database".into()).unwrap();
        let db_id: String = c.query_row("SELECT id FROM database WHERE page_id=?1", [&tasks.id], |r| r.get(0)).unwrap();
        let first = next_steps_to_tasks(&c, &id, &db_id).unwrap();
        assert_eq!(first, TasksAdded { added: 1, skipped: 0 });
        let again = next_steps_to_tasks(&c, &id, &db_id).unwrap();
        assert_eq!(again, TasksAdded { added: 0, skipped: 1 });
    }
}
