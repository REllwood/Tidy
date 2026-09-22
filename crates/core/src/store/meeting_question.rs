//! Reading a question about meetings: what kind of answer it wants, and any
//! dates or meetings it points at ("last week", "our last meeting").
use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, TimeZone};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Intent {
    /// What a meeting (or a stretch of meetings) was about.
    Overview,
    Actions,
    Decisions,
    OpenQuestions,
    /// Anything else: search the transcripts for it.
    Specific,
}

fn normalise(question: &str) -> String {
    question.to_lowercase().replace(['’', '‘'], "'")
}

fn tokens(question: &str) -> Vec<String> {
    normalise(question)
        .split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .map(|t| {
            let t = t.trim_matches('\'');
            t.strip_suffix("'s").unwrap_or(t).replace('\'', "")
        })
        .filter(|t| !t.is_empty())
        .collect()
}

/// Does the phrase appear as whole words?
fn has(q: &str, phrase: &str) -> bool {
    let padded = format!(" {} ", q.split(|c: char| !c.is_alphanumeric() && c != '\'' && c != '-').filter(|t| !t.is_empty()).collect::<Vec<_>>().join(" "));
    padded.contains(&format!(" {phrase} "))
}

const ACTIONS: &[&str] = &[
    "action item", "action items", "action point", "action points", "actions", "next step",
    "next steps", "to-do", "to-dos", "todo", "todos", "to do list", "follow-up", "follow-ups",
    "follow up", "follow ups", "followup", "followups", "tasks", "commitments", "committed to",
    "who is doing what", "what do i need to do", "what do we need to do", "what have i got to do",
    "what should i do", "deliverables", "assigned",
];
const DECISIONS: &[&str] = &["decisions", "key decision", "any decision", "what decision"];
const DECIDED: &[&str] = &[
    "what was decided", "what did we decide", "what have we decided", "what got decided",
    "what was agreed", "what did we agree", "what have we agreed", "what we agreed",
];
const OPEN: &[&str] = &[
    "open question", "open questions", "unresolved", "outstanding questions", "still to decide",
    "undecided", "loose ends", "what's pending", "whats pending", "open issues",
];

/// Words that shape a question without saying what it is about.
const FILLER: &[&str] = &[
    "a", "about", "across", "after", "again", "all", "also", "am", "an", "and", "any", "anything",
    "are", "around", "as", "at", "be", "been", "before", "between", "by", "can", "could", "did",
    "do", "does", "doing", "done", "during", "each", "else", "every", "everyone", "everything",
    "for", "from", "get", "give", "go", "going", "got", "had", "has", "have", "he", "her", "him",
    "his", "how", "i", "if", "in", "into", "is", "it", "its", "just", "know", "let", "lets",
    "me", "might", "more", "my", "need", "needs", "of", "on", "one", "ones", "or", "our", "ours",
    "out", "over", "please", "re", "regarding", "said", "say", "says", "she", "should", "show",
    "so", "some", "tell", "than", "that", "the", "their", "them", "then", "there", "these",
    "they", "thing", "things", "this", "those", "to", "up", "us", "was", "we", "were", "what",
    "whats", "when", "where", "which", "who", "whos", "why", "will", "with", "would", "you",
    "your", "happen", "happened", "happening", "discuss", "discussed", "discussing", "talk",
    "talked", "talking", "cover", "covered", "went", "spoke", "spoken", "speak", "mention",
    "mentioned", "come", "came", "list", "remind", "recall", "team", "people", "guys",
    "meeting", "meetings", "call", "calls", "catch", "catchup", "catchups", "sync", "syncs",
    "session", "sessions", "chat", "chats", "conversation", "conversations", "discussion",
    "discussions", "transcript", "transcripts", "recording", "recordings", "notes", "note",
    // The kind of answer wanted.
    "summary", "summarise", "summarize", "summarised", "summarized", "recap", "overview",
    "main", "key", "point", "points", "topic", "topics", "highlights", "highlight", "takeaway",
    "takeaways", "take", "aways", "gist", "tldr", "rundown", "run", "down", "debrief",
    "action", "actions", "item", "items", "next", "step", "steps", "todo", "todos", "follow",
    "followup", "followups", "task", "tasks", "commitment", "commitments", "committed",
    "assigned", "deliverable", "deliverables", "decision", "decisions", "decide", "decided",
    "agree", "agreed", "agreement", "agreements", "open", "question", "questions",
    "unresolved", "outstanding", "pending", "undecided", "loose", "ends", "issues", "still",
    // Dates and positions in time.
    "today", "yesterday", "tomorrow", "day", "days", "week", "weeks", "fortnight", "month",
    "months", "year", "years", "last", "latest", "past", "previous", "recent", "recently",
    "most", "first", "earliest", "earlier", "ago", "since", "until", "till", "morning",
    "afternoon", "evening", "tonight", "weekend", "january", "february", "march", "april",
    "may", "june", "july", "august", "september", "october", "november", "december", "jan",
    "feb", "mar", "apr", "jun", "jul", "aug", "sep", "sept", "oct", "nov", "dec", "monday",
    "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday", "two", "three", "four",
    "five", "six", "seven", "eight", "nine", "ten", "eleven", "twelve",
];

