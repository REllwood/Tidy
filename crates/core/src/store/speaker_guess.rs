//! Working out who "Speaker 2" is from the conversation itself. People say
//! each other's names: "What do you think, Tom?" before Tom answers, "Thanks,
//! Mia" after Mia speaks, "Hi, I'm Priya" as Priya starts. Each cue counts
//! towards a name for a speaker label, and a guess is only made when the
//! cues agree. Every guess keeps the line it came from, so the person can
//! see why.

use std::collections::HashMap;

use serde::Serialize;

use crate::store::recordings::Spoken;

/// One speaker's turn: their label and what they said, with times in ms.
#[derive(Debug, Clone)]
pub struct Turn {
    pub label: String,
    pub lines: Vec<(i64, String)>,
}

/// A name worked out for a speaker label.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Guess {
    pub label: String,
    pub name: String,
    /// How strongly the conversation points to this name.
    pub score: f32,
    /// The line that points to it most clearly, as it was said.
    pub because: String,
    /// When that line was said, in ms.
    pub at_ms: i64,
    /// Strong enough to apply without asking.
    pub certain: bool,
    /// The call app that showed this person speaking, such as "Teams", when
    /// the guess comes from the call window rather than the conversation.
    pub from_app: Option<String>,
}

const ADDRESSED: f32 = 2.0;
const THANKED: f32 = 1.5;
const INTRODUCED: f32 = 3.5;
const SUGGEST: f32 = 2.0;
const APPLY: f32 = 3.5;

/// Capitalised words that are not names, even mid-sentence.
const NOT_NAMES: &[&str] = &[
    "i", "i'm", "ok", "okay", "yes", "yeah", "no", "hi", "hello", "hey", "thanks", "thank", "sorry",
    "great", "right", "well", "so", "and", "but", "also", "just", "maybe", "everyone", "everybody",
    "team", "guys", "all", "folks", "mate", "you", "we", "they", "he", "she", "it", "the", "a",
    "monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday", "january",
    "february", "march", "april", "may", "june", "july", "august", "september", "october",
    "november", "december", "today", "tomorrow", "yesterday", "good", "cool", "nice", "sure",
    "speaker", "others", "zoom", "teams", "slack", "google", "there", "here", "what", "how", "why",
];

fn clean(word: &str) -> &str {
    word.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'')
}

/// A name as written: capitalised, letters only, and not a common word.
fn name_word(word: &str) -> Option<&str> {
    let w = clean(word);
    let mut chars = w.chars();
    let first = chars.next()?;
    let looks = first.is_uppercase()
        && w.chars().count() >= 2
        && w.chars().all(|c| c.is_alphabetic() || c == '\'' || c == '-')
        && chars.any(|c| c.is_lowercase());
    (looks && !NOT_NAMES.contains(&w.to_lowercase().as_str())).then_some(w)
}

fn sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for c in text.chars() {
        current.push(c);
        if matches!(c, '.' | '?' | '!') {
            if !current.trim().is_empty() {
                out.push(current.trim().to_string());
            }
            current.clear();
        }
    }
    if !current.trim().is_empty() {
        out.push(current.trim().to_string());
    }
    out
}

/// The name someone is spoken to by in a sentence: "What do you think,
/// Tom?", "Tom, can you take this?", "Over to you Tom." or just "Tom?".
pub fn addressed(sentence: &str) -> Option<String> {
    let words: Vec<&str> = sentence.split_whitespace().collect();
    let last = *words.last()?;
    // ", Tom?" or "you Tom?" at the end.
    if let Some(name) = name_word(last) {
        let before = words.len().checked_sub(2).map(|i| words[i]);
        let after_comma = before.is_some_and(|b| b.ends_with(','));
        // "What do you think Tom?", "Any thoughts Mia?", "How about you Sam?"
        let asking = before.is_some_and(|b| {
            ["you", "think", "reckon", "thoughts", "agree", "ideas"].contains(&clean(b).to_lowercase().as_str())
        }) && last.ends_with('?');
        if words.len() == 1 || after_comma || asking {
            return Some(name.to_string());
        }
    }
    // "Tom, …" at the start.
    let first = *words.first()?;
    if words.len() >= 3 && first.ends_with(',') {
        if let Some(name) = name_word(first) {
            return Some(name.to_string());
        }
    }
    None
}

