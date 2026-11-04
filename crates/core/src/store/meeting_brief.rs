//! "Last time": before a meeting, what earlier meetings like it decided and
//! which of their next steps are still open. Earlier meetings are found by
//! calendar series first, then by title, then by client.

use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;
use serde_json::Value;

use crate::{
    error::AppResult,
    llm::MeetingSummary,
    store::{documents, meeting_ai},
};

#[derive(Debug, Serialize, PartialEq)]
pub struct PastMeeting {
    pub page_id: String,
    pub title: String,
    pub started_at: i64,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct LastTime {
    /// The earlier meetings the brief is drawn from, newest first.
    pub meetings: Vec<PastMeeting>,
    /// Up to three points: decisions first, then the main discussion.
    pub points: Vec<String>,
    /// Up to five next steps not yet ticked off in their meeting's page.
    pub open_actions: Vec<String>,
}

const MEETINGS: usize = 3;
const POINTS: usize = 3;
const ACTIONS: usize = 5;

/// A title without dates, times, numbers or the word "meeting", so
/// "Acme weekly 12/10" and "Acme weekly 19/10" count as the same meeting.
fn base_title(title: &str) -> String {
    title
        .to_lowercase()
        .split(|c: char| !c.is_alphabetic())
        .filter(|w| !w.is_empty() && *w != "meeting")
        .collect::<Vec<_>>()
        .join(" ")
}

struct Candidate {
    page_id: String,
    title: String,
    started_at: i64,
    series: Option<String>,
    client: Option<String>,
    summary: MeetingSummary,
}

fn candidates(conn: &Connection, before: i64) -> AppResult<Vec<Candidate>> {
    let mut stmt = conn.prepare(
        "SELECT a.page_id, p.title, m.started_at, m.event, client.title, a.summary
         FROM meeting_ai a
         JOIN page p ON p.id = a.page_id AND p.deleted_at IS NULL
         JOIN meeting m ON m.page_id = a.page_id
         LEFT JOIN page client ON client.id = (SELECT target_page_id FROM link
             WHERE source_page_id = p.id AND kind = 'task_of' LIMIT 1) AND client.deleted_at IS NULL
         WHERE a.summary IS NOT NULL AND m.started_at < ?1
         ORDER BY m.started_at DESC",
    )?;
    let rows = stmt.query_map([before], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, Option<String>>(3)?,
            r.get::<_, Option<String>>(4)?,
            r.get::<_, String>(5)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (page_id, title, started_at, event, client, summary) = row?;
        let Ok(summary) = serde_json::from_str::<MeetingSummary>(&summary) else {
            continue;
        };
        let series = event
            .and_then(|e| serde_json::from_str::<Value>(&e).ok())
            .and_then(|e| e["series"].as_str().map(str::to_owned));
        out.push(Candidate { page_id, title, started_at, series, client, summary });
    }
    Ok(out)
}

/// Next steps already ticked off in a meeting's page.
fn ticked(conn: &Connection, page_id: &str) -> AppResult<Vec<String>> {
    fn walk(blocks: &Value, out: &mut Vec<String>) {
        for block in blocks.as_array().into_iter().flatten() {
            if block["type"] == "checkListItem" && block["props"]["checked"] == true {
                out.push(meeting_ai::inline_text(&block["content"]).trim().to_string());
            }
            walk(&block["children"], out);
        }
    }
    let body: Value = serde_json::from_str(&documents::core::get(conn, page_id)?).unwrap_or(Value::Null);
    let mut out = Vec::new();
    walk(&body, &mut out);
    Ok(out)
}

/// The brief for a meeting about to happen, or None when there is no
/// earlier meeting like it.
pub fn last_time(
    conn: &Connection,
    title: &str,
    series: Option<&str>,
    client: Option<&str>,
    before: i64,
) -> AppResult<Option<LastTime>> {
    let all = candidates(conn, before)?;
    let base = base_title(title);
    let client = client.map(str::trim).filter(|c| !c.is_empty());
    type Tier<'a> = Box<dyn Fn(&Candidate) -> bool + 'a>;
    let tiers: [Tier; 3] = [
        Box::new(|c| series.is_some() && c.series.as_deref() == series),
        Box::new(|c| !base.is_empty() && base_title(&c.title) == base),
        Box::new(|c| client.is_some_and(|name| c.client.as_deref().is_some_and(|x| x.eq_ignore_ascii_case(name)))),
    ];
    let Some(matched) = tiers
        .iter()
        .map(|tier| all.iter().filter(|c| tier(c)).take(MEETINGS).collect::<Vec<_>>())
        .find(|found| !found.is_empty())
    else {
        return Ok(None);
    };

    let mut points: Vec<String> = matched
        .iter()
        .flat_map(|c| c.summary.decisions.iter().cloned())
        .take(POINTS)
        .collect();
    if points.len() < POINTS {
        let latest = &matched[0].summary;
        let discussion = meeting_ai::overview_of(latest)
            .map(|o| o.to_string())
            .into_iter()
            .chain(latest.sections.iter().flat_map(|s| s.points.iter().cloned()));
        points.extend(discussion.take(POINTS - points.len()));
    }

    let mut open_actions = Vec::new();
    for c in &matched {
        let done = ticked(conn, &c.page_id)?;
        for action in &c.summary.action_items {
            if open_actions.len() >= ACTIONS {
                break;
            }
            if !done.iter().any(|d| d == action.trim()) && !open_actions.contains(action) {
                open_actions.push(action.clone());
            }
        }
    }
    Ok(Some(LastTime {
        meetings: matched
            .iter()
            .map(|c| PastMeeting { page_id: c.page_id.clone(), title: c.title.clone(), started_at: c.started_at })
            .collect(),
        points,
        open_actions,
    }))
}

/// The layout for a meeting's notes: the one chosen for it, or the one its
/// title suggests.
pub fn template_for(conn: &Connection, page_id: &str) -> AppResult<&'static super::note_templates::Template> {
    let stored: Option<String> = conn
        .query_row("SELECT template FROM meeting_ai WHERE page_id=?1", [page_id], |r| r.get(0))
        .optional()?
        .flatten();
    if let Some(template) = stored.as_deref().and_then(super::note_templates::by_id) {
        return Ok(template);
    }
    let title: String = conn
        .query_row("SELECT title FROM page WHERE id=?1", [page_id], |r| r.get(0))
        .optional()?
        .unwrap_or_default();
    Ok(super::note_templates::choose(&title))
}

