use crate::{
    error::{AppError, AppResult},
    local_ai::{db, AiState},
};
use serde::Serialize;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use tauri::{AppHandle, Manager};
use tidy_core::store::{documents, recordings};

#[derive(Default)]
pub struct TranscriptionState {
    active: Mutex<Option<(String, Arc<AtomicBool>)>>,
}
struct Release {
    app: AppHandle,
}
impl Drop for Release {
    fn drop(&mut self) {
        if let Ok(mut active) = self.app.state::<TranscriptionState>().active.lock() {
            *active = None;
        }
        self.app
            .state::<AiState>()
            .recording
            .store(false, Ordering::SeqCst);
    }
}
#[derive(Serialize)]
pub struct Entry {
    #[serde(flatten)]
    meeting: recordings::SavedMeeting,
    audio_available: bool,
}
fn checked_audio(app: &AppHandle, path: &str) -> AppResult<std::path::PathBuf> {
    let dir = crate::audio::recorder::app_recordings_dir(app)?.canonicalize()?;
    let audio=std::path::Path::new(path).canonicalize().map_err(|_|AppError::Invalid("Original audio is unavailable. Older Tidy versions removed it after transcription; existing text can still be read and exported.".into()))?;
    if !audio.starts_with(dir) || !audio.is_file() {
        return Err(AppError::Invalid(
            "Recording is outside the saved recording library.".into(),
        ));
    }
    Ok(audio)
}

#[tauri::command]
pub async fn recording_history(app: AppHandle) -> AppResult<Vec<Entry>> {
    let h = app.clone();
    db(&app, move |c| {
        // Recover completed WAVs left by an interrupted transcription or registration.
        let dir = crate::audio::recorder::app_recordings_dir(&h)?;
        let known: std::collections::HashSet<_> = recordings::list(c)?
            .into_iter()
            .filter_map(|m| m.audio_path)
            .filter_map(|path| std::path::Path::new(&path).canonicalize().ok())
            .collect();
        if dir.exists() {
            for file in std::fs::read_dir(&dir)? {
                let path = file?.path();
                if path.extension().and_then(|s| s.to_str()) != Some("wav") {
                    continue;
                }
                let path = match checked_audio(&h, &path.to_string_lossy()) {
                    Ok(path) => path,
                    Err(error) => {
                        log::warn!("A recording could not be inspected: {error}");
                        continue;
                    }
                };
                if !known.contains(&path) {
                    match hound::WavReader::open(&path) {
                        Ok(reader) if reader.duration() > 0 => {
                            let started = std::fs::metadata(&path)?
                                .modified()?
                                .duration_since(std::time::UNIX_EPOCH)
                                .map(|d| d.as_millis() as i64)
                                .unwrap_or(0);
                            recordings::register(
                                c,
                                &path.to_string_lossy(),
                                reader.duration() as i64 * 1000
                                    / reader.spec().sample_rate.max(1) as i64,
                                started,
                                "",
                            )?;
                        }
                        Ok(_) => {}
                        Err(e) => log::warn!("An incomplete recording could not be recovered: {e}"),
                    }
                }
            }
        }
        Ok(recordings::list(c)?
            .into_iter()
            .map(|meeting| {
                let audio_available = meeting.retain_audio
                    && meeting
                        .audio_path
                        .as_deref()
                        .map(|p| checked_audio(&h, p).is_ok())
                        .unwrap_or(false);
                Entry {
                    meeting,
                    audio_available,
                }
            })
            .collect())
    })
    .await
}
#[tauri::command]
pub async fn transcript_versions(
    app: AppHandle,
    page_id: String,
) -> AppResult<Vec<recordings::Version>> {
    db(&app, move |c| recordings::versions(c, &page_id)).await
}
#[tauri::command]
pub fn cancel_transcription(app: AppHandle, page_id: String) -> AppResult<()> {
    let state = app.state::<TranscriptionState>();
    let active = state
        .active
        .lock()
        .map_err(|_| AppError::Other("Transcription state unavailable".into()))?;
    if let Some((id, cancel)) = active.as_ref() {
        if *id == page_id {
            cancel.store(true, Ordering::SeqCst);
        }
    }
    Ok(())
}
#[derive(Serialize)]
pub struct ResultTranscript {
    pub body_json: String,
    pub applied: bool,
    pub segments: Vec<crate::whisper::transcribe::TranscriptSegment>,
}

