//! Persistent app-wide preferences (as opposed to `project::ProjectMeta`, which is
//! per-project). Stored as `smaragd.toml` in the platform's standard config
//! directory — `~/.config/smaragd` on Linux, `~/Library/Application Support/smaragd`
//! on macOS, `%APPDATA%\smaragd\config` on Windows — via the `directories` crate.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::editor_font::EditorFont;
use crate::shortcuts::{ShortcutAction, ShortcutMap};

/// egui's own default zoom factor — used when `Settings::ui_scale` is
/// unconfigured (`0.0`), i.e. no change from `native_pixels_per_point`.
const DEFAULT_UI_SCALE: f32 = 1.0;

/// How many entries `Settings::recent_project_paths` keeps before dropping the
/// oldest — enough to be useful without the File menu's submenu growing
/// unwieldy.
const MAX_RECENT_PROJECTS: usize = 10;

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

/// Where cards with no (or a dangling) linked document — "unplaced" cards — sit in
/// the Story Grid view (`ui::story_grid_panel`), relative to the cards that do
/// resolve to a document and so have a manuscript position. An app-wide preference
/// (unlike the cards themselves, which are per-project) since it's purely about how
/// you like to read the grid, not something tied to any one manuscript.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum UnplacedCardsPosition {
    #[default]
    Top,
    Bottom,
}

impl UnplacedCardsPosition {
    pub const ALL: [UnplacedCardsPosition; 2] = [Self::Top, Self::Bottom];

    pub fn label(self) -> &'static str {
        match self {
            Self::Top => "Top",
            Self::Bottom => "Bottom",
        }
    }
}

/// A Story Grid column (`ui::story_grid_panel`). Lives here rather than in the ui
/// panel file for the same reason `UnplacedCardsPosition` does: it's a persisted
/// view preference, and `Settings` is where those live. Column sizing (an
/// `egui_extras` concern) stays out of this struct — that's resolved in the panel
/// itself, keyed by variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum StoryGridColumn {
    Index,
    Scene,
    Document,
    Pov,
    Words,
    Cause,
    Effect,
    WhyItMatters,
    Realization,
    AndSo,
    PriorBelief,
    NewBelief,
    ValueShift,
    SubplotTags,
}

