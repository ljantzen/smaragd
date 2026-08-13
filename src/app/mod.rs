use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

mod backup;
mod bookmarks;
mod collab;
mod dictionary_download;
mod dock;
mod dock_tab_viewer;
mod dock_tabs;
mod document_history;
mod export;
mod find_replace;
mod git;
mod import;
mod menu_bar;
mod menu_nav;
mod pomodoro;
mod project_lifecycle;
mod prompt;
mod refresh;
mod settings_persist;
mod streak_events;
mod toast;
mod word_count_events;
use backup::BackupTrigger;
use dock::{
    DockTab, capture_floating_window_positions, default_dock_state, ensure_editor_tab_present,
};
use dock_tab_viewer::{AppTabViewer, DockAction};
use document_history::DocumentHistory;
use export::ExportState;
use git::GitOperation;
use menu_nav::{nav_submenu, top_menu_button};
use prompt::{PendingPrompt, PromptAction};
use refresh::{
    BacklinksState, DocumentStatusCache, MetadataState, MetadataTarget, TagsState, WordCountState,
};
use toast::Toast;

use crate::collab::{CollabRole, CollabSession, SessionUpdate};
use crate::editor::EditorState;
use crate::frontmatter::DocumentMeta;
use crate::project::{BacklinkEntry, BinderColorMode, LoadError, Project, RestoreError};
use crate::search::{self, SearchScope};
use crate::settings::PluginShortcutOverride;
use crate::settings::Settings;
use crate::shortcuts::{ShortcutAction, ShortcutTarget, sorted_by_specificity};
use crate::ui;
use crate::ui::WikilinkActivation;
use crate::ui::backlinks_panel::BacklinksEvent;
use crate::ui::binder_panel::BinderEvent;
use crate::ui::collab_panel::{CollabPanelEvent, CollabStatus};
use crate::ui::command_prompt::{
    Command, CommandPromptEvent, CommandPromptState, DarkModeChoice, GitCommand,
};
use crate::ui::corkboard_panel::{CardDraft, CardEditorOutcome, CorkboardEvent};
use crate::ui::editor_panel::EditorEvent;
use crate::ui::find_replace_panel::{FindReplaceEvent, FindReplaceState};
use crate::ui::metadata_panel::MetadataDraft;
use crate::ui::name_prompt::{NamePromptOutcome, NamePromptState};
use crate::ui::story_grid_panel::StoryGridEvent;

pub struct SmaragdApp {
    project: Option<Project>,
    editor: EditorState,
    selected_path: Option<PathBuf>,
    /// Which documents have been opened this project session, in order, plus
    /// the last known cursor position in each — backs `open_document`'s
    /// history tracking and the Go Back/Go Forward navigation
    /// (`go_back_document`/`go_forward_document`). Reset whenever a project
    /// is opened or closed (`set_project`/`close_project`): paths from one
    /// project are meaningless once a different one is open.
    document_history: DocumentHistory,
    /// A quiet, routine status-bar confirmation — "Committed", "Exported to
    /// ...", and the like. Overwritten freely by dozens of call sites, and
    /// replaced (not queued) by whichever ran most recently, since these are
    /// meant to be glanced at, not necessarily read in full before the next
    /// one lands. For anything that represents an actual problem, use
    /// `push_error_toast` instead (see `Toast`) — status-bar text is easy to
    /// miss entirely (bottom of the window, gone the instant something else
    /// happens to overwrite it), which is fine for "FYI, that worked" but not
    /// for something the user actually needs to notice. Set/cleared only via
    /// `set_status_message`/`clear_status_message` — never assigned directly —
    /// so `status_message_set_at` below always stays in sync with it.
    status_message: Option<String>,
    /// When `status_message` was last set, so `clear_status_message_if_expired`
    /// knows when `Settings::status_message_duration_secs` has elapsed. `None`
    /// exactly when `status_message` is `None`.
    status_message_set_at: Option<std::time::Instant>,
    /// Error-severity notifications currently on screen — see `Toast` and
    /// `push_error_toast`/`show_toasts`.
    toasts: Vec<Toast>,
    settings: Settings,
    show_settings: bool,
    show_about: bool,
    prompt: Option<PendingPrompt>,
    recording_shortcut: Option<ShortcutTarget>,
    settings_category: ui::settings_panel::SettingsCategory,
    /// Which of the Streak dock tab's two inner tabs is showing — reset to
    /// the sensible default (`Streak` if the newly opened project already
    /// has tracking on, `Configure` otherwise) by `set_project`
    /// (`project_lifecycle.rs`) every time a project is opened; the user can
    /// freely switch away from that default afterward.
    streak_sub_tab: ui::streak_panel::StreakSubTab,
    /// The Belief Timeline tab's currently selected POV character — starts blank
    /// (defaults to the first known character the panel finds, see
    /// `ui::belief_timeline_panel::show`) and isn't reset by `set_project`, unlike
    /// `streak_sub_tab`: there's no per-project "sensible default" character to
    /// reset it to.
    belief_timeline_character: String,
    /// Where `persist_settings` writes, in place of the real
    /// `settings::config_file_path()` — always `None` in `new()` (the real
    /// app), always `Some` (a throwaway path) in `test_fixture()`. Exists
    /// specifically so a unit test that exercises `open_project`/
    /// `create_project` (which call `persist_settings` as a side effect of
    /// `set_project`) can never clobber the developer's actual
    /// `~/.config/smaragd/smaragd.toml`, the way it did before this field
    /// existed — see `persist_settings`'s doc comment.
    settings_path_override: Option<PathBuf>,
    /// `true` only for `test_fixture`-built instances. Gates every other
    /// real-world side effect `set_project` (`project_lifecycle.rs`) would
    /// otherwise trigger on every `open_project`/`create_project` call: a
    /// blocking native "Enable Git Support" dialog (`maybe_offer_git_support`
    /// — this one doesn't just write a file, it pops up on the developer's
    /// actual screen and can hang a test run until someone dismisses it),
    /// real `git init`, reloading plugins from the developer's real global
    /// plugin directory, and spawning a background word-count-recompute
    /// thread — none of which a unit test exercising the open-project flow
    /// should trigger just to check an unrelated bit of state.
    is_test_fixture: bool,
    find_replace: FindReplaceState,
    card_draft: Option<CardDraft>,
    command_prompt: CommandPromptState,
    open_document_prompt: ui::open_document_prompt::OpenDocumentPromptState,
    new_project_template_prompt: ui::new_project_template_prompt::NewProjectTemplatePromptState,
    /// Live editing buffers for the open document's frontmatter, always kept in
    /// sync with whichever document is open (see `refresh_metadata_if_needed`) —
    /// there's no "closed" state to represent here, since the Metadata dock
    /// tab's own presence in `dock_state` is what tracks visibility.
    metadata: MetadataState,
    /// A closed document's cached frontmatter `status`, keyed by path — see
    /// `DocumentStatusCache`'s doc comment for why this exists (avoiding a
    /// disk read for every visible binder row every frame).
    document_status_cache: DocumentStatusCache,
    /// Absolute paths under the open project with uncommitted git changes
    /// (staged or not, including untracked files) — see `crate::git::status`.
    /// Powers the Binder's "modified" marker (`ui::binder_panel`). Kept
    /// empty whenever git integration is off (`Settings::
    /// git_integration_enabled`/`ProjectMeta::git_enabled`) or no project is
    /// open, rather than ever going stale-but-nonempty — see
    /// `refresh_git_dirty_paths`, which every call site funnels through.
    git_dirty_paths: std::collections::HashSet<PathBuf>,
    /// Every `[[wikilink]]` elsewhere in the project pointing at the open
    /// document, kept in sync with whichever document is open (see
    /// `refresh_backlinks_if_needed`).
    backlinks: BacklinksState,
    /// The open document's tags and the project-wide tag search box, kept in
    /// sync with whichever document is open (see `refresh_tags_if_needed`).
    tags: TagsState,
    /// The open project's word count, its background-recompute machinery, and
    /// the characters-typed activity counter (see `refresh_word_count_if_needed`/
    /// `track_char_activity`).
    word_count: WordCountState,
    /// The current dock layout — which tabs are open, and whether they're docked
    /// together, split, or floating in their own window. Persisted across
    /// restarts (see `persist_dock_layout`); defaults to `default_dock_state()`.
    dock_state: egui_dock::DockState<DockTab>,
    /// Named layouts the user has explicitly saved (Window > Save Current
    /// Layout…), switchable from Window > Layouts. Keyed by name (`BTreeMap` so
    /// the menu lists them alphabetically and re-saving under an existing name
    /// naturally overwrites it) — separate from `dock_state`/`persist_dock_layout`,
    /// which only ever tracks the one currently-active, unnamed layout. Persisted
    /// immediately on save (see `persist_saved_layouts`), not deferred to
    /// shutdown, since saving one is an explicit, infrequent action the user
    /// should be able to trust actually landed on disk.
    saved_layouts: std::collections::BTreeMap<String, egui_dock::DockState<DockTab>>,
    /// A push or pull currently running on a background thread, if any — `git push`/
    /// `git pull` hit the network and can hang or run long, so they're never run
    /// synchronously on the UI thread. `None` once `poll_git_operation` has picked up
    /// its result.
    pending_git: Option<(
        GitOperation,
        std::sync::mpsc::Receiver<Result<(), crate::git::GitError>>,
    )>,
    /// A real dictionary download (see `spellcheck::download_dictionary`)
    /// currently running on a background thread, if any — real network I/O, so
    /// it never runs synchronously on the UI thread, mirroring `pending_git`.
    /// `None` once `poll_dictionary_download` has picked up its result.
    pending_dictionary_download: Option<(
        crate::spellcheck::SpellCheckLanguage,
        std::sync::mpsc::Receiver<Result<(), String>>,
    )>,
    /// Loaded `.rhai` plugins — the global directory always, plus the open
    /// project's own `.smaragd/plugins` if it has opted in (see
    /// `ProjectMeta::plugins_enabled`). Rebuilt by `reload_plugins`.
    plugin_engine: crate::plugins::PluginEngine,
    /// The currently-active shortcut for each plugin command that has one —
    /// `plugin_engine`'s declared defaults layered with `settings`'
    /// `plugin_shortcut_overrides` and de-conflicted against built-in and other
    /// plugin shortcuts. Recomputed by `compute_effective_plugin_shortcuts`
    /// whenever either input changes (a plugin reload, or a Settings edit).
    plugin_shortcuts: Vec<(String, egui::KeyboardShortcut)>,
    /// Every selectable color theme: the 15 built-ins plus whatever `*.toml` files
    /// are in `color_theme::global_themes_dir()`. Rebuilt by `reload_color_themes`.
    color_themes: Vec<crate::color_theme::ColorTheme>,
    /// Every selectable project template: the 5 built-ins plus whatever custom
    /// templates are in `project_template::global_project_templates_dir()`.
    /// Rebuilt by `reload_project_templates`.
    project_templates: Vec<crate::project_template::ProjectTemplate>,
    /// The open Export dialog, if any — see `ExportState`.
    export: Option<ExportState>,
    /// Every selectable typesetting style: the 12 built-ins plus whatever
    /// `*.toml` files are in `export::style::global_styles_dir()`. Rebuilt by
    /// `reload_typeset_styles`.
    typeset_styles: Vec<crate::export::style::TypesetStyle>,
    /// Every custom font name a loaded style's `font_file` successfully
    /// registered with egui (see `editor_font::install_custom_fonts`) —
    /// rebuilt alongside `typeset_styles` by `reload_typeset_styles`. The
    /// Preview tab only ever trusts a font name in this list; see
    /// `ui::markdown_preview::resolve_family`.
    custom_font_names: Vec<String>,
    /// Work/break interval state — ticked once per frame regardless of whether
    /// the Pomodoro dock tab is open (see `tick_pomodoro`), so it keeps
    /// running while closed.
    pomodoro: crate::pomodoro::PomodoroState,
    /// One-shot flag set by `ShortcutAction::ToggleBinderFocus` and consumed (via
    /// `std::mem::take`) the same frame it's rendered — `binder_panel::show` needs
    /// it to know whether to grab keyboard focus this frame, but the dock's
    /// `AppTabViewer` only borrows `&mut self` fields it's handed, not `self`
    /// itself, so it can't read a shortcut result directly.
    focus_binder_requested: bool,
    /// Distraction-free writing mode (View > Focus Mode / `ShortcutAction::
    /// ToggleFocusMode`): full screen, editor only (no Binder/Backlinks/
    /// Metadata/Preview/Corkboard/menu bar/status bar), with the current
    /// paragraph highlighted and everything else dimmed — see
    /// `set_focus_mode` and the `focus_mode` branch in `ui()`.
    focus_mode: bool,
    /// The active peer-to-peer collaboration session, if any — hosting or
    /// joined, scoped to whichever document is open when it starts (see
    /// `start_collab_host`/`start_collab_join`/`end_collab_session`).
    /// `None` whenever no session is running.
    collab: Option<CollabSession>,
    /// The "unsaved changes" dialog shown when the app is asked to close while
    /// `editor.dirty` or an open, uncommitted `card_draft` — see
    /// `has_unsaved_changes` and the `close_requested()` handling in `ui()`.
    exit_confirm: ui::exit_confirm_prompt::ExitConfirmState,
}

