use crate::{
    db::Db,
    error::{AppError, AppResult},
    local_ai::{db, AiState},
};
use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, UNIX_EPOCH};
use tauri::{AppHandle, Manager};
use tidy_core::store::{documents, recordings};

/// A recording written in the last half-minute belongs to the Stop that is
/// still registering it, not to the orphan sweep.
const SETTLE: Duration = Duration::from_secs(30);

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
    /// The audio file is still there, wherever in the library it sits, so this
    /// meeting can be transcribed again.
    audio_available: bool,
    /// The audio is temporary: it is kept only until a transcript is saved.
    audio_temporary: bool,
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

/// Register recordings on disk that the database never heard about, because a
/// crash or a failed registration left them behind. Runs at startup and once
/// after a successful Stop — never on a list, which must stay cheap.
pub fn register_orphans(app: &AppHandle) -> AppResult<()> {
    let recordings_dir = crate::audio::recorder::app_recordings_dir(app)?;
    if !recordings_dir.is_dir() {
        return Ok(());
    }
    let root = recordings_dir.canonicalize()?;
    let temporary = recordings_dir.join("temporary");

    // Look at the filesystem first, with no database lock held.
    let mut candidates: Vec<(PathBuf, i64, bool)> = Vec::new();
    for (dir, retain) in [(recordings_dir.clone(), true), (temporary, false)] {
        if !dir.is_dir() {
            continue;
        }
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("wav") {
                continue;
            }
            let modified = entry.metadata()?.modified()?;
            if modified.elapsed().map(|age| age < SETTLE).unwrap_or(true) {
                continue;
            }
            let Ok(path) = path.canonicalize() else {
                continue;
            };
            if !path.starts_with(&root) {
                continue;
            }
            let fallback = modified
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            let started = entry
                .file_name()
                .to_str()
                .and_then(super::recording_preferences::recording_start)
                .unwrap_or(fallback);
            candidates.push((path, started, retain));
        }
    }
    if candidates.is_empty() {
        return Ok(());
    }

    let known: HashSet<PathBuf> = {
        let database = app.state::<Db>();
        let conn = database.lock()?;
        recordings::list(&conn)?
            .into_iter()
            .filter_map(|m| m.audio_path)
            .filter_map(|path| Path::new(&path).canonicalize().ok())
            .collect()
    };

    // Reading WAV headers is filesystem work, so it happens unlocked too.
    let mut fresh = Vec::new();
    for (path, started, retain) in candidates {
        if known.contains(&path) {
            continue;
        }
        match hound::WavReader::open(&path) {
            Ok(reader) if reader.duration() > 0 => {
                let duration =
                    reader.duration() as i64 * 1000 / reader.spec().sample_rate.max(1) as i64;
                fresh.push((path, duration, started, retain));
            }
            Ok(_) => {}
            Err(e) => log::warn!("An incomplete recording could not be recovered: {e}"),
        }
    }
    if fresh.is_empty() {
        return Ok(());
    }
    let database = app.state::<Db>();
    let conn = database.lock()?;
    for (path, duration, started, retain) in fresh {
        recordings::register_with_retention(
            &conn,
            &path.to_string_lossy(),
            duration,
            started,
            "",
            retain,
        )?;
    }
    Ok(())
}

#[tauri::command]
pub async fn recording_history(app: AppHandle) -> AppResult<Vec<Entry>> {
    let meetings = db(&app, |c| recordings::list(c)).await?;
    let recordings_dir = crate::audio::recorder::app_recordings_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let root = recordings_dir.canonicalize().ok();
        let temporary = root.as_ref().map(|r| r.join("temporary"));
        meetings
            .into_iter()
            .map(|meeting| {
                let audio = meeting
                    .audio_path
                    .as_deref()
                    .and_then(|path| Path::new(path).canonicalize().ok())
                    .filter(|path| {
                        path.is_file() && root.as_ref().is_some_and(|r| path.starts_with(r))
                    });
                let audio_temporary = audio.as_ref().is_some_and(|path| {
                    temporary.as_ref().is_some_and(|t| path.starts_with(t))
                });
                Entry {
                    audio_available: audio.is_some(),
                    audio_temporary,
                    meeting,
                }
            })
            .collect()
    })
    .await
    .map_err(|e| AppError::Other(e.to_string()))
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
    pub segments: Vec<crate::whisper::assemble::Labelled>,
}