impl StoryGridColumn {
    pub const ALL: [StoryGridColumn; 14] = [
        Self::Index,
        Self::Scene,
        Self::Document,
        Self::Pov,
        Self::Words,
        Self::Cause,
        Self::Effect,
        Self::WhyItMatters,
        Self::Realization,
        Self::AndSo,
        Self::PriorBelief,
        Self::NewBelief,
        Self::ValueShift,
        Self::SubplotTags,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Index => "#",
            Self::Scene => "Scene",
            Self::Document => "Document",
            Self::Pov => "POV",
            Self::Words => "Words",
            Self::Cause => "Cause",
            Self::Effect => "Effect",
            Self::WhyItMatters => "Why It Matters",
            Self::Realization => "Realization",
            Self::AndSo => "And So",
            Self::PriorBelief => "Prior Belief",
            Self::NewBelief => "New Belief",
            Self::ValueShift => "Value Shift",
            Self::SubplotTags => "Tags",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Reopen `last_project_path` automatically on launch.
    pub reopen_last_project: bool,
    /// The most recently opened project folder, tracked regardless of
    /// `reopen_last_project` so toggling the setting on later works immediately.
    pub last_project_path: Option<PathBuf>,
    /// Every project folder opened recently, most-recent-first and capped at
    /// `MAX_RECENT_PROJECTS` — backs `File > Recent Projects`. Distinct from
    /// `last_project_path` above (which drives auto-reopen-on-launch and is a
    /// single path, not a list); updated alongside it in `set_project`. Not
    /// pruned when a folder stops existing — same "let the open attempt fail
    /// with a normal error toast" handling `open_project`'s own missing-marker
    /// case already uses, rather than silently vanishing an entry the user
    /// might still recognize (e.g. an unmounted external drive).
    pub recent_project_paths: Vec<PathBuf>,
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
    /// strftime-style format string for `${{date}}` in "New From Template"
    /// substitution (see `templates::substitute`). Blank means "not yet
    /// configured" — `templates::format_date` falls back to
    /// `templates::DEFAULT_DATE_FORMAT` rather than emitting an empty string, the
    /// same fallback it also applies to a format that fails to render at all.
    pub template_date_format: String,
    /// The font the Editor renders in (see `editor_font::EditorFont`). The
    /// Preview tab does *not* use this — its body font comes from whichever
    /// `export::style::TypesetStyle` is selected there instead, so Preview
    /// always shows what will actually get exported.
    pub editor_font: EditorFont,
    /// Body text size (points) for the Editor. `0.0` (this struct's derived
    /// `Default`, and TOML's own implicit default for a missing float key)
    /// means "not yet configured" — resolved to `editor_font::DEFAULT_FONT_SIZE`
    /// at the point of use (`editor_font::resolve_size`) rather than fought
    /// with a custom `Default` impl, the same blank-means-unset convention
    /// `template_date_format` above already uses. Like `editor_font`, the
    /// Preview tab doesn't use this — see its doc comment.
    pub editor_font_size: f32,
    /// The font used for the rest of the UI chrome — menus, the Binder tree,
    /// buttons, headings, the Settings window itself — everywhere that isn't
    /// the Editor/Preview body text (`editor_font` above) or an export's own
    /// typeset style. Defaults to `EditorFont::Proportional`, egui's own
    /// built-in default face, so upgrading changes nothing until a user
    /// picks something else. See `editor_font::apply_ui_font`, which this is
    /// applied through (a global `TextStyle` remap, not a per-widget font
    /// choice like `editor_font` is).
    pub ui_font: EditorFont,
    /// Pomodoro timer durations (minutes) and long-break cadence. `0` means
    /// "not yet configured," resolved to a real default at the point of use
    /// (`pomodoro::resolve_durations`) — same blank-means-unset convention as
    /// `editor_font_size` above.
    pub pomodoro_work_minutes: u32,
    pub pomodoro_short_break_minutes: u32,
    pub pomodoro_long_break_minutes: u32,
    pub pomodoro_cycles_before_long_break: u32,
    /// Show an OS-level desktop notification (`notifications::show`) whenever
    /// a Pomodoro phase completes on its own — see
    /// `app::pomodoro::tick_pomodoro`. Off by default, same as every other
    /// field here (this whole struct's container-level `#[serde(default)]`
    /// above covers an old settings file predating this key).
    pub pomodoro_notifications_enabled: bool,
    /// Rewrite straight typewriter punctuation (`"`, `'`, `--`, `...`) into curly
    /// quotes, an em dash, and an ellipsis wherever markdown is rendered from —
    /// the Preview pane and every export format (see
    /// `markdown::apply_typewriter_quotes`). Off by default: source `.md` files
    /// keep exactly the punctuation the author typed either way, since this only
    /// transforms the parsed `Block`/`Span` tree handed to a renderer, never the
    /// file on disk.
    pub typewriter_quotes: bool,
    /// How long an error-severity toast notification (`app::Toast`) stays on
    /// screen before auto-dismissing, in seconds. `0` means "not yet
    /// configured," resolved to a real default at the point of use
    /// (`app::resolve_toast_duration`) — same blank-means-unset convention as
    /// `editor_font_size`/the Pomodoro durations above.
    pub toast_duration_secs: u32,
    /// How long a routine status-bar confirmation ("Committed", "Exported to
    /// ...") stays visible before clearing itself, in seconds. Same
    /// blank-means-unset convention as `toast_duration_secs`
    /// (`app::resolve_status_message_duration`).
    pub status_message_duration_secs: u32,
    /// Every `ShortcutAction::id()` this settings file has ever been loaded
    /// with — internal bookkeeping for `backfill_new_shortcut_defaults`, not
    /// user-facing. `shortcuts` (above) treats an action absent from its map as
    /// unbound, which is correct when the user explicitly clicked "Clear" in
    /// the Settings window (see `settings_panel.rs`), but wrong when the
    /// absence is really "this `ShortcutAction` variant didn't exist yet when
    /// this file was last saved" — both look identical as plain absence. This
    /// set is what tells them apart: an id missing from *both* `shortcuts` and
    /// here is a newly added action that should start bound to its default; one
    /// missing from `shortcuts` but present here was deliberately cleared and
    /// should stay that way.
    pub shortcuts_seen: BTreeSet<String>,
    /// A manual multiplier on top of whatever `native_pixels_per_point` the
    /// windowing backend reports (`egui::Context::set_zoom_factor`), for
    /// platforms/compositors where automatic HiDPI detection comes back wrong
    /// (reported: a tiny, unresponsive-to-toolkit-scaling UI on some
    /// Wayland/Hyprland setups — winit reads the compositor's advertised output
    /// scale, not desktop-toolkit env vars like `GDK_SCALE`, and can also fall
    /// back silently to unscaled XWayland). `0.0` means "not yet configured,"
    /// resolved to `1.0` (no change from today's behavior) at the point of use
    /// (`app::resolve_ui_scale`) — same blank-means-unset convention as
    /// `editor_font_size`. Harmless to leave untouched on any platform: it's a
    /// pure multiplier on top of whatever the OS already reports, not a
    /// replacement for it.
    pub ui_scale: f32,
    /// Where unplaced cards sit in the Story Grid view — see
    /// [`UnplacedCardsPosition`].
    pub unplaced_story_cards_position: UnplacedCardsPosition,
    /// The user's chosen Story Grid column order — see [`StoryGridColumn`]. Empty
    /// (this struct's `Default`) means "no custom order yet," resolved to
    /// `StoryGridColumn::ALL`'s built-in order by `resolved_story_grid_column_order`
    /// — same blank-means-unset convention `template_date_format` uses.
    pub story_grid_column_order: Vec<StoryGridColumn>,
    /// Story Grid columns the user has hidden via its "Columns" menu — a column
    /// absent from this set is shown. See [`StoryGridColumn`].
    pub story_grid_hidden_columns: BTreeSet<StoryGridColumn>,
}

/// The full path to the settings file, e.g. `~/.config/smaragd/smaragd.toml` on
/// Linux. `None` if the platform's config directory can't be determined.
pub fn config_file_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "smaragd")
        .map(|dirs| dirs.config_dir().join("smaragd.toml"))
}

