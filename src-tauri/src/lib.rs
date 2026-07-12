mod audio;
mod commands;
mod vault;
mod whisper;

// The DB layer, error type, LLM client, and store now live in the shared core
// crate. Re-export db/error at the app crate root so the app's audio/whisper
// modules keep referring to `crate::db` / `crate::error` unchanged.
pub use appflower_core::{db, error};

use tauri::Manager;

use commands::recording::RecorderState;
use db::Db;
use vault::VaultState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }

            // Open the local database in the app data dir.
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let db_path = dir.join("appflower.db");
            log::info!("AppFlower database: {}", db_path.display());
            let database = Db::open(&db_path).map_err(|e| {
                log::error!("failed to open database: {e}");
                std::io::Error::new(std::io::ErrorKind::Other, e.to_string())
            })?;
            app.manage(database);
            app.manage(RecorderState::default());
            app.manage(VaultState::default());

            // One-time: backfill links/tags from existing page bodies so an
            // upgraded DB's graph/backlinks aren't empty on first v3 launch.
            {
                let db = app.state::<Db>();
                let conn = db.conn.lock().unwrap();
                if let Err(e) = appflower_core::store::knowledge::core::maybe_backfill(&conn) {
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
            commands::recording::start_recording,
            commands::recording::stop_recording,
            commands::recording::is_recording,
            commands::recording::record_meeting,
            whisper::models::list_models,
            whisper::models::download_model,
            whisper::models::select_model,
            whisper::models::delete_model,
            whisper::transcribe::transcribe,
            whisper::diarize::diarize,
            whisper::diarize::diarization_available,
            whisper::diarize::download_diarization_models,
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
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