impl SmaragdApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        crate::editor_font::install(&cc.egui_ctx);

        let settings = crate::settings::config_file_path()
            .map(|path| Settings::load_from_path(&path))
            .unwrap_or_default();
        cc.egui_ctx.set_theme(settings.theme_preference);
        cc.egui_ctx.set_zoom_factor(settings.resolve_ui_scale());
        crate::editor_font::apply_ui_font(&cc.egui_ctx, settings.ui_font);
        let initial_pomodoro_durations = crate::pomodoro::resolve_durations(&settings);
        // Match the editor's background to the surrounding chrome instead of egui's
        // default `extreme_bg_color`, which renders TextEdit widgets noticeably darker
        // (dark mode) than the panels around them.
        for theme in [egui::Theme::Dark, egui::Theme::Light] {
            cc.egui_ctx.style_mut_of(theme, |style| {
                style.visuals.text_edit_bg_color = Some(style.visuals.panel_fill);
                crate::color_theme::show_input_frame(&mut style.visuals);
            });
        }

        let mut app = Self {
            project: None,
            editor: EditorState::default(),
            selected_path: None,
            document_history: DocumentHistory::default(),
            status_message: None,
            status_message_set_at: None,
            toasts: Vec::new(),
            settings,
            show_settings: false,
            show_about: false,
            prompt: None,
            recording_shortcut: None,
            settings_category: ui::settings_panel::SettingsCategory::General,
            streak_sub_tab: ui::streak_panel::StreakSubTab::Configure,
            belief_timeline_character: String::new(),
            settings_path_override: None,
            is_test_fixture: false,
            find_replace: FindReplaceState::default(),
            card_draft: None,
            command_prompt: CommandPromptState::default(),
            open_document_prompt: ui::open_document_prompt::OpenDocumentPromptState::default(),
            new_project_template_prompt:
                ui::new_project_template_prompt::NewProjectTemplatePromptState::default(),
            metadata: MetadataState::default(),
            document_status_cache: DocumentStatusCache::default(),
            git_dirty_paths: std::collections::HashSet::new(),
            backlinks: BacklinksState::default(),
            tags: TagsState::default(),
            word_count: WordCountState::default(),
            dock_state: Self::load_dock_state(),
            saved_layouts: Self::load_saved_layouts(),
            pending_git: None,
            pending_dictionary_download: None,
            plugin_engine: crate::plugins::PluginEngine::default(),
            plugin_shortcuts: Vec::new(),
            color_themes: Vec::new(),
            project_templates: Vec::new(),
            export: None,
            typeset_styles: Vec::new(),
            custom_font_names: Vec::new(),
            pomodoro: crate::pomodoro::PomodoroState::new(&initial_pomodoro_durations),
            focus_binder_requested: false,
            focus_mode: false,
            collab: None,
            exit_confirm: ui::exit_confirm_prompt::ExitConfirmState::default(),
        };
        app.reload_typeset_styles(&cc.egui_ctx);
        app.reload_project_templates();

        // Before applying the persisted theme below, which needs `color_themes`
        // populated to find it by id.
        app.reload_color_themes(&cc.egui_ctx);

        if let Some(id) = &app.settings.color_theme
            && let Some(theme) = crate::color_theme::find(&app.color_themes, id)
        {
            crate::color_theme::apply(&cc.egui_ctx, theme);
        }

        // Before `open_project` below, which — if the reopened project has its own
        // plugins enabled — needs `reload_plugins` to pick up its directory too;
        // calling it here first means global-only plugins are loaded even when
        // there's no project to reopen.
        app.reload_plugins();

        if app.settings.reopen_last_project
            && let Some(path) = app.settings.last_project_path.clone()
        {
            app.open_project(&cc.egui_ctx, &path);
        }

        app
    }

    /// A minimal `SmaragdApp` for unit tests of routing/state logic (command
    /// execution, event handlers, the word-count/char-activity trackers) —
    /// unlike `new()`, needs no live `eframe::CreationContext` and touches no
    /// real config directory: no font/image-loader installation, no
    /// persisted settings/dock-layout/saved-layouts loaded from disk, no
    /// disk-scanned themes/plugins/templates. Every field starts at the same
    /// value `new()` gives it before those disk/context-dependent steps run.
    #[cfg(test)]
    fn test_fixture() -> Self {
        let settings = Settings::default();
        let pomodoro_durations = crate::pomodoro::resolve_durations(&settings);
        Self {
            project: None,
            editor: EditorState::default(),
            selected_path: None,
            document_history: DocumentHistory::default(),
            status_message: None,
            status_message_set_at: None,
            toasts: Vec::new(),
            settings,
            show_settings: false,
            show_about: false,
            prompt: None,
            recording_shortcut: None,
            settings_category: ui::settings_panel::SettingsCategory::General,
            streak_sub_tab: ui::streak_panel::StreakSubTab::Configure,
            belief_timeline_character: String::new(),
            // Always set, unconditionally — see this field's doc comment.
            // Any test built on `test_fixture` must never be able to reach
            // the developer's real `~/.config/smaragd/smaragd.toml`, even
            // if the test author never thinks about persistence at all.
            settings_path_override: Some(std::env::temp_dir().join(format!(
                "smaragd-test-settings-{}.toml",
                uuid::Uuid::new_v4()
            ))),
            is_test_fixture: true,
            find_replace: FindReplaceState::default(),
            card_draft: None,
            command_prompt: CommandPromptState::default(),
            open_document_prompt: ui::open_document_prompt::OpenDocumentPromptState::default(),
            new_project_template_prompt:
                ui::new_project_template_prompt::NewProjectTemplatePromptState::default(),
            metadata: MetadataState::default(),
            document_status_cache: DocumentStatusCache::default(),
            git_dirty_paths: std::collections::HashSet::new(),
            backlinks: BacklinksState::default(),
            tags: TagsState::default(),
            word_count: WordCountState::default(),
            dock_state: default_dock_state(),
            saved_layouts: std::collections::BTreeMap::new(),
            pending_git: None,
            pending_dictionary_download: None,
            plugin_engine: crate::plugins::PluginEngine::default(),
            plugin_shortcuts: Vec::new(),
            color_themes: Vec::new(),
            project_templates: Vec::new(),
            export: None,
            typeset_styles: Vec::new(),
            custom_font_names: Vec::new(),
            pomodoro: crate::pomodoro::PomodoroState::new(&pomodoro_durations),
            focus_binder_requested: false,
            focus_mode: false,
            collab: None,
            exit_confirm: ui::exit_confirm_prompt::ExitConfirmState::default(),
        }
    }

    /// The plugin directories that currently apply: the global directory always,
    /// plus the open project's own `.smaragd/plugins` if it has opted in.
    fn plugin_dirs(&self) -> Vec<PathBuf> {
        let mut dirs: Vec<PathBuf> = crate::plugins::global_plugins_dir().into_iter().collect();
        if let Some(project) = &self.project
            && project.meta.plugins_enabled
        {
            dirs.push(project.root.join(".smaragd").join("plugins"));
        }
        dirs
    }

    /// Rebuild `plugin_engine` from whichever directories currently apply (see
    /// `plugin_dirs`) — called on startup, whenever a project is opened, and from
    /// the "Reload Plugins" menu item so a plugin author can iterate without
    /// restarting the app. Any load errors (a script that failed to compile/run, a
    /// `:` command name collision) become the status message.
    fn reload_plugins(&mut self) {
        let dirs = self.plugin_dirs();
        let dir_refs: Vec<&Path> = dirs.iter().map(PathBuf::as_path).collect();
        let project_root = self.project.as_ref().map(|project| project.root.as_path());
        let (engine, errors) = crate::plugins::load(&dir_refs, project_root);
        self.plugin_engine = engine;
        self.plugin_shortcuts = self.compute_effective_plugin_shortcuts();
        if !errors.is_empty() {
            self.push_error_toast(errors.join("; "));
        }
    }

    /// Rebuild `color_themes` from the built-ins plus whatever `*.toml` files are
    /// currently in `color_theme::global_themes_dir()` — called on startup, and
    /// from the View > Theme menu's "Reload Custom Themes" so a theme author can
    /// iterate without restarting the app. If the currently active theme no
    /// longer resolves afterward (e.g. the file being edited now has a syntax
    /// error), falls back to the default (no theme) rather than leaving a now-
    /// orphaned palette applied with nothing in the menu showing as selected.
    fn reload_color_themes(&mut self, ctx: &egui::Context) {
        let themes_dir = crate::color_theme::global_themes_dir();
        let dirs: Vec<&Path> = themes_dir.as_deref().into_iter().collect();
        let (themes, errors) = crate::color_theme::load(&dirs);
        self.color_themes = themes;
        if !errors.is_empty() {
            self.push_error_toast(errors.join("; "));
        }
        if let Some(id) = self.settings.color_theme.clone()
            && crate::color_theme::find(&self.color_themes, &id).is_none()
        {
            crate::color_theme::reset(ctx);
            self.settings.color_theme = None;
            self.persist_settings();
        }
    }

    /// Rebuild `project_templates` from the 5 built-ins plus whatever custom
    /// templates are currently in `project_template::global_project_templates_dir()`
    /// — called on startup, and after `save_project_as_template` succeeds so a
    /// newly saved template is immediately selectable without restarting.
    fn reload_project_templates(&mut self) {
        let templates_dir = crate::project_template::global_project_templates_dir();
        let dirs: Vec<&Path> = templates_dir.as_deref().into_iter().collect();
        let (templates, errors) = crate::project_template::load(&dirs);
        self.project_templates = templates;
        if !errors.is_empty() {
            self.push_error_toast(errors.join("; "));
        }
    }

    /// Reload the selectable typesetting styles (`self.typeset_styles`): the 12
    /// built-ins plus every `*.toml` file in `export::style::global_styles_dir()`.
    /// Also (re)registers any custom font a loaded style names via `font_file`
    /// with `ctx` (`self.custom_font_names`) — see `editor_font::install_custom_fonts`.
    /// Called at startup and from the Export dialog's "Reload Styles" action.
    fn reload_typeset_styles(&mut self, ctx: &egui::Context) {
        let styles_dir = crate::export::style::global_styles_dir();
        let dirs: Vec<&Path> = styles_dir.as_deref().into_iter().collect();
        let (styles, mut errors) = crate::export::style::load(&dirs);
        let font_files = crate::export::style::custom_font_files(&styles);
        let (registered, font_errors) = crate::editor_font::install_custom_fonts(ctx, &font_files);
        errors.extend(font_errors);
        self.typeset_styles = styles;
        self.custom_font_names = registered;
        if !errors.is_empty() {
            self.push_error_toast(errors.join("; "));
        }
    }

    /// Resolve each loaded plugin command's default shortcut
    /// (`plugin_engine.shortcut_defaults`) against `settings.plugin_shortcut_overrides`
    /// and the currently-bound built-in shortcuts, producing the set the per-frame
    /// consumption loop and the Settings panel actually use. An explicit `Unbound`
    /// override always wins; an explicit `Bound` override always wins too (the
    /// user's own remap, which already claimed the combo away from anyone else at
    /// the moment it was made — see `Settings::set_plugin_shortcut`). Absent an
    /// override, the script's own default applies only if it doesn't collide with
    /// a built-in or an already-placed plugin shortcut earlier in this pass —
    /// a collision leaves that command with no shortcut this session rather than
    /// erroring, since the same script default might become free again on a later
    /// reload.
    fn compute_effective_plugin_shortcuts(&self) -> Vec<(String, egui::KeyboardShortcut)> {
        let mut taken: std::collections::HashSet<egui::KeyboardShortcut> = self
            .settings
            .shortcuts
            .bindings()
            .into_iter()
            .map(|(_, shortcut)| shortcut)
            .collect();

        let mut effective = Vec::new();
        for (name, default_shortcut) in self.plugin_engine.shortcut_defaults() {
            let shortcut = match self.settings.plugin_shortcut_overrides.get(name) {
                Some(PluginShortcutOverride::Unbound) => continue,
                Some(PluginShortcutOverride::Bound(shortcut)) => *shortcut,
                None => {
                    if taken.contains(&default_shortcut) {
                        continue;
                    }
                    default_shortcut
                }
            };
            taken.insert(shortcut);
            effective.push((name.to_string(), shortcut));
        }
        effective
    }

    /// Run the action a keyboard shortcut just triggered. Contextual actions
    /// (New File/Folder, Rename, Delete, Restore) act on `selected_path` — the
    /// currently open document — and no-op if nothing's selected (or, for Restore,
    /// if what's selected isn't actually trashed), matching how the equivalent
    /// binder right-click item simply wouldn't be there.
    fn dispatch_shortcut_action(&mut self, ctx: &egui::Context, action: ShortcutAction) {
        match action {
            ShortcutAction::NewProject => self.start_new_project(),
            ShortcutAction::OpenProject => self.browse_for_project(ctx),
            ShortcutAction::CloseProject => self.close_project(ctx),
            ShortcutAction::OpenSettings => self.show_settings = true,
            ShortcutAction::Exit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            ShortcutAction::TogglePreview => {
                self.toggle_dock_tab_near(DockTab::Preview, DockTab::Editor)
            }
            ShortcutAction::ToggleCorkboard => {
                self.toggle_dock_tab_near(DockTab::Corkboard, DockTab::Editor)
            }
            ShortcutAction::ToggleStoryGrid => {
                self.toggle_dock_tab_near(DockTab::StoryGrid, DockTab::Editor)
            }
            ShortcutAction::ToggleBeliefTimeline => {
                self.toggle_dock_tab_near(DockTab::BeliefTimeline, DockTab::Editor)
            }
            ShortcutAction::Save => {
                if let Err(err) = self.save_editor() {
                    self.push_error_toast(format!("Save failed: {err}"));
                }
            }
            ShortcutAction::GoBack => self.go_back_document(),
            ShortcutAction::GoForward => self.go_forward_document(),
            ShortcutAction::NewFile => self.keyboard_new_file(),
            ShortcutAction::NewFolder => self.keyboard_new_folder(),
            ShortcutAction::Rename => {
                if let Some(path) = self.selected_path.clone() {
                    self.prompt_rename(path);
                }
            }
            ShortcutAction::Delete => {
                if let Some(path) = self.selected_path.clone() {
                    self.delete_node(&path);
                }
            }
            ShortcutAction::Restore => {
                if let Some(path) = self.selected_path.clone()
                    && self
                        .project
                        .as_ref()
                        .is_some_and(|project| project.trashed_origin(&path).is_some())
                {
                    self.restore_node(&path);
                }
            }
            ShortcutAction::ToggleDarkMode => {
                let is_dark = match self.settings.theme_preference {
                    egui::ThemePreference::Dark => true,
                    egui::ThemePreference::Light => false,
                    egui::ThemePreference::System => ctx.theme() == egui::Theme::Dark,
                };
                self.settings.theme_preference = if is_dark {
                    egui::ThemePreference::Light
                } else {
                    egui::ThemePreference::Dark
                };
                ctx.set_theme(self.settings.theme_preference);
                // A color theme (`:theme`/View > Theme) only ever customizes the one
                // base (Dark or Light) it's built for — toggling to the *other* base
                // would otherwise silently show plain default styling there while
                // settings.color_theme (and the View > Theme menu) still claimed the
                // theme was active. Toggling dark/light mode is an explicit request to
                // leave that theme's own base, so clear it rather than leave that
                // inconsistent state behind.
                if self.settings.color_theme.is_some() {
                    self.set_color_theme(ctx, None);
                }
                self.persist_settings();
            }
            ShortcutAction::ToggleFullscreen => {
                let is_fullscreen = ctx.input(|i| i.viewport().fullscreen).unwrap_or(false);
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!is_fullscreen));
            }
            ShortcutAction::ToggleFocusMode => self.set_focus_mode(ctx, !self.focus_mode),
            ShortcutAction::OpenDocument => {
                if self.project.is_some() {
                    self.open_document_prompt.request_open();
                } else {
                    self.push_error_toast("No project open");
                }
            }
            ShortcutAction::CloseDocument => self.close_document(ctx),
            ShortcutAction::FindReplace => self.find_replace.request_open(),
            ShortcutAction::CommandPrompt => self.command_prompt.request_open(),
            ShortcutAction::GitCommit => {
                if self.settings.git_integration_enabled() {
                    self.prompt_git_commit(false);
                }
            }
            ShortcutAction::GitPush => {
                if self.settings.git_integration_enabled() {
                    self.run_git_push(ctx);
                }
            }
            ShortcutAction::ToggleBacklinks => self.toggle_dock_tab(DockTab::Backlinks),
            ShortcutAction::ToggleTags => self.toggle_dock_tab(DockTab::Tags),
            ShortcutAction::EditMetadata => self.toggle_dock_tab(DockTab::Metadata),
            ShortcutAction::ToggleBinderFocus => {
                let editor_id = ui::editor_panel::editor_text_edit_id();
                if ctx.memory(|m| m.focused()) == Some(editor_id) {
                    // Bring the Binder tab to the front in case it's currently
                    // buried behind Backlinks/Metadata in the same dock node —
                    // otherwise `binder_panel::show` would never render this frame
                    // and the focus request would just sit there unclaimed.
                    if let Some(path) = self.dock_state.find_tab(&DockTab::Binder) {
                        let _ = self.dock_state.set_active_tab(path);
                    }
                    self.focus_binder_requested = true;
                } else {
                    // Bring the Editor tab to the front in case it's currently
                    // buried behind Preview/Corkboard in the same dock node (or
                    // closed outright) — same reasoning as the Binder side above.
                    if let Some(path) = self.dock_state.find_tab(&DockTab::Editor) {
                        let _ = self.dock_state.set_active_tab(path);
                    }
                    ctx.memory_mut(|m| m.request_focus(editor_id));
                }
            }
            // Filtered out of the consumption pass above and handled inline in
            // `editor_panel::show` instead — never actually reached, but the match
            // above has to stay exhaustive over `ShortcutAction`.
            ShortcutAction::ActivateWikilink => {}
            ShortcutAction::TogglePomodoro => self.toggle_dock_tab(DockTab::Pomodoro),
            ShortcutAction::ToggleWordCount => self.toggle_dock_tab(DockTab::WordCount),
            ShortcutAction::RefreshWordCount => self.spawn_word_count_recompute(ctx),
            ShortcutAction::ToggleCollabPanel => self.toggle_dock_tab(DockTab::Collab),
            ShortcutAction::ToggleStreak => self.toggle_dock_tab(DockTab::Streak),
            ShortcutAction::CycleBinderColorMode => self.cycle_binder_color_mode(),
            // Filtered out of the consumption pass above and handled inline in
            // `editor_panel::show` instead — never actually reached, but the match
            // above has to stay exhaustive over `ShortcutAction`, same as
            // `ActivateWikilink` above.
            ShortcutAction::ToggleBookmark => {}
            ShortcutAction::ToggleBookmarksPanel => self.toggle_dock_tab(DockTab::Bookmarks),
            ShortcutAction::NextBookmark => self.goto_next_bookmark(),
            ShortcutAction::PreviousBookmark => self.goto_previous_bookmark(),
            ShortcutAction::ToggleDocumentStats => {
                self.settings.show_document_stats_in_binder =
                    !self.settings.show_document_stats_in_binder;
                self.persist_settings();
            }
        }
    }

    /// Whether exiting right now would silently lose something: the open document
    /// has unsaved edits, or a card editor modal is open with a draft that hasn't
    /// been saved (or explicitly discarded) yet — see the `close_requested()`
    /// handling in `ui()`, the only caller.
    fn has_unsaved_changes(&self) -> bool {
        self.editor.dirty || self.card_draft.is_some()
    }

    /// Save the open document, first running every loaded plugin's `on_save` hook
    /// over the buffer (see `plugins::PluginEngine::run_on_save`) — a hook that
    /// errors just leaves the text as-is rather than blocking the save, so a
    /// broken plugin can never stop the user from saving. Used by the explicit
    /// save actions (`:w`/`Ctrl+S`, `:wq`) only; the focus-loss autosave in
    /// `editor_panel.rs` and the save-before-switching-documents path inside
    /// `EditorState::open` both stay plugin-agnostic (see `plugins.rs`'s v1 scope
    /// note) rather than threading plugin awareness into those lower layers. Also
    /// flags (never blocks on) a frontmatter block that fails to parse as YAML
    /// after the save — catches a hand-edit that just broke it, complementing
    /// `refresh_metadata_if_needed`'s equivalent check on opening a document.
    fn save_editor(&mut self) -> std::io::Result<()> {
        let (transformed, errors) = self.plugin_engine.run_on_save(&self.editor.buffer);
        if !errors.is_empty() {
            self.push_error_toast(errors.join("; "));
        }
        if transformed != self.editor.buffer {
            self.editor.buffer = transformed;
            self.editor.mark_dirty();
        }
        let result = self.editor.save();
        // Only report a frontmatter problem if nothing else already claimed the
        // status message this save (a plugin error above, or an I/O failure below)
        // — a fresh hand-edit that broke the YAML block is worth flagging, but not
        // at the cost of hiding a more pressing failure.
        if errors.is_empty()
            && result.is_ok()
            && let Some(err) = crate::frontmatter::validate(&self.editor.buffer)
        {
            self.push_error_toast(err.to_string());
        }
        if result.is_ok() {
            self.run_backup(BackupTrigger::ManualSave);
            self.refresh_git_dirty_paths();
        }
        result
    }

    /// Run a command parsed from the `:` command prompt.
    fn execute_command(&mut self, ctx: &egui::Context, command: Command) {
        match command {
            Command::Save => {
                if let Err(err) = self.save_editor() {
                    self.push_error_toast(format!("Save failed: {err}"));
                }
            }
            Command::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Command::SaveAndQuit => {
                if let Err(err) = self.save_editor() {
                    self.push_error_toast(format!("Save failed: {err}"));
                    return;
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Command::Open(title) => {
                let Some(project) = &self.project else {
                    self.push_error_toast("No project open");
                    return;
                };
                match project.tree.find_document_by_stem(&title) {
                    Some(node) => {
                        let path = node.path.clone();
                        self.open_document(&path);
                    }
                    None => self.push_error_toast(format!("No note found for \"{title}\"")),
                }
            }
            Command::New(title) => {
                let Some(project) = &self.project else {
                    self.push_error_toast("No project open");
                    return;
                };
                let parent = self
                    .selected_path
                    .as_deref()
                    .and_then(Path::parent)
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| project.root.clone());
                self.create_document(&parent, &title);
            }
            Command::DarkMode(choice) => {
                self.settings.theme_preference = match choice {
                    DarkModeChoice::Dark => egui::ThemePreference::Dark,
                    DarkModeChoice::Light => egui::ThemePreference::Light,
                    DarkModeChoice::System => egui::ThemePreference::System,
                };
                ctx.set_theme(self.settings.theme_preference);
                self.persist_settings();
            }
            Command::ColorTheme(choice) => self.set_color_theme(ctx, choice.as_deref()),
            Command::Git(git_command) => {
                if !self.settings.git_integration_enabled() {
                    self.push_error_toast(
                        "Git integration is disabled — enable it in Settings > History",
                    );
                    return;
                }
                match git_command {
                    GitCommand::Enable => self.enable_git_support_manually(),
                    GitCommand::Commit(Some(message)) => self.run_git_commit(ctx, &message, false),
                    GitCommand::Commit(None) => self.prompt_git_commit(false),
                    GitCommand::Push => self.run_git_push(ctx),
                    GitCommand::Pull => self.run_git_pull(ctx),
                    GitCommand::Backup(Some(message)) => self.run_git_commit(ctx, &message, true),
                    GitCommand::Backup(None) => self.prompt_git_commit(true),
                }
            }
            Command::Find(query) => {
                if !query.is_empty() {
                    self.find_replace.query = query;
                }
                self.find_replace.request_open();
            }
            Command::Tag(query) => {
                if !query.is_empty() {
                    self.tags.search_text = query;
                }
                if self.dock_state.find_tab(&DockTab::Tags).is_none() {
                    self.dock_state.push_to_focused_leaf(DockTab::Tags);
                }
            }
            Command::Plugin(name, arg) => self.run_plugin_command(&name, &arg),
        }
    }

    /// Open the Tags dock (if not already open) and switch its search box to `tag` —
    /// the vault-wide "documents carrying this tag" view. Shared by clicking a `#tag`
    /// in the Preview tab and (with an always-nonempty query, unlike `Command::Tag`,
    /// which tolerates an empty one to just open the dock) the `:tag` command.
    fn activate_tag(&mut self, tag: String) {
        self.tags.search_text = tag;
        if self.dock_state.find_tab(&DockTab::Tags).is_none() {
            self.dock_state.push_to_focused_leaf(DockTab::Tags);
        }
    }

    /// Run a plugin-registered `:` command, giving it the open document's live
    /// buffer to read via `smaragd_document_text()`, its file name (minus
    /// `.md`) via `smaragd_document_basename()`, and its path relative to the
    /// project root (`.md` included) via `smaragd_document_filename()`, and
    /// applying whatever effects it produced (a status message, and/or a new
    /// buffer if it called `smaragd_set_document_text`) back onto real app
    /// state. Never saves — like any other edit, the user's own save action does
    /// that.
    fn run_plugin_command(&mut self, name: &str, arg: &str) {
        let document_open = self.editor.open_path.is_some();
        let document_text = document_open.then_some(self.editor.buffer.as_str());
        let document_basename = self
            .editor
            .open_path
            .as_deref()
            .and_then(Path::file_stem)
            .and_then(|s| s.to_str());
        let document_filename = self.editor.open_path.as_deref().and_then(|path| {
            let relative: &Path = self
                .project
                .as_ref()
                .and_then(|project| path.strip_prefix(&project.root).ok())
                .unwrap_or(path);
            relative.to_str()
        });
        let (effects, result) = self.plugin_engine.run_command(
            name,
            arg,
            document_text,
            document_basename,
            document_filename,
        );
        if let Err(err) = result {
            self.push_error_toast(err);
            return;
        }
        // Only meaningful with a document open — a plugin can't fabricate one.
        if document_open && let Some(text) = effects.set_document_text {
            self.editor.buffer = text;
            self.editor.mark_dirty();
        }
        if let Some(message) = effects.status_message {
            self.set_status_message(message);
        }
    }

    /// Apply a Helix-style color theme by id (`Some`), or clear back to plain
    /// `:dmode` dark/light styling (`None`) — shared by `:theme`, the View > Theme
    /// menu, and reapplying the persisted choice on startup. Also updates
    /// `theme_preference` to match the theme's own dark/light base, since a theme
    /// picks its appearance along with its palette.
    fn set_color_theme(&mut self, ctx: &egui::Context, id: Option<&str>) {
        match id {
            Some(id) => {
                let Some(theme) = crate::color_theme::find(&self.color_themes, id) else {
                    self.push_error_toast(format!("Unknown theme: {id}"));
                    return;
                };
                crate::color_theme::apply(ctx, theme);
                self.settings.theme_preference = if theme.dark {
                    egui::ThemePreference::Dark
                } else {
                    egui::ThemePreference::Light
                };
                self.settings.color_theme = Some(theme.id.to_string());
            }
            None => {
                crate::color_theme::reset(ctx);
                self.settings.color_theme = None;
            }
        }
        self.persist_settings();
    }

    /// Renders every modal/dialog overlay that isn't the menu or status bar:
    /// Settings, the name-prompt, Find and Replace, the command prompt, the
    /// Open Document quick-switcher, the New Project template picker, and
    /// Help > About. Extracted from `ui()` verbatim (2026-07-31 code-quality
    /// review).
    fn show_modals(&mut self, ui: &mut egui::Ui) {
        let plugin_shortcut_rows: Vec<(String, Option<egui::KeyboardShortcut>)> = self
            .plugin_engine
            .shortcut_defaults()
            .map(|(name, _default)| {
                let current = self
                    .plugin_shortcuts
                    .iter()
                    .find(|(bound_name, _)| bound_name == name)
                    .map(|(_, shortcut)| *shortcut);
                (name.to_string(), current)
            })
            .collect();

        let previous_ui_font = self.settings.ui_font;
        let dictionary_downloading = self
            .pending_dictionary_download
            .as_ref()
            .map(|(language, _)| *language);
        let mut dictionary_download_request = None;
        if ui::settings_panel::show(
            ui.ctx(),
            &mut self.show_settings,
            &mut self.settings,
            &mut self.settings_category,
            &mut self.recording_shortcut,
            &plugin_shortcut_rows,
            dictionary_downloading,
            &mut dictionary_download_request,
        ) {
            if self.settings.ui_font != previous_ui_font {
                crate::editor_font::apply_ui_font(ui.ctx(), self.settings.ui_font);
            }
            self.persist_settings();
            self.plugin_shortcuts = self.compute_effective_plugin_shortcuts();
        }
        if let Some(language) = dictionary_download_request {
            self.spawn_dictionary_download(ui.ctx(), language);
        }

        if self.prompt.is_some() {
            let outcome = {
                let pending = self.prompt.as_mut().expect("checked above");
                ui::name_prompt::show(ui.ctx(), &mut pending.state)
            };
            if let Some(outcome) = outcome {
                self.finish_prompt(ui.ctx(), outcome);
            }
        }

        if self.exit_confirm.open
            && let Some(outcome) = ui::exit_confirm_prompt::show(ui.ctx(), &mut self.exit_confirm)
        {
            self.exit_confirm.open = false;
            match outcome {
                ui::exit_confirm_prompt::ExitConfirmOutcome::Save => {
                    let mut ok = true;
                    if self.editor.dirty
                        && let Err(err) = self.save_editor()
                    {
                        self.push_error_toast(format!("Save failed: {err}"));
                        ok = false;
                    }
                    if self.card_draft.is_some() {
                        self.finish_card_editor(CardEditorOutcome::Save);
                    }
                    if ok {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                }
                ui::exit_confirm_prompt::ExitConfirmOutcome::Discard => {
                    self.editor.dirty = false;
                    self.finish_card_editor(CardEditorOutcome::Cancel);
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
                ui::exit_confirm_prompt::ExitConfirmOutcome::Cancel => {}
            }
        }

        if let Some(event) = ui::find_replace_panel::show(ui.ctx(), &mut self.find_replace) {
            let ctx = ui.ctx().clone();
            self.handle_find_replace_event(&ctx, event);
        }

        if self.command_prompt.open {
            // Only walk the document tree for titles while the prompt (and its
            // `:open`/`:o` completion) is actually visible, rather than every frame.
            let note_titles = self
                .project
                .as_ref()
                .map(|project| project.tree.document_names())
                .unwrap_or_default();
            let plugin_commands: Vec<String> = self
                .plugin_engine
                .command_names()
                .map(str::to_string)
                .collect();
            let theme_ids: Vec<String> = self.color_themes.iter().map(|t| t.id.clone()).collect();
            let tag_names: Vec<String> = self
                .project
                .as_ref()
                .map(|project| project.all_tags())
                .unwrap_or_default();
            if let Some(event) = ui::command_prompt::show(
                ui.ctx(),
                &mut self.command_prompt,
                &note_titles,
                &plugin_commands,
                &theme_ids,
                &tag_names,
                self.settings.git_integration_enabled(),
            ) {
                let ctx = ui.ctx().clone();
                match event {
                    CommandPromptEvent::Run(command) => self.execute_command(&ctx, command),
                    CommandPromptEvent::Error(err) => self.push_error_toast(err),
                }
            }
        }

        if self.open_document_prompt.open {
            // Only walk the document tree while the dialog is actually visible,
            // same reasoning as the command prompt's own note-title gathering above.
            let candidates: Vec<(String, PathBuf)> = self
                .project
                .as_ref()
                .map(|project| {
                    project
                        .tree
                        .document_paths()
                        .into_iter()
                        .map(|path| {
                            let relative = path.strip_prefix(&project.root).unwrap_or(&path);
                            let display =
                                crate::project::model::document_label(&relative.to_string_lossy())
                                    .to_string();
                            (display, path)
                        })
                        .collect()
                })
                .unwrap_or_default();
            if let Some(path) = ui::open_document_prompt::show(
                ui.ctx(),
                &mut self.open_document_prompt,
                &candidates,
            ) {
                self.open_document(&path);
            }
        }

        if self.new_project_template_prompt.open
            && let Some(template_id) = ui::new_project_template_prompt::show(
                ui.ctx(),
                &mut self.new_project_template_prompt,
                &self.project_templates,
            )
        {
            self.start_new_project_with_template(ui.ctx(), template_id);
        }

        if self.show_about && ui::about_panel::show(ui.ctx()) {
            self.show_about = false;
        }
    }

    /// Renders the bottom status bar — hidden during Focus Mode. Extracted
    /// from `ui()` verbatim (2026-07-31 code-quality review).
    fn show_status_bar(&mut self, ui: &mut egui::Ui) {
        if !self.focus_mode {
            // Set from inside the closures below (which only ever borrow
            // `self` immutably, alongside egui's own `&mut Ui`) and acted on
            // afterward, once `self` is available mutably again — calling
            // `self.toggle_dock_tab` (which needs `&mut self`) from within
            // those closures would conflict with the immutable `self.*`
            // reads happening in the same scope.
            let mut streak_glyph_clicked = false;
            let mut color_mode_clicked = false;
            egui::Panel::bottom("status_bar").show(ui, |ui| {
                ui.horizontal(|ui| {
                    if let Some(path) = &self.editor.open_path {
                        ui.label(path.display().to_string());
                        if self.editor.dirty {
                            ui.label("*");
                        }
                    }
                    if let Some(msg) = &self.status_message {
                        ui.separator();
                        // No special color: now that error-severity messages go
                        // through `push_error_toast` instead (see its doc
                        // comment), everything left here is a routine
                        // confirmation, not a problem — coloring it red the way
                        // this label used to unconditionally do would misread as
                        // an error for messages like "Committed".
                        ui.label(msg);
                    }
                    // Both the Pomodoro countdown and the Draft Target's word
                    // count live on the right edge of the bar, independent of
                    // `status_message` above (which ~40 other call sites
                    // overwrite freely) and visible regardless of whether their
                    // dock tabs are open. Deliberately *one* shared
                    // `with_layout` rather than two separate sibling calls: each
                    // `with_layout(right_to_left, ...)` claims the *whole*
                    // remaining width of the row and right-aligns its own
                    // content within it, so two sibling calls land on top of
                    // each other instead of stacking — the first widget added
                    // inside a single shared right_to_left layout is what ends
                    // up rightmost, so Pomodoro (added first) stays anchored on
                    // the far right, with the word count just to its left, same
                    // as before this was one block.
                    let draft_target_set = self
                        .project
                        .as_ref()
                        .is_some_and(|project| project.meta.draft_target_words.is_some());
                    let streak_visible = self
                        .project
                        .as_ref()
                        .is_some_and(|project| project.meta.streak_enabled);
                    let color_mode_visible = self.project.as_ref().is_some_and(|project| {
                        project.meta.binder_color_mode != BinderColorMode::Off
                    });
                    if self.pomodoro.has_started()
                        || draft_target_set
                        || streak_visible
                        || color_mode_visible
                    {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            // A running/paused-mid-session Pomodoro timer needs a
                            // segment of its own that survives `status_message`
                            // being overwritten (see `tick_pomodoro`). Not shown
                            // once nothing's ever been started this session.
                            if self.pomodoro.has_started() {
                                let remaining = self.pomodoro.remaining().as_secs();
                                ui.label(format!(
                                    "⏱ {} {:02}:{:02}",
                                    self.pomodoro.phase().label(),
                                    remaining / 60,
                                    remaining % 60
                                ));
                            }
                            // Only shown once a Draft Target is actually set —
                            // Session Target progress is dock-panel-only, not
                            // surfaced here.
                            if let Some(project) = &self.project
                                && let Some(target) = project.meta.draft_target_words
                            {
                                ui.label(format!(
                                    "{} : {} / {} words",
                                    self.word_count.char_activity, self.word_count.cache, target
                                ));
                            }
                            // Compact traffic-light glyph for the Writing
                            // Streak feature, plus a live current-week
                            // percentage next to it — the dot alone only ever
                            // reflects the *last completed* week (see
                            // `streak::evaluate_streak`'s doc comment), so the
                            // percentage is what gives an at-a-glance read on
                            // today/this week without opening the dock tab.
                            // Placed after Pomodoro but before the Draft
                            // Target segment above (added second in this
                            // right_to_left layout, so it renders between
                            // them on screen: Pomodoro | Streak | word count).
                            if streak_visible && let Some(project) = &self.project {
                                let streak_config = crate::streak::resolve_streak_config(
                                    project.meta.streak_enabled,
                                    project.meta.streak_schedule,
                                    project.meta.streak_evaluation_mode,
                                    project.meta.streak_red_threshold_weeks,
                                );
                                let today = chrono::Local::now().date_naive();
                                let status = crate::streak::evaluate_streak(
                                    &streak_config.schedule,
                                    streak_config.mode,
                                    streak_config.red_threshold_weeks,
                                    &project.meta.daily_word_counts,
                                    today,
                                );
                                let today_words_so_far =
                                    self.word_count.cache.saturating_sub(
                                        project.meta.session_baseline_words as usize,
                                    ) as u32;
                                let (week_actual, week_target) =
                                    crate::streak::current_week_progress(
                                        &streak_config.schedule,
                                        &project.meta.daily_word_counts,
                                        today_words_so_far,
                                        today,
                                    );

                                let (rect, response) = ui.allocate_exact_size(
                                    egui::vec2(14.0, 14.0),
                                    egui::Sense::click(),
                                );
                                ui.painter().circle_filled(
                                    rect.center(),
                                    6.0,
                                    ui::streak_panel::status_color(ui, status),
                                );
                                let response =
                                    response.on_hover_text(streak_status_hover_text(status));
                                if response.clicked() {
                                    streak_glyph_clicked = true;
                                }

                                if week_target > 0 {
                                    let percent =
                                        ((week_actual as f32 / week_target as f32) * 100.0).round()
                                            as u32;
                                    let label_response = ui.add(
                                        egui::Label::new(format!("{percent}%"))
                                            .sense(egui::Sense::click()),
                                    );
                                    let label_response = label_response.on_hover_text(format!(
                                        "Progress this week: {week_actual} / {week_target} words"
                                    ));
                                    if label_response.clicked() {
                                        streak_glyph_clicked = true;
                                    }
                                }
                            }

                            // The active Binder coloring mode — added last in
                            // this shared right_to_left layout, so it lands
                            // at the left edge of this cluster of indicators
                            // (see this fn's opening comment on why one
                            // shared `with_layout` is used instead of a
                            // second sibling call). Click to cycle, same
                            // action as the `CycleBinderColorMode` shortcut.
                            // Hidden entirely while `Off` — nothing to
                            // indicate, and it'd otherwise sit in the status
                            // bar as dead weight for the (now default) case
                            // where binder coloring is disabled.
                            if color_mode_visible && let Some(project) = &self.project {
                                ui.separator();
                                let mode = project.meta.binder_color_mode;
                                let response = ui.add(
                                    egui::Label::new(format!("\u{1F3A8} {}", mode.label()))
                                        .sense(egui::Sense::click()),
                                );
                                let response = response.on_hover_text(format!(
                                    "Binder color mode: {} — click to cycle",
                                    mode.label()
                                ));
                                if response.clicked() {
                                    color_mode_clicked = true;
                                }
                            }
                        });
                    }
                });
            });
            if streak_glyph_clicked {
                self.toggle_dock_tab(DockTab::Streak);
            }
            if color_mode_clicked {
                self.cycle_binder_color_mode();
            }
        }
    }
}