/// The full path to the persisted dock layout (which tabs are open, and how
/// they're split/floated), e.g. `~/.config/smaragd/dock_layout.json` on Linux.
/// Kept in its own file rather than as a field on `Settings`: `egui_dock::DockState`
/// is a recursive tree of nodes, which doesn't round-trip through TOML (its
/// derived `Serialize` impl emits constructs — like a sequence of tables mixed with
/// non-table values — that TOML's format can't represent), but does through JSON.
/// `None` if the platform's config directory can't be determined.
pub fn dock_layout_file_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "smaragd")
        .map(|dirs| dirs.config_dir().join("dock_layout.json"))
}

/// The full path to the user's named, saved dock layouts (Window > Save Current
/// Layout…/Layouts), e.g. `~/.config/smaragd/saved_layouts.json` on Linux. Kept
/// separate from `dock_layout_file_path` — that one tracks only the single
/// currently-active layout, persisted on shutdown; this one is a named map,
/// persisted immediately whenever the user explicitly saves one. Same JSON (not
/// TOML) reasoning as `dock_layout_file_path`. `None` if the platform's config
/// directory can't be determined.
pub fn saved_layouts_file_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "smaragd")
        .map(|dirs| dirs.config_dir().join("saved_layouts.json"))
}

impl Settings {
    /// Load settings from `path`, falling back to defaults if the file is missing or
    /// its contents can't be parsed — a first launch or a hand-edited file should
    /// never prevent the app from starting.
    pub fn load_from_path(path: &Path) -> Self {
        let mut settings: Self = std::fs::read_to_string(path)
            .ok()
            .and_then(|contents| toml::from_str(&contents).ok())
            .unwrap_or_default();
        settings.backfill_new_shortcut_defaults();
        settings
    }

    /// Bind any `ShortcutAction` this settings file has never seen before to its
    /// `default_shortcut()` — see `shortcuts_seen`'s doc comment for why plain
    /// absence from `shortcuts` isn't enough to tell "brand new action" apart
    /// from "user cleared it." Called once after loading (both from a real file
    /// and the empty-defaults fallback, so a fresh install's `shortcuts_seen`
    /// starts fully populated too and a Clear on launch #1 is never mistaken for
    /// "new" on launch #2).
    fn backfill_new_shortcut_defaults(&mut self) {
        for action in ShortcutAction::ALL {
            if self.shortcuts_seen.insert(action.id().to_string())
                && self.shortcuts.get(*action).is_none()
            {
                self.shortcuts.set(*action, Some(action.default_shortcut()));
            }
        }
    }

