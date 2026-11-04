//! Whisper GGML model management: list / download (with progress) / select /
//! delete. Models come from Hugging Face `ggerganov/whisper.cpp` as `ggml-*.bin`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

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

/// Cancellation flags for downloads that are running right now, keyed by model.
fn in_flight() -> &'static Mutex<HashMap<String, Arc<AtomicBool>>> {
    static IN_FLIGHT: OnceLock<Mutex<HashMap<String, Arc<AtomicBool>>>> = OnceLock::new();
    IN_FLIGHT.get_or_init(|| Mutex::new(HashMap::new()))
}
fn downloads() -> std::sync::MutexGuard<'static, HashMap<String, Arc<AtomicBool>>> {
    in_flight()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Removes its file unless the download finished and asked to keep it, so no
/// error path can leave a half-written model behind.
struct PartFile(Option<PathBuf>);
impl PartFile {
    fn new(path: PathBuf) -> Self {
        Self(Some(path))
    }
    fn path(&self) -> &Path {
        self.0.as_deref().unwrap_or(Path::new(""))
    }
    fn keep(mut self) -> PathBuf {
        self.0.take().unwrap_or_default()
    }
}
impl Drop for PartFile {
    fn drop(&mut self) {
        if let Some(path) = self.0.take() {
            if let Err(error) = std::fs::remove_file(&path) {
                if error.kind() != std::io::ErrorKind::NotFound {
                    log::warn!("An interrupted model download could not be removed: {error}");
                }
            }
        }
    }
}

/// Delete `.part` files an interrupted download left behind. Called at startup,
/// where nothing can be downloading yet.
pub fn sweep_partials(app: &AppHandle) -> AppResult<()> {
    let dir = models_dir(app)?;
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        if entry.path().extension().and_then(|s| s.to_str()) == Some("part") {
            if let Err(error) = std::fs::remove_file(entry.path()) {
                log::warn!("A leftover model download could not be removed: {error}");
            }
        }
    }
    Ok(())
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
        let was_selected = selected_id(conn)?.as_deref() == Some(id);
        let path = models_dir(app)?.join(&d.file);
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        conn.execute("DELETE FROM model WHERE id = ?1", params![id])?;
        // Removing the model in use must not silently disable transcription.
        if was_selected {
            let dir = models_dir(app)?;
            let installed: Vec<&ModelDef> = catalogue()?
                .transcription
                .iter()
                .filter(|m| {
                    m.id != id
                        && dir
                            .join(&m.file)
                            .metadata()
                            .is_ok_and(|meta| meta.len() == m.size)
                })
                .collect();
            // Prefer a model of the same tier so quality and speed stay similar.
            if let Some(next) = installed
                .iter()
                .find(|m| m.tier == d.tier)
                .or_else(|| installed.first())
            {
                select(conn, &next.id)?;
            }
        }
        Ok(())
    }

    /// The model the user chose, whether or not its file is present.
    pub fn selected_id(conn: &Connection) -> AppResult<Option<String>> {
        Ok(conn
            .query_row("SELECT id FROM model WHERE is_selected = 1 LIMIT 1", [], |r| {
                r.get(0)
            })
            .optional()?)
    }

    /// Whether any transcription model has been downloaded at all.
    pub fn any_downloaded(app: &AppHandle) -> AppResult<bool> {
        let dir = models_dir(app)?;
        Ok(catalogue()?.transcription.iter().any(|m| {
            dir.join(&m.file)
                .metadata()
                .is_ok_and(|meta| meta.len() == m.size)
        }))
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
        let id = selected_id(conn)?
            .ok_or_else(|| AppError::Invalid("no Whisper model selected".into()))?;
        let path = models_dir(app)?.join(&def(&id)?.file);
        if !path.exists() {
            return Err(AppError::Invalid(format!("model '{id}' not downloaded")));
        }
        Ok(path)
    }
}

#[tauri::command]
pub fn list_models(app: AppHandle, db: State<'_, Db>) -> AppResult<Vec<ModelInfo>> {
    let conn = db.lock()?;
    core::list(&conn, &app)
}

#[tauri::command]
pub fn select_model(db: State<'_, Db>, id: String) -> AppResult<()> {
    let conn = db.lock()?;
    core::select(&conn, &id)
}

