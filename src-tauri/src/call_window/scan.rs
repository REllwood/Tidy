//! Finding names in a call window's accessibility tree: the people in the
//! call, who is shown speaking, and caption lines with their speakers. The
//! tree is read elsewhere, so everything here works on plain data and can be
//! tested without a real call.
//!
//! What each app shows was found in the open: Teams labels every video tile
//! "<name>, video is on…" and marks the tile of whoever is speaking, and
//! Google Meet puts captions in a region labelled "Captions", each line led
//! by its speaker's name. Both change from time to time, so anything not
//! recognised is simply skipped.

use tidy_core::store::recordings::{CallRecord, Spoken};

/// One element of the tree, as read.
#[derive(Debug, Clone, Default)]
pub struct Node {
    pub role: String,
    pub subrole: String,
    pub title: String,
    pub desc: String,
    pub value: String,
    /// The web page's class names, for web content.
    pub classes: String,
    pub children: Vec<Node>,
    /// Which live element this was read from, so it can be read again.
    pub handle: Option<usize>,
}

impl Node {
    fn label(&self) -> &str {
        if self.desc.trim().is_empty() {
            self.title.trim()
        } else {
            self.desc.trim()
        }
    }

    fn is_text(&self) -> bool {
        self.role == "AXStaticText"
    }

    /// The words a text element shows.
    fn shown(&self) -> &str {
        if self.value.trim().is_empty() {
            self.title.trim()
        } else {
            self.value.trim()
        }
    }

    fn is_control(&self) -> bool {
        matches!(
            self.role.as_str(),
            "AXButton" | "AXCheckBox" | "AXMenuButton" | "AXPopUpButton" | "AXRadioButton" | "AXTextField"
                | "AXTextArea" | "AXComboBox" | "AXLink" | "AXSlider"
        )
    }

    /// Text shown inside this element, in reading order, up to a depth.
    fn texts(&self, depth: usize, out: &mut Vec<String>) {
        if self.is_text() && !self.shown().is_empty() {
            out.push(self.shown().to_string());
        }
        if depth > 0 {
            for child in &self.children {
                child.texts(depth - 1, out);
            }
        }
    }

    fn has_class(&self, class: &str) -> bool {
        self.classes.split_whitespace().any(|c| c == class)
    }
}

/// Which call a window belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Call {
    Teams,
    Meet,
}

impl Call {
    pub fn app(self) -> &'static str {
        match self {
            Call::Teams => "Teams",
            Call::Meet => "Google Meet",
        }
    }
}

/// Browser names a window title may end with, after the page's own title.
const BROWSERS: &[&str] = &[
    "Google Chrome", "Microsoft Edge", "Brave", "Arc", "Vivaldi", "Opera", "Safari", "Mozilla Firefox", "Firefox",
];

/// A page title without the browser's name and profile after it.
fn page_title(window: &str) -> &str {
    for separator in [" - ", " – ", " — "] {
        for browser in BROWSERS {
            if let Some(at) = window.find(&format!("{separator}{browser}")) {
                return window[..at].trim();
            }
        }
    }
    window.trim()
}

/// The call a browser window shows, from its title.
pub fn browser_call(window: &str) -> Option<Call> {
    let page = page_title(window);
    if page.contains("Microsoft Teams") {
        return Some(Call::Teams);
    }
    let meet = page == "Meet"
        || page == "Google Meet"
        || page.starts_with("Meet - ")
        || page.starts_with("Meet – ")
        || page.starts_with("Meet: ")
        || page.contains("meet.google.com");
    meet.then_some(Call::Meet)
}

/// Parts of the Teams app that are not a meeting.
const TEAMS_SECTIONS: &[&str] = &[
    "Chat", "Activity", "Calendar", "Teams", "Calls", "Files", "Apps", "Copilot", "OneDrive", "People", "Search",
    "Settings", "Assignments", "Communities", "Notifications", "Microsoft Teams", "Meet",
];

