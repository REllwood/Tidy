use crate::{
    db::Db,
    error::{AppError, AppResult},
    local_ai::db,
};
use rusqlite::{Connection, OptionalExtension};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

const KEEP_AUDIO: &str = "recording.keep_audio";
const INTERRUPTED: &str =
    "Transcription was interrupted. Transcribe it again from Recording history.";

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

/// Delete a recording's audio, refusing anything that is not a WAV inside the
/// recording library.
fn remove_within(path: &str, root: &Path) -> AppResult<()> {
    let path = Path::new(path);
    if !path.exists() {
        return Ok(());
    }
    let source = path.canonicalize()?;
    if !source.starts_with(root.canonicalize()?)
        || source.extension().and_then(|s| s.to_str()) != Some("wav")
    {
        return Err(AppError::Invalid(
            "Recording audio is outside its recording folder.".into(),
        ));
    }
    std::fs::remove_file(source)?;
    Ok(())
}

// Only recordings explicitly created with retention off may be discarded, and
// only once their transcript is saved. A meeting that failed to transcribe
// keeps its audio so it can be transcribed again from Recording history.
// Previously retained recordings are never touched when the setting changes.
pub fn discard(c: &Connection, temporary_dir: &Path, page: &str) -> AppResult<()> {
    let row: Option<(Option<String>, String)> = c
        .query_row(
            "SELECT audio_path,transcript_state FROM meeting WHERE page_id=?1 AND retain_audio=0",
            [page],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((path, state)) = row else {
        return Ok(());
    };
    if state != "saved" {
        return Ok(());
    }
    let Some(path) = path else {
        return Ok(());
    };
    remove_within(&path, temporary_dir)?;
    c.execute(
        "UPDATE meeting SET audio_path=NULL WHERE page_id=?1 AND retain_audio=0",
        [page],
    )?;
    Ok(())
}

/// Remove a meeting's audio on request, wherever in the library it sits. The
/// saved transcript is left exactly as it is.
pub fn discard_audio(c: &Connection, recordings_dir: &Path, page: &str) -> AppResult<()> {
    let path: Option<String> = c
        .query_row(
            "SELECT audio_path FROM meeting WHERE page_id=?1 ORDER BY started_at DESC LIMIT 1",
            [page],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    if let Some(path) = path {
        remove_within(&path, recordings_dir)?;
    }
    c.execute("UPDATE meeting SET audio_path=NULL WHERE page_id=?1", [page])?;
    Ok(())
}

#[tauri::command]
pub async fn finish_recording(app: AppHandle, page_id: String) -> AppResult<()> {
    let dir = crate::audio::recorder::app_recordings_dir(&app)?.join("temporary");
    db(&app, move |c| discard(c, &dir, &page_id)).await
}

#[tauri::command]
pub async fn discard_recording_audio(app: AppHandle, page_id: String) -> AppResult<()> {
    let dir = crate::audio::recorder::app_recordings_dir(&app)?;
    db(&app, move |c| discard_audio(c, &dir, &page_id)).await
}

/// The start time encoded in a recording's file name (`{started_at}-{uuid}`).
/// Anything else in the folder is left alone.
pub fn recording_start(name: &str) -> Option<i64> {
    let stem = name.rsplit_once('.').map(|(s, _)| s).unwrap_or(name);
    let (started, id) = stem.split_once('-')?;
    let started: i64 = started.parse().ok()?;
    let looks_like_id =
        id.len() == 36 && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-');
    looks_like_id.then_some(started)
}

/// Finish off `.partial` files an interrupted recording left behind: repair the
/// header, rename the ones that hold audio, and delete the ones that do not.
/// Returns the recordings that are now complete.
pub fn finalise_partials(dir: &Path) -> AppResult<Vec<PathBuf>> {
    let mut finished = Vec::new();
    if !dir.is_dir() {
        return Ok(finished);
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("partial") {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if recording_start(&name).is_none() {
            continue;
        }
        match crate::audio::recorder::recover_partial(&path) {
            Ok(0) => {
                if let Err(error) = std::fs::remove_file(&path) {
                    log::warn!("An empty interrupted recording could not be removed: {error}");
                }
            }
            Ok(_) => {
                let complete = path.with_extension("wav");
                if complete.exists() {
                    let _ = std::fs::remove_file(&path);
                    continue;
                }
                match std::fs::rename(&path, &complete) {
                    Ok(()) => finished.push(complete),
                    Err(error) => {
                        log::warn!("An interrupted recording could not be completed: {error}")
                    }
                }
            }
            Err(error) => {
                log::warn!("An interrupted recording could not be read: {error}");
                let _ = std::fs::remove_file(&path);
            }
        }
    }
    Ok(finished)
}

/// Startup recovery for the recording library. Runs once, before the interface
/// asks for history, so nothing is left half-finished by a crash or a quit.
pub fn recover(app: &AppHandle) -> AppResult<()> {
    let recordings = crate::audio::recorder::app_recordings_dir(app)?;
    let temporary = recordings.join("temporary");

    // A transcription that was running when the app stopped never finishes.
    // Say so plainly, and leave any other error message alone.
    {
        let database = app.state::<Db>();
        let conn = database.lock()?;
        conn.execute(
            "UPDATE meeting SET transcript_state='error',transcript_error=?1 WHERE transcript_state='transcribing'",
            [INTERRUPTED],
        )?;
    }

    for dir in [&recordings, &temporary] {
        if let Err(error) = finalise_partials(dir) {
            log::warn!("Interrupted recordings could not be completed: {error}");
        }
    }

    // Temporary audio only goes once its transcript is safely saved.
    let pending: Vec<String> = {
        let database = app.state::<Db>();
        let conn = database.lock()?;
        let mut q = conn.prepare(
            "SELECT page_id FROM meeting WHERE retain_audio=0 AND audio_path IS NOT NULL AND page_id IS NOT NULL",
        )?;
        let rows = q.query_map([], |r| r.get::<_, String>(0))?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    for id in pending {
        let database = app.state::<Db>();
        let conn = database.lock()?;
        if let Err(error) = discard(&conn, &temporary, &id) {
            log::warn!("Temporary audio could not be discarded: {error}");
        }
    }

    // Recordings the database never heard about, including the ones just
    // completed above, become entries in Recording history.
    super::recording_history::register_orphans(app)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tidy_core::store::recordings;

    fn workspace() -> (PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!("tidy-retention-test-{}", crate::db::new_id()));
        let temporary = root.join("temporary");
        std::fs::create_dir_all(&temporary).unwrap();
        (root, temporary)
    }

    fn wav(path: &Path, seconds: usize) {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(path, spec).unwrap();
        for i in 0..16_000 * seconds {
            writer
                .write_sample(((i as f32 / 40.0).sin() * 8000.0) as i16)
                .unwrap();
        }
        writer.finalize().unwrap();
    }

    #[test]
    fn retention_requires_an_explicit_opt_in() {
        let db = crate::db::Db::open(std::path::Path::new(":memory:")).unwrap();
        let c = db.lock().unwrap();
        assert!(!retention_enabled(&c).unwrap());
        crate::db::set_setting(&c, KEEP_AUDIO, "true").unwrap();
        assert!(retention_enabled(&c).unwrap());
        crate::db::set_setting(&c, KEEP_AUDIO, "false").unwrap();
        assert!(!retention_enabled(&c).unwrap());
    }

    #[test]
    fn temporary_audio_goes_only_once_the_transcript_is_saved() {
        let (root, temporary) = workspace();
        let scratch = temporary.join("1700000000-aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee.wav");
        wav(&scratch, 1);
        let retained = root.join("1700000001-aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeef.wav");
        wav(&retained, 1);
        let db = crate::db::Db::open(std::path::Path::new(":memory:")).unwrap();
        let c = db.lock().unwrap();
        let page = recordings::register_with_retention(
            &c,
            scratch.to_str().unwrap(),
            1000,
            1,
            "",
            false,
        )
        .unwrap();
        let kept = recordings::register(&c, retained.to_str().unwrap(), 1000, 2, "").unwrap();
        let text = r#"[{"type":"paragraph","content":"Client handover transcript"}]"#;
        tidy_core::store::documents::core::update(&c, &page, text).unwrap();
        c.execute(
            "UPDATE meeting SET transcript_state='saved' WHERE page_id=?1",
            [&page],
        )
        .unwrap();

        discard(&c, &temporary, &page).unwrap();
        discard(&c, &temporary, &kept).unwrap();
        assert!(!scratch.exists());
        assert!(retained.exists(), "retained audio is never discarded");
        assert_eq!(
            tidy_core::store::documents::core::get(&c, &page).unwrap(),
            text
        );
        assert!(recordings::source(&c, &page).is_err());
        assert_eq!(
            recordings::list(&c)
                .unwrap()
                .into_iter()
                .find(|m| m.page_id.as_deref() == Some(&page))
                .unwrap()
                .transcript_state,
            "saved",
            "discarding audio must not rewrite the transcript state"
        );
    }

    #[test]
    fn a_failed_meeting_keeps_its_audio_so_it_can_be_transcribed_again() {
        let (_root, temporary) = workspace();
        let scratch = temporary.join("1700000002-aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeee01.wav");
        wav(&scratch, 1);
        let db = crate::db::Db::open(std::path::Path::new(":memory:")).unwrap();
        let c = db.lock().unwrap();
        let page = recordings::register_with_retention(
            &c,
            scratch.to_str().unwrap(),
            1000,
            1,
            "",
            false,
        )
        .unwrap();
        c.execute("UPDATE meeting SET transcript_state='error',transcript_error='No speech was recognised.' WHERE page_id=?1",[&page]).unwrap();

        discard(&c, &temporary, &page).unwrap();
        assert!(scratch.exists(), "a failed meeting keeps its audio");
        assert_eq!(recordings::source(&c, &page).unwrap(), scratch.to_str().unwrap());

        // A pending recording is not discarded either.
        c.execute(
            "UPDATE meeting SET transcript_state='pending',transcript_error=NULL WHERE page_id=?1",
            [&page],
        )
        .unwrap();
        discard(&c, &temporary, &page).unwrap();
        assert!(scratch.exists());
    }

    #[test]
    fn discarding_audio_on_request_keeps_the_transcript() {
        let (root, _temporary) = workspace();
        let audio = root.join("1700000003-aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeee02.wav");
        wav(&audio, 1);
        let db = crate::db::Db::open(std::path::Path::new(":memory:")).unwrap();
        let c = db.lock().unwrap();
        let page = recordings::register(&c, audio.to_str().unwrap(), 1000, 1, "").unwrap();
        let text = r#"[{"type":"paragraph","content":"Kept transcript"}]"#;
        tidy_core::store::documents::core::update(&c, &page, text).unwrap();

        discard_audio(&c, &root, &page).unwrap();
        assert!(!audio.exists());
        assert!(recordings::source(&c, &page).is_err());
        assert_eq!(
            tidy_core::store::documents::core::get(&c, &page).unwrap(),
            text
        );
    }

    #[test]
    fn changing_preference_does_not_discard_retained_recordings() {
        let db = crate::db::Db::open(std::path::Path::new(":memory:")).unwrap();
        let c = db.lock().unwrap();
        let p = recordings::register(&c, "/retained.wav", 1000, 1, "").unwrap();
        discard(&c, std::path::Path::new("/temporary"), &p).unwrap();
        assert_eq!(recordings::source(&c, &p).unwrap(), "/retained.wav");
    }

    #[test]
    fn recording_file_names_are_matched_by_their_start_and_id() {
        assert_eq!(
            recording_start("1700000000-aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee.wav"),
            Some(1_700_000_000)
        );
        assert_eq!(
            recording_start("1700000000-aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee.partial"),
            Some(1_700_000_000)
        );
        // The names the old recovery looked for never existed on disk.
        assert_eq!(recording_start("tidy-temporary-test.wav"), None);
        assert_eq!(recording_start("notes.wav"), None);
        assert_eq!(recording_start("1700000000-short.wav"), None);
    }

    #[test]
    fn interrupted_recordings_are_completed_and_empty_ones_removed() {
        let (root, _temporary) = workspace();
        let crashed = root.join("1700000004-aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeee03.partial");
        wav(&crashed, 1);
        // The header a crash leaves behind claims there is no audio at all.
        {
            use std::io::{Seek, SeekFrom, Write};
            let mut file = std::fs::OpenOptions::new().write(true).open(&crashed).unwrap();
            file.seek(SeekFrom::Start(40)).unwrap();
            file.write_all(&0u32.to_le_bytes()).unwrap();
            file.sync_all().unwrap();
        }
        let empty = root.join("1700000005-aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeee04.partial");
        std::fs::write(&empty, b"not a recording").unwrap();
        let unrelated = root.join("notes.partial");
        std::fs::write(&unrelated, b"someone else's file").unwrap();

        let finished = finalise_partials(&root).unwrap();
        let complete = root.join("1700000004-aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeee03.wav");
        assert_eq!(finished, vec![complete.clone()]);
        assert!(!crashed.exists());
        assert!(!empty.exists());
        assert!(unrelated.exists(), "only Tidy's own files are touched");
        assert_eq!(
            hound::WavReader::open(&complete).unwrap().duration(),
            16_000
        );
    }
}