    /// Record `path` as the most recently opened project: moved to the front if
    /// already present rather than duplicated, and the list capped at
    /// `MAX_RECENT_PROJECTS`, oldest dropped first.
    pub fn record_recent_project(&mut self, path: &Path) {
        self.recent_project_paths.retain(|p| p != path);
        self.recent_project_paths.insert(0, path.to_path_buf());
        self.recent_project_paths.truncate(MAX_RECENT_PROJECTS);
    }

    /// Whether smaragd has never opened a project before — no `last_project_path`
    /// and no `recent_project_paths` entries. Both are set together in
    /// `SmaragdApp::set_project`, so either alone suffices, but checking both
    /// guards against a hand-edited settings file clearing just one. Used to
    /// decide whether "New Project" should default to the richer World-Building
    /// template instead of Blank (see `SmaragdApp::start_new_project`).
    pub fn is_first_launch(&self) -> bool {
        self.last_project_path.is_none() && self.recent_project_paths.is_empty()
    }

    /// Resolve `ui_scale`'s blank-means-unset (`0.0`) convention to an actual
    /// `egui::Context::set_zoom_factor` multiplier — same shape as
    /// `editor_font::resolve_size`/`pomodoro::resolve_durations`.
    pub fn resolve_ui_scale(&self) -> f32 {
        if self.ui_scale > 0.0 {
            self.ui_scale
        } else {
            DEFAULT_UI_SCALE
        }
    }

    /// The full Story Grid column order, every `StoryGridColumn` present — the
    /// "Columns" menu needs to list hidden columns too, only
    /// `story_grid_hidden_columns` controls visibility. Appends any variant
    /// missing from the saved order (covers both "never configured," where this
    /// starts empty and every variant is appended in `StoryGridColumn::ALL`'s
    /// order, and a column added in a later smaragd version) at the end.
    pub fn resolved_story_grid_column_order(&self) -> Vec<StoryGridColumn> {
        let mut order = self.story_grid_column_order.clone();
        for kind in StoryGridColumn::ALL {
            if !order.contains(&kind) {
                order.push(kind);
            }
        }
        order
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
        assert!(settings.recent_project_paths.is_empty());
    }

    #[test]
    fn is_first_launch_is_true_only_before_any_project_has_ever_been_opened() {
        let mut settings = Settings::default();
        assert!(settings.is_first_launch());

        settings.record_recent_project(Path::new("/home/author/first"));
        assert!(
            !settings.is_first_launch(),
            "recording a recent project should end first-launch status even without last_project_path set"
        );
    }

    #[test]
    fn is_first_launch_is_false_once_last_project_path_is_set() {
        let settings = Settings {
            last_project_path: Some(PathBuf::from("/home/author/first")),
            ..Default::default()
        };
        assert!(!settings.is_first_launch());
    }

    #[test]
    fn record_recent_project_inserts_at_the_front() {
        let mut settings = Settings::default();
        settings.record_recent_project(Path::new("/home/author/first"));
        settings.record_recent_project(Path::new("/home/author/second"));
        assert_eq!(
            settings.recent_project_paths,
            vec![
                PathBuf::from("/home/author/second"),
                PathBuf::from("/home/author/first"),
            ]
        );
    }

    #[test]
    fn record_recent_project_moves_an_existing_entry_to_the_front_instead_of_duplicating() {
        let mut settings = Settings::default();
        settings.record_recent_project(Path::new("/home/author/first"));
        settings.record_recent_project(Path::new("/home/author/second"));
        settings.record_recent_project(Path::new("/home/author/first"));
        assert_eq!(
            settings.recent_project_paths,
            vec![
                PathBuf::from("/home/author/first"),
                PathBuf::from("/home/author/second"),
            ]
        );
    }

