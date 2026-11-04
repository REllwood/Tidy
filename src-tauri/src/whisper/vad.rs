//! Whisper's voice activity model. With it, Whisper transcribes only the parts
//! of a recording where someone is speaking: long silences are skipped, which
//! is faster and stops Whisper inventing lines to fill them.

use std::path::PathBuf;

use sha2::{Digest, Sha256};
use tauri::AppHandle;

const FILE: &str = "ggml-silero-v6.2.0.bin";
const URL: &str = "https://huggingface.co/ggml-org/whisper-vad/resolve/9ffd54a1e1ee413ddf265af9913beaf518d1639b/ggml-silero-v6.2.0.bin";
const SIZE: u64 = 885_098;
const SHA256: &str = "2aa269b785eeb53a82983a20501ddf7c1d9c48e33ab63a41391ac6c9f7fb6987";

fn path(app: &AppHandle) -> PathBuf {
    crate::workspace::directory(app).join("models").join(FILE)
}

fn verified(bytes: &[u8]) -> bool {
    bytes.len() as u64 == SIZE && format!("{:x}", Sha256::digest(bytes)) == SHA256
}

/// The voice activity model, fetched the first time it is needed. It is small
/// (under 1 MB) and pinned by checksum. Transcription carries on without it
/// when it cannot be fetched, for example offline.
pub async fn ensure(app: &AppHandle) -> Option<PathBuf> {
    let path = path(app);
    if std::fs::metadata(&path).is_ok_and(|m| m.len() == SIZE) {
        return Some(path);
    }
    match fetch(&path).await {
        Ok(()) => Some(path),
        Err(error) => {
            log::warn!("Whisper's voice activity model is unavailable: {error}");
            None
        }
    }
}

async fn fetch(path: &std::path::Path) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .user_agent("Tidy")
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;
    let bytes = client
        .get(URL)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| e.to_string())?
        .bytes()
        .await
        .map_err(|e| e.to_string())?;
    if !verified(&bytes) {
        return Err("the download did not match its checksum".into());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let part = path.with_extension(format!("{}.part", crate::db::new_id()));
    std::fs::write(&part, &bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&part, path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_the_pinned_model_is_accepted() {
        assert!(!super::verified(b"not a model"));
        assert!(!super::verified(&vec![0u8; super::SIZE as usize]));
    }
}