/// Hover tooltip for the status bar's compact streak glyph. A fixed string
/// per status is enough for a tooltip; the dock tab (`ui::streak_panel::show`)
/// is where the fuller, threshold-number-aware label lives.
fn streak_status_hover_text(status: crate::streak::StreakStatus) -> &'static str {
    match status {
        crate::streak::StreakStatus::Disabled => "Streak tracking is disabled",
        crate::streak::StreakStatus::InsufficientData => "Streak: not enough data yet",
        crate::streak::StreakStatus::Green => "Streak: on track",
        crate::streak::StreakStatus::Yellow => "Streak: off track this week",
        crate::streak::StreakStatus::Red => "Streak: off track for multiple weeks",
    }
}

impl eframe::App for SmaragdApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll_git_operation(ui.ctx());
        self.poll_dictionary_download();
        self.poll_word_count();
        self.poll_collab_events(ui.ctx());
        self.tick_pomodoro(ui.ctx());
        self.show_toasts(ui.ctx());
        self.clear_status_message_if_expired(ui.ctx());

        // Save the dock layout right as shutdown starts (a window-close click, or
        // `ShortcutAction::Exit`'s `ViewportCommand::Close`, both surface here)
        // rather than in `eframe::App::on_exit` — that hook runs after the last
        // frame and isn't handed a `Context`, but capturing a floating panel's
        // current on-screen position (see `capture_floating_window_positions`)
        // needs one. Runs every frame from here until the app actually closes,
        // which in practice is just the one closing frame or two; re-saving the
        // same state each of those times is harmless.
        if ui.ctx().input(|i| i.viewport().close_requested()) {
            let ctx = ui.ctx().clone();
            self.persist_dock_layout(&ctx);

            // Veto the close and pop the Save/Discard/Cancel modal (rendered
            // further down, alongside the other prompts) the first time we see
            // unsaved edits. `!self.exit_confirm.open` guards against re-vetoing
            // every frame while the modal itself is up: `CancelClose` only needs
            // sending once, and by the time Save/Discard re-requests the close,
            // `has_unsaved_changes` is already false so this whole branch is
            // skipped.
            if self.has_unsaved_changes() && !self.exit_confirm.open {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.exit_confirm.open = true;
            }
        }

        if self.recording_shortcut.is_none() {
            let ctx = ui.ctx().clone();
            let mut pairs: Vec<(ShortcutTarget, egui::KeyboardShortcut)> = self
                .settings
                .shortcuts
                .bindings()
                .into_iter()
                // `ActivateWikilink`/`ToggleBookmark` are consumed inline in
                // `editor_panel::show` instead (see their doc comments) —
                // including either here too would let this pass steal the key
                // event first, so `editor_panel::show` would never see it.
                .filter(|(action, _)| {
                    *action != ShortcutAction::ActivateWikilink
                        && *action != ShortcutAction::ToggleBookmark
                })
                .map(|(action, shortcut)| (ShortcutTarget::BuiltIn(action), shortcut))
                .collect();
            pairs.extend(
                self.plugin_shortcuts
                    .iter()
                    .map(|(name, shortcut)| (ShortcutTarget::Plugin(name.clone()), *shortcut)),
            );
            let bindings = sorted_by_specificity(pairs);
            let triggered: Vec<ShortcutTarget> = bindings
                .into_iter()
                .filter(|(_, shortcut)| ctx.input_mut(|i| i.consume_shortcut(shortcut)))
                .map(|(target, _)| target)
                .collect();
            for target in triggered {
                match target {
                    ShortcutTarget::BuiltIn(action) => self.dispatch_shortcut_action(&ctx, action),
                    ShortcutTarget::Plugin(name) => self.run_plugin_command(&name, ""),
                }
            }
        }

        // Escape exits Focus Mode — the first top-level Escape consumption in
        // this file; every other Escape handler (name-prompt, command prompt,
        // wikilink-autocomplete popup, shortcut-recording) lives inside its own
        // modal's `show`, scoped to just that modal. Gated on no modal being
        // open so a single Escape press doesn't also exit Focus Mode while
        // dismissing one of those, mirroring `self.recording_shortcut.is_none()`
        // guarding the shortcut-dispatch pass above.
        if self.focus_mode
            && self.prompt.is_none()
            && !self.show_settings
            && !self.find_replace.open
            && !self.command_prompt.open
            && self.card_draft.is_none()
            && self.export.is_none()
            && ui.ctx().input(|i| i.key_pressed(egui::Key::Escape))
        {
            let ctx = ui.ctx().clone();
            self.set_focus_mode(&ctx, false);
        }

        self.show_menu_bar(ui);

        self.show_modals(ui);

        self.show_status_bar(ui);

        self.refresh_backlinks_if_needed();
        self.refresh_tags_if_needed();
        self.refresh_metadata_if_needed();
        self.refresh_folder_metadata_if_needed();
        self.refresh_word_count_if_needed(ui.ctx());

        if self.focus_mode {
            egui::CentralPanel::default().show(ui, |ui| {
                // A comfortable-width column, centered — Scrivener's Composition
                // Mode does the same rather than stretching text edge-to-edge
                // across a (likely now fullscreen) window. Proportional (70% of
                // the available width, clamped to a sane range) rather than a
                // fixed point value: a fixed width like 900.0 looked fine on
                // paper but left next to no margin in practice, since egui's
                // "points" shrink relative to the screen under any real
                // fractional display-scaling factor above ~1x — a fixed number
                // has no way to account for that, a proportion of whatever
                // space is actually available does.
                //
                // Built as an explicit child `Ui` over a manually computed
                // `Rect` (`new_child`), rather than `ui.horizontal(|ui|
                // { ui.add_space(margin); ui.vertical(...) })`: that nested-
                // container approach only ever centers the *width* — the
                // vertical child auto-shrinks to its content's height instead
                // of inheriting the panel's full height, so the editor ended
                // up a short box hugging the top-left corner instead of
                // filling the screen. An explicit `Rect` with both dimensions
                // set up front sidesteps that entirely.
                let available = ui.available_size();
                let column_width = (available.x * 0.7).clamp(400.0, 1000.0).min(available.x);
                let margin_x = ((available.x - column_width) / 2.0).max(0.0);
                let rect = egui::Rect::from_min_size(
                    ui.min_rect().min + egui::vec2(margin_x, 0.0),
                    egui::vec2(column_width, available.y),
                );
                let mut column_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect));
                let note_titles = self
                    .project
                    .as_ref()
                    .map(|project| project.tree.document_names())
                    .unwrap_or_default();
                let tag_names = self
                    .project
                    .as_ref()
                    .map(|project| project.all_tags())
                    .unwrap_or_default();
                let activate_wikilink_shortcut = self
                    .settings
                    .shortcuts
                    .get(ShortcutAction::ActivateWikilink);
                let toggle_bookmark_shortcut =
                    self.settings.shortcuts.get(ShortcutAction::ToggleBookmark);
                let bookmarked_lines = self
                    .editor
                    .open_path
                    .as_deref()
                    .zip(self.project.as_ref())
                    .map(|(path, project)| project.bookmarked_lines_for(path))
                    .unwrap_or_default();
                match ui::editor_panel::show(
                    &mut column_ui,
                    &mut self.editor,
                    &note_titles,
                    &tag_names,
                    activate_wikilink_shortcut,
                    true,
                    self.settings.editor_font,
                    crate::editor_font::resolve_size(self.settings.editor_font_size),
                    self.collab.is_some(),
                    self.settings.spell_check_language,
                    self.settings.show_editor_gutter,
                    &bookmarked_lines,
                    toggle_bookmark_shortcut,
                ) {
                    Some(EditorEvent::SaveError(err)) => self.push_error_toast(err),
                    Some(EditorEvent::Wikilink(activation)) => self.activate_wikilink(activation),
                    Some(EditorEvent::ToggleBookmark(line)) => {
                        if let Some(path) = self.editor.open_path.clone() {
                            self.toggle_bookmark(&path, line);
                        }
                    }
                    None => {}
                }
            });
        } else {
            egui::CentralPanel::default().show(ui, |ui| {
                let collab_status = match &self.collab {
                    None => CollabStatus::Idle,
                    Some(session) if session.session_ended => CollabStatus::Disconnected {
                        peer_fingerprint: session.peer_fingerprint.as_deref(),
                    },
                    Some(session) if session.peer_connected => CollabStatus::Connected {
                        peer_fingerprint: session
                            .peer_fingerprint
                            .as_deref()
                            .unwrap_or("unknown peer"),
                    },
                    Some(session) => match &session.code {
                        Some(code) => CollabStatus::Hosting { code },
                        None => CollabStatus::Connecting,
                    },
                };
                let today_words_so_far = self
                    .project
                    .as_ref()
                    .map(|project| {
                        self.word_count
                            .cache
                            .saturating_sub(project.meta.session_baseline_words as usize)
                            as u32
                    })
                    .unwrap_or(0);
                let book_style_id = self.resolve_book_style_id();
                let mut viewer = AppTabViewer {
                    project: self.project.as_ref(),
                    selected_path: self.selected_path.as_deref(),
                    open_path: self.editor.open_path.clone(),
                    backlinks: &self.backlinks.entries,
                    tags: &self.tags.entries,
                    tags_search_text: &mut self.tags.search_text,
                    tag_search_results: &self.tags.search_results,
                    metadata_draft: &mut self.metadata.draft,
                    metadata_target: self.metadata.target.clone(),
                    folder_metadata_draft: &mut self.metadata.folder_draft,
                    document_status_cache: &self.document_status_cache,
                    folder_word_counts: &self.word_count.folder_totals,
                    git_dirty_paths: &self.git_dirty_paths,
                    editor: &mut self.editor,
                    settings: &self.settings,
                    typeset_styles: &self.typeset_styles,
                    book_style_id: &book_style_id,
                    custom_fonts: &self.custom_font_names,
                    pomodoro: &self.pomodoro,
                    pomodoro_durations: crate::pomodoro::resolve_durations(&self.settings),
                    word_count_cache: self.word_count.cache,
                    char_activity: self.word_count.char_activity,
                    today_words_so_far,
                    streak_sub_tab: &mut self.streak_sub_tab,
                    belief_timeline_character: &mut self.belief_timeline_character,
                    actions: Vec::new(),
                    focus_binder_requested: std::mem::take(&mut self.focus_binder_requested),
                    collab_status,
                    collaborating: self.collab.is_some(),
                };
                egui_dock::DockArea::new(&mut self.dock_state)
                    .style(egui_dock::Style::from_egui(ui.style().as_ref()))
                    .show_inside(ui, &mut viewer);
                for action in viewer.actions {
                    match action {
                        DockAction::OpenDocument(path) => self.open_document(&path),
                        DockAction::Binder(event) => self.handle_binder_event(ui.ctx(), event),
                        DockAction::ProjectMeta(event) => self.handle_project_meta_event(event),
                        DockAction::Metadata(event) => self.handle_metadata_form_event(event),
                        DockAction::RefreshBacklinks => self.recompute_backlinks(),
                        DockAction::RefreshTags => self.recompute_tags(),
                        DockAction::RenameTag(tag) => self.prompt_rename_tag(tag),
                        DockAction::PreviewTagClicked(tag) => self.activate_tag(tag),
                        DockAction::EditorSaveError(err) => self.push_error_toast(err),
                        DockAction::Wikilink(activation) => self.activate_wikilink(activation),
                        DockAction::SetBookStyle(style_id) => {
                            if let Some(project) = &mut self.project
                                && let Err(err) = project.set_book_style(style_id)
                            {
                                self.push_error_toast(format!("Couldn't save settings: {err}"));
                            }
                        }
                        DockAction::Corkboard(event) => self.handle_corkboard_event(event),
                        DockAction::StoryGrid(event) => self.handle_story_grid_event(event),
                        DockAction::BeliefTimeline(event) => {
                            self.handle_belief_timeline_event(event)
                        }
                        DockAction::Pomodoro(event) => self.handle_pomodoro_event(event),
                        DockAction::WordCount(event) => {
                            self.handle_word_count_event(ui.ctx(), event)
                        }
                        DockAction::Collab(event) => {
                            self.handle_collab_panel_event(ui.ctx(), event)
                        }
                        DockAction::Streak(event) => self.handle_streak_event(event),
                        DockAction::RequestNewProject => self.start_new_project(),
                        DockAction::RequestOpenProject => self.browse_for_project(ui.ctx()),
                        DockAction::ToggleBookmark(line) => {
                            if let Some(path) = self.editor.open_path.clone() {
                                self.toggle_bookmark(&path, line);
                            }
                        }
                        DockAction::Bookmarks(event) => self.handle_bookmarks_event(event),
                    }
                }
            });
        }

        self.apply_metadata_edits_if_changed();
        self.apply_folder_metadata_edits_if_changed();
        self.sync_local_collab_edit();
        self.track_char_activity();
        self.refresh_tag_search_if_needed();

        if let Some(draft) = &mut self.card_draft {
            // Only walk the document tree for titles while the card editor (and its
            // linked-document completion) is actually open, rather than every frame.
            // Restricted to manuscript documents (see
            // `Project::manuscript_document_stems`), unlike the wikilink/`:open`
            // completions elsewhere in the app, which suggest every document.
            let note_titles = self
                .project
                .as_ref()
                .map(|project| project.manuscript_document_stems())
                .unwrap_or_default();
            let pov_titles: Vec<String> = self
                .project
                .as_ref()
                .map(|project| {
                    project
                        .picklist_documents(crate::project::PicklistField::Pov)
                        .iter()
                        .map(|node| crate::project::model::document_label(&node.name).to_string())
                        .collect()
                })
                .unwrap_or_default();
            if let Some(outcome) =
                ui::corkboard_panel::show_card_editor(ui.ctx(), draft, &note_titles, &pov_titles)
            {
                self.finish_card_editor(outcome);
            }
        }

        if let Some(state) = &mut self.export {
            let action = ui::export_panel::show(
                ui.ctx(),
                &state.source_label,
                &mut state.meta,
                &mut state.style_id,
                &self.typeset_styles,
            );
            if let Some(action) = action {
                self.finish_export(ui.ctx(), action);
            }
        }
    }
}