/// The words that say what a question is about, once its phrasing, the kind
/// of answer and any dates are set aside. Empty means the question only
/// points at meetings ("What happened last week?").
pub fn subject_terms(question: &str) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    tokens(question)
        .into_iter()
        .filter(|t| t.chars().count() >= 2)
        .filter(|t| !t.chars().all(|c| c.is_ascii_digit()))
        .filter(|t| !FILLER.contains(&t.as_str()))
        .filter(|t| seen.insert(t.clone()))
        .collect()
}

pub fn intent(question: &str) -> Intent {
    intent_with(question, &subject_terms(question))
}

/// As `intent`, with the subject already narrowed (for instance with a named
/// meeting's title set aside).
pub fn intent_with(question: &str, subject: &[String]) -> Intent {
    let q = normalise(question);
    let any = |phrases: &[&str]| phrases.iter().any(|p| has(&q, p));
    if any(OPEN) {
        return Intent::OpenQuestions;
    }
    if any(DECISIONS) || (any(DECIDED) && subject.is_empty()) {
        return Intent::Decisions;
    }
    if any(ACTIONS) {
        return Intent::Actions;
    }
    // Without a subject there is nothing to search for ("What happened last
    // week?", "Recap the budget review"): describe the meetings instead.
    if subject.is_empty() {
        return Intent::Overview;
    }
    Intent::Specific
}

/// Which meetings a question singles out by position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pointer {
    /// The most recent `n` meetings.
    Latest(usize),
    First,
}

const MEETING_WORDS: &[&str] = &[
    "meeting", "meetings", "call", "calls", "catch-up", "catch-ups", "catchup", "catch", "sync",
    "session", "sessions", "recording", "recordings", "conversation", "conversations", "chat",
];

pub fn pointer(question: &str) -> Option<Pointer> {
    let t = tokens(&question.replace("catch-up", "catchup").replace("catch up", "catchup"));
    let meeting = |i: usize| t.get(i).is_some_and(|w| MEETING_WORDS.contains(&w.as_str()));
    for i in 0..t.len() {
        let w = t[i].as_str();
        if matches!(w, "last" | "latest" | "previous") && meeting(i + 1) {
            return Some(Pointer::Latest(1));
        }
        if w == "most" && t.get(i + 1).is_some_and(|n| n == "recent") && meeting(i + 2) {
            return Some(Pointer::Latest(1));
        }
        if matches!(w, "last" | "latest" | "past" | "previous") {
            if let Some(n) = t.get(i + 1).and_then(|n| number(n)) {
                if meeting(i + 2) {
                    return Some(Pointer::Latest(n.clamp(1, 20)));
                }
            }
        }
        if matches!(w, "first" | "earliest") && meeting(i + 1) {
            return Some(Pointer::First);
        }
    }
    None
}

fn number(word: &str) -> Option<usize> {
    word.parse().ok().or_else(|| {
        [
            "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten",
            "eleven", "twelve",
        ]
        .iter()
        .position(|n| *n == word)
        .map(|i| i + 1)
    })
}

/// A span of time named in a question, as local-time milliseconds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DateScope {
    pub from: i64,
    pub until: i64,
    pub label: String,
}

const MONTHS: &[&str] = &[
    "january", "february", "march", "april", "may", "june", "july", "august", "september",
    "october", "november", "december",
];

fn month_index(word: &str) -> Option<u32> {
    let short = match word {
        "jan" => "january",
        "feb" => "february",
        "mar" => "march",
        "apr" => "april",
        "jun" => "june",
        "jul" => "july",
        "aug" => "august",
        "sep" | "sept" => "september",
        "oct" => "october",
        "nov" => "november",
        "dec" => "december",
        other => other,
    };
    MONTHS
        .iter()
        .position(|m| *m == short)
        .map(|i| i as u32 + 1)
}

fn midnight(date: NaiveDate) -> i64 {
    let naive = date.and_hms_opt(0, 0, 0).unwrap_or_default();
    Local
        .from_local_datetime(&naive)
        .earliest()
        .map(|d| d.timestamp_millis())
        // A skipped midnight (daylight saving) falls back to UTC.
        .unwrap_or_else(|| naive.and_utc().timestamp_millis())
}

fn scope(from: NaiveDate, until: NaiveDate, label: String) -> DateScope {
    DateScope {
        from: midnight(from),
        until: midnight(until),
        label,
    }
}

