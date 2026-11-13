mod model_catalogue;
mod audio;
mod calendar;
mod call_window;
mod commands;
mod lifecycle;
mod local_ai;
mod meeting_monitor;
mod vault;
mod whisper;
mod workspace;

// The DB layer, error type, LLM client, and store now live in the shared core
// crate. Re-export db/error at the app crate root so the app's audio/whisper
// modules keep referring to `crate::db` / `crate::error` unchanged.
pub use tidy_core::{db, error};

use tauri::{Emitter, Manager};

use commands::recording::RecorderState;
use db::Db;
use lifecycle::CloseState;
use vault::VaultState;

/// How often to look for sidecar writes. Slow enough to be free, quick enough
/// that an agent's edit shows up while the user is still looking at the window.
const MCP_POLL: std::time::Duration = std::time::Duration::from_secs(2);

/// Poll the change counter the MCP sidecar bumps after every write and turn it
/// into the same refresh events the app's own commands emit. The sidecar is a
/// separate process, so this is the only way an agent's write reaches the UI.
fn spawn_mcp_change_poller(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        // Start from wherever the counter already is: writes made while Tidy
        // was closed are already in the data the UI just loaded.
        let mut seen = read_change_seq(&app).unwrap_or(0);
        loop {
            tokio::time::sleep(MCP_POLL).await;
            // The lock is held for the read alone, never across an await or an
            // emit, so polling cannot stall a write.
            let changed = {
                let db = app.state::<Db>();
                let drained = match db.lock() {
                    Ok(conn) => match tidy_core::db::change_seq(&conn) {
                        Ok(seq) if seq != seen => {
                            seen = seq;
                            Some(tidy_core::db::take_changed_pages(&conn).unwrap_or_default())
                        }
                        _ => None,
                    },
                    Err(_) => None,
                };
                drained
            };
            let Some(pages) = changed else { continue };
            let _ = app.emit("pages-changed", ());
            let _ = app.emit("database-changed", ());
            let _ = app.emit("links-changed", ());
            for page_id in pages {
                let _ = app.emit("document-changed", serde_json::json!({ "page_id": page_id }));
            }
        }
    });
}

