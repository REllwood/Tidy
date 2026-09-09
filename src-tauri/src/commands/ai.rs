//! Tauri command wrappers over the shared LLM client in `appflower_core::llm`.

use crate::error::AppResult;
use appflower_core::llm::{ollama, LlmStatus, MeetingSummary};

#[tauri::command]
pub async fn ollama_status() -> LlmStatus {
    ollama::status().await
}

#[tauri::command]
pub async fn summarize_transcript(
    app: tauri::AppHandle,
    transcript: String,
) -> AppResult<MeetingSummary> {
    crate::local_ai::summarise(&app, &transcript).await
}

#[tauri::command]
pub async fn ai_generate(
    app: tauri::AppHandle,
    instruction: String,
    context: Option<String>,
) -> AppResult<String> {
    crate::local_ai::generate(&app, &instruction, context.as_deref()).await
}
