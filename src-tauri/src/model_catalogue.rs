//! One pinned catalogue shared by the native app and browser preview.
use crate::error::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

#[derive(Clone, Deserialize, Serialize)]
pub struct ModelDefinition {
    pub id: String,
    pub name: String,
    pub file: String,
    pub size: u64,
    pub purpose: String,
    pub description: String,
    /// Suggested total system RAM, not measured model allocation or a hard limit.
    pub recommended_ram_gb: u64,
    pub tier: String,
    pub url: String,
    pub sha256: String,
}
#[derive(Deserialize)]
pub struct Catalogue {
    pub transcription: Vec<ModelDefinition>,
    pub ai: Vec<ModelDefinition>,
}
pub fn catalogue() -> AppResult<&'static Catalogue> {
    static CATALOGUE: OnceLock<Result<Catalogue, String>> = OnceLock::new();
    CATALOGUE
        .get_or_init(|| {
            serde_json::from_str(include_str!("../../src/lib/modelCatalogue.json"))
                .map_err(|e| e.to_string())
        })
        .as_ref()
        .map_err(|e| AppError::Other(format!("Could not load the model catalogue: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalogue_has_pinned_complete_downloads_and_preserves_existing_ids() {
        let c = catalogue().unwrap();
        let mut ids = std::collections::HashSet::new();
        for m in c.transcription.iter().chain(&c.ai) {
            assert!(ids.insert(&m.id));
            assert!(m.size > 0 && m.recommended_ram_gb >= 4);
            assert_eq!(m.sha256.len(), 64);
            assert!(m.sha256.bytes().all(|b| b.is_ascii_hexdigit()));
            assert!(m.url.starts_with("https://huggingface.co/"));
            let revision = m
                .url
                .split("/resolve/")
                .nth(1)
                .unwrap()
                .split('/')
                .next()
                .unwrap();
            assert_eq!(revision.len(), 40);
            assert!(revision.bytes().all(|b| b.is_ascii_hexdigit()));
            assert!(m.url.ends_with(&m.file));
            assert!(!m.file.contains('/') && !m.file.contains(".."));
        }
        for id in ["tiny", "base", "small", "medium"] {
            assert!(c
                .transcription
                .iter()
                .any(|m| m.id == id && m.file == format!("ggml-{id}.bin")));
        }
        for id in ["qwen3-small", "qwen3-4b", "nomic-embed"] {
            assert!(ids.contains(&id.to_string()));
        }
        assert!(c.ai.iter().any(|m| m.id == "qwen3-32b"));
        assert!(c.transcription.iter().any(|m| m.id == "large-v3-turbo"));
    }
}