fn span_label(from: NaiveDate, until: NaiveDate) -> String {
    let last = until - Duration::days(1);
    if from == last {
        from.format("%-d %b %Y").to_string()
    } else if from.year() == last.year() && from.month() == last.month() {
        format!("{} – {}", from.format("%-d"), last.format("%-d %b %Y"))
    } else if from.year() == last.year() {
        format!("{} – {}", from.format("%-d %b"), last.format("%-d %b %Y"))
    } else {
        format!("{} – {}", from.format("%-d %b %Y"), last.format("%-d %b %Y"))
    }
}

fn add_months(date: NaiveDate, months: i32) -> NaiveDate {
    let total = date.year() * 12 + date.month0() as i32 + months;
    NaiveDate::from_ymd_opt(total.div_euclid(12), total.rem_euclid(12) as u32 + 1, 1)
        .unwrap_or(date)
}

/// Read a time span from the question: "today", "last week", "past 3 months",
/// "in March", "since August". Weeks start on Monday.
pub fn date_scope(question: &str, now: DateTime<Local>) -> Option<DateScope> {
    let t = tokens(question);
    let today = now.date_naive();
    let tomorrow = today + Duration::days(1);
    let monday = today - Duration::days(today.weekday().num_days_from_monday() as i64);
    let month_start = NaiveDate::from_ymd_opt(today.year(), today.month(), 1)?;
    let year_start = NaiveDate::from_ymd_opt(today.year(), 1, 1)?;
    let make = |from: NaiveDate, until: NaiveDate, words: &str| {
        let label = format!("“{words}” as {}", span_label(from, until));
        Some(scope(from, until, label))
    };
    for i in 0..t.len() {
        let w = t[i].as_str();
        let next = t.get(i + 1).map(String::as_str);
        let after = t.get(i + 2).map(String::as_str);
        // "last 3 weeks", "past two months", "last 10 days"
        if matches!(w, "last" | "past" | "previous") {
            if let (Some(n), Some(unit)) = (next.and_then(number), after) {
                let from = match unit {
                    "day" | "days" => today - Duration::days(n as i64 - 1),
                    "week" | "weeks" => tomorrow - Duration::days(7 * n as i64),
                    "fortnight" | "fortnights" => tomorrow - Duration::days(14 * n as i64),
                    "month" | "months" => add_months(tomorrow, -(n as i32)),
                    _ => continue,
                };
                return make(from, tomorrow, &format!("{w} {} {unit}", t[i + 1]));
            }
        }
        match (w, next) {
            ("today", _) => return make(today, tomorrow, "today"),
            ("yesterday", _) => {
                return make(today - Duration::days(1), today, "yesterday");
            }
            ("this", Some("week")) => return make(monday, monday + Duration::days(7), "this week"),
            ("last" | "previous", Some("week")) => {
                return make(monday - Duration::days(7), monday, &format!("{w} week"));
            }
            ("past", Some("week")) => {
                return make(tomorrow - Duration::days(7), tomorrow, "past week");
            }
            ("last" | "past" | "previous", Some("fortnight")) => {
                return make(tomorrow - Duration::days(14), tomorrow, &format!("{w} fortnight"));
            }
            ("this", Some("month")) => {
                return make(month_start, add_months(month_start, 1), "this month");
            }
            ("last" | "previous", Some("month")) => {
                return make(add_months(month_start, -1), month_start, &format!("{w} month"));
            }
            ("past", Some("month")) => {
                return make(add_months(tomorrow, -1), tomorrow, "past month");
            }
            ("this", Some("year")) => {
                return make(year_start, add_months(year_start, 12), "this year");
            }
            ("last" | "previous", Some("year")) => {
                return make(add_months(year_start, -12), year_start, &format!("{w} year"));
            }
            _ => {}
        }
        // "in March", "during March 2026", "since August", "March 2026". Not
        // "from": "from May" is as likely to be a person.
        let introduced = matches!(w, "in" | "during" | "since" | "throughout");
        let (month_word, year_word, since) = if introduced {
            (next, after, w == "since")
        } else {
            (Some(w), next, false)
        };
        let Some(month) = month_word.and_then(month_index) else {
            continue;
        };
        let year = year_word
            .filter(|y| y.len() == 4)
            .and_then(|y| y.parse::<i32>().ok());
        if !introduced && year.is_none() {
            // A bare "may" or "march" is too often an ordinary word.
            continue;
        }
        let mut start = NaiveDate::from_ymd_opt(year.unwrap_or(today.year()), month, 1)?;
        if year.is_none() && start > today {
            start = add_months(start, -12);
        }
        let last_word = i + usize::from(introduced) + usize::from(year.is_some());
        let words = t[i..=last_word.min(t.len() - 1)].join(" ");
        return if since {
            make(start, tomorrow, &words)
        } else {
            make(start, add_months(start, 1), &words)
        };
    }
    None
}