#[cfg(test)]
mod execute_command_tests {
    use super::*;

    #[test]
    fn dark_mode_command_updates_theme_preference() {
        let mut app = SmaragdApp::test_fixture();
        let ctx = egui::Context::default();

        app.execute_command(&ctx, Command::DarkMode(DarkModeChoice::Dark));

        assert_eq!(app.settings.theme_preference, egui::ThemePreference::Dark);
    }

    #[test]
    fn find_command_with_a_query_sets_it_and_opens_the_panel() {
        let mut app = SmaragdApp::test_fixture();
        let ctx = egui::Context::default();

        app.execute_command(&ctx, Command::Find("dragon".to_string()));

        assert_eq!(app.find_replace.query, "dragon");
        assert!(app.find_replace.open);
    }

    #[test]
    fn find_command_with_an_empty_query_opens_the_panel_without_clearing_it() {
        let mut app = SmaragdApp::test_fixture();
        app.find_replace.query = "earlier query".to_string();
        let ctx = egui::Context::default();

        app.execute_command(&ctx, Command::Find(String::new()));

        assert_eq!(app.find_replace.query, "earlier query");
        assert!(app.find_replace.open);
    }

    #[test]
    fn tag_command_sets_search_text_and_opens_the_tags_tab() {
        let mut app = SmaragdApp::test_fixture();
        let ctx = egui::Context::default();

        assert!(app.dock_state.find_tab(&DockTab::Tags).is_none());

        app.execute_command(&ctx, Command::Tag("worldbuilding".to_string()));

        assert_eq!(app.tags.search_text, "worldbuilding");
        assert!(app.dock_state.find_tab(&DockTab::Tags).is_some());
    }