/// A name someone is thanked by at the start of a sentence: "Thanks, Mia."
pub fn thanked(sentence: &str) -> Option<String> {
    let lower = sentence.to_lowercase();
    let rest = ["thanks", "thank you", "cheers", "good point", "great point", "nice one"]
        .iter()
        .find_map(|opening| lower.strip_prefix(opening).map(|_| &sentence[opening.len()..]))?;
    let word = rest.trim_start_matches([',', ' ']).split_whitespace().next()?;
    name_word(word).map(str::to_string)
}

/// A name someone gives themselves: "I'm Priya", "This is Priya", "Priya here".
pub fn introduced(sentence: &str) -> Option<String> {
    let words: Vec<&str> = sentence.split_whitespace().collect();
    for (i, pair) in words.windows(2).enumerate() {
        let opener = clean(pair[0]).to_lowercase();
        let is_intro = match opener.as_str() {
            "i'm" | "im" => true,
            "is" => i >= 1 && {
                let before = clean(words[i - 1]).to_lowercase();
                before == "this" || before == "name"
            },
            _ => false,
        };
        // "I'm Priya", but not "I'm Priya's manager".
        if is_intro && !pair[1].contains("'s") {
            if let Some(name) = name_word(pair[1]) {
                return Some(name.to_string());
            }
        }
        if clean(pair[1]).eq_ignore_ascii_case("here") && i == 0 {
            if let Some(name) = name_word(pair[0]) {
                return Some(name.to_string());
            }
        }
    }
    None
}

/// A label that is still a placeholder, which a name could replace. "You"
/// is the person recording and "Others" may be several people, so neither
/// is guessed.
pub fn guessable(label: &str) -> bool {
    label.starts_with("Speaker ") && label["Speaker ".len()..].chars().all(|c| c.is_ascii_digit())
}

/// The invitee a spoken name refers to, by full or first name. Without a
/// list of invitees any name is taken as said.
fn resolve(name: &str, invitees: &[String]) -> Option<String> {
    if invitees.is_empty() {
        return Some(name.to_string());
    }
    let lower = name.to_lowercase();
    invitees
        .iter()
        .find(|i| {
            let i = i.to_lowercase();
            i == lower || i.split_whitespace().next() == Some(lower.as_str())
        })
        .cloned()
}

#[derive(Default)]
struct Tally {
    score: f32,
    best: f32,
    because: String,
    at_ms: i64,
}

