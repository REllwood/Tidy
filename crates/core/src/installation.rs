//! Locate upgraded workspaces in place: no copying, renaming or deleting user data.
use crate::error::AppResult;
use serde::Deserialize;
use std::path::{Path, PathBuf};

pub const DATA_DIRECTORY: &str = "com.tidy.desktop";
pub const DATABASE_FILE: &str = "tidy.db";

#[derive(Deserialize)]
pub struct PreviousInstallation {
    pub data_directory: String,
    pub database_file: String,
    pub database_environment: String,
    pub token_environment: String,
}
pub fn previous() -> AppResult<PreviousInstallation> {
    Ok(serde_json::from_str(include_str!(
        "../../../compatibility/previous-installation.json"
    ))?)
}
pub fn data_directory(preferred: &Path) -> AppResult<PathBuf> {
    let previous = previous()?;
    // A newer workspace always wins; an empty directory must not hide existing notes.
    if preferred.join(DATABASE_FILE).is_file() || preferred.join(&previous.database_file).is_file()
    {
        return Ok(preferred.to_path_buf());
    }
    if let Some(parent) = preferred.parent() {
        let old = parent.join(previous.data_directory);
        if old.join(previous.database_file).is_file() {
            return Ok(old);
        }
    }
    Ok(preferred.to_path_buf())
}
pub fn database_path(directory: &Path) -> AppResult<PathBuf> {
    let current = directory.join(DATABASE_FILE);
    let previous = directory.join(previous()?.database_file);
    Ok(if current.is_file() || !previous.is_file() {
        current
    } else {
        previous
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn upgrades_reuse_the_same_database_and_model_directory() {
        let root = tempfile::tempdir().unwrap();
        let previous = previous().unwrap();
        let old = root.path().join(&previous.data_directory);
        std::fs::create_dir_all(old.join("models")).unwrap();
        let db = old.join(&previous.database_file);
        std::fs::write(&db, b"existing notes").unwrap();
        std::fs::write(old.join("models/model.gguf"), b"existing model").unwrap();
        let preferred = root.path().join(DATA_DIRECTORY);
        std::fs::create_dir_all(&preferred).unwrap();
        let selected = data_directory(&preferred).unwrap();
        assert_eq!(selected, old);
        assert_eq!(database_path(&selected).unwrap(), db);
        assert_eq!(std::fs::read(&db).unwrap(), b"existing notes");
        assert!(selected.join("models/model.gguf").is_file());
        std::fs::write(preferred.join(DATABASE_FILE), b"newer notes").unwrap();
        assert_eq!(data_directory(&preferred).unwrap(), preferred);
    }
    #[test]
    fn new_installs_use_tidy_names() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join(DATA_DIRECTORY);
        assert_eq!(data_directory(&dir).unwrap(), dir);
        assert_eq!(database_path(&dir).unwrap(), dir.join("tidy.db"));
    }
}