fn read_change_seq(app: &tauri::AppHandle) -> Option<i64> {
    let db = app.state::<Db>();
    let conn = db.lock().ok()?;
    tidy_core::db::change_seq(&conn).ok()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        // Meeting playback: recordings served as mono, a range at a time.
        .register_asynchronous_uri_scheme_protocol(audio::playback::SCHEME, |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            tauri::async_runtime::spawn_blocking(move || {
                responder.respond(audio::playback::serve(&app, &request));
            });
        })
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    // Only the mark shortcut is registered, and only while recording.
                    if event.state() == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                        commands::recording::mark_now(app);
                    }
                })
                .build(),
        )
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }

            // Open the local database in the app data dir.
            let dir = tidy_core::installation::data_directory(&app.path().app_data_dir()?)?;
            app.manage(workspace::WorkspaceDirectory(dir.clone()));
            std::fs::create_dir_all(&dir)?;
            let db_path = tidy_core::installation::database_path(&dir)?;
            log::info!("Tidy database: {}", db_path.display());
            let database = Db::open(&db_path).map_err(|e| {
                log::error!("failed to open database: {e}");
                std::io::Error::other(e.to_string())
            })?;
            app.manage(database);
            app.manage(RecorderState::default());
            app.manage(meeting_monitor::MonitorState::default());
            app.manage(commands::updates::UpdateState::default());
            app.manage(commands::recording_history::TranscriptionState::default());
            app.manage(VaultState::default());
            app.manage(CloseState::default());
            app.manage(local_ai::AiState::default());
            local_ai::start_worker(app.handle().clone());
            meeting_monitor::start(app.handle().clone());

            // One-time: backfill links/tags from existing page bodies so an
            // upgraded DB's graph/backlinks aren't empty on first v3 launch.
            // It parses every page body, so it runs off the setup thread —
            // otherwise a large workspace would hold the window closed.
            {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    let backfill = workspace::run_db(&handle, |conn| {
                        tidy_core::store::knowledge::core::maybe_backfill(conn)
                    })
                    .await;
                    match backfill {
                        Ok(()) => {
                            let _ = handle.emit("links-changed", ());
                        }
                        Err(e) => log::warn!("links/tags backfill failed: {e}"),
                    }
                });
            }

            // Startup recovery for the recording library: complete anything a
            // crash or a forced quit left half-written, before Recording
            // history is asked about it. Filesystem work, so it keeps off the
            // setup thread rather than holding the window closed.
            {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    let recovery = handle.clone();
                    let done = tauri::async_runtime::spawn_blocking(move || {
                        if let Err(e) = whisper::models::sweep_partials(&recovery) {
                            log::warn!("leftover model downloads were not swept: {e}");
                        }
                        commands::recording_preferences::recover(&recovery)
                    })
                    .await;
                    match done {
                        // Recovery can register a rescued recording, which
                        // creates a page the sidebar has not seen yet.
                        Ok(Ok(())) => {
                            let _ = handle.emit("pages-changed", ());
                        }
                        Ok(Err(e)) => log::warn!("recording recovery failed: {e}"),
                        Err(e) => log::warn!("recording recovery did not run: {e}"),
                    }
                });
            }

            // Watch for writes made by the MCP sidecar, which has its own
            // connection to the same database and cannot emit our events.
            spawn_mcp_change_poller(app.handle().clone());

            // If a vault was configured previously, resume watching it.
            vault::resume_if_configured(&app.handle().clone());

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::pages::create_page,
            commands::pages::get_page,
            commands::pages::list_pages,
            commands::pages::rename_page,
            commands::pages::set_page_icon,
            commands::pages::set_page_favorite,
            commands::pages::move_page,
            commands::pages::delete_page,
            commands::documents::get_document,
            commands::documents::update_document,
            commands::documents::update_document_if_unchanged,
            commands::knowledge::set_page_links,
            commands::knowledge::get_backlinks,
            commands::knowledge::get_page_tags,
            commands::knowledge::get_graph,
            commands::databases::get_database,
            commands::databases::get_database_by_id,
            commands::databases::list_databases,
            commands::databases::promote_row,
            commands::databases::create_field,
            commands::databases::update_field,
            commands::databases::delete_field,
            commands::databases::create_row,
            commands::databases::create_row_with_cells,
            commands::databases::delete_row,
            commands::databases::move_row,
            commands::databases::set_cell,
            commands::databases::update_view,
            commands::search::search,
            commands::updates::check_app_update,
            commands::updates::install_app_update,
            commands::recording_preferences::get_recording_preferences,
            commands::recording_preferences::set_recording_preferences,
            commands::recording_preferences::get_transcription_vocabulary,
            commands::recording::mark_moment,
            commands::recording::live_transcript,
            local_ai::live_recap,
            meeting_monitor::meeting_hud_state,
            meeting_monitor::meeting_hud_respond,
            meeting_monitor::get_meeting_automation,
            meeting_monitor::set_meeting_automation,
            calendar::calendar_status,
            calendar::calendar_connect,
            calendar::calendar_disconnect,
            calendar::calendar_events,
            call_window::call_names_status,
            call_window::call_names_enable,
            call_window::call_names_disable,
            call_window::call_names_open_settings,
            commands::meeting_notes::note_templates,
            commands::meeting_notes::set_meeting_template,
            commands::meeting_notes::meeting_last_time,
            commands::meeting_notes::meeting_event,
            commands::meeting_notes::meeting_people,
            commands::recording_preferences::set_transcription_vocabulary,
            commands::recording_preferences::finish_recording,
            commands::recording_preferences::discard_recording_audio,
            commands::recording::start_recording,
            commands::recording::stop_recording,
            commands::recording::is_recording,
            commands::recording_history::recording_history,
            commands::recording_history::transcript_versions,
            commands::recording_history::retranscribe_meeting,
            commands::recording_history::cancel_transcription,
            commands::recording_history::export_recording_text,
            commands::documents::save_text_file,
            commands::ingest::note_summary_status,
            commands::ingest::note_summary_retry,
            commands::knowledge::list_tags,
            commands::meeting_notes::meeting_speakers,
            commands::meeting_notes::rename_meeting_speakers,
            commands::meeting_notes::add_meeting_notes_to_page,
            commands::meeting_notes::meeting_next_steps_to_tasks,
            commands::meeting_notes::set_next_step_done,
            commands::recording_history::export_recording_audio,
            whisper::models::list_models,
            whisper::models::download_model,
            whisper::models::cancel_model_download,
            whisper::models::select_model,
            whisper::models::delete_model,
            whisper::transcribe::transcribe,
            whisper::diarize::diarize,
            whisper::diarize::diarization_available,
            whisper::diarize::download_diarization_models,
            local_ai::ai_end_recording,
            local_ai::local_ai_status,
            local_ai::local_ai_configure,
            local_ai::local_ai_setup,
            local_ai::local_ai_download,
            local_ai::local_ai_remove,
            local_ai::local_ai_cancel,
            local_ai::meeting_ai_jobs,
            local_ai::meeting_ai_retry,
            local_ai::ask_meetings,
            commands::ai::ollama_status,
            commands::ai::ai_generate,
            commands::ingest::ingest_note,
            commands::mcp::mcp_get_token,
            commands::mcp::mcp_enable,
            commands::mcp::mcp_disable,
            vault::set_vault_dir,
            vault::get_vault_dir,
            vault::export_vault,
            vault::flush_page,
            lifecycle::confirm_close,
        ])
        .on_window_event(|window, event| {
            // Closing the window is a request, not an order: a recording may
            // still need finalising, and the front end may need to save.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if lifecycle::intercept(window.app_handle()) {
                    api.prevent_close();
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| match event {
            // Quitting from the Dock or the menu bar goes through here rather
            // than a window close, so it needs the same guard.
            tauri::RunEvent::ExitRequested { ref api, .. } => {
                if lifecycle::intercept(app) {
                    api.prevent_exit();
                }
            }
            tauri::RunEvent::Exit => {
                app.state::<local_ai::AiState>().shutdown();
            }
            _ => {}
        });
}
