use super::{db, AiState, Config, Progress};
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
        verify(&target, m, state).await?;
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
        });
    }
    output.flush().await?;
    output.sync_all().await?;
    drop(output);
    state.progress(Progress::new("Verifying download", Some(id)));
    verify(&partial, m, state).await?;
    tokio::fs::rename(&partial, &target).await?;
    Ok(())
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
}
