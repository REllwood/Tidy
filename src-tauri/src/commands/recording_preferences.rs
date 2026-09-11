use crate::{
    error::{AppError, AppResult},
    local_ai::db,
};
use rusqlite::{Connection, OptionalExtension};
use tauri::AppHandle;

const KEEP_AUDIO: &str = "recording.keep_audio";
pub fn retention_enabled(c: &Connection) -> AppResult<bool> {
    Ok(crate::db::get_setting(c, KEEP_AUDIO)?.as_deref() == Some("true"))
}
#[tauri::command]
pub async fn get_recording_preferences(app: AppHandle) -> AppResult<bool> {
    db(&app, |c| retention_enabled(c)).await
}
#[tauri::command]
pub async fn set_recording_preferences(app: AppHandle, keep_audio: bool) -> AppResult<()> {
    db(&app, move |c| {
        crate::db::set_setting(c, KEEP_AUDIO, if keep_audio { "true" } else { "false" })
    })
    .await
}

// Only recordings explicitly created with retention off may be discarded.
// Previously retained recordings are never touched when the setting changes.
pub fn discard(c: &Connection, temporary_dir: &std::path::Path, page: &str) -> AppResult<()> {
    let path: Option<String> = c
        .query_row(
            "SELECT audio_path FROM meeting WHERE page_id=?1 AND retain_audio=0",
            [page],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    let Some(path) = path else {
        return Ok(());
    };
    let path = std::path::Path::new(&path);
    if path.exists() {
        let source = path.canonicalize()?;
        if !source.starts_with(temporary_dir.canonicalize()?)
            || source.extension().and_then(|s| s.to_str()) != Some("wav")
        {
            return Err(AppError::Invalid(
                "Temporary audio is outside its recording folder.".into(),
            ));
        }
        std::fs::remove_file(source)?;
    }
    c.execute("UPDATE meeting SET audio_path=NULL,transcript_state=CASE WHEN transcript_state='saved' THEN 'saved' ELSE 'error' END,transcript_error=CASE WHEN transcript_state!='saved' THEN coalesce(transcript_error,'Transcription did not finish.') || ' Audio was not kept.' ELSE transcript_error END WHERE page_id=?1 AND retain_audio=0",[page])?;
    Ok(())
}
#[tauri::command]
pub async fn finish_recording(app: AppHandle, page_id: String) -> AppResult<()> {
    let dir = crate::audio::recorder::app_recordings_dir(&app)?.join("temporary");
    db(&app, move |c| discard(c, &dir, &page_id)).await
}
pub fn recover(app: &AppHandle, c: &Connection) -> AppResult<()> {
    let dir = crate::audio::recorder::app_recordings_dir(app)?.join("temporary");
    let mut q=c.prepare("SELECT page_id FROM meeting WHERE retain_audio=0 AND audio_path IS NOT NULL AND page_id IS NOT NULL")?;
    let ids = q
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    for id in ids {
        discard(c, &dir, &id)?;
    }
    // Also clear app-created scratch files left before their database registration.
    if dir.is_dir() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if entry.file_type()?.is_file()
                && name.starts_with("tidy-temporary-")
                && (name.ends_with(".wav") || name.ends_with(".partial"))
            {
                std::fs::remove_file(entry.path())?;
            }
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retention_requires_an_explicit_opt_in() {
        let db = crate::db::Db::open(std::path::Path::new(":memory:")).unwrap();
        let c = db.conn.lock().unwrap();
        assert!(!retention_enabled(&c).unwrap());
        crate::db::set_setting(&c, KEEP_AUDIO, "true").unwrap();
        assert!(retention_enabled(&c).unwrap());
        crate::db::set_setting(&c, KEEP_AUDIO, "false").unwrap();
        assert!(!retention_enabled(&c).unwrap());
    }
    #[test]
    fn discards_only_opt_out_audio_and_keeps_the_text() {
        let root =
            std::env::temp_dir().join(format!("tidy-retention-test-{}", crate::db::new_id()));
        let temporary = root.join("temporary");
        std::fs::create_dir_all(&temporary).unwrap();
        let scratch = temporary.join("tidy-temporary-test.wav");
        std::fs::write(&scratch, b"synthetic temporary recording").unwrap();
        let retained = root.join("retained.wav");
        std::fs::write(&retained, b"synthetic retained recording").unwrap();
        let db = crate::db::Db::open(std::path::Path::new(":memory:")).unwrap();
        let c = db.conn.lock().unwrap();
        let page = tidy_core::store::recordings::register_with_retention(
            &c,
            scratch.to_str().unwrap(),
            1000,
            1,
            "",
            false,
        )
        .unwrap();
        let kept =
            tidy_core::store::recordings::register(&c, retained.to_str().unwrap(), 1000, 2, "")
                .unwrap();
        let text = r#"[{"type":"paragraph","content":"Client handover transcript"}]"#;
        tidy_core::store::documents::core::update(&c, &page, text).unwrap();
        discard(&c, &temporary, &page).unwrap();
        discard(&c, &temporary, &kept).unwrap();
        assert!(!scratch.exists());
        assert!(retained.exists());
        assert_eq!(
            tidy_core::store::documents::core::get(&c, &page).unwrap(),
            text
        );
        assert!(tidy_core::store::recordings::source(&c, &page).is_err());
        assert_eq!(
            tidy_core::store::recordings::list(&c)
                .unwrap()
                .into_iter()
                .find(|m| m.page_id.as_deref() == Some(&page))
                .unwrap()
                .transcript_state,
            "error"
        );
    }
    #[test]
    fn changing_preference_does_not_discard_retained_recordings() {
        let db = crate::db::Db::open(std::path::Path::new(":memory:")).unwrap();
        let c = db.conn.lock().unwrap();
        let p = tidy_core::store::recordings::register(&c, "/retained.wav", 1000, 1, "").unwrap();
        discard(&c, std::path::Path::new("/temporary"), &p).unwrap();
        assert_eq!(
            tidy_core::store::recordings::source(&c, &p).unwrap(),
            "/retained.wav"
        );
    }
}
