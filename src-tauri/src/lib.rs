mod model_catalogue;
mod audio;
mod commands;
mod local_ai;
mod vault;
mod whisper;
mod workspace;

// The DB layer, error type, LLM client, and store now live in the shared core
// crate. Re-export db/error at the app crate root so the app's audio/whisper
// modules keep referring to `crate::db` / `crate::error` unchanged.
pub use tidy_core::{db, error};

use tauri::Manager;

use commands::recording::RecorderState;
use db::Db;
use vault::VaultState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
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
                std::io::Error::new(std::io::ErrorKind::Other, e.to_string())
            })?;
            app.manage(database);
            app.manage(RecorderState::default());
            app.manage(commands::updates::UpdateState::default());
            app.manage(commands::recording_history::TranscriptionState::default());
            app.manage(VaultState::default());
            app.manage(local_ai::AiState::default());
            local_ai::start_worker(app.handle().clone());

            // One-time: backfill links/tags from existing page bodies so an
            // upgraded DB's graph/backlinks aren't empty on first v3 launch.
            {
                let db = app.state::<Db>();
                let conn = db.conn.lock().unwrap();
                if let Err(e) = tidy_core::store::knowledge::core::maybe_backfill(&conn) {
                    log::warn!("links/tags backfill failed: {e}");
                }
            }

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
            commands::databases::delete_row,
            commands::databases::move_row,
            commands::databases::set_cell,
            commands::databases::update_view,
            commands::search::search,
            commands::updates::check_app_update,
            commands::updates::install_app_update,
            commands::recording_preferences::get_recording_preferences,
            commands::recording_preferences::set_recording_preferences,
            commands::recording_preferences::finish_recording,
            commands::recording::start_recording,
            commands::recording::stop_recording,
            commands::recording::is_recording,
            commands::recording::record_meeting,
            commands::recording_history::recording_history,
            commands::recording_history::transcript_versions,
            commands::recording_history::retranscribe_meeting,
            commands::recording_history::cancel_transcription,
            commands::recording_history::export_recording_text,
            commands::recording_history::export_recording_audio,
            whisper::models::list_models,
            whisper::models::download_model,
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
            commands::ai::summarize_transcript,
            commands::ai::ai_generate,
            commands::ingest::ingest_note,
            commands::mcp::mcp_get_token,
            commands::mcp::mcp_enable,
            commands::mcp::mcp_disable,
            vault::set_vault_dir,
            vault::get_vault_dir,
            vault::export_vault,
            vault::flush_page,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if matches!(event, tauri::RunEvent::Exit) {
                app.state::<local_ai::AiState>().shutdown();
            }
        });
}