    #[test]
    fn tag_command_does_not_reopen_an_already_open_tags_tab() {
        let mut app = SmaragdApp::test_fixture();
        app.dock_state.push_to_focused_leaf(DockTab::Tags);
        let ctx = egui::Context::default();

        app.execute_command(&ctx, Command::Tag("worldbuilding".to_string()));

        // A single Tags tab, not a second one pushed alongside it.
        let tag_tab_count = app
            .dock_state
            .iter_all_tabs()
            .filter(|(_, tab)| **tab == DockTab::Tags)
            .count();
        assert_eq!(tag_tab_count, 1);
    }

    #[test]
    fn activate_tag_sets_search_text_and_opens_the_tags_tab() {
        let mut app = SmaragdApp::test_fixture();

        assert!(app.dock_state.find_tab(&DockTab::Tags).is_none());

        app.activate_tag("worldbuilding".to_string());

        assert_eq!(app.tags.search_text, "worldbuilding");
        assert!(app.dock_state.find_tab(&DockTab::Tags).is_some());
    }

    #[test]
    fn open_command_without_a_project_pushes_an_error_toast() {
        let mut app = SmaragdApp::test_fixture();
        let ctx = egui::Context::default();

        app.execute_command(&ctx, Command::Open("Chapter One".to_string()));

        assert_eq!(app.toasts.len(), 1);
        assert_eq!(app.toasts[0].message, "No project open");
    }