#[tauri::command]
pub async fn retranscribe_meeting(
    app: AppHandle,
    page_id: String,
    model_id: Option<String>,
    language: Option<String>,
) -> AppResult<ResultTranscript> {
    let lang = language.unwrap_or_else(|| "en".into());
    if !["en", "auto"].contains(&lang.as_str()) {
        return Err(AppError::Invalid(
            "Choose English or automatic language detection.".into(),
        ));
    }
    app.state::<AiState>().reserve_recording()?;
    let cancel = Arc::new(AtomicBool::new(false));
    let release = Release { app: app.clone() };
    *app.state::<TranscriptionState>()
        .active
        .lock()
        .map_err(|_| AppError::Other("Transcription state unavailable".into()))? =
        Some((page_id.clone(), cancel.clone()));
    // Register cancellation before waiting for another AI operation to stop.
    // The release guard also handles the caller disappearing during that wait.
    app.state::<AiState>().wait_until_idle().await;
    let h = app.clone();
    let id = page_id.clone();
    let result=tauri::async_runtime::spawn_blocking(move|| {
  let _release=release; // Keep the processing lock until Whisper really exits, even if its caller disappears.
  let state=h.state::<crate::db::Db>();
  let outcome: AppResult<ResultTranscript> = (|| {
  let (audio,model,model_name,expected)={
   let c=state.conn.lock().map_err(|_|AppError::Other("Database unavailable".into()))?;
   let audio=checked_audio(&h,&recordings::source(&c,&id)?)?;
   let model=match model_id {Some(ref id)=>crate::whisper::models::core::path_for(&h,id)?,None=>crate::whisper::models::core::selected_path(&c,&h)?};
   let name=model.file_stem().and_then(|s|s.to_str()).unwrap_or("Whisper").to_owned();
   let expected=documents::core::get(&c,&id)?;
   c.execute("UPDATE meeting SET transcript_state='transcribing',transcript_error=NULL WHERE page_id=?1",[&id])?;
   (audio,model,name,expected)
  };
  let segments=crate::whisper::transcribe::run(&h,&model,&audio,&lang,cancel.clone())?;
  if cancel.load(Ordering::SeqCst){return Err(AppError::Other("Transcription cancelled; saved audio and text have been kept.".into()));}
  let mut blocks=vec![serde_json::json!({"type":"heading","props":{"level":2},"content":"Transcript"})];
  for s in &segments {let seconds=s.start_ms.max(0)/1000; blocks.push(serde_json::json!({"type":"paragraph","content":format!("[{:02}:{:02}:{:02}] {}",seconds/3600,(seconds/60)%60,seconds%60,s.text)}));}
  let body_json=serde_json::to_string(&blocks)?;
  let c=state.conn.lock().map_err(|_|AppError::Other("Database unavailable".into()))?;
  let applied=recordings::save_result(&c,&id,&expected,&body_json,&model_name,&lang)?;
  if applied { use tauri::Emitter; let _=h.emit("meeting-transcript-updated",serde_json::json!({"page_id":id,"body_json":body_json})); }
  Ok(ResultTranscript{body_json,applied,segments})
  })();
  if let Err(ref error)=outcome {
    let c=state.conn.lock().map_err(|_|AppError::Other("Database unavailable".into()))?;
    c.execute("UPDATE meeting SET transcript_state='error',transcript_error=?2 WHERE page_id=?1",rusqlite::params![id,error.to_string()])?;
  }
  outcome
 }).await.map_err(|e|AppError::Other(format!("Transcription worker: {e}")))?;

    result
}