/// A meeting window's title, and the people it names: Teams calls one-off
/// calls after the people in them, "Priya Shah, Tom Baker | Microsoft Teams".
pub fn window_title(call: Call, window: &str) -> (Option<String>, Vec<String>) {
    let page = page_title(window);
    match call {
        Call::Teams => {
            let parts: Vec<&str> = page.split(" | ").map(str::trim).collect();
            let head = parts.first().copied().unwrap_or("");
            if head.is_empty() || TEAMS_SECTIONS.iter().any(|s| s.eq_ignore_ascii_case(head)) {
                return (None, Vec::new());
            }
            let names: Vec<String> = head.split(", ").filter_map(clean_name).collect();
            if names.len() >= 2 && names.len() == head.split(", ").count() {
                return (None, names);
            }
            (Some(head.to_string()), Vec::new())
        }
        Call::Meet => {
            let rest = ["Google Meet", "Meet - ", "Meet – ", "Meet: ", "Meet"]
                .iter()
                .find_map(|p| page.strip_prefix(p))
                .unwrap_or("")
                .trim_start_matches([' ', '-', '–', ':'])
                .trim();
            (Some(rest.to_string()).filter(|r| !r.is_empty() && !is_meet_code(r)), Vec::new())
        }
    }
}

/// A Google Meet code such as "abc-defg-hij", which is not a title.
fn is_meet_code(text: &str) -> bool {
    let parts: Vec<&str> = text.split('-').collect();
    parts.len() == 3
        && [3, 4, 3].iter().zip(&parts).all(|(n, p)| p.len() == *n && p.chars().all(|c| c.is_ascii_lowercase()))
}

/// Words in labels that are not people.
const NOT_PEOPLE: &[&str] = &[
    "mute", "unmute", "muted", "camera", "video", "share", "sharing", "chat", "people", "participants", "raise",
    "lower", "leave", "more", "reactions", "react", "captions", "caption", "settings", "meeting", "call", "calls",
    "microphone", "mic", "record", "recording", "transcript", "transcription", "apps", "view", "search", "in",
    "invite", "admit", "present", "presenting", "presentation", "screen", "options", "turn", "join", "lobby",
    "everyone", "audio", "speaker", "speakers", "notes", "whiteboard", "breakout", "rooms", "layout", "you",
    "host", "organiser", "organizer", "attendee", "attendees", "guest", "contributors", "waiting", "hand",
];

/// Marks after a display name that are not part of it.
const NAME_TAGS: &[&str] = &[
    "(Guest)", "(External)", "(Unverified)", "(Organiser)", "(Organizer)", "(Host)", "(Co-host)", "(Presenter)",
    "(Presentation)",
];

/// A display name as shown, without tags such as "(Guest)", when the text
/// looks like a person's name rather than a button or a sentence.
pub fn clean_name(text: &str) -> Option<String> {
    let mut name = text.trim();
    loop {
        let before = name;
        for tag in NAME_TAGS {
            if let Some(rest) = name.strip_suffix(tag) {
                name = rest.trim_end();
            }
        }
        if name == before {
            break;
        }
    }
    let chars = name.chars().count();
    let words: Vec<&str> = name.split_whitespace().collect();
    let first = name.chars().next()?;
    let looks = (2..=60).contains(&chars)
        && (1..=6).contains(&words.len())
        && first.is_alphabetic()
        && !first.is_lowercase()
        && !name.chars().any(|c| c.is_ascii_digit() || "@/:?!|•;=+#".contains(c))
        // "Baker, Tom" is a name, a list of several is not.
        && name.matches(',').count() <= 1
        && !name.ends_with('.')
        && !words.iter().any(|w| NOT_PEOPLE.contains(&w.to_lowercase().trim_matches(|c: char| !c.is_alphabetic())));
    looks.then(|| name.to_string())
}

/// The person recording, as their own name is marked.
fn yourself(text: &str) -> Option<String> {
    let trimmed = text.trim();
    let lower = trimmed.to_lowercase();
    let at = lower.rfind("(you)")?;
    clean_name(&trimmed[..at])
}

/// What one look at a call window found.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Look {
    /// People shown in the call, as named.
    pub people: Vec<String>,
    /// The person recording, when the window marks them.
    pub me: Vec<String>,
    /// People shown speaking right now.
    pub speaking: Vec<String>,
    /// Caption lines: who, and what they are shown saying.
    pub captions: Vec<(String, String)>,
    /// Elements worth reading again each second: video tiles and captions.
    pub hot: Vec<usize>,
}