    #[test]
    fn new_command_without_a_project_pushes_an_error_toast() {
        let mut app = SmaragdApp::test_fixture();
        let ctx = egui::Context::default();

        app.execute_command(&ctx, Command::New("Chapter One".to_string()));

        assert_eq!(app.toasts.len(), 1);
        assert_eq!(app.toasts[0].message, "No project open");
    }

    #[test]
    fn open_command_with_no_matching_document_pushes_an_error_toast_naming_it() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::initialize(dir.path()).unwrap();
        let mut app = SmaragdApp::test_fixture();
        app.project = Some(project);
        let ctx = egui::Context::default();

        app.execute_command(&ctx, Command::Open("Nonexistent".to_string()));

        assert_eq!(app.toasts.len(), 1);
        assert_eq!(app.toasts[0].message, "No note found for \"Nonexistent\"");
    }

    #[test]
    fn git_command_is_a_no_op_toast_when_integration_is_disabled() {
        let mut app = SmaragdApp::test_fixture();
        app.settings.git_integration_disabled = true;
        let ctx = egui::Context::default();

        app.execute_command(
            &ctx,
            Command::Git(crate::ui::command_prompt::GitCommand::Enable),
        );

        assert_eq!(app.toasts.len(), 1);
        assert!(
            app.toasts[0]
                .message
                .contains("Git integration is disabled")
        );
    }

    #[test]
    fn git_shortcuts_are_silent_no_ops_when_integration_is_disabled() {
        // With no project open, `prompt_git_commit`/`run_git_push` would
        // themselves push a "No project open" toast if reached at all — an
        // empty toast list here proves the disabled check short-circuits
        // before either runs, not just that they happened to no-op anyway.
        let mut app = SmaragdApp::test_fixture();
        app.settings.git_integration_disabled = true;
        let ctx = egui::Context::default();

        app.dispatch_shortcut_action(&ctx, ShortcutAction::GitCommit);
        app.dispatch_shortcut_action(&ctx, ShortcutAction::GitPush);

        assert!(app.toasts.is_empty());
    }
}

