use crate::{
    error::{AppError, AppResult},
    local_ai::AiState,
};
use serde::Serialize;
use std::{
    sync::{atomic::Ordering, Mutex},
    time::Duration,
};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

#[derive(Default)]
pub struct UpdateState {
    pending: Mutex<Option<Update>>,
    operation: tokio::sync::Mutex<()>,
}
#[derive(Serialize)]
pub struct AvailableUpdate {
    version: String,
    notes: Option<String>,
}
#[derive(Clone, Serialize)]
struct Progress {
    stage: &'static str,
    downloaded: u64,
    total: Option<u64>,
}
fn update_error(error: tauri_plugin_updater::Error) -> AppError {
    use tauri_plugin_updater::Error;
    log::warn!("Updater: {error}");
    let message = match error {
        Error::ReleaseNotFound => "The update feed is not available yet. Please try again later.",
        Error::TargetNotFound(_) | Error::TargetsNotFound(_) => {
            "This release does not include an update for your Mac."
        }
        Error::Minisign(_) | Error::Base64(_) | Error::SignatureUtf8(_) => {
            "The update could not be verified. Nothing was installed. Please try again later."
        }
        Error::Reqwest(_) | Error::Network(_) => {
            "Could not reach GitHub. Check your connection and try again."
        }
        Error::Io(_) | Error::AuthenticationFailed => {
            "The update could not be installed. Check that Tidy is in Applications and try again."
        }
        _ => "The update could not be completed. Please try again later.",
    };
    AppError::Other(message.into())
}
fn validate_download(url: &str) -> AppResult<()> {
    if !url.starts_with("https://github.com/REllwood/Tidy/releases/download/")
        || !url.ends_with(".app.tar.gz")
    {
        return Err(AppError::Invalid(
            "The update download is not a Tidy release.".into(),
        ));
    }
    Ok(())
}
#[tauri::command]
pub async fn check_app_update(app: AppHandle) -> AppResult<Option<AvailableUpdate>> {
    let state = app.state::<UpdateState>();
    let _guard = state
        .operation
        .try_lock()
        .map_err(|_| AppError::Other("An update is already being checked or installed.".into()))?;
    let mut update = app
        .updater_builder()
        .timeout(Duration::from_secs(20))
        .no_proxy()
        .build()
        .map_err(update_error)?
        .check()
        .await
        .map_err(update_error)?;
    if let Some(ref mut next) = update {
        validate_download(next.download_url.as_str())?;
        next.timeout = Some(Duration::from_secs(600));
    }
    let result = update.as_ref().map(|u| AvailableUpdate {
        version: u.version.clone(),
        notes: u.body.clone(),
    });
    *state
        .pending
        .lock()
        .map_err(|_| AppError::Other("Update state is unavailable.".into()))? = update;
    Ok(result)
}
struct Installation(AppHandle);
impl Drop for Installation {
    fn drop(&mut self) {
        self.0
            .state::<AiState>()
            .recording
            .store(false, Ordering::SeqCst);
    }
}
#[tauri::command]
pub async fn install_app_update(app: AppHandle, version: String) -> AppResult<()> {
    let state = app.state::<UpdateState>();
    let _guard = state
        .operation
        .try_lock()
        .map_err(|_| AppError::Other("An update is already in progress.".into()))?;
    let update = state
        .pending
        .lock()
        .map_err(|_| AppError::Other("Update state is unavailable.".into()))?
        .clone()
        .filter(|u| u.version == version)
        .ok_or_else(|| AppError::Invalid("Check for updates again before installing.".into()))?;
    validate_download(update.download_url.as_str())?;
    if !std::env::current_exe()?
        .components()
        .any(|c| c.as_os_str().to_string_lossy().ends_with(".app"))
    {
        return Err(AppError::Invalid(
            "Install updates from the Tidy app in Applications.".into(),
        ));
    }
    app.state::<AiState>().reserve_recording().map_err(|_| {
        AppError::Other("Finish recording or transcribing before installing an update.".into())
    })?;
    let installation = Installation(app.clone());
    app.state::<AiState>().wait_until_idle().await;
    let mut downloaded = 0;
    let bytes = update
        .download(
            |chunk, total| {
                downloaded += chunk as u64;
                let _ = app.emit(
                    "app-update-progress",
                    Progress {
                        stage: "downloading",
                        downloaded,
                        total,
                    },
                );
            },
            || {},
        )
        .await
        .map_err(update_error)?;
    let _ = app.emit(
        "app-update-progress",
        Progress {
            stage: "installing",
            downloaded,
            total: Some(downloaded),
        },
    );
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _installation = installation;
        update.install(bytes).map_err(update_error)?;
        let _ = handle.emit(
            "app-update-progress",
            Progress {
                stage: "restarting",
                downloaded,
                total: Some(downloaded),
            },
        );
        handle.restart();
        #[allow(unreachable_code)]
        Ok::<(), AppError>(())
    })
    .await
    .map_err(|e| AppError::Other(format!("Update worker: {e}")))?
}
#[cfg(test)]
mod tests {
    use super::*;
    // Exercises the actual plugin installer using only a disposable fixture path.
    // The production endpoint and HTTPS requirements are unchanged.
    #[tokio::test]
    #[ignore = "requires TIDY_UPDATE_TEST_URL and TIDY_UPDATE_INSTALL_FIXTURE"]
    async fn install_signed_update_in_isolated_fixture() {
        let endpoint = std::env::var("TIDY_UPDATE_TEST_URL").unwrap();
        assert!(endpoint.starts_with("http://127.0.0.1:"));
        let root = std::path::PathBuf::from(std::env::var("TIDY_UPDATE_INSTALL_FIXTURE").unwrap()).canonicalize().unwrap();
        assert!(root.join("tidy-updater-test-fixture").is_file(), "A dedicated fixture marker is required");
        let destination = root.join("Tidy.app");
        assert!(!destination.exists(), "Use a fresh fixture directory");
        std::fs::create_dir_all(destination.join("Contents/MacOS")).unwrap();
        std::fs::write(destination.join("Contents/MacOS/tidy"), b"Synthetic previous application").unwrap();
        let config: serde_json::Value = serde_json::from_str(include_str!("../../tauri.conf.json")).unwrap();
        let pubkey = config["plugins"]["updater"]["pubkey"].as_str().unwrap();
        let mut context = tauri::test::mock_context(tauri::test::noop_assets());
        context.config_mut().plugins.0.insert("updater".into(), config["plugins"]["updater"].clone());
        let app = tauri::test::mock_builder()
            .plugin(tauri_plugin_updater::Builder::new().pubkey(pubkey).build())
            .build(context)
            .unwrap();
        let updater = app.handle().updater_builder()
            .executable_path(destination.join("Contents/MacOS/tidy"))
            .endpoints(vec![endpoint.parse().unwrap()]).unwrap()
            .version_comparator(|_, _| true)
            .no_proxy().build().unwrap();
        let update = updater.check().await.unwrap().unwrap();
        let bytes = update.download(|_, _| {}, || {}).await.unwrap();
        update.install(bytes).unwrap();
        assert!(destination.join("Contents/Info.plist").is_file());
        assert!(destination.join("Contents/MacOS/tidy-ai").is_file());
        assert!(std::process::Command::new("/usr/bin/codesign")
            .args(["--verify", "--deep", "--strict"]).arg(&destination).status().unwrap().success());
        assert!(std::process::Command::new("/usr/bin/xcrun")
            .args(["stapler", "validate"]).arg(&destination).status().unwrap().success());
    }

