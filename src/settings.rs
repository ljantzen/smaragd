//! Persistent app-wide preferences (as opposed to `project::ProjectMeta`, which is
//! per-project). Stored as `tachylite.toml` in the platform's standard config
//! directory — `~/.config/tachylite` on Linux, `~/Library/Application Support/tachylite`
//! on macOS, `%APPDATA%\tachylite\config` on Windows — via the `directories` crate.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::shortcuts::ShortcutMap;

/// A user's explicit choice for a plugin-registered `:` command's shortcut, kept
/// separate from a plain `Option<KeyboardShortcut>` so `Unbound` can be told apart
/// from "no override recorded yet" (see `plugin_shortcut_overrides`'s doc comment)
/// — and represented as an enum rather than `Option` because TOML has no null, so
/// an `Option::None` stored as a map *value* (as opposed to a whole field) can't
/// round-trip through it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PluginShortcutOverride {
    Bound(egui::KeyboardShortcut),
    Unbound,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Reopen `last_project_path` automatically on launch.
    pub reopen_last_project: bool,
    /// The most recently opened project folder, tracked regardless of
    /// `reopen_last_project` so toggling the setting on later works immediately.
    pub last_project_path: Option<PathBuf>,
    /// Ensure every opened project has a Research and a Trash folder (roles already
    /// assigned — Scrivener's Fiction-template starter experience), each checked and
    /// created independently on every open, not just when a project is first
    /// created — see `Project::ensure_role_folder`. Off by default: a project's
    /// folder layout is otherwise left exactly as found.
    pub create_starter_folders: bool,
    /// Keyboard shortcut bindings, remappable from the Settings window.
    pub shortcuts: ShortcutMap,
    /// Dark/light/system *appearance* preference, changeable from the Settings
    /// window or `:dmode`. Defaults to `System` (egui's own default), matching this
    /// struct's otherwise-untouched-until-configured philosophy. Independent of
    /// `color_theme` below — see that field's doc comment.
    pub theme_preference: egui::ThemePreference,
    /// The selected Helix-style color *theme*'s id (`color_theme::ColorTheme::id`),
    /// if any — `None` means no theme is applied, just plain dark/light styling per
    /// `theme_preference`. Set via `:theme <id>` or the View > Theme menu.
    pub color_theme: Option<String>,
    /// User overrides for plugin `:` command shortcuts, keyed by command name (see
    /// `plugins::PluginEngine::shortcut_defaults`). Unlike `shortcuts` above (whose
    /// fixed set of built-in ids never changes), a plugin re-declares its own
    /// default shortcut every time it's (re)loaded, so an id simply absent from
    /// this map doesn't mean "unbound" — it means "no override, use the plugin
    /// script's own default if that combo is currently free." An explicit
    /// `Unbound` entry is what actually keeps a command unbound across reloads.
    pub plugin_shortcut_overrides: BTreeMap<String, PluginShortcutOverride>,
}

/// The full path to the settings file, e.g. `~/.config/tachylite/tachylite.toml` on
/// Linux. `None` if the platform's config directory can't be determined.
pub fn config_file_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "tachylite")
        .map(|dirs| dirs.config_dir().join("tachylite.toml"))
}

/// The full path to the persisted dock layout (which tabs are open, and how
/// they're split/floated), e.g. `~/.config/tachylite/dock_layout.json` on Linux.
/// Kept in its own file rather than as a field on `Settings`: `egui_dock::DockState`
/// is a recursive tree of nodes, which doesn't round-trip through TOML (its
/// derived `Serialize` impl emits constructs — like a sequence of tables mixed with
/// non-table values — that TOML's format can't represent), but does through JSON.
/// `None` if the platform's config directory can't be determined.
pub fn dock_layout_file_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "tachylite")
        .map(|dirs| dirs.config_dir().join("dock_layout.json"))
}