#[tauri::command]
pub fn delete_model(app: AppHandle, db: State<'_, Db>, id: String) -> AppResult<()> {
    let conn = db.lock()?;
    core::delete(&conn, &app, &id)
}

/// Stop a download that is running. The partial file is removed by the
/// download itself as it unwinds.
#[tauri::command]
pub fn cancel_model_download(id: String) -> AppResult<()> {
    def(&id)?;
    if let Some(cancel) = downloads().get(&id) {
        cancel.store(true, Ordering::SeqCst);
    }
    Ok(())
}

/// Removes this download's cancellation flag when it finishes, however it ends.
struct Registration(String);
impl Drop for Registration {
    fn drop(&mut self) {
        downloads().remove(&self.0);
    }
}

#[tauri::command]
pub async fn download_model(app: AppHandle, db: State<'_, Db>, id: String) -> AppResult<()> {
    let d = def(&id)?;
    let dir = models_dir(&app)?;
    tokio::fs::create_dir_all(&dir).await?;
    let path = dir.join(&d.file);
    let url = &d.url;

    let cancel = Arc::new(AtomicBool::new(false));
    if downloads().insert(id.clone(), cancel.clone()).is_some() {
        return Err(AppError::Invalid(
            "This model is already downloading.".into(),
        ));
    }
    let _registration = Registration(id.clone());

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

    // The guard removes this file on every path out of here but the last one.
    let tmp = PartFile::new(path.with_extension(format!("{}.part", crate::db::new_id())));
    let mut hash = Sha256::new();
    let mut file = tokio::fs::File::create(tmp.path()).await?;
    let mut downloaded: u64 = 0;
    let mut last_emit: u64 = 0;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = tokio::time::timeout(std::time::Duration::from_secs(60), stream.next())
        .await
        .map_err(|_| {
            AppError::Other("Model download stalled. Check your connection and retry.".into())
        })?
    {
        if cancel.load(Ordering::SeqCst) {
            return Err(AppError::Other("Download cancelled.".into()));
        }
        let chunk = chunk.map_err(|e| AppError::Other(format!("download stream: {e}")))?;
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
    if cancel.load(Ordering::SeqCst) {
        return Err(AppError::Other("Download cancelled.".into()));
    }
    file.flush().await?;
    file.sync_all().await?;
    drop(file);
    if downloaded != d.size || format!("{:x}", hash.finalize()) != d.sha256 {
        return Err(AppError::Other(
            "Model verification failed. Download it again before use.".into(),
        ));
    }
    tokio::fs::rename(tmp.keep(), &path).await?;
    let _ = app.emit(
        "model-download-progress",
        serde_json::json!({ "id": id, "downloaded": total, "total": total }),
    );

    // record in DB
    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(d.size);
    {
        let conn = db.lock()?;
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
    // Fetch Whisper's small voice activity model now, while online, so the
    // first transcription does not have to.
    let _ = crate::whisper::vad::ensure(&app).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tidy-whisper-{}", crate::db::new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn an_interrupted_download_removes_its_own_file() {
        let dir = scratch();
        let partial = dir.join("ggml-base.abc.part");
        std::fs::write(&partial, b"half a model").unwrap();
        {
            let _guard = PartFile::new(partial.clone());
        }
        assert!(!partial.exists(), "the guard cleans up on every error path");

        let kept = dir.join("ggml-base.def.part");
        std::fs::write(&kept, b"a whole model").unwrap();
        let guard = PartFile::new(kept.clone());
        assert_eq!(guard.keep(), kept);
        assert!(kept.exists(), "a verified download is handed on, not deleted");
    }

    #[test]
    fn cancelling_an_unknown_or_idle_download_is_harmless() {
        assert!(cancel_model_download("tiny".into()).is_ok());
        assert!(cancel_model_download("not-a-model".into()).is_err());
    }

    #[test]
    fn a_registered_download_observes_its_cancel_flag() {
        let cancel = Arc::new(AtomicBool::new(false));
        downloads().insert("tiny".into(), cancel.clone());
        let registration = Registration("tiny".into());
        cancel_model_download("tiny".into()).unwrap();
        assert!(cancel.load(Ordering::SeqCst));
        drop(registration);
        assert!(downloads().get("tiny").is_none());
    }
}
