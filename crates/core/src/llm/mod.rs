//! The shapes local AI works with (meeting notes, model status) and Ollama
//! detection. Summaries themselves run in the app's local AI runtime.

pub mod ollama;

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
}

#[derive(Debug, Serialize, Clone)]
pub struct LlmStatus {
    pub available: bool,
    pub models: Vec<String>,
}
