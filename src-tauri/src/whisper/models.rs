//! Whisper GGML model management: list / download (with progress) / select /
//! delete. Models come from Hugging Face `ggerganov/whisper.cpp` as `ggml-*.bin`.

use std::path::PathBuf;

use futures_util::StreamExt;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, State};
use tokio::io::AsyncWriteExt;

use crate::db::{now_ms, Db};
use crate::error::{AppError, AppResult};

use crate::model_catalogue::{catalogue, ModelDefinition as ModelDef};

#[derive(Serialize)]
pub struct ModelInfo {
    pub description: String,
    pub recommended_ram_gb: u64,
    pub tier: String,
    pub id: String,
    pub name: String,
    pub size: u64,
    pub downloaded: bool,
    pub selected: bool,
}

fn def(id: &str) -> AppResult<&'static ModelDef> {
    catalogue()?
        .transcription
        .iter()
        .find(|m| m.id == id)
        .ok_or_else(|| AppError::Invalid(format!("unknown model '{id}'")))
}

fn models_dir(app: &AppHandle) -> AppResult<PathBuf> {
    let dir = crate::workspace::directory(app).join("models");
    Ok(dir)
}

pub mod core {
    use super::*;

    pub fn list(conn: &Connection, app: &AppHandle) -> AppResult<Vec<ModelInfo>> {
        let dir = models_dir(app)?;
        let mut out = Vec::new();
        for m in &catalogue()?.transcription {
            let path = dir.join(&m.file);
            let selected: bool = conn
                .query_row(
                    "SELECT is_selected FROM model WHERE id = ?1",
                    params![m.id],
                    |r| r.get::<_, i64>(0),
                )
                .optional()?
                .map(|v| v != 0)
                .unwrap_or(false);
            out.push(ModelInfo {
                description: m.description.clone(),
                recommended_ram_gb: m.recommended_ram_gb,
                tier: m.tier.clone(),
                id: m.id.to_string(),
                name: m.name.to_string(),
                size: m.size,
                downloaded: path.metadata().is_ok_and(|meta| meta.len() == m.size),
                selected,
            });
        }
        Ok(out)
    }

    pub fn select(conn: &Connection, id: &str) -> AppResult<()> {
        def(id)?;
        conn.execute("UPDATE model SET is_selected = 0", [])?;
        conn.execute(
            "INSERT INTO model (id, name, size, is_selected) VALUES (?1, ?2, 0, 1)
             ON CONFLICT(id) DO UPDATE SET is_selected = 1",
            params![id, def(id)?.name],
        )?;
        Ok(())
    }

    pub fn delete(conn: &Connection, app: &AppHandle, id: &str) -> AppResult<()> {
        let d = def(id)?;
        let path = models_dir(app)?.join(&d.file);
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        conn.execute("DELETE FROM model WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn path_for(app: &AppHandle, id: &str) -> AppResult<PathBuf> {
        let path = models_dir(app)?.join(&def(id)?.file);
        if !path.is_file() {
            return Err(AppError::Invalid(
                "Download this Whisper model in Settings first.".into(),
            ));
        }
        Ok(path)
    }

    /// Filesystem path of the currently-selected, downloaded model.
    pub fn selected_path(conn: &Connection, app: &AppHandle) -> AppResult<PathBuf> {
        let id: Option<String> = conn
            .query_row(
                "SELECT id FROM model WHERE is_selected = 1 LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()?;
        let id = id.ok_or_else(|| AppError::Invalid("no Whisper model selected".into()))?;
        let path = models_dir(app)?.join(&def(&id)?.file);
        if !path.exists() {
            return Err(AppError::Invalid(format!("model '{id}' not downloaded")));
        }
        Ok(path)
    }
}

#[tauri::command]
pub fn list_models(app: AppHandle, db: State<'_, Db>) -> AppResult<Vec<ModelInfo>> {
    let conn = db.conn.lock().unwrap();
    core::list(&conn, &app)
}

#[tauri::command]
pub fn select_model(db: State<'_, Db>, id: String) -> AppResult<()> {
    let conn = db.conn.lock().unwrap();
    core::select(&conn, &id)
}

#[tauri::command]
pub fn delete_model(app: AppHandle, db: State<'_, Db>, id: String) -> AppResult<()> {
    let conn = db.conn.lock().unwrap();
    core::delete(&conn, &app, &id)
}

#[tauri::command]
pub async fn download_model(app: AppHandle, db: State<'_, Db>, id: String) -> AppResult<()> {
    let d = def(&id)?;
    let dir = models_dir(&app)?;
    tokio::fs::create_dir_all(&dir).await?;
    let path = dir.join(&d.file);
    let url = &d.url;

    let client = reqwest::Client::builder()
        .user_agent("Tidy")
        .connect_timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| AppError::Other(format!("http client: {e}")))?;
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| AppError::Other(format!("download request: {e}")))?
        .error_for_status()
        .map_err(|e| AppError::Other(format!("download status: {e}")))?;
    let total = resp.content_length().unwrap_or(d.size);

    let tmp = path.with_extension(format!("{}.part", crate::db::new_id()));
    let mut hash = Sha256::new();
    let mut file = tokio::fs::File::create(&tmp).await?;
    let mut downloaded: u64 = 0;
    let mut last_emit: u64 = 0;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = tokio::time::timeout(std::time::Duration::from_secs(60), stream.next())
        .await
        .map_err(|_| {
            AppError::Other("Model download stalled. Check your connection and retry.".into())
        })?
    {
        let chunk = match chunk {
            Ok(c) => c,
            Err(e) => {
                // Keep partial data out of the selectable model path.
                drop(file);
                return Err(AppError::Other(format!("download stream: {e}")));
            }
        };
        downloaded += chunk.len() as u64;
        if downloaded > d.size {
            return Err(AppError::Other(
                "Unexpected transcription model size.".into(),
            ));
        }
        hash.update(&chunk);
        file.write_all(&chunk).await?;
        if downloaded - last_emit > 1_000_000 {
            last_emit = downloaded;
            let _ = app.emit(
                "model-download-progress",
                serde_json::json!({ "id": id, "downloaded": downloaded, "total": total }),
            );
        }
    }
    file.flush().await?;
    file.sync_all().await?;
    drop(file);
    if downloaded != d.size || format!("{:x}", hash.finalize()) != d.sha256 {
        return Err(AppError::Other(
            "Model verification failed. Download it again before use.".into(),
        ));
    }
    tokio::fs::rename(&tmp, &path).await?;
    let _ = app.emit(
        "model-download-progress",
        serde_json::json!({ "id": id, "downloaded": total, "total": total }),
    );

    // record in DB
    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(d.size);
    {
        let conn = db.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO model (id, name, size, path, is_selected, downloaded_at)
             VALUES (?1, ?2, ?3, ?4, 0, ?5)
             ON CONFLICT(id) DO UPDATE SET size = excluded.size, path = excluded.path, downloaded_at = excluded.downloaded_at",
            params![id, d.name, size as i64, path.to_string_lossy(), now_ms()],
        )?;
        // auto-select if nothing selected yet
        let any: i64 = conn.query_row(
            "SELECT count(*) FROM model WHERE is_selected = 1",
            [],
            |r| r.get(0),
        )?;
        if any == 0 {
            core::select(&conn, &id)?;
        }
    }
    Ok(())
}