/// Names for the placeholder labels in a conversation, strongest first.
pub fn guess(turns: &[Turn], invitees: &[String]) -> Vec<Guess> {
    let mut tallies: HashMap<(String, String), Tally> = HashMap::new();
    let mut add = |label: &str, name: &str, weight: f32, line: &str, at: i64| {
        let tally = tallies.entry((label.to_string(), name.to_string())).or_default();
        tally.score += weight;
        if weight > tally.best {
            tally.best = weight;
            tally.because = line.to_string();
            tally.at_ms = at;
        }
    };
    for (index, turn) in turns.iter().enumerate() {
        let previous = index.checked_sub(1).map(|i| &turns[i]).filter(|t| t.label != turn.label);
        let next = turns.get(index + 1).filter(|t| t.label != turn.label);
        let count = turn.lines.len();
        for (position, (at, line)) in turn.lines.iter().enumerate() {
            let said = sentences(line);
            let ends_turn = position + 1 == count;
            for (s, sentence) in said.iter().enumerate() {
                if let Some(name) = introduced(sentence).and_then(|n| resolve(&n, invitees)) {
                    add(&turn.label, &name, INTRODUCED, line, *at);
                }
                if let Some(name) = thanked(sentence).and_then(|n| resolve(&n, invitees)) {
                    if position == 0 && s == 0 {
                        if let Some(previous) = previous {
                            add(&previous.label, &name, THANKED, line, *at);
                        }
                    }
                    add(&turn.label, &name, -THANKED, line, *at);
                }
                if let Some(name) = addressed(sentence).and_then(|n| resolve(&n, invitees)) {
                    // Spoken to just before someone else starts talking.
                    if ends_turn && s + 1 == said.len() {
                        if let Some(next) = next {
                            add(&next.label, &name, ADDRESSED, line, *at);
                        }
                    }
                    // Nobody speaks to themselves by name.
                    add(&turn.label, &name, -ADDRESSED, line, *at);
                }
            }
        }
    }

    // The best name for each label, when it clearly beats the rest.
    let mut by_label: HashMap<String, Vec<(String, Tally)>> = HashMap::new();
    for ((label, name), tally) in tallies {
        if guessable(&label) {
            by_label.entry(label).or_default().push((name, tally));
        }
    }
    let mut guesses: Vec<Guess> = by_label
        .into_iter()
        .filter_map(|(label, mut names)| {
            names.sort_by(|a, b| b.1.score.total_cmp(&a.1.score));
            let (name, best) = names.first()?;
            let runner_up = names.get(1).map_or(0.0, |(_, t)| t.score.max(0.0));
            (best.score >= SUGGEST && best.score - runner_up >= 1.5).then(|| Guess {
                label,
                name: name.clone(),
                score: best.score,
                because: best.because.clone(),
                at_ms: best.at_ms,
                certain: best.score >= APPLY && runner_up == 0.0,
                from_app: None,
            })
        })
        .collect();
    guesses.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.label.cmp(&b.label)));
    // One name per person: the strongest claim keeps it.
    let mut taken: Vec<String> = Vec::new();
    guesses.retain(|g| {
        let free = !taken.contains(&g.name);
        taken.push(g.name.clone());
        free
    });
    guesses
}

/// A turn runs until the next one starts, but no further than this past its
/// last line, so a long pause does not count as talking.
const TURN_TAIL_MS: i64 = 15_000;
/// Shown speaking for this long during a voice's turns, and for most of the
/// time anyone was shown, suggests that name.
const CALL_SUGGEST_MS: i64 = 3_000;
const CALL_SUGGEST_SHARE: f64 = 0.6;
/// Longer and clearer still, and the name is applied without asking.
const CALL_APPLY_MS: i64 = 8_000;
const CALL_APPLY_SHARE: f64 = 0.75;

fn overlap(a: (i64, i64), b: (i64, i64)) -> i64 {
    (a.1.min(b.1) - a.0.max(b.0)).max(0)
}

