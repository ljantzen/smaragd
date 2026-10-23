//! App-wide preferences for the Tauri shell — the portable counterpart of
//! the real egui app's `Settings` (`app/src/settings.rs`, ~43 fields). Not
//! a byte-for-byte port: persisted to its own `smaragd-tauri.toml` (see
//! `config_file_path`), not the egui app's `smaragd.toml`, so the two apps
//! can coexist on one machine during the migration without either's save
//! ever risking corrupting or silently dropping fields from the other's
//! settings file. Fields this struct doesn't (yet) have a real consumer for
//! in the Tauri app (sync server config — Phase 6's Sync panel is demo-only;
//! plugin shortcut overrides — no plugin engine exists here) are left out
//! rather than carried along inert; everything that *is* here either already
//! has a real effect or is wired alongside this module.
//!
//! `egui`-coupled fields from the original (`theme_preference:
//! egui::ThemePreference`, `editor_font`/`ui_font: EditorFont` holding an
//! egui font handle, shortcuts keyed by `egui::KeyboardShortcut`) are
//! replaced here with plain portable equivalents — see `ThemePreference`,
//! `EditorFont`, and `KeyCombo` below.

use std::collections::BTreeMap;
use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::project::store::ProjectStore;

/// A keyboard shortcut as plain data — the portable replacement for
/// `egui::KeyboardShortcut`. `key` is a DOM `KeyboardEvent.key` value
/// (lowercased for letters, e.g. `"s"`, `"f2"`, `"arrowleft"`, `","`) rather
/// than egui's own `Key` enum, since the frontend matches it directly
/// against browser keyboard events. `ctrl_or_cmd` collapses egui's
/// platform-abstracted `Modifiers::COMMAND` (Ctrl on Windows/Linux, Cmd on
/// macOS) into one field — which physical key that means is resolved at
/// match time in the frontend, not stored here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyCombo {
    pub key: String,
    pub ctrl_or_cmd: bool,
    pub shift: bool,
    pub alt: bool,
}

impl KeyCombo {
    pub fn new(key: &str) -> Self {
        Self {
            key: key.to_string(),
            ctrl_or_cmd: false,
            shift: false,
            alt: false,
        }
    }

    pub fn cmd(key: &str) -> Self {
        Self {
            ctrl_or_cmd: true,
            ..Self::new(key)
        }
    }

    pub fn cmd_shift(key: &str) -> Self {
        Self {
            ctrl_or_cmd: true,
            shift: true,
            ..Self::new(key)
        }
    }

    pub fn cmd_alt(key: &str) -> Self {
        Self {
            ctrl_or_cmd: true,
            alt: true,
            ..Self::new(key)
        }
    }

    pub fn cmd_alt_shift(key: &str) -> Self {
        Self {
            ctrl_or_cmd: true,
            alt: true,
            shift: true,
            ..Self::new(key)
        }
    }

    pub fn alt(key: &str) -> Self {
        Self {
            alt: true,
            ..Self::new(key)
        }
    }
}

/// Portable replacement for `egui::ThemePreference` (same three variants —
/// egui's own enum already has this exact shape, so this isn't a new
/// design, just decoupled from the egui crate).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ThemePreference {
    #[default]
    System,
    Dark,
    Light,
}

/// Portable replacement for the real app's `editor_font::EditorFont` (same
/// 5 variants) — maps to a CSS font-family on the frontend; the two
/// non-system faces' bytes are already in `crate::fonts` from the Phase 0
/// workspace split, reusable for the Preview/export path exactly as today.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum EditorFont {
    #[default]
    Proportional,
    Monospace,
    LibertinusSerif,
    DejaVuSansMono,
    AtkinsonHyperlegible,
}

impl EditorFont {
    pub const ALL: [EditorFont; 5] = [
        EditorFont::Proportional,
        EditorFont::Monospace,
        EditorFont::LibertinusSerif,
        EditorFont::DejaVuSansMono,
        EditorFont::AtkinsonHyperlegible,
    ];

    pub fn label(self) -> &'static str {
        match self {
            EditorFont::Proportional => "Proportional (system default)",
            EditorFont::Monospace => "Monospace (system default)",
            EditorFont::LibertinusSerif => "Libertinus Serif",
            EditorFont::DejaVuSansMono => "DejaVu Sans Mono",
            EditorFont::AtkinsonHyperlegible => "Atkinson Hyperlegible",
        }
    }
}

/// A string that must never appear in logs or `{:?}` output — the sync
/// passphrase field's type, kept for shape-compatibility with the real
/// app's `SecretString` even though `sync_passphrase` itself isn't part of
/// this struct yet (see the module doc) — ready for whenever Sync grows
/// real pairing instead of Phase 6's demo mode.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SecretString(pub String);

impl std::fmt::Debug for SecretString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.0.is_empty() {
            "SecretString(<unset>)"
        } else {
            "SecretString(<redacted>)"
        })
    }
}

