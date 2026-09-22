//! Tauri command wrappers over the shared LLM client in `tidy_core::llm`.

use crate::error::AppResult;
use tidy_core::llm::{ollama, LlmStatus};

#[tauri::command]
pub async fn ollama_status() -> LlmStatus {
    ollama::status().await
}

#[tauri::command]
pub async fn ai_generate(
    app: tauri::AppHandle,
    instruction: String,
    context: Option<String>,
    request_id: Option<String>,
    mode: Option<String>,
) -> AppResult<String> {
    crate::local_ai::generate(
        &app,
        &instruction,
        context.as_deref(),
        request_id,
        mode.as_deref(),
    )
    .await
}