/// Does the question name this meeting or client? Short or generic names
/// ("Meeting", "Call") never count.
pub fn names(question: &str, name: &str) -> bool {
    let name_tokens = tokens(name);
    if name_tokens.is_empty()
        || name.trim().chars().count() < 3
        || name_tokens.iter().all(|t| FILLER.contains(&t.as_str()) || t.chars().all(|c| c.is_ascii_digit()))
    {
        return false;
    }
    let q = format!(" {} ", tokens(question).join(" "));
    q.contains(&format!(" {} ", name_tokens.join(" ")))
}

/// The subject words left once a named meeting or client is set aside.
pub fn without_names(terms: Vec<String>, names: &[&str]) -> Vec<String> {
    let named: std::collections::HashSet<String> =
        names.iter().flat_map(|n| tokens(n)).collect();
    terms.into_iter().filter(|t| !named.contains(t)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(y: i32, m: u32, d: u32) -> DateTime<Local> {
        Local
            .from_local_datetime(&NaiveDate::from_ymd_opt(y, m, d).unwrap().and_hms_opt(15, 0, 0).unwrap())
            .unwrap()
    }
    fn day(ms: i64) -> String {
        chrono::DateTime::from_timestamp_millis(ms)
            .unwrap()
            .with_timezone(&Local)
            .format("%Y-%m-%d")
            .to_string()
    }

    #[test]
    fn reads_what_kind_of_answer_is_wanted() {
        use Intent::*;
        for (q, want) in [
            ("What was spoken about?", Overview),
            ("Can you recap last week?", Overview),
            ("Summarise the budget review", Specific),
            ("What happened in our last meeting?", Overview),
            ("Tell me about the last meeting", Overview),
            ("What did we discuss about pricing?", Specific),
            ("What are my action items?", Actions),
            ("What tasks did Jordan take on?", Actions),
            ("List the next steps from this week", Actions),
            ("What decisions were made last month?", Decisions),
            ("What was decided?", Decisions),
            ("What did we agree about the launch date?", Specific),
            ("Any open questions?", OpenQuestions),
            ("What's still unresolved with Acme?", OpenQuestions),
            ("Who is responsible for updating the account notes?", Specific),
            ("When is the Acme renewal?", Specific),
        ] {
            assert_eq!(intent(q), want, "{q}");
        }
        assert_eq!(subject_terms("What did Priya say about pricing last week?"), ["priya", "pricing"]);
    }

    #[test]
    fn finds_meetings_by_position() {
        assert_eq!(pointer("What did we cover in the last meeting?"), Some(Pointer::Latest(1)));
        assert_eq!(pointer("Summarise our most recent call"), Some(Pointer::Latest(1)));
        assert_eq!(pointer("recap the last 3 meetings"), Some(Pointer::Latest(3)));
        assert_eq!(pointer("What happened at the first catch-up?"), Some(Pointer::First));
        assert_eq!(pointer("What happened last week?"), None);
    }

    #[test]
    fn reads_dates_in_local_time() {
        // Wednesday 23 September 2026.
        let now = at(2026, 9, 23);
        let check = |q: &str, from: &str, last_day: &str| {
            let s = date_scope(q, now).unwrap_or_else(|| panic!("no scope for {q}"));
            assert_eq!(day(s.from), from, "{q}");
            assert_eq!(day(s.until - 1), last_day, "{q}");
        };
        check("What happened today?", "2026-09-23", "2026-09-23");
        check("yesterday's call", "2026-09-22", "2026-09-22");
        check("this week", "2026-09-21", "2026-09-27");
        check("What did we decide last week?", "2026-09-14", "2026-09-20");
        check("over the past week", "2026-09-17", "2026-09-23");
        check("in the last 3 days", "2026-09-21", "2026-09-23");
        check("last month", "2026-08-01", "2026-08-31");
        check("in March", "2026-03-01", "2026-03-31");
        check("since August", "2026-08-01", "2026-09-23");
        check("in November", "2025-11-01", "2025-11-30");
        check("october 2025", "2025-10-01", "2025-10-31");
        assert!(date_scope("Did Jordan say he may join?", now).is_none());
        assert!(date_scope("What was the last meeting about?", now).is_none());
        let label = date_scope("last week", now).unwrap().label;
        assert_eq!(label, "“last week” as 14 – 20 Sep 2026");
    }

    #[test]
    fn matches_named_meetings_and_clients_as_whole_words() {
        assert!(names("What did we decide in the Budget review?", "Budget review"));
        assert!(!names("What was the budget?", "Budget review"));
        assert!(names("Acme's renewal", "Acme"));
        assert!(!names("What happened in the meeting?", "Meeting"));
        assert_eq!(
            without_names(subject_terms("pricing in the Budget review"), &["Budget review"]),
            ["pricing"]
        );
    }
}
