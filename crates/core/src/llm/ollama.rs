//! Ollama HTTP client (localhost). Detect / list models / summarize. Degrades
//! gracefully when Ollama isn't running (connection refused → unavailable).

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::llm::{LlmStatus, MeetingSummary};

const HOST: &str = "http://localhost:11434";
const NUM_CTX: u32 = 8192;
const CHUNK_CHARS: usize = 10_000;

#[derive(Deserialize)]
struct TagsResp {
    models: Vec<TagModel>,
}
#[derive(Deserialize)]
struct TagModel {
    name: String,
}

#[derive(Serialize)]
struct ChatReq<'a> {
    model: &'a str,
    messages: Vec<Msg<'a>>,
    stream: bool,
    format: &'a str,
    options: Opts,
}
#[derive(Serialize)]
struct Msg<'a> {
    role: &'a str,
    content: &'a str,
}
#[derive(Serialize)]
struct Opts {
    num_ctx: u32,
}
#[derive(Deserialize)]
struct ChatResp {
    message: ChatMsg,
}
#[derive(Deserialize)]
struct ChatMsg {
    content: String,
}

/// Lenient parse of the model's JSON output.
#[derive(Deserialize, Default)]
struct RawSummary {
    #[serde(default)]
    summary: String,
    #[serde(default)]
    action_items: Vec<String>,
    #[serde(default)]
    decisions: Vec<String>,
}

const SYSTEM_PROMPT: &str = "You are a meeting-notes assistant. Read the transcript and reply with ONLY a JSON object with keys: \"summary\" (a concise paragraph), \"action_items\" (array of short strings), and \"decisions\" (array of short strings). Do not include any text outside the JSON.";

async fn fetch_tags() -> AppResult<Vec<String>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .map_err(|e| AppError::Other(e.to_string()))?;
    let resp = client
        .get(format!("{HOST}/api/tags"))
        .send()
        .await
        .map_err(|e| AppError::Other(format!("ollama unavailable: {e}")))?;
    let tags: TagsResp = resp
        .json()
        .await
        .map_err(|e| AppError::Other(format!("ollama tags parse: {e}")))?;
    Ok(tags.models.into_iter().map(|m| m.name).collect())
}

pub async fn status() -> LlmStatus {
    match fetch_tags().await {
        Ok(models) => LlmStatus { available: true, models },
        Err(_) => LlmStatus { available: false, models: vec![] },
    }
}

fn pick_model(models: &[String]) -> String {
    models
        .iter()
        .find(|m| m.contains("llama3.2"))
        .or_else(|| models.iter().find(|m| m.contains("llama")))
        .or_else(|| models.first())
        .cloned()
        .unwrap_or_else(|| "llama3.2".to_string())
}

async fn chat(model: &str, user: &str) -> AppResult<String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(300))
        .build()
        .map_err(|e| AppError::Other(e.to_string()))?;
    let body = ChatReq {
        model,
        messages: vec![
            Msg { role: "system", content: SYSTEM_PROMPT },
            Msg { role: "user", content: user },
        ],
        stream: false,
        format: "json",
        options: Opts { num_ctx: NUM_CTX },
    };
    let resp = client
        .post(format!("{HOST}/api/chat"))
        .json(&body)
        .send()
        .await
        .map_err(|e| AppError::Other(format!("ollama chat: {e}")))?
        .error_for_status()
        .map_err(|e| AppError::Other(format!("ollama chat status: {e}")))?;
    let parsed: ChatResp = resp
        .json()
        .await
        .map_err(|e| AppError::Other(format!("ollama chat parse: {e}")))?;
    Ok(parsed.message.content)
}

fn parse_summary(content: &str) -> MeetingSummary {
    match serde_json::from_str::<RawSummary>(content) {
        Ok(r) => MeetingSummary {
            summary: r.summary,
            action_items: r.action_items,
            decisions: r.decisions,
        },
        // If the model didn't return clean JSON, keep the raw text as the summary.
        Err(_) => MeetingSummary {
            summary: content.trim().to_string(),
            action_items: vec![],
            decisions: vec![],
        },
    }
}

fn chunk(text: &str, size: usize) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    chars.chunks(size).map(|c| c.iter().collect()).collect()
}

/// Hard cap on transcript size to bound map-reduce work (~a very long meeting).
const MAX_TRANSCRIPT_CHARS: usize = 400_000;

pub async fn summarize(transcript: &str) -> AppResult<MeetingSummary> {
    if transcript.chars().count() > MAX_TRANSCRIPT_CHARS {
        return Err(AppError::Invalid(
            "transcript too long to summarize".into(),
        ));
    }
    let models = fetch_tags()
        .await
        .map_err(|_| AppError::Other("Ollama is not running".into()))?;
    if models.is_empty() {
        return Err(AppError::Other("No Ollama models installed".into()));
    }
    let model = pick_model(&models);

    // Map-reduce for long transcripts so we don't blow the context window.
    if transcript.chars().count() > CHUNK_CHARS + 2000 {
        // Tolerate individual chunk failures: keep the partials we got rather
        // than discarding all summary work (and mislabeling Ollama as absent).
        let mut partials = Vec::new();
        for c in chunk(transcript, CHUNK_CHARS) {
            match chat(&model, &c).await {
                Ok(out) => partials.push(parse_summary(&out).summary),
                Err(e) => log::warn!("summary chunk failed: {e}"),
            }
        }
        if partials.is_empty() {
            return Err(AppError::Other("summarization failed for all chunks".into()));
        }
        let combined = partials.join("\n\n");
        Ok(parse_summary(&chat(&model, &combined).await?))
    } else {
        Ok(parse_summary(&chat(&model, transcript).await?))
    }
}

/// Freeform generation for the in-editor AI assistant (no JSON formatting).
pub async fn generate(system: &str, user: &str) -> AppResult<String> {
    let models = fetch_tags()
        .await
        .map_err(|_| AppError::Other("Ollama is not running".into()))?;
    if models.is_empty() {
        return Err(AppError::Other("No Ollama models installed".into()));
    }
    let model = pick_model(&models);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(300))
        .build()
        .map_err(|e| AppError::Other(e.to_string()))?;
    let body = serde_json::json!({
        "model": model,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user},
        ],
        "stream": false,
        "options": { "num_ctx": NUM_CTX },
    });
    let resp = client
        .post(format!("{HOST}/api/chat"))
        .json(&body)
        .send()
        .await
        .map_err(|e| AppError::Other(format!("ollama chat: {e}")))?
        .error_for_status()
        .map_err(|e| AppError::Other(format!("ollama chat status: {e}")))?;
    let parsed: ChatResp = resp
        .json()
        .await
        .map_err(|e| AppError::Other(format!("ollama chat parse: {e}")))?;
    Ok(parsed.message.content.trim().to_string())
}

const AI_SYSTEM: &str = "You are a concise writing assistant embedded in a notes app. Follow the user's instruction and reply with only the resulting text — no preamble, no markdown fences.";

/// The in-editor AI assistant. Tauri-free so the sidecar can reuse it too.
pub async fn ai_assist(instruction: String, context: Option<String>) -> AppResult<String> {
    if instruction.chars().count() + context.as_deref().unwrap_or("").chars().count()
        > MAX_TRANSCRIPT_CHARS
    {
        return Err(AppError::Invalid("input too long".into()));
    }
    let user = match context.as_deref() {
        Some(c) if !c.is_empty() => format!("{instruction}\n\nText:\n{c}"),
        _ => instruction,
    };
    generate(AI_SYSTEM, &user).await
}
