//! Ollama detection (localhost). Lists the models a local Ollama serves so
//! Settings can offer them; summaries and chat go through the app's local AI
//! runtime, which talks to Ollama itself when it is chosen. Degrades
//! gracefully when Ollama isn't running (connection refused → unavailable).

use std::time::Duration;

use serde::Deserialize;

use crate::error::{AppError, AppResult};
use crate::llm::LlmStatus;

const HOST: &str = "http://localhost:11434";

#[derive(Deserialize)]
struct TagsResp {
    models: Vec<TagModel>,
}
#[derive(Deserialize)]
struct TagModel {
    name: String,
}

/// A client for the local Ollama daemon: never via a proxy (localhost traffic
/// must not leave the machine) and always bounded by a timeout.
fn client(timeout: Duration) -> AppResult<reqwest::Client> {
    reqwest::Client::builder()
        .no_proxy()
        .connect_timeout(Duration::from_secs(3))
        .timeout(timeout)
        .build()
        .map_err(|e| AppError::Other(e.to_string()))
}

async fn fetch_tags() -> AppResult<Vec<String>> {
    let client = client(Duration::from_secs(3))?;
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

/// Whether Ollama is running, and the models it serves from this machine.
pub async fn status() -> LlmStatus {
    match fetch_tags().await {
        Ok(models) => LlmStatus {
            available: true,
            models: models.into_iter().filter(|m| is_local_model(m)).collect(),
        },
        Err(_) => LlmStatus {
            available: false,
            models: vec![],
        },
    }
}

/// Is this model actually served from the local machine? Ollama lists
/// cloud-hosted models alongside local ones, and a host-qualified name points
/// at a remote registry. Tidy is local-first: a note must never be shipped off
/// the machine because a cloud model happened to be listed.
fn is_local_model(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    if name.contains("cloud") {
        return false;
    }
    match name.split_once('/') {
        // "registry.example.com/user/model" or "host:11434/model"
        Some((host, _)) if host.contains('.') || host.contains(':') => {
            host.starts_with("localhost") || host.starts_with("127.0.0.1")
        }
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_models_served_here_count_as_local() {
        assert!(is_local_model("llama3.2:3b"));
        assert!(is_local_model("localhost:11434/llama3.2"));
        assert!(!is_local_model("gpt-oss:120b-cloud"));
        assert!(!is_local_model("registry.example.com/acme/qwen"));
        assert!(!is_local_model("ollama.com/library/gpt-oss:cloud"));
    }
}
