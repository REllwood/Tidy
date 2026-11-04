use super::{db, AiState, Config, Progress};
use serde::{Deserialize, Serialize};
use crate::error::{AppError, AppResult};
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tauri::AppHandle;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub use crate::model_catalogue::ModelDefinition as Model;
pub fn all() -> AppResult<&'static [Model]> {
    Ok(&crate::model_catalogue::catalogue()?.ai)
}
pub fn model(id: &str) -> AppResult<&'static Model> {
    all()?
        .iter()
        .find(|m| m.id == id)
        .ok_or_else(|| AppError::Invalid("Unknown local AI model".into()))
}
pub fn dir(app: &AppHandle) -> AppResult<PathBuf> {
    Ok(crate::workspace::directory(app).join("models/local-ai"))
}
pub fn path(app: &AppHandle, id: &str) -> AppResult<PathBuf> {
    model(id)?;
    Ok(dir(app)?.join(format!("{id}.gguf")))
}
pub fn installed(app: &AppHandle, m: &Model) -> bool {
    path(app, &m.id)
        .ok()
        .and_then(|p| std::fs::metadata(p).ok())
        .is_some_and(|meta| meta.len() == m.size)
}
/// What a model file looked like when it was last verified in full.
#[derive(Deserialize, Serialize)]
struct Fingerprint {
    size: u64,
    mtime_ms: i64,
    sha256: String,
}
fn fingerprint_key(id: &str) -> String {
    format!("local_ai.verified.{id}")
}
fn file_stamp(path: &Path) -> AppResult<(u64, i64)> {
    let meta = std::fs::metadata(path)?;
    let modified = meta
        .modified()?
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    Ok((meta.len(), modified))
}

/// Remember that this exact file passed a full hash check.
pub async fn remember_verified(app: &AppHandle, id: &str, path: &Path) -> AppResult<()> {
    let sha256 = model(id)?.sha256.clone();
    let (size, mtime_ms) = file_stamp(path)?;
    let value = serde_json::to_string(&Fingerprint {
        size,
        mtime_ms,
        sha256,
    })?;
    let key = fingerprint_key(id);
    db(app, move |c| crate::db::set_setting(c, &key, &value)).await
}

/// Whether a stored fingerprint still describes the file on disk. The pinned
/// hash is part of it, so a change to the catalogue forces a fresh check.
fn fingerprint_matches(stored: Option<&str>, size: u64, mtime_ms: i64, sha256: &str) -> bool {
    stored
        .and_then(|raw| serde_json::from_str::<Fingerprint>(raw).ok())
        .is_some_and(|f| f.size == size && f.mtime_ms == mtime_ms && f.sha256 == sha256)
}

/// Hash a model in full the first time it is used, then remember what the file
/// looked like. Later starts compare only its size and timestamp, so opening
/// the app does not re-read gigabytes from disk every time.
pub async fn ensure_verified(
    app: Option<&AppHandle>,
    path: &Path,
    m: &Model,
    state: &AiState,
) -> AppResult<()> {
    let Some(app) = app else {
        return verify(path, m, state).await;
    };
    let (size, mtime_ms) = file_stamp(path)?;
    let key = fingerprint_key(&m.id);
    let stored = db(app, move |c| crate::db::get_setting(c, &key)).await?;
    if fingerprint_matches(stored.as_deref(), size, mtime_ms, &m.sha256) {
        return Ok(());
    }
    verify(path, m, state).await?;
    remember_verified(app, &m.id, path).await
}