impl Look {
    /// Nothing about a call: no people, speakers, captions or tiles.
    pub fn is_empty(&self) -> bool {
        self.people.is_empty() && self.me.is_empty() && self.speaking.is_empty() && self.captions.is_empty()
            && self.hot.is_empty()
    }

    pub fn add(&mut self, other: Look) {
        self.people.extend(other.people);
        self.me.extend(other.me);
        self.speaking.extend(other.speaking);
        self.captions.extend(other.captions);
        self.hot.extend(other.hot);
    }
}

/// A Teams video tile: its label starts with the person's name and says
/// whether their video is on.
fn tile_name(node: &Node) -> Option<Option<String>> {
    let label = node.label();
    let lower = label.to_lowercase();
    let at = lower.find(", video is ")?;
    // The person recording's own tile.
    if lower.starts_with("myself") {
        return Some(None);
    }
    // The tile's own text holds the full name, "Last, First" included.
    let mut texts = Vec::new();
    node.texts(4, &mut texts);
    let name = texts.iter().find_map(|t| clean_name(t)).or_else(|| clean_name(&label[..at]));
    Some(name)
}

/// Whether a tile is shown speaking: Teams adds a frame to it, and other
/// apps may say so in its label.
fn tile_speaking(node: &Node) -> bool {
    let lower = node.label().to_lowercase();
    lower.contains(", speaking") || lower.contains(" is speaking")
        || node.children.iter().any(|c| c.has_class("vdi-frame-occlusion") || c.children.iter().any(|g| g.has_class("vdi-frame-occlusion")))
}

/// "Priya Shah is speaking" or "Priya Shah, speaking" on anything else.
fn speaking_label(node: &Node) -> Option<String> {
    let label = node.label();
    let lower = label.to_lowercase();
    let at = lower.find(", speaking").or_else(|| lower.find(" is speaking"))?;
    clean_name(&label[..at])
}

fn is_captions(node: &Node) -> bool {
    if node.is_control() || node.is_text() {
        return false;
    }
    let label = node.label().to_lowercase();
    matches!(label.as_str(), "captions" | "live captions" | "closed captions" | "subtitles")
}

/// Caption lines: each line shows its speaker's name, then what they said.
fn caption_lines(area: &Node) -> Vec<(String, String)> {
    // Step through wrappers that hold a single element.
    let mut rows = area;
    while rows.children.len() == 1 && !rows.children[0].is_text() {
        rows = &rows.children[0];
    }
    let mut lines = Vec::new();
    for row in &rows.children {
        let mut texts = Vec::new();
        row.texts(8, &mut texts);
        if texts.len() >= 2 {
            let speaker = &texts[0];
            if speaker.eq_ignore_ascii_case("you") || clean_name(speaker).is_some() || yourself(speaker).is_some() {
                lines.push((speaker.clone(), texts[1..].join(" ")));
            }
        } else if let Some((speaker, said)) = texts.first().and_then(|t| t.split_once(": ")) {
            if clean_name(speaker).is_some() {
                lines.push((speaker.to_string(), said.to_string()));
            }
        }
    }
    lines
}

fn is_roster(node: &Node) -> bool {
    if node.is_control() || node.is_text() {
        return false;
    }
    let label = node.label().to_lowercase();
    ["participants", "people", "attendees", "in this meeting", "in the call", "in call", "roster"]
        .iter()
        .any(|k| label == *k || label.starts_with(&format!("{k} ")) || label.starts_with(&format!("{k},")))
}

/// Names in a list of people, one per row.
fn roster_names(list: &Node, look: &mut Look) {
    let rows: Vec<&Node> = list
        .children
        .iter()
        .flat_map(|c| if c.children.len() > 1 && c.role == "AXGroup" && c.label().is_empty() { c.children.iter().collect() } else { vec![c] })
        .collect();
    for row in rows {
        if row.role == "AXHeading" || row.role == "AXButton" {
            continue;
        }
        // "Priya Shah, Organiser, Muted": the name comes first.
        let label = row.label();
        let head = label.split(", ").next().unwrap_or("");
        let mut texts = Vec::new();
        row.texts(6, &mut texts);
        let candidates = std::iter::once(head.to_string()).chain(texts);
        for text in candidates {
            if let Some(me) = yourself(&text) {
                look.me.push(me);
                break;
            }
            if let Some(name) = clean_name(&text) {
                look.people.push(name);
                break;
            }
        }
    }
}