/// Names for the placeholder labels from who the call window showed
/// speaking during each voice's turns. Several voices may get the same name,
/// as one person is sometimes split into two voices.
pub fn from_call(turns: &[Turn], speaking: &[Spoken], app: &str) -> Vec<Guess> {
    // Each line, from when it starts until the next line or turn starts.
    let mut spans: Vec<(&str, i64, i64, &str)> = Vec::new();
    for (index, turn) in turns.iter().enumerate() {
        let next_turn = turns.get(index + 1).and_then(|t| t.lines.first()).map(|(at, _)| *at);
        let Some(&(last, _)) = turn.lines.last() else { continue };
        let turn_end = next_turn.filter(|n| *n > last).unwrap_or(i64::MAX).min(last + TURN_TAIL_MS);
        for (i, (at, line)) in turn.lines.iter().enumerate() {
            let end = turn.lines.get(i + 1).map_or(turn_end, |(next, _)| *next).max(at + 1_000);
            spans.push((&turn.label, *at, end, line));
        }
    }

    // How long each name was shown speaking during each label's lines.
    let mut shown: HashMap<(&str, &str), i64> = HashMap::new();
    let mut best_line: HashMap<(&str, &str), (i64, i64, &str)> = HashMap::new();
    for &(label, start, end, line) in &spans {
        if !guessable(label) {
            continue;
        }
        for s in speaking {
            let ms = overlap((start, end), (s.from_ms, s.to_ms));
            if ms == 0 {
                continue;
            }
            *shown.entry((label, &s.name)).or_default() += ms;
            let best = best_line.entry((label, &s.name)).or_insert((0, start, line));
            if ms > best.0 {
                *best = (ms, start, line);
            }
        }
    }

    let mut by_label: HashMap<&str, Vec<(&str, i64)>> = HashMap::new();
    for (&(label, name), &ms) in &shown {
        by_label.entry(label).or_default().push((name, ms));
    }
    let mut guesses: Vec<Guess> = by_label
        .into_iter()
        .filter_map(|(label, mut names)| {
            names.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
            let total: i64 = names.iter().map(|(_, ms)| ms).sum();
            let (name, ms) = *names.first()?;
            let share = ms as f64 / total as f64;
            if ms < CALL_SUGGEST_MS || share < CALL_SUGGEST_SHARE {
                return None;
            }
            let (_, at_ms, because) = best_line[&(label, name)];
            Some(Guess {
                label: label.to_string(),
                name: name.to_string(),
                score: ms as f32 / 1000.0,
                because: because.to_string(),
                at_ms,
                certain: ms >= CALL_APPLY_MS && share >= CALL_APPLY_SHARE,
                from_app: Some(app.to_string()),
            })
        })
        .collect();
    guesses.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.label.cmp(&b.label)));
    guesses
}