    #[test]
    fn record_recent_project_caps_the_list_and_drops_the_oldest() {
        let mut settings = Settings::default();
        for n in 0..MAX_RECENT_PROJECTS + 3 {
            settings.record_recent_project(&PathBuf::from(format!("/home/author/project-{n}")));
        }
        assert_eq!(settings.recent_project_paths.len(), MAX_RECENT_PROJECTS);
        assert_eq!(
            settings.recent_project_paths[0],
            PathBuf::from(format!("/home/author/project-{}", MAX_RECENT_PROJECTS + 2))
        );
        assert!(
            !settings
                .recent_project_paths
                .contains(&PathBuf::from("/home/author/project-0"))
        );
    }

    #[test]
    fn toast_and_status_message_durations_default_to_unconfigured() {
        let settings = Settings::default();
        assert_eq!(settings.toast_duration_secs, 0);
        assert_eq!(settings.status_message_duration_secs, 0);
    }

    #[test]
    fn resolve_ui_scale_falls_back_to_the_default_when_unconfigured() {
        let settings = Settings {
            ui_scale: 0.0,
            ..Default::default()
        };
        assert_eq!(settings.resolve_ui_scale(), DEFAULT_UI_SCALE);
    }

    #[test]
    fn resolve_ui_scale_uses_the_configured_value() {
        let settings = Settings {
            ui_scale: 1.5,
            ..Default::default()
        };
        assert_eq!(settings.resolve_ui_scale(), 1.5);
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
        let path = dir.path().join("smaragd.toml");
        std::fs::write(&path, "reopen_last_project = true\n").unwrap();

        let loaded = Settings::load_from_path(&path);

        assert_eq!(loaded.theme_preference, egui::ThemePreference::System);
    }

    #[test]
    fn defaults_to_unplaced_cards_on_top() {
        assert_eq!(
            Settings::default().unplaced_story_cards_position,
            UnplacedCardsPosition::Top
        );
    }

    #[test]
    fn settings_file_without_an_unplaced_story_cards_position_loads_the_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("smaragd.toml");
        std::fs::write(&path, "reopen_last_project = true\n").unwrap();

        let loaded = Settings::load_from_path(&path);

