//! Working out who "Speaker 2" is from the conversation itself. People say
//! each other's names: "What do you think, Tom?" before Tom answers, "Thanks,
//! Mia" after Mia speaks, "Hi, I'm Priya" as Priya starts. Each cue counts
//! towards a name for a speaker label, and a guess is only made when the
//! cues agree. Every guess keeps the line it came from, so the person can
//! see why.

use std::collections::HashMap;

use serde::Serialize;

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