/// Rewrite a meeting's notes with another layout.
pub fn set_template(conn: &Connection, page_id: &str, template: &str) -> AppResult<()> {
    if super::note_templates::by_id(template).is_none() {
        return Err(crate::error::AppError::Invalid("That notes layout does not exist.".into()));
    }
    let changed = conn.execute(
        "UPDATE meeting_ai SET template=?2, state='queued', error=NULL, regenerate=1
         WHERE page_id=?1 AND state NOT IN ('summarising','indexing')",
        rusqlite::params![page_id, template],
    )?;
    if changed == 0 {
        return Err(crate::error::AppError::Invalid(
            "The notes are being written right now. Try again when they are done.".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::llm::SummarySection;
    use crate::store::recordings;

    fn meeting(c: &Connection, title: &str, started: i64, client: &str, series: Option<&str>, summary: MeetingSummary) -> String {
        let page = recordings::register(c, &format!("/r/{title}{started}.wav"), 1000, started, client).unwrap();
        c.execute("UPDATE page SET title=?2 WHERE id=?1", rusqlite::params![page, title]).unwrap();
        if let Some(series) = series {
            let event = recordings::MeetingEvent { title: title.into(), series: Some(series.into()), ..Default::default() };
            recordings::set_event(c, &page, &event).unwrap();
        }
        c.execute(
            "INSERT INTO meeting_ai(page_id,state,summary,updated_at) VALUES(?1,'ready',?2,0)
             ON CONFLICT(page_id) DO UPDATE SET summary=excluded.summary",
            rusqlite::params![page, serde_json::to_string(&summary).unwrap()],
        )
        .unwrap();
        page
    }

    fn notes(decision: &str, actions: &[&str]) -> MeetingSummary {
        MeetingSummary {
            summary: "The team checked progress.".into(),
            decisions: vec![decision.into()],
            action_items: actions.iter().map(|a| a.to_string()).collect(),
            sections: vec![SummarySection { heading: "Progress".into(), points: vec!["Design is done.".into()] }],
            ..Default::default()
        }
    }

    #[test]
    fn last_time_comes_from_the_same_series_and_skips_ticked_steps() {
        let db = Db::open_in_memory().unwrap();
        let c = db.conn.lock().unwrap();
        let older = meeting(&c, "Acme weekly", 1_000, "Acme", Some("s1"), notes("Launch moves to 28 October.", &["Mia sends the timeline."]));
        let newer = meeting(&c, "Acme weekly", 2_000, "Acme", Some("s1"), notes("Budget stays at $500.", &["Sam sets up redirects.", "Priya chases photos."]));
        meeting(&c, "Board review", 2_500, "", None, notes("Unrelated.", &["Not this."]));
        // Sam's step was ticked off in that meeting's page.
        documents::core::update(&c, &newer, r#"[{"type":"checkListItem","props":{"checked":true},"content":"Sam sets up redirects."}]"#).unwrap();

        let brief = last_time(&c, "Acme weekly", Some("s1"), None, 3_000).unwrap().unwrap();
        assert_eq!(brief.meetings.iter().map(|m| m.page_id.as_str()).collect::<Vec<_>>(), vec![newer.as_str(), older.as_str()]);
        assert_eq!(brief.points[..2], ["Budget stays at $500.".to_string(), "Launch moves to 28 October.".to_string()]);
        assert_eq!(brief.points.len(), 3);
        assert_eq!(brief.open_actions, vec!["Priya chases photos.", "Mia sends the timeline."]);
    }

    #[test]
    fn a_title_or_client_finds_earlier_meetings_when_there_is_no_series() {
        let db = Db::open_in_memory().unwrap();
        let c = db.conn.lock().unwrap();
        meeting(&c, "Design crit 12/10", 1_000, "", None, notes("Keep the dark theme.", &[]));
        meeting(&c, "Kickoff", 1_500, "Harbourside", None, notes("Relaunch in October.", &[]));
        let by_title = last_time(&c, "Design crit 19/10", None, None, 5_000).unwrap().unwrap();
        assert_eq!(by_title.points[0], "Keep the dark theme.");
        let by_client = last_time(&c, "Meeting 23/10", None, Some("harbourside"), 5_000).unwrap().unwrap();
        assert_eq!(by_client.points[0], "Relaunch in October.");
        assert!(last_time(&c, "Meeting 23/10", None, None, 5_000).unwrap().is_none());
        // Only meetings before this one count.
        assert!(last_time(&c, "Design crit", None, None, 500).unwrap().is_none());
    }

    #[test]
    fn a_meeting_keeps_the_layout_chosen_for_it() {
        let db = Db::open_in_memory().unwrap();
        let c = db.conn.lock().unwrap();
        let page = meeting(&c, "Daily standup", 1_000, "", None, notes("x", &[]));
        assert_eq!(template_for(&c, &page).unwrap().id, "standup");
        set_template(&c, &page, "customer").unwrap();
        assert_eq!(template_for(&c, &page).unwrap().id, "customer");
        assert!(set_template(&c, &page, "nope").is_err());
        let state: (String, i64) = c
            .query_row("SELECT state, regenerate FROM meeting_ai WHERE page_id=?1", [&page], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap();
        assert_eq!(state, ("queued".into(), 1));
    }
}