    #[test]
    #[ignore = "requires TIDY_UPDATE_FIXTURE and its .sig file"]
    fn verify_real_update_signature() {
        use base64::Engine;
        let path = std::env::var("TIDY_UPDATE_FIXTURE").unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let encoded_signature = std::fs::read_to_string(format!("{path}.sig")).unwrap();
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../../tauri.conf.json")).unwrap();
        let decode = |value: &str| {
            String::from_utf8(
                base64::engine::general_purpose::STANDARD
                    .decode(value.trim())
                    .unwrap(),
            )
            .unwrap()
        };
        let key = minisign_verify::PublicKey::decode(&decode(
            config["plugins"]["updater"]["pubkey"].as_str().unwrap(),
        ))
        .unwrap();
        let signature = minisign_verify::Signature::decode(&decode(&encoded_signature)).unwrap();
        key.verify(&bytes, &signature, true).unwrap();
        let mut modified = bytes;
        modified[0] ^= 1;
        assert!(key.verify(&modified, &signature, true).is_err());
    }
    #[test]
    fn only_tidy_release_archives_are_accepted() {
        assert!(validate_download(
            "https://github.com/REllwood/Tidy/releases/download/v0.2.2/Tidy.app.tar.gz"
        )
        .is_ok());
        for url in [
            "http://github.com/REllwood/Tidy/releases/download/v1/Tidy.app.tar.gz",
            "https://github.com/other/Tidy/releases/download/v1/Tidy.app.tar.gz",
            "https://github.com/REllwood/Tidy/releases/download/v1/Tidy.dmg",
            "https://github.com.evil.test/REllwood/Tidy/releases/download/v1/Tidy.app.tar.gz",
        ] {
            assert!(validate_download(url).is_err(), "{url}");
        }
    }
}