/// Transcribe a meeting's saved audio. Each side of a call is transcribed on
/// its own; with `speakers` on, the other side is also split into speakers.
/// `speaker_count` caps how many, when the meeting's attendees are known.
#[tauri::command]
pub async fn retranscribe_meeting(
    app: AppHandle,
    page_id: String,
    model_id: Option<String>,
    language: Option<String>,
    speakers: Option<bool>,
    speaker_count: Option<i32>,
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
    let vad_model = crate::whisper::vad::ensure(&app).await;
    let diarize = speakers.unwrap_or(false) && crate::whisper::diarize::models_present(&app);
    let count = speaker_count.unwrap_or(0);
    let h = app.clone();
    let id = page_id.clone();
    let temporary = crate::audio::recorder::app_recordings_dir(&app)?.join("temporary");
    let result=tauri::async_runtime::spawn_blocking(move|| {
  let _release=release; // Keep the processing lock until Whisper really exits, even if its caller disappears.
  let state=h.state::<Db>();
  let outcome: AppResult<ResultTranscript> = (|| {
  let (audio,model,model_name,expected,terms,invited)={
   let c=state.lock()?;
   let audio=checked_audio(&h,&recordings::source(&c,&id)?)?;
   let model=match model_id {Some(ref id)=>crate::whisper::models::core::path_for(&h,id)?,None=>crate::whisper::models::core::selected_path(&c,&h)?};
   let name=model.file_stem().and_then(|s|s.to_str()).unwrap_or("Whisper").to_owned();
   let expected=documents::core::get(&c,&id)?;
   let terms=recordings::vocabulary(&c,&id)?;
   // No more voices than people invited (and the person recording).
   let invited=recordings::event(&c,&id)?.map(|e| e.attendees.len() as i32 + 1).unwrap_or(0);
   c.execute("UPDATE meeting SET transcript_state='transcribing',transcript_error=NULL WHERE page_id=?1",[&id])?;
   (audio,model,name,expected,terms,invited)
  };
  let options=crate::whisper::transcribe::Options{language:lang.clone(),vocabulary:crate::whisper::transcribe::vocabulary_prompt(&terms),vad_model};
  let speaker_app=h.clone();
  let cap=if count>0 {count} else {invited};
  let diarizer=move |pcm:&[f32]| {
    let turns=crate::whisper::diarize::turns(&speaker_app,pcm,if count>0 {count} else {-1})?;
    let found:std::collections::HashSet<i32>=turns.iter().map(|t|t.speaker).collect();
    // Fewer people were invited than voices found: split again into that many.
    if count<=0 && cap>0 && found.len() as i32>cap { crate::whisper::diarize::turns(&speaker_app,pcm,cap) } else { Ok(turns) }
  };
  let progress_app=h.clone();
  let segments=crate::whisper::transcribe::transcribe_recording(&model,&audio,&options,diarize.then_some(&diarizer as crate::whisper::transcribe::Diarizer<'_>),cancel.clone(),move |p| { use tauri::Emitter; let _=progress_app.emit("transcribe-progress",p); })?;
  if cancel.load(Ordering::SeqCst){return Err(AppError::Other("Transcription cancelled; saved audio and text have been kept.".into()));}
  let blocks=crate::whisper::assemble::transcript_blocks(&segments);
  let body_json=serde_json::to_string(&blocks)?;
  let c=state.lock()?;
  let applied=recordings::save_result(&c,&id,&expected,&body_json,&model_name,&lang)?;
  if applied { use tauri::Emitter; let _=h.emit("meeting-transcript-updated",serde_json::json!({"page_id":id,"body_json":body_json})); }
  // Keep-audio-off meetings hand their temporary file back now that a
  // transcript exists. A failure above keeps the audio for another attempt.
  if let Err(error)=super::recording_preferences::discard(&c,&temporary,&id) { log::warn!("Temporary audio could not be discarded: {error}"); }
  Ok(ResultTranscript{body_json,applied,segments})
  })();
  if let Err(ref error)=outcome {
    let c=state.lock()?;
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
                .create(true)
                .truncate(true)
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
                .create(true)
                .truncate(true)
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