/// Everything a window or part of one shows about who is in the call.
pub fn look(root: &Node) -> Look {
    let mut look = Look::default();
    visit(root, &mut look);
    look
}

fn visit(node: &Node, look: &mut Look) {
    if let Some(tile) = tile_name(node) {
        if let Some(name) = tile {
            if tile_speaking(node) {
                look.speaking.push(name.clone());
            }
            look.people.push(name);
        }
        look.hot.extend(node.handle);
        return;
    }
    if is_captions(node) {
        look.captions.extend(caption_lines(node));
        look.hot.extend(node.handle);
        return;
    }
    if is_roster(node) {
        roster_names(node, look);
        return;
    }
    if let Some(name) = speaking_label(node) {
        look.speaking.push(name.clone());
        look.people.push(name);
    }
    for child in &node.children {
        visit(child, look);
    }
}

/// How long after someone speaks a tile shows it, and a caption line, in ms.
const TILE_LAG_MS: i64 = 500;
const CAPTION_LAG_MS: i64 = 1_500;
/// Two stretches of the same person this close together become one.
const JOIN_MS: i64 = 1_500;

/// Everything seen during a recording, built up one look at a time.
#[derive(Debug, Default)]
pub struct Tracker {
    app: Option<Call>,
    title: Option<String>,
    people: Vec<String>,
    me: Vec<String>,
    last_captions: Option<Vec<(String, String)>>,
    last_at: Option<i64>,
    speaking: Vec<Spoken>,
}

