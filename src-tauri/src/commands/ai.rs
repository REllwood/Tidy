//! Tauri command wrappers over the shared LLM client in `appflower_core::llm`.

use crate::error::AppResult;
use appflower_core::llm::{ollama, LlmStatus, MeetingSummary};

#[tauri::command]
pub async fn ollama_status() -> LlmStatus {
    ollama::status().await
}

#[tauri::command]
pub async fn summarize_transcript(transcript: String) -> AppResult<MeetingSummary> {
    ollama::summarize(&transcript).await
}

#[tauri::command]
pub async fn ai_generate(instruction: String, context: Option<String>) -> AppResult<String> {
    ollama::ai_assist(instruction, context).await
}
