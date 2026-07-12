//! Local LLM summarization. v1 talks to a locally-running Ollama; the trait
//! keeps the door open for an embedded runtime later.

pub mod ollama;

use serde::Serialize;

#[derive(Debug, Serialize, Clone, Default)]
pub struct MeetingSummary {
    pub summary: String,
    pub action_items: Vec<String>,
    pub decisions: Vec<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct LlmStatus {
    pub available: bool,
    pub models: Vec<String>,
}