#[cfg(test)]
mod save_editor_tests {
    use super::*;

    #[test]
    fn save_editor_backs_up_the_project_when_backup_on_manual_save_is_enabled() {
        let dir = tempfile::tempdir().unwrap();
        Project::initialize(dir.path()).unwrap();
        let backup_dir = tempfile::tempdir().unwrap();
        let mut app = SmaragdApp::test_fixture();
        let ctx = egui::Context::default();
        app.open_project(&ctx, dir.path());
        let doc_path = dir.path().join("doc.md");
        std::fs::write(&doc_path, "original").unwrap();
        app.open_document_internal(&doc_path);
        app.editor.buffer = "edited".to_string();
        app.editor.mark_dirty();
        app.settings.backup_enabled = true;
        app.settings.backup_on_manual_save = true;
        app.settings.backup_dir = Some(backup_dir.path().to_path_buf());

        app.save_editor().unwrap();

        assert_eq!(std::fs::read_dir(backup_dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn save_editor_does_not_back_up_when_the_manual_save_trigger_is_disabled() {
        let dir = tempfile::tempdir().unwrap();
        Project::initialize(dir.path()).unwrap();
        let backup_dir = tempfile::tempdir().unwrap();
        let mut app = SmaragdApp::test_fixture();
        let ctx = egui::Context::default();
        app.open_project(&ctx, dir.path());
        let doc_path = dir.path().join("doc.md");
        std::fs::write(&doc_path, "original").unwrap();
        app.open_document_internal(&doc_path);
        app.editor.buffer = "edited".to_string();
        app.editor.mark_dirty();
        app.settings.backup_enabled = true;
        app.settings.backup_on_manual_save = false;
        app.settings.backup_dir = Some(backup_dir.path().to_path_buf());

        app.save_editor().unwrap();

        assert_eq!(std::fs::read_dir(backup_dir.path()).unwrap().count(), 0);
    }
}

#[cfg(test)]
mod unsaved_changes_tests {
    use super::*;

    #[test]
    fn has_unsaved_changes_is_false_by_default() {
        let app = SmaragdApp::test_fixture();
        assert!(!app.has_unsaved_changes());
    }

    #[test]
    fn has_unsaved_changes_is_true_when_the_editor_is_dirty() {
        let mut app = SmaragdApp::test_fixture();
        app.editor.dirty = true;
        assert!(app.has_unsaved_changes());
    }

    #[test]
    fn has_unsaved_changes_is_true_when_a_card_draft_is_open() {
        let mut app = SmaragdApp::test_fixture();
        app.card_draft = Some(CardDraft::new());
        assert!(app.has_unsaved_changes());
    }

    #[test]
    fn discarding_clears_the_card_draft_without_saving_it() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::initialize(dir.path()).unwrap();
        let mut app = SmaragdApp::test_fixture();
        app.project = Some(project);
        app.card_draft = Some(CardDraft::new());

        app.finish_card_editor(CardEditorOutcome::Cancel);

        assert!(app.card_draft.is_none());
        assert!(app.project.as_ref().unwrap().meta.story_cards.is_empty());
    }

    #[test]
    fn saving_persists_the_card_draft_and_clears_it() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::initialize(dir.path()).unwrap();
        let mut app = SmaragdApp::test_fixture();
        app.project = Some(project);
        app.card_draft = Some(CardDraft::new());

        app.finish_card_editor(CardEditorOutcome::Save);

        assert!(app.card_draft.is_none());
        assert_eq!(app.project.as_ref().unwrap().meta.story_cards.len(), 1);
    }
}