const MAX_RECENT_PROJECTS: usize = 10;
const DEFAULT_BACKUP_KEEP_COUNT: u32 = 10;
const DEFAULT_UI_SCALE: f32 = 1.0;
const DEFAULT_GIT_COMMIT_MESSAGE_TEMPLATE: &str = "Smaragd backup\n\n{{fileList}}";

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub reopen_last_project: bool,
    pub last_project_path: Option<PathBuf>,
    pub recent_project_paths: Vec<PathBuf>,
    pub create_starter_folders: bool,

    /// Keyed by the same stable snake_case action ids the real app's
    /// `ShortcutAction::id()` uses (see frontend/src/shortcuts.ts, the
    /// single source of truth for the action catalog on this side of the
    /// port) — an id absent from this map is unbound.
    pub shortcuts: BTreeMap<String, KeyCombo>,

    pub theme_preference: ThemePreference,
    pub color_theme: Option<String>,

    pub template_date_format: String,
    pub template_time_format: String,

    pub editor_font: EditorFont,
    pub editor_font_size: f32,
    pub ui_font: EditorFont,

    pub git_integration_disabled: bool,
    /// `None` means "not yet configured" — resolved to
    /// `DEFAULT_GIT_COMMIT_MESSAGE_TEMPLATE` by `resolve_git_commit_message_template`.
    /// Added after this struct's initial port, once a real git-commit menu
    /// action actually needed it.
    pub git_commit_message_template: Option<String>,

    pub backup_enabled: bool,
    pub backup_on_open: bool,
    pub backup_on_close: bool,
    pub backup_on_manual_save: bool,
    pub backup_keep_count: u32,
    pub backup_dir: Option<PathBuf>,

    pub pomodoro_work_minutes: u32,
    pub pomodoro_short_break_minutes: u32,
    pub pomodoro_long_break_minutes: u32,
    pub pomodoro_cycles_before_long_break: u32,
    pub pomodoro_notifications_enabled: bool,

    pub typewriter_quotes: bool,
    pub show_editor_gutter: bool,
    pub multi_tab_editor: bool,

    pub ui_scale: f32,
    pub double_shift_search_disabled: bool,
}

/// `~/.config/smaragd-tauri/smaragd-tauri.toml` on Linux, the platform
/// equivalent elsewhere — deliberately a different `directories`
/// qualifier/app name than the real app's `smaragd`, not just a different
/// filename inside the same dir, so the two never share a config directory
/// at all. See the module doc for why.
pub fn config_file_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "smaragd-tauri")
        .map(|dirs| dirs.config_dir().join("smaragd-tauri.toml"))
}

impl Settings {
    pub fn load_from_path(store: &dyn ProjectStore, path: &std::path::Path) -> Self {
        let Ok(contents) = store.read_to_string(path) else {
            return Self::default();
        };
        toml::from_str(&contents).unwrap_or_default()
    }

    pub fn save_to_path(&self, store: &dyn ProjectStore, path: &std::path::Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            store.create_dir_all(parent)?;
        }
        let contents = toml::to_string_pretty(self)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
        store.write_private(path, contents.as_bytes())
    }

    /// Records `root` as the most recently opened project, most-recent-first,
    /// capped at `MAX_RECENT_PROJECTS` and de-duplicated (re-opening an
    /// already-recent project just moves it to the front rather than
    /// appearing twice).
    pub fn record_recent_project(&mut self, root: PathBuf) {
        self.last_project_path = Some(root.clone());
        self.recent_project_paths.retain(|p| p != &root);
        self.recent_project_paths.insert(0, root);
        self.recent_project_paths.truncate(MAX_RECENT_PROJECTS);
    }

    pub fn resolve_backup_keep_count(&self) -> u32 {
        if self.backup_keep_count == 0 {
            DEFAULT_BACKUP_KEEP_COUNT
        } else {
            self.backup_keep_count
        }
    }

    pub fn resolve_backup_dir(&self) -> Option<PathBuf> {
        self.backup_dir
            .clone()
            .or_else(crate::backup::default_backup_dir)
    }

    pub fn resolve_ui_scale(&self) -> f32 {
        if self.ui_scale <= 0.0 {
            DEFAULT_UI_SCALE
        } else {
            self.ui_scale
        }
    }

    pub fn resolve_git_commit_message_template(&self) -> String {
        self.git_commit_message_template
            .clone()
            .unwrap_or_else(|| DEFAULT_GIT_COMMIT_MESSAGE_TEMPLATE.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::store::NativeStore;

    #[test]
    fn round_trips_through_toml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("smaragd-tauri.toml");
        let store = NativeStore;

        let mut settings = Settings::default();
        settings.shortcuts.insert("save".into(), KeyCombo::cmd("s"));
        settings.record_recent_project(dir.path().join("project-a"));
        settings.record_recent_project(dir.path().join("project-b"));

        settings.save_to_path(&store, &path).unwrap();
        let loaded = Settings::load_from_path(&store, &path);

        assert_eq!(loaded, settings);
    }

    #[test]
    fn missing_file_falls_back_to_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("does-not-exist.toml");
        let store = NativeStore;

        assert_eq!(Settings::load_from_path(&store, &path), Settings::default());
    }

    #[test]
    fn record_recent_project_dedupes_and_caps() {
        let mut settings = Settings::default();
        for i in 0..15 {
            settings.record_recent_project(PathBuf::from(format!("/p{i}")));
        }
        assert_eq!(settings.recent_project_paths.len(), MAX_RECENT_PROJECTS);
        assert_eq!(settings.recent_project_paths[0], PathBuf::from("/p14"));

        settings.record_recent_project(PathBuf::from("/p10"));
        assert_eq!(settings.recent_project_paths[0], PathBuf::from("/p10"));
        assert_eq!(
            settings
                .recent_project_paths
                .iter()
                .filter(|p| p.as_path() == std::path::Path::new("/p10"))
                .count(),
            1
        );
    }

    #[test]
    fn resolve_methods_apply_blank_means_unset_convention() {
        let settings = Settings::default();
        assert_eq!(
            settings.resolve_backup_keep_count(),
            DEFAULT_BACKUP_KEEP_COUNT
        );
        assert_eq!(settings.resolve_ui_scale(), DEFAULT_UI_SCALE);
    }
}