        assert_eq!(
            loaded.unplaced_story_cards_position,
            UnplacedCardsPosition::Top
        );
    }

    #[test]
    fn unplaced_story_cards_position_persists_across_a_reload() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("smaragd.toml");
        let settings = Settings {
            unplaced_story_cards_position: UnplacedCardsPosition::Bottom,
            ..Default::default()
        };
        settings.save_to_path(&path).unwrap();

        let loaded = Settings::load_from_path(&path);

        assert_eq!(
            loaded.unplaced_story_cards_position,
            UnplacedCardsPosition::Bottom
        );
    }

    #[test]
    fn resolved_story_grid_column_order_defaults_to_all_columns_when_unconfigured() {
        let settings = Settings::default();

        assert_eq!(
            settings.resolved_story_grid_column_order(),
            StoryGridColumn::ALL.to_vec()
        );
    }

    #[test]
    fn resolved_story_grid_column_order_appends_a_column_added_after_this_file_was_saved() {
        let mut order = StoryGridColumn::ALL.to_vec();
        let missing = order.remove(3); // Simulates a column that didn't exist yet.
        let settings = Settings {
            story_grid_column_order: order,
            ..Default::default()
        };

        let resolved = settings.resolved_story_grid_column_order();

        assert_eq!(resolved.last(), Some(&missing));
        assert_eq!(resolved.len(), StoryGridColumn::ALL.len());
    }

    #[test]
    fn settings_file_without_a_color_theme_loads_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("smaragd.toml");
        std::fs::write(&path, "reopen_last_project = true\n").unwrap();

        let loaded = Settings::load_from_path(&path);

        assert_eq!(loaded.color_theme, None);
    }

    #[test]
    fn settings_round_trip_through_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("smaragd.toml");
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
        // Every action already "seen," so `load_from_path`'s
        // `backfill_new_shortcut_defaults` is a no-op and this round-trips
        // exactly rather than picking up freshly backfilled entries.
        let shortcuts_seen = crate::shortcuts::ShortcutAction::ALL
            .iter()
            .map(|action| action.id().to_string())
            .collect();
        let settings = Settings {
            reopen_last_project: true,
            last_project_path: Some(PathBuf::from("/home/author/my-novel")),
            recent_project_paths: vec![
                PathBuf::from("/home/author/my-novel"),
                PathBuf::from("/home/author/old-draft"),
            ],
            create_starter_folders: true,
            shortcuts,
            theme_preference: egui::ThemePreference::Dark,
            color_theme: Some("dracula".to_string()),
            plugin_shortcut_overrides,
            template_date_format: "%d %B %Y".to_string(),
            editor_font: EditorFont::LibertinusSerif,
            editor_font_size: 16.0,
            ui_font: EditorFont::AtkinsonHyperlegible,
            pomodoro_work_minutes: 50,
            pomodoro_short_break_minutes: 10,
            pomodoro_long_break_minutes: 30,
            pomodoro_cycles_before_long_break: 3,
            pomodoro_notifications_enabled: true,
            typewriter_quotes: true,
            toast_duration_secs: 10,
            status_message_duration_secs: 12,
            shortcuts_seen,
            ui_scale: 1.25,
            unplaced_story_cards_position: UnplacedCardsPosition::Bottom,
            story_grid_column_order: vec![StoryGridColumn::Words, StoryGridColumn::Scene],
            story_grid_hidden_columns: BTreeSet::from([
                StoryGridColumn::Cause,
                StoryGridColumn::Effect,
            ]),
        };

        settings.save_to_path(&path).unwrap();
        let loaded = Settings::load_from_path(&path);

        assert_eq!(loaded, settings);
    }

    #[test]
    fn settings_file_without_a_shortcuts_table_loads_default_shortcuts() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("smaragd.toml");
        std::fs::write(&path, "reopen_last_project = true\n").unwrap();

        let loaded = Settings::load_from_path(&path);

        assert_eq!(loaded.shortcuts, ShortcutMap::default());
    }

    /// The expected result of loading a missing/corrupt settings file:
    /// `Settings::default()` with every `ShortcutAction` marked as already
    /// "seen" — `load_from_path` runs `backfill_new_shortcut_defaults` on
    /// *every* path, including this fallback one, so a fresh install's
    /// `shortcuts_seen` starts fully populated too (see that method's doc
    /// comment for why).
    fn default_settings_after_load() -> Settings {
        let mut settings = Settings::default();
        settings.backfill_new_shortcut_defaults();
        settings
    }

    #[test]
    fn missing_settings_file_falls_back_to_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("does-not-exist.toml");

        assert_eq!(
            Settings::load_from_path(&path),
            default_settings_after_load()
        );
    }

    #[test]
    fn corrupt_settings_file_falls_back_to_default_instead_of_panicking() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("smaragd.toml");
        std::fs::write(&path, "not valid { toml").unwrap();

        assert_eq!(
            Settings::load_from_path(&path),
            default_settings_after_load()
        );
    }

    #[test]
    fn loading_an_older_settings_file_backfills_a_newly_added_action_to_its_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("smaragd.toml");
        // Simulates a settings.toml saved by an older build that predates
        // `ShortcutAction::ToggleWordCount` — the `[shortcuts]` table exists
        // but has no `toggle_word_count` key at all, the exact bug scenario
        // (a codebase-added default silently reads as "unbound" otherwise).
        std::fs::write(&path, "[shortcuts]\nsave = \"Ctrl+S\"\n").unwrap();

        let loaded = Settings::load_from_path(&path);

        assert_eq!(
            loaded.shortcuts.get(ShortcutAction::ToggleWordCount),
            Some(ShortcutAction::ToggleWordCount.default_shortcut())
        );
    }

    #[test]
    fn loading_a_settings_file_does_not_resurrect_a_deliberately_cleared_shortcut() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("smaragd.toml");
        let mut settings = Settings::default();
        // The Settings window's "Clear" button (`ShortcutMap::set(_, None)`)
        // removes the key outright — indistinguishable, by itself, from an
        // action this file has simply never encountered. `shortcuts_seen` is
        // what tells them apart: it must already list `save` for the clear to
        // stick across a reload.
        settings.shortcuts.set(ShortcutAction::Save, None);
        settings
            .shortcuts_seen
            .insert(ShortcutAction::Save.id().to_string());
        settings.save_to_path(&path).unwrap();

        let loaded = Settings::load_from_path(&path);

        assert_eq!(loaded.shortcuts.get(ShortcutAction::Save), None);
    }

    #[test]
    fn save_creates_missing_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir
            .path()
            .join("nested")
            .join("config")
            .join("smaragd.toml");

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