fn text_from_body(body: &str) -> AppResult<String> {
    fn inline(v: &serde_json::Value) -> String {
        match v {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Array(a) => a.iter().map(inline).collect::<Vec<_>>().join(""),
            _ => v
                .get("text")
                .or_else(|| v.get("content"))
                .map(inline)
                .unwrap_or_default(),
        }
    }
    fn blocks(v: &serde_json::Value, out: &mut Vec<String>) {
        if let Some(a) = v.as_array() {
            for b in a {
                if let Some(c) = b.get("content") {
                    let s = inline(c);
                    if !s.is_empty() {
                        out.push(s);
                    }
                }
                if let Some(c) = b.get("children") {
                    blocks(c, out);
                }
            }
        }
    }
    let v: serde_json::Value = serde_json::from_str(body)?;
    if !v.is_array() {
        return Err(AppError::Invalid(
            "Saved transcript is not a document.".into(),
        ));
    }
    let mut out = Vec::new();
    blocks(&v, &mut out);
    Ok(out.join("\n\n"))
}
#[tauri::command]
pub async fn export_recording_text(
    app: AppHandle,
    page_id: String,
    version_id: Option<String>,
) -> AppResult<()> {
    use tauri_plugin_dialog::DialogExt;
    let body = db(&app, move |c| {
        if let Some(id) = version_id {
            recordings::versions(c, &page_id)?
                .into_iter()
                .find(|v| v.id == id)
                .map(|v| v.body_json)
                .ok_or_else(|| AppError::Invalid("Transcript version not found".into()))
        } else {
            documents::core::get(c, &page_id)
        }
    })
    .await?;
    tauri::async_runtime::spawn_blocking(move || {
        let text = text_from_body(&body)?;
        if let Some(file) = app
            .dialog()
            .file()
            .set_file_name("meeting-transcript.txt")
            .add_filter("Text", &["txt"])
            .blocking_save_file()
        {
            use std::io::Write;
            let path = file
                .into_path()
                .map_err(|e| AppError::Other(e.to_string()))?;
            let mut f = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(path)?;
            f.write_all(text.as_bytes())?;
            f.sync_all()?;
        }
        Ok(())
    })
    .await
    .map_err(|e| AppError::Other(e.to_string()))?
}
#[tauri::command]
pub async fn export_recording_audio(app: AppHandle, page_id: String) -> AppResult<()> {
    use tauri_plugin_dialog::DialogExt;
    let path = db(&app, move |c| recordings::source(c, &page_id)).await?;
    tauri::async_runtime::spawn_blocking(move || {
        let source = checked_audio(&app, &path)?;
        if let Some(file) = app
            .dialog()
            .file()
            .set_file_name("meeting-audio.wav")
            .add_filter("WAV audio", &["wav"])
            .blocking_save_file()
        {
            let path = file
                .into_path()
                .map_err(|e| AppError::Other(e.to_string()))?;
            let mut destination = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(path)?;
            std::io::copy(&mut std::fs::File::open(source)?, &mut destination)?;
            destination.sync_all()?;
        }
        Ok(())
    })
    .await
    .map_err(|e| AppError::Other(e.to_string()))?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn export_keeps_nested_text_and_link_labels() {
        let body = r#"[{"content":"Transcript","children":[{"content":[{"text":"[00:14] "},{"type":"link","content":[{"text":"Client checklist"}]}]}]}]"#;
        assert_eq!(
            text_from_body(body).unwrap(),
            "Transcript\n\n[00:14] Client checklist"
        );
    }
    #[test]
    fn export_rejects_invalid_document() {
        assert!(text_from_body("invalid").is_err());
        assert!(text_from_body(r#"{"unexpected":true}"#).is_err());
    }
}
