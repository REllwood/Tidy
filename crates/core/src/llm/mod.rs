//! The shapes local AI works with (meeting notes, model status) and Ollama
//! detection. Summaries themselves run in the app's local AI runtime.

pub mod ollama;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct SummarySection {
    pub heading: String,
    pub points: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct MeetingSummary {
    pub summary: String,
    pub action_items: Vec<String>,
    pub decisions: Vec<String>,
    #[serde(default)]
    pub sections: Vec<SummarySection>,
    #[serde(default)]
    pub open_questions: Vec<String>,
    /// Moments marked while recording, each with what was being said then.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub moments: Vec<String>,
    /// When each point was said: the transcript time ("00:12:34") of the
    /// line its supporting quote came from, keyed by the point's text.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub times: BTreeMap<String, String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct LlmStatus {
    pub available: bool,
    pub models: Vec<String>,
}
