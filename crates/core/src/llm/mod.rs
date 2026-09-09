//! Local LLM summarization. v1 talks to a locally-running Ollama; the trait
//! keeps the door open for an embedded runtime later.

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
}

#[derive(Debug, Serialize, Clone)]
pub struct LlmStatus {
    pub available: bool,
    pub models: Vec<String>,
}