/// One guess per label, from the call window where it saw enough, otherwise
/// from the conversation. A name the call window gave a voice is not also
/// given to another voice from the conversation.
pub fn combine(call: Vec<Guess>, talk: Vec<Guess>) -> Vec<Guess> {
    let mut out: Vec<Guess> = Vec::new();
    for g in &call {
        let spoken = talk.iter().find(|t| t.label == g.label);
        // A certain name from the conversation beats a mere suggestion here.
        if g.certain || !spoken.is_some_and(|t| t.certain) {
            out.push(g.clone());
        }
    }
    for t in talk {
        let label_free = !out.iter().any(|g| g.label == t.label);
        let name_free = !out.iter().any(|g| g.name == t.name);
        if label_free && name_free {
            out.push(t);
        }
    }
    out.sort_by(|a, b| b.certain.cmp(&a.certain).then(b.score.total_cmp(&a.score)).then(a.label.cmp(&b.label)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(label: &str, lines: &[&str]) -> Turn {
        Turn { label: label.into(), lines: lines.iter().enumerate().map(|(i, l)| (i as i64 * 1000, l.to_string())).collect() }
    }

    #[test]
    fn spoken_names_are_found_in_their_usual_places() {
        assert_eq!(addressed("What do you think, Tom?").as_deref(), Some("Tom"));
        assert_eq!(addressed("What do you think Tom?").as_deref(), Some("Tom"));
        assert_eq!(addressed("Tom, can you take the redirects?").as_deref(), Some("Tom"));
        assert_eq!(addressed("Over to you, Mia.").as_deref(), Some("Mia"));
        assert_eq!(addressed("Priya?").as_deref(), Some("Priya"));
        assert_eq!(addressed("What do you think about Cliniko?"), None, "a product, not a person");
        assert_eq!(addressed("Right, let's start."), None);
        assert_eq!(addressed("Sounds good, thanks."), None);
        assert_eq!(thanked("Thanks, Mia.").as_deref(), Some("Mia"));
        assert_eq!(thanked("Thank you Sam, that's helpful.").as_deref(), Some("Sam"));
        assert_eq!(thanked("Thanks everyone."), None);
        assert_eq!(introduced("Hi all, I'm Priya from the design team.").as_deref(), Some("Priya"));
        assert_eq!(introduced("This is Jordan.").as_deref(), Some("Jordan"));
        assert_eq!(introduced("Sam here, sorry I'm late.").as_deref(), Some("Sam"));
        assert_eq!(introduced("I'm sure that works."), None);
        assert_eq!(introduced("I'm Priya's manager."), None);
        assert_eq!(addressed("Have you seen Cliniko?"), None);
    }

    #[test]
    fn the_person_spoken_to_is_the_one_who_answers() {
        let turns = vec![
            turn("Speaker 1", &["Let's look at the launch.", "What do you think, Tom?"]),
            turn("Speaker 2", &["I'd move it to the 28th."]),
            turn("Speaker 1", &["Thanks, Tom. Mia, can you send the timeline?"]),
            turn("Speaker 3", &["Yes, by Friday."]),
        ];
        let guesses = guess(&turns, &[]);
        let tom = guesses.iter().find(|g| g.label == "Speaker 2").expect("Speaker 2 is Tom");
        assert_eq!(tom.name, "Tom");
        assert!(tom.certain, "asked and thanked: {tom:?}");
        assert_eq!(tom.because, "What do you think, Tom?");
        let mia = guesses.iter().find(|g| g.label == "Speaker 3").unwrap();
        assert_eq!(mia.name, "Mia");
        assert!(!mia.certain, "asked once is a suggestion");
        assert!(guesses.iter().all(|g| g.label != "Speaker 1"), "the one asking is not named after who they ask");
    }

    #[test]
    fn introductions_count_most_and_invitees_give_full_names() {
        let turns = vec![turn("Speaker 1", &["Hi everyone, I'm Priya."]), turn("Speaker 2", &["Morning."])];
        let invitees = vec!["Priya Shah".to_string(), "Tom Baker".to_string()];
        let guesses = guess(&turns, &invitees);
        assert_eq!(guesses[0].name, "Priya Shah");
        assert!(guesses[0].certain);
        // A name nobody invited is not taken when the invitees are known.
        let stranger = vec![turn("Speaker 1", &["I'm Zara."])];
        assert!(guess(&stranger, &invitees).is_empty());
    }

    #[test]
    fn conflicting_cues_and_group_labels_get_no_guess() {
        let turns = vec![
            turn("Speaker 1", &["What do you think, Tom?"]),
            turn("Speaker 2", &["Not sure."]),
            turn("Speaker 1", &["Mia, any thoughts?"]),
            turn("Speaker 2", &["Maybe Friday."]),
            turn("You", &["Over to you, Sam."]),
            turn("Others", &["Sure."]),
        ];
        let guesses = guess(&turns, &[]);
        assert!(guesses.iter().all(|g| g.label != "Speaker 2"), "Tom or Mia is unclear: {guesses:?}");
        assert!(guesses.iter().all(|g| g.label != "Others"), "Others may be several people");
        assert!(guessable("Speaker 12") && !guessable("You") && !guessable("Others") && !guessable("Tom"));
    }

    fn line(label: &str, at_s: &[i64]) -> Turn {
        Turn { label: label.into(), lines: at_s.iter().map(|s| (s * 1000, format!("{label} at {s}s."))).collect() }
    }

    fn shown(name: &str, from_s: i64, to_s: i64) -> Spoken {
        Spoken { from_ms: from_s * 1000, to_ms: to_s * 1000, name: name.into() }
    }

    #[test]
    fn the_call_window_names_who_was_speaking_during_each_voice() {
        let turns = vec![
            line("Speaker 1", &[0, 5]),
            line("You", &[10]),
            line("Speaker 2", &[14, 18]),
            line("Speaker 1", &[25]),
        ];
        let speaking = vec![
            shown("Priya Shah", 0, 10),
            shown("Tom Baker", 14, 22),
            shown("Priya Shah", 25, 31),
            // A blip of Tom highlighted while Priya talks is outweighed.
            shown("Tom Baker", 26, 27),
        ];
        let guesses = from_call(&turns, &speaking, "Teams");
        let priya = guesses.iter().find(|g| g.label == "Speaker 1").unwrap();
        assert_eq!(priya.name, "Priya Shah");
        assert!(priya.certain, "{priya:?}");
        assert_eq!(priya.from_app.as_deref(), Some("Teams"));
        assert_eq!(priya.at_ms, 25_000, "the line shown speaking longest");
        let tom = guesses.iter().find(|g| g.label == "Speaker 2").unwrap();
        assert_eq!(tom.name, "Tom Baker");
        assert!(tom.certain, "8 s with nobody else shown: {tom:?}");
        assert!(guesses.iter().all(|g| g.label != "You"), "the person recording is never guessed");
    }

    #[test]
    fn little_or_mixed_evidence_is_only_a_suggestion_or_nothing() {
        let turns = vec![line("Speaker 1", &[0]), line("Speaker 2", &[20]), line("Speaker 3", &[40])];
        let speaking = vec![
            // Speaker 1: four seconds of Priya, a suggestion only.
            shown("Priya Shah", 0, 4),
            // Speaker 2: Tom and Mia shown about equally, so no name.
            shown("Tom Baker", 20, 25),
            shown("Mia Chen", 25, 30),
        ];
        let guesses = from_call(&turns, &speaking, "Google Meet");
        let priya = guesses.iter().find(|g| g.label == "Speaker 1").unwrap();
        assert!(!priya.certain);
        assert!(guesses.iter().all(|g| g.label != "Speaker 2"), "{guesses:?}");
        assert!(guesses.iter().all(|g| g.label != "Speaker 3"), "nobody shown");
    }

    #[test]
    fn one_person_split_into_two_voices_gets_their_name_twice() {
        let turns = vec![line("Speaker 1", &[0]), line("Speaker 2", &[12]), line("Speaker 1", &[24])];
        let speaking = vec![shown("Priya Shah", 0, 36)];
        let guesses = from_call(&turns, &speaking, "Teams");
        assert_eq!(guesses.len(), 2);
        assert!(guesses.iter().all(|g| g.name == "Priya Shah" && g.certain));
    }

    #[test]
    fn the_call_window_wins_but_a_certain_conversation_keeps_a_weak_call() {
        let call = |label: &str, name: &str, certain: bool| Guess {
            label: label.into(),
            name: name.into(),
            score: 5.0,
            because: String::new(),
            at_ms: 0,
            certain,
            from_app: Some("Teams".into()),
        };
        let talk = |label: &str, name: &str, certain: bool| Guess { from_app: None, ..call(label, name, certain) };
        let combined = combine(
            vec![call("Speaker 1", "Priya Shah", true), call("Speaker 2", "Tom Baker", false)],
            vec![
                talk("Speaker 1", "Mia", true),
                talk("Speaker 2", "Sam", true),
                talk("Speaker 3", "Priya Shah", true),
                talk("Speaker 4", "Jo", false),
            ],
        );
        let named = |label: &str| combined.iter().find(|g| g.label == label).map(|g| g.name.as_str());
        assert_eq!(named("Speaker 1"), Some("Priya Shah"), "a certain call beats the conversation");
        assert_eq!(named("Speaker 2"), Some("Sam"), "a certain conversation beats a call suggestion");
        assert_eq!(named("Speaker 3"), None, "Priya is already Speaker 1");
        assert_eq!(named("Speaker 4"), Some("Jo"));
    }

    #[test]
    fn one_name_goes_to_one_speaker() {
        let turns = vec![
            turn("Speaker 1", &["I'm Tom."]),
            turn("Speaker 2", &["What do you think, Tom?"]),
            turn("Speaker 3", &["Hmm."]),
        ];
        let guesses = guess(&turns, &[]);
        let toms: Vec<_> = guesses.iter().filter(|g| g.name == "Tom").collect();
        assert_eq!(toms.len(), 1);
        assert_eq!(toms[0].label, "Speaker 1", "the introduction wins");
    }
}