fn same(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

impl Tracker {
    pub fn saw_window(&mut self, call: Call, title: Option<String>, people: Vec<String>) {
        self.app.get_or_insert(call);
        if self.title.is_none() {
            self.title = title;
        }
        self.add_people(people);
    }

    fn add_people(&mut self, names: impl IntoIterator<Item = String>) {
        for name in names {
            if !self.people.iter().any(|p| same(p, &name)) {
                self.people.push(name);
            }
        }
    }

    fn is_me(&self, name: &str) -> bool {
        same(name, "you") || self.me.iter().any(|m| same(m, name)) || yourself(name).is_some()
    }

    fn shown_speaking(&mut self, name: &str, from_ms: i64, to_ms: i64) {
        if let Some(last) = self.speaking.last_mut() {
            if same(&last.name, name) && from_ms <= last.to_ms + JOIN_MS {
                last.to_ms = last.to_ms.max(to_ms);
                return;
            }
        }
        self.speaking.push(Spoken { from_ms: from_ms.max(0), to_ms: to_ms.max(0), name: name.to_string() });
    }

    /// Take in one look, made `at_ms` into the recording.
    pub fn add(&mut self, at_ms: i64, look: &Look) {
        let since = self.last_at.map_or(1_000, |last| (at_ms - last).clamp(250, 3_000));
        self.last_at = Some(at_ms);
        for me in &look.me {
            if !self.me.iter().any(|m| same(m, me)) {
                self.me.push(me.clone());
            }
        }
        self.add_people(look.people.clone());

        // Tiles: one person shown speaking. Two at once says nothing.
        let mut tiles: Vec<&String> = look.speaking.iter().filter(|n| !self.is_me(n)).collect();
        tiles.sort();
        tiles.dedup_by(|a, b| same(a.as_str(), b.as_str()));
        if let [name] = tiles.as_slice() {
            let at = at_ms - TILE_LAG_MS;
            let name = (*name).clone();
            self.shown_speaking(&name, at - since / 2, at + since / 2);
        }

        // Captions: a line that is new or still growing is someone speaking.
        let changed: Vec<String> = match &self.last_captions {
            None => Vec::new(),
            Some(before) => look
                .captions
                .iter()
                .filter(|line| !before.contains(line))
                .map(|(speaker, _)| speaker.clone())
                .collect(),
        };
        self.last_captions = Some(look.captions.clone());
        let others: Vec<String> = look.captions.iter().map(|(s, _)| s.clone()).filter(|s| !self.is_me(s)).collect();
        self.add_people(others.into_iter().filter_map(|s| clean_name(&s)));
        let mut talking: Vec<String> = changed.into_iter().filter(|s| !self.is_me(s)).collect();
        talking.sort();
        talking.dedup_by(|a, b| same(a.as_str(), b.as_str()));
        if let [name] = talking.as_slice() {
            let end = at_ms - CAPTION_LAG_MS;
            let name = name.clone();
            self.shown_speaking(&name, end - since, end);
        }
    }

    /// What was seen, without the person recording.
    pub fn finish(self) -> CallRecord {
        let me = self.me;
        let mine = |name: &str| me.iter().any(|m| same(m, name));
        CallRecord {
            app: self.app.map(Call::app).unwrap_or_default().to_string(),
            title: self.title,
            people: self.people.into_iter().filter(|p| !mine(p)).collect(),
            speaking: self.speaking.into_iter().filter(|s| !mine(&s.name)).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn el(role: &str, label: &str, children: Vec<Node>) -> Node {
        Node { role: role.into(), desc: label.into(), children, ..Default::default() }
    }
    fn text(words: &str) -> Node {
        Node { role: "AXStaticText".into(), value: words.into(), ..Default::default() }
    }
    fn group(children: Vec<Node>) -> Node {
        el("AXGroup", "", children)
    }

    /// A Teams video tile as Teams 26 shows it, optionally marked speaking.
    fn tile(name: &str, speaking: bool) -> Node {
        let mut frame = group(vec![]);
        if speaking {
            frame.classes = "fui-1x8 vdi-frame-occlusion".into();
        }
        el("AXMenuItem", &format!("{name}, video is off, muted"), vec![group(vec![text(name)]), frame])
    }

    #[test]
    fn teams_tiles_give_the_people_and_who_is_speaking() {
        let mut stage = group(vec![
            tile("Priya Shah", true),
            tile("Baker, Tom", false),
            el("AXMenuItem", "Myself video, video is on", vec![text("Rhys Ellwood")]),
            el("AXButton", "Leave", vec![]),
        ]);
        stage.children[0].handle = Some(7);
        let look = look(&group(vec![stage]));
        assert_eq!(look.people, vec!["Priya Shah", "Baker, Tom"], "the tile's text keeps Last, First names");
        assert_eq!(look.speaking, vec!["Priya Shah"]);
        assert_eq!(look.hot, vec![7], "tiles are read again each second");
    }

    #[test]
    fn meet_captions_pair_each_line_with_its_speaker() {
        let row = |who: &str, said: &str| group(vec![el("AXImage", "", vec![]), group(vec![text(who)]), group(vec![text(said)])]);
        let captions = Node {
            role: "AXGroup".into(),
            subrole: "AXLandmarkRegion".into(),
            desc: "Captions".into(),
            children: vec![group(vec![
                row("Priya Shah", "So the launch moves to March."),
                row("You", "That works for me."),
            ])],
            handle: Some(3),
            ..Default::default()
        };
        let page = group(vec![el("AXButton", "Turn off captions", vec![]), captions]);
        let look = look(&page);
        assert_eq!(
            look.captions,
            vec![
                ("Priya Shah".to_string(), "So the launch moves to March.".to_string()),
                ("You".to_string(), "That works for me.".to_string())
            ]
        );
        assert_eq!(look.hot, vec![3]);
        assert!(look.people.is_empty(), "the captions button is not a person");
    }

    #[test]
    fn a_people_list_gives_names_and_marks_the_person_recording() {
        let list = el(
            "AXList",
            "Participants",
            vec![
                el("AXGroup", "Priya Shah, Organiser, Muted", vec![text("Priya Shah")]),
                el("AXGroup", "", vec![el("AXImage", "", vec![]), text("Sam Okafor (Guest)")]),
                el("AXGroup", "", vec![text("Rhys Ellwood (You)")]),
                el("AXButton", "Invite people", vec![]),
            ],
        );
        let look = look(&group(vec![list]));
        assert_eq!(look.people, vec!["Priya Shah", "Sam Okafor"]);
        assert_eq!(look.me, vec!["Rhys Ellwood"]);
    }

    #[test]
    fn names_are_told_apart_from_buttons_and_sentences() {
        for name in ["Priya Shah", "Zoë Müller", "José", "Mary-Jane O'Neil", "Baker Tom"] {
            assert_eq!(clean_name(name).as_deref(), Some(name), "{name}");
        }
        assert_eq!(clean_name("Sam Okafor (Guest)").as_deref(), Some("Sam Okafor"));
        for not in ["Turn off microphone", "Raise hand", "More options", "the launch", "iPhone 15", "10:30",
            "So the launch moves to March.", "", "Leave", "Share screen", "Live captions"] {
            assert_eq!(clean_name(not), None, "{not}");
        }
    }

    #[test]
    fn window_titles_give_the_meeting_or_the_people() {
        assert_eq!(browser_call("Meet - abc-defg-hij - Google Chrome - Rhys"), Some(Call::Meet));
        assert_eq!(browser_call("Meet - abc-defg-hij"), Some(Call::Meet));
        assert_eq!(browser_call("Weekly sync | Microsoft Teams"), Some(Call::Teams));
        assert_eq!(browser_call("Inbox (3) - Gmail - Google Chrome"), None);
        assert_eq!(browser_call("Meeting notes - Google Docs"), None);

        assert_eq!(window_title(Call::Meet, "Meet - abc-defg-hij - Google Chrome"), (None, vec![]));
        assert_eq!(window_title(Call::Meet, "Meet - Acme weekly"), (Some("Acme weekly".into()), vec![]));
        assert_eq!(window_title(Call::Teams, "Acme weekly | Microsoft Teams"), (Some("Acme weekly".into()), vec![]));
        assert_eq!(window_title(Call::Teams, "Chat | Priya Shah | Microsoft Teams"), (None, vec![]));
        assert_eq!(window_title(Call::Teams, "Microsoft Teams"), (None, vec![]));
        assert_eq!(
            window_title(Call::Teams, "Priya Shah, Tom Baker | Microsoft Teams"),
            (None, vec!["Priya Shah".to_string(), "Tom Baker".to_string()])
        );
    }

    fn captions(lines: &[(&str, &str)]) -> Look {
        Look { captions: lines.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect(), ..Default::default() }
    }

    #[test]
    fn growing_captions_and_highlighted_tiles_become_stretches_of_speaking() {
        let mut t = Tracker::default();
        t.saw_window(Call::Meet, None, vec![]);
        // Captions already on screen when recording starts say nothing.
        t.add(1_000, &captions(&[("Priya Shah", "Earlier words.")]));
        t.add(2_000, &captions(&[("Priya Shah", "Earlier words."), ("Tom Baker", "So")]));
        t.add(3_000, &captions(&[("Priya Shah", "Earlier words."), ("Tom Baker", "So the launch")]));
        // Your own lines are not someone else speaking.
        t.add(4_000, &captions(&[("Tom Baker", "So the launch"), ("You", "Sounds good")]));
        // Two people's lines growing at once says nothing about either.
        t.add(5_000, &captions(&[("You", "Sounds good"), ("Priya Shah", "Yes"), ("Tom Baker", "Right")]));
        t.add(6_000, &captions(&[("Priya Shah", "Yes, and the pilot")]));
        let record = t.finish();
        assert_eq!(record.app, "Google Meet");
        assert_eq!(record.people, vec!["Priya Shah", "Tom Baker"]);
        assert_eq!(
            record.speaking,
            vec![
                Spoken { from_ms: 0, to_ms: 1_500, name: "Tom Baker".into() },
                Spoken { from_ms: 3_500, to_ms: 4_500, name: "Priya Shah".into() },
            ]
        );
    }

    #[test]
    fn the_person_recording_is_left_out() {
        let mut t = Tracker::default();
        t.saw_window(Call::Teams, Some("Acme weekly".into()), vec![]);
        let seen = Look {
            people: vec!["Priya Shah".into(), "Rhys Ellwood".into()],
            me: vec!["Rhys Ellwood".into()],
            speaking: vec!["Rhys Ellwood".into()],
            ..Default::default()
        };
        t.add(1_000, &seen);
        let priya = Look { speaking: vec!["Priya Shah".into()], ..Default::default() };
        t.add(2_000, &priya);
        t.add(3_000, &priya);
        let record = t.finish();
        assert_eq!(record.title.as_deref(), Some("Acme weekly"));
        assert_eq!(record.people, vec!["Priya Shah"]);
        assert_eq!(record.speaking, vec![Spoken { from_ms: 1_000, to_ms: 3_000, name: "Priya Shah".into() }]);
    }
}