pub async fn verify(path: &Path, m: &Model, state: &AiState) -> AppResult<()> {
    let mut f = tokio::fs::File::open(path).await?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    let mut total = 0;
    loop {
        state.check_cancel()?;
        let n = f.read(&mut buffer).await?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
        total += n as u64;
    }
    if total != m.size || format!("{:x}", hash.finalize()) != m.sha256 {
        return Err(AppError::Other(
            "Model verification failed. Download it again before use.".into(),
        ));
    }
    Ok(())
}
pub async fn download(app: &AppHandle, id: &str, state: &AiState) -> AppResult<()> {
    let m = model(id)?;
    let target = path(app, id)?;
    tokio::fs::create_dir_all(dir(app)?).await?;
    state.progress(Progress::new("Checking model", Some(id)));
    if tokio::fs::try_exists(&target).await? {
        ensure_verified(Some(app), &target, m, state).await?;
        return Ok(());
    }
    let folder = dir(app)?;
    let free = tokio::task::spawn_blocking(move || -> AppResult<u64> {
        let output = std::process::Command::new("/bin/df")
            .args(["-Pk"])
            .arg(folder)
            .output()?;
        let text = String::from_utf8_lossy(&output.stdout);
        let kb = text
            .lines()
            .last()
            .and_then(|l| l.split_whitespace().nth(3))
            .and_then(|n| n.parse::<u64>().ok())
            .ok_or_else(|| AppError::Other("Could not check available disk space".into()))?;
        Ok(kb * 1024)
    })
    .await
    .map_err(|e| AppError::Other(e.to_string()))??;
    if free < m.size + 512_000_000 {
        return Err(AppError::Other(
            "Not enough disk space for this model. Free some space and retry.".into(),
        ));
    }
    // Retain interrupted downloads for diagnosis. A fresh, unique file is verified
    // before it is made available to inference; a partial can never be selected.
    let partial = target.with_extension(format!("{}.part", crate::db::new_id()));
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| AppError::Other(e.to_string()))?;
    let response = tokio::select! {
        _=state.cancelled()=>return Err(AppError::Other("Download cancelled".into())),
        r=client.get(&m.url).send()=>r.map_err(|e|AppError::Other(e.to_string()))?.error_for_status().map_err(|e|AppError::Other(e.to_string()))?
    };
    let mut output = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&partial)
        .await?;
    let mut stream = response.bytes_stream();
    let mut bytes = 0;
    loop {
        let chunk = tokio::select! {
            _=state.cancelled()=>return Err(AppError::Other("Download cancelled. You can retry in Settings.".into())),
            r=tokio::time::timeout(std::time::Duration::from_secs(60),stream.next())=>r.map_err(|_|AppError::Other("Download stalled. Check your connection and retry.".into()))?
        };
        let Some(chunk) = chunk else {
            break;
        };
        let chunk = chunk.map_err(|e| AppError::Other(e.to_string()))?;
        bytes += chunk.len() as u64;
        if bytes > m.size {
            return Err(AppError::Other("Unexpected model size".into()));
        }
        output.write_all(&chunk).await?;
        state.progress(Progress {
            label: format!("Downloading {}", m.name),
            model_id: Some(id.into()),
            completed: bytes,
            total: m.size,
            ..Progress::default()
        });
    }
    output.flush().await?;
    output.sync_all().await?;
    drop(output);
    state.progress(Progress::new("Verifying download", Some(id)));
    verify(&partial, m, state).await?;
    tokio::fs::rename(&partial, &target).await?;
    // A freshly hashed file never needs hashing again unless it changes.
    remember_verified(app, id, &target).await
}
pub async fn configure(app: &AppHandle, config: Config) -> AppResult<()> {
    if !["disabled", "builtin", "ollama"].contains(&config.provider.as_str()) {
        return Err(AppError::Invalid("Unknown AI provider".into()));
    }
    if model(&config.model_id)?.purpose != "chat" {
        return Err(AppError::Invalid("Select an answer model".into()));
    }
    if config.provider == "builtin" && !installed(app, model(&config.model_id)?) {
        return Err(AppError::Invalid(
            "Download the selected model first".into(),
        ));
    }
    if config.provider == "ollama" {
        let name = config.ollama_model.trim();
        if name.is_empty() || name.ends_with(":cloud") || name.contains("cloud") {
            return Err(AppError::Invalid(
                "Choose an installed local Ollama chat model".into(),
            ));
        }
        let client = super::runtime::client()?;
        let response = client
            .post("http://127.0.0.1:11434/api/show")
            .json(&serde_json::json!({"model":name}))
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| AppError::Other(e.to_string()))?
            .error_for_status()
            .map_err(|e| AppError::Other(e.to_string()))?;
        let info: serde_json::Value = response
            .json()
            .await
            .map_err(|e| AppError::Other(e.to_string()))?;
        if info.get("remote_host").is_some()
            || !info["capabilities"]
                .as_array()
                .is_some_and(|a| a.iter().any(|v| v == "completion"))
        {
            return Err(AppError::Invalid("This is not a local chat model".into()));
        }
    }
    db(app, move |c| {
        crate::db::set_setting(c, super::CONFIG_KEY, &serde_json::to_string(&config)?)
    })
    .await
}
pub async fn remove(app: &AppHandle, id: &str, confirmed: bool) -> AppResult<()> {
    if !confirmed {
        return Err(AppError::Invalid(
            "Confirm removal of this downloaded model".into(),
        ));
    }
    let target = path(app, id)?;
    if tokio::fs::try_exists(&target).await? {
        tokio::fs::remove_file(target).await?;
    }
    // Remove retained partial downloads only as part of the user's confirmed removal.
    if tokio::fs::try_exists(dir(app)?).await? {
        let mut entries = tokio::fs::read_dir(dir(app)?).await?;
        while let Some(e) = entries.next_entry().await? {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with(&format!("{id}.")) && name.ends_with(".part") {
                tokio::fs::remove_file(e.path()).await?;
            }
        }
    }
    let id = id.to_string();
    db(app, move |c| {
        let mut config = super::read_config(c)?;
        if config.provider == "builtin" && config.model_id == id {
            config.provider = "disabled".into();
            crate::db::set_setting(c, super::CONFIG_KEY, &serde_json::to_string(&config)?)?;
        }
        // A removed model's fingerprint must not vouch for the next download.
        c.execute(
            "DELETE FROM setting WHERE key=?1",
            [fingerprint_key(&id)],
        )?;
        Ok(())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn rejects_corrupt_model_and_cancelled_verification() {
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), b"not a model").unwrap();
        let state = AiState::default();
        assert!(verify(file.path(), model("nomic-embed").unwrap(), &state)
            .await
            .is_err());
        state
            .cancel
            .store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(verify(file.path(), model("nomic-embed").unwrap(), &state)
            .await
            .unwrap_err()
            .to_string()
            .contains("cancelled"));
        assert!(model("../outside").is_err());
    }

    #[test]
    fn a_remembered_model_is_not_hashed_again_unless_the_file_changed() {
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), b"a model").unwrap();
        let (size, mtime) = file_stamp(file.path()).unwrap();
        let stored = serde_json::to_string(&Fingerprint {
            size,
            mtime_ms: mtime,
            sha256: "abc".into(),
        })
        .unwrap();
        assert!(fingerprint_matches(Some(&stored), size, mtime, "abc"));
        // A replaced file, a touched file or a re-pinned hash all fail closed.
        assert!(!fingerprint_matches(Some(&stored), size + 1, mtime, "abc"));
        assert!(!fingerprint_matches(Some(&stored), size, mtime + 1, "abc"));
        assert!(!fingerprint_matches(Some(&stored), size, mtime, "def"));
        assert!(!fingerprint_matches(None, size, mtime, "abc"));
        assert!(!fingerprint_matches(Some("not json"), size, mtime, "abc"));
    }
}
