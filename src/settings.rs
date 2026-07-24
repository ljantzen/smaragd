//! Persistent app-wide preferences (as opposed to `project::ProjectMeta`, which is
//! per-project). Stored as `tachylite.toml` in the platform's standard config
//! directory — `~/.config/tachylite` on Linux, `~/Library/Application Support/tachylite`
//! on macOS, `%APPDATA%\tachylite\config` on Windows — via the `directories` crate.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Reopen `last_project_path` automatically on launch.
    pub reopen_last_project: bool,
    /// The most recently opened project folder, tracked regardless of
    /// `reopen_last_project` so toggling the setting on later works immediately.
    pub last_project_path: Option<PathBuf>,
    /// When creating a new project (not when adopting an existing folder of notes),
    /// pre-seed it with empty Research and Trash folders, roles already assigned —
    /// Scrivener's Fiction-template starter experience. Off by default: a fresh
    /// project starts completely empty unless the user opts in.
    pub create_starter_folders: bool,
}

/// The full path to the settings file, e.g. `~/.config/tachylite/tachylite.toml` on
/// Linux. `None` if the platform's config directory can't be determined.
pub fn config_file_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "tachylite")
        .map(|dirs| dirs.config_dir().join("tachylite.toml"))
}

impl Settings {
    /// Load settings from `path`, falling back to defaults if the file is missing or
    /// its contents can't be parsed — a first launch or a hand-edited file should
    /// never prevent the app from starting.
    pub fn load_from_path(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|contents| toml::from_str(&contents).ok())
            .unwrap_or_default()
    }

    pub fn save_to_path(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let contents =
            toml::to_string_pretty(self).expect("Settings always serializes to valid TOML");
        std::fs::write(path, contents)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_do_not_reopen_and_have_no_last_project() {
        let settings = Settings::default();
        assert!(!settings.reopen_last_project);
        assert_eq!(settings.last_project_path, None);
    }

    #[test]
    fn settings_round_trip_through_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tachylite.toml");
        let settings = Settings {
            reopen_last_project: true,
            last_project_path: Some(PathBuf::from("/home/author/my-novel")),
            create_starter_folders: true,
        };

        settings.save_to_path(&path).unwrap();
        let loaded = Settings::load_from_path(&path);

        assert_eq!(loaded, settings);
    }

    #[test]
    fn missing_settings_file_falls_back_to_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("does-not-exist.toml");

        assert_eq!(Settings::load_from_path(&path), Settings::default());
    }

    #[test]
    fn corrupt_settings_file_falls_back_to_default_instead_of_panicking() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tachylite.toml");
        std::fs::write(&path, "not valid { toml").unwrap();

        assert_eq!(Settings::load_from_path(&path), Settings::default());
    }

    #[test]
    fn save_creates_missing_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir
            .path()
            .join("nested")
            .join("config")
            .join("tachylite.toml");

        Settings::default().save_to_path(&path).unwrap();

        assert!(path.exists());
    }
}