/// The full path to the user's named, saved dock layouts (Window > Save Current
/// Layout…/Layouts), e.g. `~/.config/tachylite/saved_layouts.json` on Linux. Kept
/// separate from `dock_layout_file_path` — that one tracks only the single
/// currently-active layout, persisted on shutdown; this one is a named map,
/// persisted immediately whenever the user explicitly saves one. Same JSON (not
/// TOML) reasoning as `dock_layout_file_path`. `None` if the platform's config
/// directory can't be determined.
pub fn saved_layouts_file_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "tachylite")
        .map(|dirs| dirs.config_dir().join("saved_layouts.json"))
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

    /// Bind a plugin `:` command's shortcut to `shortcut` (`None` to unbind it —
    /// which, unlike a plain removal, must be recorded explicitly so it sticks
    /// across plugin reloads; see `plugin_shortcut_overrides`'s doc comment).
    /// First clears that exact combo from whichever built-in action or other
    /// plugin command in `current_plugin_shortcuts` currently holds it, mirroring
    /// `ShortcutMap::set`'s 1:1 invariant but extended across both namespaces —
    /// they draw from the same physical keyboard. `current_plugin_shortcuts` is
    /// the caller's last-computed effective set (`app.rs`'s
    /// `compute_effective_plugin_shortcuts`), since `Settings` alone doesn't have
    /// the loaded `PluginEngine` needed to derive it.
    pub fn set_plugin_shortcut(
        &mut self,
        command_name: &str,
        shortcut: Option<egui::KeyboardShortcut>,
        current_plugin_shortcuts: &[(String, Option<egui::KeyboardShortcut>)],
    ) {
        let Some(shortcut) = shortcut else {
            self.plugin_shortcut_overrides
                .insert(command_name.to_string(), PluginShortcutOverride::Unbound);
            return;
        };

        if let Some(action) = self
            .shortcuts
            .bindings()
            .into_iter()
            .find(|(_, bound)| *bound == shortcut)
            .map(|(action, _)| action)
        {
            self.shortcuts.set(action, None);
        }

        let other_owner = current_plugin_shortcuts.iter().find_map(|(name, current)| {
            (name.as_str() != command_name && *current == Some(shortcut)).then(|| name.clone())
        });
        if let Some(other_owner) = other_owner {
            self.plugin_shortcut_overrides
                .insert(other_owner, PluginShortcutOverride::Unbound);
        }

        self.plugin_shortcut_overrides.insert(
            command_name.to_string(),
            PluginShortcutOverride::Bound(shortcut),
        );
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
    fn defaults_to_following_the_system_theme() {
        assert_eq!(
            Settings::default().theme_preference,
            egui::ThemePreference::System
        );
    }

    #[test]
    fn settings_file_without_a_theme_preference_loads_the_system_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tachylite.toml");
        std::fs::write(&path, "reopen_last_project = true\n").unwrap();

        let loaded = Settings::load_from_path(&path);

        assert_eq!(loaded.theme_preference, egui::ThemePreference::System);
    }

    #[test]
    fn settings_file_without_a_color_theme_loads_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tachylite.toml");
        std::fs::write(&path, "reopen_last_project = true\n").unwrap();

        let loaded = Settings::load_from_path(&path);

        assert_eq!(loaded.color_theme, None);
    }

    #[test]
    fn settings_round_trip_through_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tachylite.toml");
        let mut shortcuts = ShortcutMap::default();
        shortcuts.set(
            crate::shortcuts::ShortcutAction::Save,
            Some(egui::KeyboardShortcut::new(
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                egui::Key::S,
            )),
        );
        let mut plugin_shortcut_overrides = BTreeMap::new();
        plugin_shortcut_overrides.insert(
            "wordcount".to_string(),
            PluginShortcutOverride::Bound(egui::KeyboardShortcut::new(
                egui::Modifiers::COMMAND,
                egui::Key::L,
            )),
        );
        plugin_shortcut_overrides.insert("hello".to_string(), PluginShortcutOverride::Unbound);
        let settings = Settings {
            reopen_last_project: true,
            last_project_path: Some(PathBuf::from("/home/author/my-novel")),
            create_starter_folders: true,
            shortcuts,
            theme_preference: egui::ThemePreference::Dark,
            color_theme: Some("dracula".to_string()),
            plugin_shortcut_overrides,
        };

        settings.save_to_path(&path).unwrap();
        let loaded = Settings::load_from_path(&path);

        assert_eq!(loaded, settings);
    }

    #[test]
    fn settings_file_without_a_shortcuts_table_loads_default_shortcuts() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tachylite.toml");
        std::fs::write(&path, "reopen_last_project = true\n").unwrap();

        let loaded = Settings::load_from_path(&path);

        assert_eq!(loaded.shortcuts, ShortcutMap::default());
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

    #[test]
    fn set_plugin_shortcut_binds_with_no_conflicts() {
        let mut settings = Settings::default();
        let shortcut = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::L);

        settings.set_plugin_shortcut("wordcount", Some(shortcut), &[]);

        assert_eq!(
            settings.plugin_shortcut_overrides.get("wordcount"),
            Some(&PluginShortcutOverride::Bound(shortcut))
        );
    }

    #[test]
    fn set_plugin_shortcut_none_records_an_explicit_unbind() {
        let mut settings = Settings::default();

        settings.set_plugin_shortcut("wordcount", None, &[]);

        assert_eq!(
            settings.plugin_shortcut_overrides.get("wordcount"),
            Some(&PluginShortcutOverride::Unbound)
        );
    }

    #[test]
    fn set_plugin_shortcut_steals_from_a_built_in_action_that_currently_holds_it() {
        let mut settings = Settings::default();
        let save_shortcut = settings
            .shortcuts
            .get(crate::shortcuts::ShortcutAction::Save)
            .unwrap();

        settings.set_plugin_shortcut("wordcount", Some(save_shortcut), &[]);

        assert_eq!(
            settings
                .shortcuts
                .get(crate::shortcuts::ShortcutAction::Save),
            None
        );
        assert_eq!(
            settings.plugin_shortcut_overrides.get("wordcount"),
            Some(&PluginShortcutOverride::Bound(save_shortcut))
        );
    }

    #[test]
    fn set_plugin_shortcut_steals_from_another_plugin_command_that_currently_holds_it() {
        let mut settings = Settings::default();
        let shortcut = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::L);
        let current = vec![("other_command".to_string(), Some(shortcut))];

        settings.set_plugin_shortcut("wordcount", Some(shortcut), &current);

        assert_eq!(
            settings.plugin_shortcut_overrides.get("other_command"),
            Some(&PluginShortcutOverride::Unbound)
        );
        assert_eq!(
            settings.plugin_shortcut_overrides.get("wordcount"),
            Some(&PluginShortcutOverride::Bound(shortcut))
        );
    }
}
