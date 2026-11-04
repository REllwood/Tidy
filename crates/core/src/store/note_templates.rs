//! Note layouts. A meeting's notes group what was discussed under headings
//! that suit the kind of meeting, picked from whole words in its title: a
//! standup has Yesterday, Today and Blockers; a customer call has Goals, Pain
//! points, Requests and Objections. Decisions, next steps and open questions
//! keep their own sections in every layout.

use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Section {
    pub heading: &'static str,
    /// What belongs under the heading, for the model organising the notes.
    pub hint: &'static str,
}

#[derive(Debug, Serialize)]
pub struct Template {
    pub id: &'static str,
    pub name: &'static str,
    #[serde(skip)]
    pub keywords: &'static [&'static str],
    /// Empty for the general layout, which picks its own topic headings.
    pub sections: &'static [Section],
}

pub const GENERAL: &str = "general";
/// Where points that fit none of a layout's headings go.
pub const OTHER: &str = "Other discussion";

const fn section(heading: &'static str, hint: &'static str) -> Section {
    Section { heading, hint }
}

/// In the order titles are checked, so "Sprint retro" is a retro rather than
/// planning. General comes last and matches anything.
pub static TEMPLATES: &[Template] = &[
    Template {
        id: "retro",
        name: "Retro",
        keywords: &["retro", "retrospective", "post-mortem", "postmortem", "debrief", "lessons learned"],
        sections: &[
            section("Went well", "what worked and should continue"),
            section("To improve", "what went badly or slowly"),
            section("Ideas", "suggestions to try next time"),
        ],
    },
    Template {
        id: "standup",
        name: "Standup",
        keywords: &["standup", "stand-up", "stand up", "daily", "scrum", "huddle"],
        sections: &[
            section("Yesterday", "work finished since the last standup"),
            section("Today", "what people plan to work on next"),
            section("Blockers", "anything in someone's way"),
        ],
    },
    Template {
        id: "one_on_one",
        name: "1:1",
        keywords: &["1:1", "1-1", "1on1", "one on one", "one-on-one", "catch up", "catch-up", "check in", "check-in"],
        sections: &[
            section("Updates", "news and progress since last time"),
            section("Feedback", "feedback given or asked for, in either direction"),
            section("Concerns", "worries or problems raised"),
            section("Growth", "goals, development and career"),
        ],
    },
    Template {
        id: "interview",
        name: "Interview",
        keywords: &["interview", "candidate", "screening", "hiring"],
        sections: &[
            section("Background", "the candidate's experience and history"),
            section("Strengths", "what stood out well"),
            section("Concerns", "gaps or worries"),
            section("Their questions", "what the candidate asked"),
        ],
    },
    Template {
        id: "customer",
        name: "Customer call",
        keywords: &["customer", "client", "demo", "discovery", "sales", "prospect", "onboarding"],
        sections: &[
            section("Goals", "what the customer wants to achieve"),
            section("Pain points", "problems they have now"),
            section("Requests", "features, changes or help they asked for"),
            section("Objections", "concerns about price, fit or timing"),
        ],
    },
    Template {
        id: "planning",
        name: "Planning",
        keywords: &["planning", "plan", "sprint", "roadmap", "kickoff", "kick-off", "strategy", "workshop"],
        sections: &[
            section("Goals", "what the work is for"),
            section("Scope", "what is in and what is out"),
            section("Estimates and dates", "sizes, deadlines and milestones"),
            section("Risks", "what could go wrong"),
        ],
    },
    Template { id: GENERAL, name: "General", keywords: &[], sections: &[] },
];

/// Lower case, with everything but letters, digits, ':' and '-' as spaces, so
/// keywords match whole words: "Retro" in "Sprint retro" but not "retrofit".
fn words(title: &str) -> String {
    let cleaned: String = title
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() || c == ':' || c == '-' { c } else { ' ' })
        .collect();
    let words: Vec<&str> = cleaned
        .split_whitespace()
        .map(|w| w.trim_matches(|c| c == ':' || c == '-'))
        .filter(|w| !w.is_empty())
        .collect();
    format!(" {} ", words.join(" "))
}

/// The layout a meeting's title suggests.
pub fn choose(title: &str) -> &'static Template {
    let title = words(title);
    TEMPLATES
        .iter()
        .find(|t| t.keywords.iter().any(|k| title.contains(&format!(" {k} "))))
        .unwrap_or_else(general)
}

pub fn by_id(id: &str) -> Option<&'static Template> {
    TEMPLATES.iter().find(|t| t.id == id)
}

pub fn general() -> &'static Template {
    TEMPLATES.last().expect("the general layout")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_title_picks_the_layout_by_whole_words() {
        assert_eq!(choose("Daily standup").id, "standup");
        assert_eq!(choose("Sprint retro").id, "retro");
        assert_eq!(choose("Sprint planning").id, "planning");
        assert_eq!(choose("Rhys / Priya 1:1").id, "one_on_one");
        assert_eq!(choose("Weekly catch-up").id, "one_on_one");
        assert_eq!(choose("Harbourside Dental client call").id, "customer");
        assert_eq!(choose("Interview: senior designer").id, "interview");
        assert_eq!(choose("Retrofit budget review").id, GENERAL, "not a retro");
        assert_eq!(choose("Planet Fitness sync").id, GENERAL, "not planning");
        assert_eq!(choose("").id, GENERAL);
    }

    #[test]
    fn every_layout_can_be_found_by_id() {
        for t in TEMPLATES {
            assert_eq!(by_id(t.id).unwrap().name, t.name);
        }
        assert!(by_id("missing").is_none());
        assert!(general().sections.is_empty());
    }
}
