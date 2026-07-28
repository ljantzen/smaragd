use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::editor::EditorState;
use crate::frontmatter::DocumentMeta;
use crate::project::{BacklinkEntry, LoadError, Project, RestoreError};
use crate::search::{self, SearchScope};
use crate::settings::PluginShortcutOverride;
use crate::settings::Settings;
use crate::shortcuts::{ShortcutAction, ShortcutTarget, sorted_by_specificity};
use crate::ui;
use crate::ui::WikilinkActivation;
use crate::ui::backlinks_panel::BacklinksEvent;
use crate::ui::binder_panel::BinderEvent;
use crate::ui::command_prompt::{
    Command, CommandPromptEvent, CommandPromptState, DarkModeChoice, GitCommand,
};
use crate::ui::corkboard_panel::{CardDraft, CardEditorOutcome, CorkboardEvent};
use crate::ui::editor_panel::EditorEvent;
use crate::ui::find_replace_panel::{FindReplaceEvent, FindReplaceState};
use crate::ui::metadata_panel::MetadataDraft;
use crate::ui::name_prompt::{NamePromptOutcome, NamePromptState};

/// Shows `label` as a menu-bar button, with `shortcut`'s formatted text (if any)
/// dimmed on the right, matching `egui::Button::shortcut_text`'s intended use.
fn menu_button_with_shortcut(
    ui: &mut egui::Ui,
    label: &str,
    shortcut: Option<egui::KeyboardShortcut>,
) -> egui::Response {
    let mut button = egui::Button::new(label);
    if let Some(shortcut) = shortcut {
        button = button.shortcut_text(ui.ctx().format_shortcut(&shortcut));
    }
    ui.add(button)
}

/// What a `NamePromptState` modal should do with the name once confirmed.
enum PromptAction {
    NewFile {
        parent: PathBuf,
    },
    NewFolder {
        parent: PathBuf,
    },
    NewFileFromTemplate {
        parent: PathBuf,
        template_path: PathBuf,
    },
    Rename {
        path: PathBuf,
    },
    NewProject {
        location: PathBuf,
    },
    /// Commit with the (editable) message the prompt was confirmed with; `push_after`
    /// carries through whether this was "Commit" or "Commit and Push".
    GitCommit {
        push_after: bool,
    },
    /// Save the current dock layout under the confirmed name (see
    /// `save_named_layout`).
    SaveLayout,
}

struct PendingPrompt {
    action: PromptAction,
    state: NamePromptState,
}

/// State for the open Export dialog (`ui::export_panel`), from the binder's
/// "Export…" context-menu entry — which folder to compile and the book
/// title/author fields being edited live.
struct ExportState {
    source: PathBuf,
    source_label: String,
    meta: crate::export::BookMeta,
    style_id: String,
}

/// A dockable tab in `dock_state`. Binder/Backlinks/Metadata used to be
/// (respectively) a fixed left panel or a blocking modal; Editor/Preview/
/// Corkboard used to be the three mutually-exclusive `ViewMode`s of a separate
/// `CentralPanel`, entirely outside the dock. All six now live in one shared
/// `egui_dock::DockState`, so any of them can be freely dragged, split, and
/// resized against any other — see `AppTabViewer` and the single
/// `DockArea::show_inside` call in `eframe::App::ui`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum DockTab {
    Binder,
    Backlinks,
    Metadata,
    Editor,
    Preview,
    Corkboard,
}

/// The initial dock layout for a fresh install (no persisted `dock_layout.json`
/// yet), and the "Restore Default Layout" Window-menu action: a narrow Binder
/// column on the left, Editor filling the rest — the same visual arrangement
/// this app always had back when Binder's dock and the editor were two
/// separate, non-`egui_dock` layout systems (a fixed-width side `Panel` and a
/// `CentralPanel`, respectively).
fn default_dock_state() -> egui_dock::DockState<DockTab> {
    let mut state = egui_dock::DockState::new(vec![DockTab::Editor]);
    // `split_left`'s `fraction` is the *new* (left/Binder) node's share, despite
    // its doc comment's wording ("how much of the parent's area the old node
    // will occupy") — confirmed empirically: 0.78 here actually gave Binder 78%
    // of the width and Editor the remaining 22%, the opposite of intended. 0.22
    // gives Binder a narrow column and leaves Editor the majority.
    state
        .main_surface_mut()
        .split_left(egui_dock::NodeIndex::root(), 0.22, vec![DockTab::Binder]);
    state
}

/// Guards against a layout that deserializes fine but has no `Editor` tab
/// anywhere — e.g. one persisted by a build from before the editor became a
/// dock tab, or a hand-edited file — which would otherwise leave no way to
/// edit any document at all. A no-op if `Editor` is already present.
fn ensure_editor_tab_present(state: &mut egui_dock::DockState<DockTab>) {
    if state
        .iter_all_tabs()
        .all(|(_, tab)| *tab != DockTab::Editor)
    {
        state.push_to_focused_leaf(DockTab::Editor);
    }
}

/// The `egui::Id` `egui_dock` renders a floating surface's `egui::Window` under —
/// duplicated here (rather than exposed by the crate) because nothing public
/// exposes it; see `capture_floating_window_positions`'s doc comment for why this
/// is needed at all. Matches `show_window_surface`'s own `id` exactly (egui_dock
/// 0.20.1, `src/widgets/dock_area/show/window_surface.rs`): `format!("window
/// {surf_index:?}").into()`, i.e. `Id::new` of that same formatted string.
fn floating_window_id(surface: egui_dock::SurfaceIndex) -> egui::Id {
    egui::Id::new(format!("window {surface:?}"))
}

/// `DockState`'s tree structure (tabs, splits, which surface each lives on)
/// round-trips through our JSON persistence just fine, but a floating surface's
/// on-screen *position* isn't actually part of that tree at all: `WindowState`
/// (the part of `DockState` that records it) only ever gets its `next_position`/
/// `next_size` populated once, right when a tab is first dragged out live (see
/// `DockState::detach_tab`) — `egui_dock` 0.20.1 never writes back to those
/// fields afterward (nor to `WindowState`'s `screen_rect`, which is otherwise
/// dead code in this version). The window's actual current position instead
/// lives only in egui's own per-session `Memory` (keyed by `floating_window_id`),
/// which starts out empty on every fresh launch — so a restored floating panel
/// would otherwise land wherever egui's built-in cascade default is, rather than
/// where it was left (exactly the bug reported: tabs reopened correctly, but
/// stacked at the top-left instead of docked to the right edge).
///
/// Called right before persisting (see `persist_dock_layout`), with the live
/// `ctx` still available: reads each floating surface's actual current rect out
/// of egui's `Memory` and writes it into that surface's `next_position`/
/// `next_size` — fields that *do* round-trip through our JSON serialization, and
/// that `WindowState::create_window` picks up automatically (exactly as it would
/// for a freshly-detached tab) the very first time this restored layout is shown.
fn capture_floating_window_positions<Tab>(
    state: &mut egui_dock::DockState<Tab>,
    ctx: &egui::Context,
) {
    let floating_surfaces: Vec<egui_dock::SurfaceIndex> = state
        .iter_surfaces_indexed()
        .filter(|(_, surface)| matches!(surface, egui_dock::Surface::Window(..)))
        .map(|(index, _)| index)
        .collect();
    for index in floating_surfaces {
        let Some(rect) = ctx.memory(|mem| mem.area_rect(floating_window_id(index))) else {
            continue;
        };
        if !rect.is_finite() {
            continue;
        }
        if let Some(window_state) = state.get_window_state_mut(index) {
            window_state.set_position(rect.min);
            window_state.set_size(rect.size());
        }
    }
}

pub struct TachyliteApp {
    project: Option<Project>,
    editor: EditorState,
    selected_path: Option<PathBuf>,
    status_message: Option<String>,
    settings: Settings,
    show_settings: bool,
    show_about: bool,
    prompt: Option<PendingPrompt>,
    recording_shortcut: Option<ShortcutTarget>,
    find_replace: FindReplaceState,
    card_draft: Option<CardDraft>,
    command_prompt: CommandPromptState,
    open_document_prompt: ui::open_document_prompt::OpenDocumentPromptState,
    /// Live editing buffers for the open document's frontmatter, always kept in sync
    /// with whichever document is open (see `refresh_metadata_if_needed`) — there's
    /// no "closed" state to represent here, since the Metadata dock tab's own
    /// presence in `dock_state` is what tracks visibility.
    metadata_draft: MetadataDraft,
    /// Which document `metadata_draft`/`metadata_last_applied` were last computed
    /// for, so a later frame can tell whether `editor.open_path` has since changed.
    metadata_computed_for: Option<PathBuf>,
    /// The `DocumentMeta` last written into `editor.buffer` — compared against
    /// `metadata_draft.to_meta()` each frame to notice a live edit (see
    /// `apply_metadata_edits_if_changed`) without re-writing the buffer when nothing
    /// changed.
    metadata_last_applied: DocumentMeta,
    /// Every `[[wikilink]]` elsewhere in the project pointing at the open document,
    /// kept in sync with whichever document is open (see
    /// `refresh_backlinks_if_needed`).
    backlinks: Vec<BacklinkEntry>,
    /// Which document `backlinks` was last computed for.
    backlinks_computed_for: Option<PathBuf>,
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
    /// Loaded `.rhai` plugins — the global directory always, plus the open
    /// project's own `.tachylite/plugins` if it has opted in (see
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
    /// The open Export dialog, if any — see `ExportState`.
    export: Option<ExportState>,
    /// Every selectable typesetting style: the 2 built-ins plus whatever
    /// `*.toml` files are in `export::style::global_styles_dir()`. Rebuilt by
    /// `reload_typeset_styles`.
    typeset_styles: Vec<crate::export::style::TypesetStyle>,
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
}

/// Which of the two network-bound git actions a `pending_git` background thread is
/// running — needed to know how to react (e.g. rescan on pull) once it finishes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GitOperation {
    Push,
    Pull,
}

impl GitOperation {
    fn label(self) -> &'static str {
        match self {
            GitOperation::Push => "Push",
            GitOperation::Pull => "Pull",
        }
    }
}

impl TachyliteApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        crate::editor_font::install(&cc.egui_ctx);

        let settings = crate::settings::config_file_path()
            .map(|path| Settings::load_from_path(&path))
            .unwrap_or_default();
        cc.egui_ctx.set_theme(settings.theme_preference);
        // Match the editor's background to the surrounding chrome instead of egui's
        // default `extreme_bg_color`, which renders TextEdit widgets noticeably darker
        // (dark mode) than the panels around them.
        for theme in [egui::Theme::Dark, egui::Theme::Light] {
            cc.egui_ctx.style_mut_of(theme, |style| {
                style.visuals.text_edit_bg_color = Some(style.visuals.panel_fill);
            });
        }

        let mut app = Self {
            project: None,
            editor: EditorState::default(),
            selected_path: None,
            status_message: None,
            settings,
            show_settings: false,
            show_about: false,
            prompt: None,
            recording_shortcut: None,
            find_replace: FindReplaceState::default(),
            card_draft: None,
            command_prompt: CommandPromptState::default(),
            open_document_prompt: ui::open_document_prompt::OpenDocumentPromptState::default(),
            metadata_draft: MetadataDraft::from_meta(&DocumentMeta::default()),
            metadata_computed_for: None,
            metadata_last_applied: DocumentMeta::default(),
            backlinks: Vec::new(),
            backlinks_computed_for: None,
            dock_state: Self::load_dock_state(),
            saved_layouts: Self::load_saved_layouts(),
            pending_git: None,
            plugin_engine: crate::plugins::PluginEngine::default(),
            plugin_shortcuts: Vec::new(),
            color_themes: Vec::new(),
            export: None,
            typeset_styles: Vec::new(),
            focus_binder_requested: false,
            focus_mode: false,
        };
        app.reload_typeset_styles();

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
            app.open_project(&path);
        }

        app
    }

    /// The plugin directories that currently apply: the global directory always,
    /// plus the open project's own `.tachylite/plugins` if it has opted in.
    fn plugin_dirs(&self) -> Vec<PathBuf> {
        let mut dirs: Vec<PathBuf> = crate::plugins::global_plugins_dir().into_iter().collect();
        if let Some(project) = &self.project
            && project.meta.plugins_enabled
        {
            dirs.push(project.root.join(".tachylite").join("plugins"));
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
            self.status_message = Some(errors.join("; "));
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
            self.status_message = Some(errors.join("; "));
        }
        if let Some(id) = self.settings.color_theme.clone()
            && crate::color_theme::find(&self.color_themes, &id).is_none()
        {
            crate::color_theme::reset(ctx);
            self.settings.color_theme = None;
            self.persist_settings();
        }
    }

    /// Reload the selectable typesetting styles (`self.typeset_styles`): the 2
    /// built-ins plus every `*.toml` file in `export::style::global_styles_dir()`.
    /// Called at startup and from the Export dialog's "Reload Styles" action.
    fn reload_typeset_styles(&mut self) {
        let styles_dir = crate::export::style::global_styles_dir();
        let dirs: Vec<&Path> = styles_dir.as_deref().into_iter().collect();
        let (styles, errors) = crate::export::style::load(&dirs);
        self.typeset_styles = styles;
        if !errors.is_empty() {
            self.status_message = Some(errors.join("; "));
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

    /// Open `path` as a project. Used for the automatic "reopen last project" path at
    /// startup, where a missing `.tachylite` marker must just be reported (not
    /// interactively resolved) — the user didn't just explicitly ask to open this
    /// folder, so an unprompted modal dialog on launch would be wrong.
    fn open_project(&mut self, path: &Path) {
        match Project::load_from_folder(path) {
            Ok(project) => self.set_project(project, path),
            Err(err) => {
                self.status_message = Some(format!("Couldn't open {}: {err}", path.display()));
            }
        }
    }

    fn set_project(&mut self, mut project: Project, path: &Path) {
        if self.settings.create_starter_folders {
            Self::ensure_starter_folders(&mut project);
        }
        self.project = Some(project);
        self.editor = EditorState::default();
        self.selected_path = None;
        self.status_message = None;
        self.settings.last_project_path = Some(path.to_path_buf());
        self.persist_settings();
        self.maybe_offer_git_support();
        if let Some(project) = &self.project
            && let Err(err) = Self::ensure_git_repo(project)
        {
            self.status_message = Some(format!("Couldn't initialize git: {err}"));
        }
        self.reload_plugins();
    }

    /// If git support is enabled for `project` but its `.git` directory is missing —
    /// deleted outside the app, or `project.json` synced somewhere that never had one
    /// — recreate it. A no-op both when git isn't enabled and when the repo already
    /// exists, so it's safe to call on every project open (not just once at enable
    /// time) — the same "checked and healed independently of when it was set up"
    /// philosophy `Project::ensure_role_folder` uses for the Research/Trash folders.
    fn ensure_git_repo(project: &Project) -> Result<(), crate::git::GitError> {
        if project.meta.git_enabled
            && crate::git::is_available()
            && !crate::git::is_repo(&project.root)
        {
            crate::git::init(&project.root)?;
        }
        Ok(())
    }

    /// The one-time "enable git support?" dialog (modeled after the Obsidian Git
    /// plugin), shown at most once per project — see `ProjectMeta::git_prompted`.
    /// A no-op if `git` isn't on `PATH`, or the project's already been asked.
    fn maybe_offer_git_support(&mut self) {
        let Some(project) = &self.project else {
            return;
        };
        if project.meta.git_prompted || project.meta.git_enabled || !crate::git::is_available() {
            return;
        }

        let already_repo = crate::git::is_repo(&project.root);
        let description = if already_repo {
            "This project is already a git repository. Enable Tachylite's git integration (commit/push/pull from the Versions menu)?"
        } else {
            "Git was detected on your system. Initialize a git repository for this project and enable version control from the Versions menu?"
        };
        let enable = rfd::MessageDialog::new()
            .set_title("Enable Git Support")
            .set_description(description)
            .set_level(rfd::MessageLevel::Info)
            .set_buttons(rfd::MessageButtons::YesNo)
            .show();

        let Some(project) = &mut self.project else {
            return;
        };
        if enable == rfd::MessageDialogResult::Yes {
            if let Err(err) = Self::init_repo_if_needed(&project.root) {
                self.status_message = Some(format!("Couldn't initialize git: {err}"));
                return;
            }
            if let Err(err) = project.enable_git_support() {
                self.status_message = Some(format!("Couldn't save settings: {err}"));
            }
        } else if let Err(err) = project.decline_git_support() {
            self.status_message = Some(format!("Couldn't save settings: {err}"));
        }
    }

    /// `git init` `root` unless it's already a repository. Shared by
    /// `maybe_offer_git_support` and `enable_git_support_manually`, which both need
    /// this exact "become a repo if not already one" step as part of turning git
    /// support on.
    fn init_repo_if_needed(root: &Path) -> Result<(), crate::git::GitError> {
        if !crate::git::is_repo(root) {
            crate::git::init(root)?;
        }
        Ok(())
    }

    /// "Enable Git Support" from the Versions menu or `:git enable` — unlike
    /// `maybe_offer_git_support`, always runs when asked, regardless of whether the
    /// project's already been prompted (this is how a user who declined the one-time
    /// dialog turns it on later).
    fn enable_git_support_manually(&mut self) {
        let Some(project) = &self.project else {
            self.status_message = Some("No project open".to_string());
            return;
        };
        if !crate::git::is_available() {
            self.status_message = Some("git was not found on this system".to_string());
            return;
        }
        if let Err(err) = Self::init_repo_if_needed(&project.root) {
            self.status_message = Some(format!("Couldn't initialize git: {err}"));
            return;
        }

        let Some(project) = &mut self.project else {
            return;
        };
        match project.enable_git_support() {
            Ok(()) => self.status_message = Some("Git support enabled".to_string()),
            Err(err) => self.status_message = Some(format!("Couldn't save settings: {err}")),
        }
    }

    /// Open the commit-message prompt (the existing name-prompt modal, reused),
    /// pre-filled with a default message. Shared by the Versions menu, the
    /// `GitCommit` shortcut, and `:git commit`/`:git backup` with no inline message.
    fn prompt_git_commit(&mut self, push_after: bool) {
        let Some(project) = &self.project else {
            self.status_message = Some("No project open".to_string());
            return;
        };
        if !project.meta.git_enabled {
            self.status_message = Some("Git support isn't enabled for this project".to_string());
            return;
        }
        self.prompt = Some(PendingPrompt {
            action: PromptAction::GitCommit { push_after },
            state: NamePromptState::new(
                "Commit",
                if push_after {
                    "Commit and Push"
                } else {
                    "Commit"
                },
                "Tachylite backup",
            ),
        });
    }

    fn run_git_commit(&mut self, ctx: &egui::Context, message: &str, push_after: bool) {
        let Some(project) = &self.project else {
            self.status_message = Some("No project open".to_string());
            return;
        };
        if !project.meta.git_enabled {
            self.status_message = Some("Git support isn't enabled for this project".to_string());
            return;
        }
        if let Err(err) = Self::ensure_git_repo(project) {
            self.status_message = Some(format!("Couldn't initialize git: {err}"));
            return;
        }
        match crate::git::commit_all(&project.root, message) {
            Ok(()) => {
                self.status_message = Some("Committed".to_string());
                if push_after {
                    self.run_git_push(ctx);
                }
            }
            Err(crate::git::GitError::NothingToCommit) => {
                self.status_message = Some("Nothing to commit".to_string());
            }
            Err(err) => self.status_message = Some(format!("Commit failed: {err}")),
        }
    }

    fn run_git_push(&mut self, ctx: &egui::Context) {
        let Some(project) = &self.project else {
            self.status_message = Some("No project open".to_string());
            return;
        };
        if !project.meta.git_enabled {
            self.status_message = Some("Git support isn't enabled for this project".to_string());
            return;
        }
        self.spawn_git_operation(ctx, GitOperation::Push, project.root.clone());
    }

    /// Pulls, then (once `poll_git_operation` picks up the result) rescans the binder
    /// tree so any files the pull added/removed show up. Deliberately doesn't reload
    /// the currently open document even if its on-disk content changed — that could
    /// silently clobber unsaved local edits; the user can reopen it themselves if they
    /// want the pulled version.
    fn run_git_pull(&mut self, ctx: &egui::Context) {
        let Some(project) = &self.project else {
            self.status_message = Some("No project open".to_string());
            return;
        };
        if !project.meta.git_enabled {
            self.status_message = Some("Git support isn't enabled for this project".to_string());
            return;
        }
        self.spawn_git_operation(ctx, GitOperation::Pull, project.root.clone());
    }

    /// Kick off `operation` against `root` on a background thread — `git push`/`pull`
    /// hit the network and can hang or take a long time, so neither ever runs
    /// synchronously on the UI thread. Refuses to start a second operation while one
    /// is already in flight rather than queuing or racing it. The spawned thread
    /// requests a repaint once it has a result, so `poll_git_operation` (called every
    /// frame) picks it up promptly instead of waiting for unrelated UI activity.
    fn spawn_git_operation(&mut self, ctx: &egui::Context, operation: GitOperation, root: PathBuf) {
        if self.pending_git.is_some() {
            self.status_message = Some("A git operation is already in progress".to_string());
            return;
        }
        let (sender, receiver) = std::sync::mpsc::channel();
        let repaint_ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = match operation {
                GitOperation::Push => crate::git::push(&root),
                GitOperation::Pull => crate::git::pull(&root),
            };
            let _ = sender.send(result);
            repaint_ctx.request_repaint();
        });
        self.status_message = Some(format!("{}ing…", operation.label()));
        self.pending_git = Some((operation, receiver));
    }

    /// Check whether the in-flight `pending_git` operation (if any) has finished, and
    /// apply its result — a status message, plus a binder rescan on a successful pull.
    /// Called every frame; a no-op whenever nothing is pending or the background
    /// thread hasn't sent its result yet.
    fn poll_git_operation(&mut self) {
        let Some((_, receiver)) = &self.pending_git else {
            return;
        };
        let result = match receiver.try_recv() {
            Ok(result) => result,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                let (operation, _) = self.pending_git.take().expect("checked above");
                self.status_message = Some(format!(
                    "{} failed: background thread panicked",
                    operation.label()
                ));
                return;
            }
        };
        let (operation, _) = self.pending_git.take().expect("checked above");
        match result {
            Ok(()) => {
                if operation == GitOperation::Pull
                    && let Some(project) = &mut self.project
                {
                    project.rescan();
                }
                self.status_message = Some(format!("{}ed", operation.label()));
            }
            Err(err) => {
                self.status_message = Some(format!("{} failed: {err}", operation.label()));
            }
        }
    }

    fn persist_settings(&mut self) {
        let Some(path) = crate::settings::config_file_path() else {
            return;
        };
        if let Err(err) = self.settings.save_to_path(&path) {
            self.status_message = Some(format!("Couldn't save settings: {err}"));
        }
    }

    /// Load the dock layout persisted by a previous run (see `persist_dock_layout`),
    /// falling back to `default_dock_state()` if there's nothing on disk yet or it
    /// fails to parse. Never a hard error: a missing/corrupt layout file shouldn't
    /// prevent the app from starting.
    ///
    /// Also guards against a layout that deserializes fine but has no `Editor` tab
    /// anywhere — e.g. one persisted by a build from before the editor became a
    /// dock tab, or a hand-edited file — which would otherwise leave no way to
    /// edit any document at all.
    fn load_dock_state() -> egui_dock::DockState<DockTab> {
        let mut state = crate::settings::dock_layout_file_path()
            .and_then(|path| fs::read_to_string(path).ok())
            .and_then(|contents| serde_json::from_str(&contents).ok())
            .unwrap_or_else(default_dock_state);
        ensure_editor_tab_present(&mut state);
        state
    }

    /// Save which dock tabs are open and how they're split/floated, so the layout
    /// is exactly as the user left it next launch. Called once, when a window
    /// close is first requested (see the `close_requested` check in `ui`) — that's
    /// the one point that both still has a live `ctx` (needed by
    /// `capture_floating_window_positions`) and is guaranteed to see the final
    /// state; layout changes (dragging, splitting, closing a tab) happen far less
    /// often than every-frame writes would justify.
    fn persist_dock_layout(&mut self, ctx: &egui::Context) {
        capture_floating_window_positions(&mut self.dock_state, ctx);
        let Some(path) = crate::settings::dock_layout_file_path() else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(contents) = serde_json::to_string_pretty(&self.dock_state) {
            let _ = fs::write(path, contents);
        }
    }

    /// Load the user's named, saved dock layouts (see `saved_layouts`), falling
    /// back to an empty map if there's nothing on disk yet or it fails to parse.
    fn load_saved_layouts() -> std::collections::BTreeMap<String, egui_dock::DockState<DockTab>> {
        crate::settings::saved_layouts_file_path()
            .and_then(|path| fs::read_to_string(path).ok())
            .and_then(|contents| serde_json::from_str(&contents).ok())
            .unwrap_or_default()
    }

    /// Persist `saved_layouts` immediately — called right after a save, not
    /// deferred to shutdown like `persist_dock_layout`, since this only happens
    /// on an explicit user action rather than every frame.
    fn persist_saved_layouts(&self) {
        let Some(path) = crate::settings::saved_layouts_file_path() else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(contents) = serde_json::to_string_pretty(&self.saved_layouts) {
            let _ = fs::write(path, contents);
        }
    }

    /// Open the "Save Layout" name-prompt modal, so the user can name the current
    /// dock arrangement before it's added to `saved_layouts` (see `finish_prompt`'s
    /// `PromptAction::SaveLayout` arm).
    fn prompt_save_layout(&mut self) {
        self.prompt = Some(PendingPrompt {
            action: PromptAction::SaveLayout,
            state: NamePromptState::new("Save Layout", "Save", ""),
        });
    }

    /// Snapshot the current dock layout under `name` (overwriting any existing
    /// layout of that name) and persist it right away.
    fn save_named_layout(&mut self, ctx: &egui::Context, name: &str) {
        let mut snapshot = self.dock_state.clone();
        capture_floating_window_positions(&mut snapshot, ctx);
        self.saved_layouts.insert(name.to_string(), snapshot);
        self.persist_saved_layouts();
    }

    /// Open `path` as a project in response to an explicit user action (the "Open
    /// Project" menu item). If `path` has never been opened by tachylite before (no
    /// `.tachylite/project.json`), offers via a native Yes/No dialog to set it up in
    /// place, matching `delete_node`'s confirmation pattern.
    fn open_project_or_offer_to_adopt(&mut self, path: &Path) {
        match Project::load_from_folder(path) {
            Ok(project) => self.set_project(project, path),
            Err(LoadError::NotInitialized(_)) => {
                let adopt = rfd::MessageDialog::new()
                    .set_title("Set Up Project")
                    .set_description(format!(
                        "\"{}\" hasn't been opened in tachylite before. Set it up as a tachylite project here?",
                        path.display()
                    ))
                    .set_level(rfd::MessageLevel::Info)
                    .set_buttons(rfd::MessageButtons::YesNo)
                    .show();
                if adopt == rfd::MessageDialogResult::Yes {
                    match Project::initialize(path) {
                        Ok(project) => self.set_project(project, path),
                        Err(err) => {
                            self.status_message =
                                Some(format!("Couldn't set up {}: {err}", path.display()));
                        }
                    }
                }
            }
            Err(err) => {
                self.status_message = Some(format!("Couldn't open {}: {err}", path.display()));
            }
        }
    }

    /// Open the OS's native folder-picker dialog and, if the user selects a folder,
    /// open it as a project immediately (offering to adopt it if needed).
    fn browse_for_project(&mut self) {
        if let Some(path) = rfd::FileDialog::new().pick_folder() {
            self.open_project_or_offer_to_adopt(&path);
        }
    }

    /// Start the "New Project" flow: pick a parent folder via the native folder
    /// picker, then prompt for the new project's name via the existing name-prompt
    /// modal.
    fn start_new_project(&mut self) {
        let Some(location) = rfd::FileDialog::new().pick_folder() else {
            return;
        };
        self.prompt = Some(PendingPrompt {
            action: PromptAction::NewProject { location },
            state: NamePromptState::new("New Project", "Create", ""),
        });
    }

    fn create_project(&mut self, location: &Path, name: &str) {
        let root = location.join(name);
        if root.exists() {
            // Unlike the adopt flow, "New Project" should only ever create a fresh
            // folder — silently folding an unrelated existing folder in as a project
            // would be surprising.
            self.status_message = Some(format!("{} already exists", root.display()));
            return;
        }
        match Project::initialize(&root) {
            Ok(project) => self.set_project(project, &root),
            Err(err) => {
                self.status_message = Some(format!("Couldn't create project: {err}"));
            }
        }
    }

    /// Ensure the project has a Research and a Trash folder, checked and healed
    /// independently of each other (see `Project::ensure_role_folder`) — run every
    /// time a project is opened, not just when freshly created, so turning the
    /// "Create Research and Trash folders" setting on later, or manually deleting
    /// one of these folders outside the app, gets fixed on the next open rather than
    /// staying that way indefinitely. Best-effort: a failure here (e.g. a read-only
    /// filesystem) shouldn't block opening the project.
    fn ensure_starter_folders(project: &mut Project) {
        for (name, role) in [
            ("Research", crate::project::FolderRole::Research),
            ("Trash", crate::project::FolderRole::Trash),
        ] {
            let _ = project.ensure_role_folder(role, name);
        }
    }

    fn open_document(&mut self, path: &Path) {
        match self.editor.open(path) {
            Ok(()) => self.selected_path = Some(path.to_path_buf()),
            Err(err) => {
                self.status_message = Some(format!("Couldn't open {}: {err}", path.display()));
            }
        }
    }

    /// Close the currently open document (silently autosaving first if dirty — same
    /// convention as `open_document`/`rename_node`, no discard/cancel prompt).
    fn close_document(&mut self, ctx: &egui::Context) {
        if let Err(err) = self.editor.close() {
            self.status_message = Some(format!("Couldn't save before closing: {err}"));
            return;
        }
        self.selected_path = None;
        if self.focus_mode {
            // Nothing left to show if the closed document was the one Focus Mode
            // was displaying — same reasoning as `set_focus_mode`'s own "refuses to
            // enter with no document open" guard, applied here on the way out.
            self.set_focus_mode(ctx, false);
        }
    }

    /// Refresh `metadata_draft` from the open document's current frontmatter
    /// (parsed from the live buffer, not necessarily what's on disk yet, so it
    /// reflects any unsaved edits to the block itself) whenever the open document
    /// has changed since the last computation — a no-op most frames. Called before
    /// the dock renders each frame, alongside `refresh_backlinks_if_needed`.
    fn refresh_metadata_if_needed(&mut self) {
        if self.editor.open_path == self.metadata_computed_for {
            return;
        }
        let meta = match &self.editor.open_path {
            Some(_) => crate::frontmatter::parse(&self.editor.buffer),
            None => DocumentMeta::default(),
        };
        self.metadata_draft = MetadataDraft::from_meta(&meta);
        self.metadata_last_applied = meta;
        self.metadata_computed_for = self.editor.open_path.clone();
    }

    /// Notice a live edit to `metadata_draft` (typed into the Metadata dock tab this
    /// frame, if it was visible) and, if anything actually changed, rewrite the
    /// editor buffer's frontmatter block in place (preserving any keys the form
    /// doesn't expose — see `frontmatter::write_back`) and mark it dirty, same as
    /// any other in-buffer edit; the existing save path (explicit Save, autosave on
    /// focus loss, etc.) takes it from there. Called after the dock renders each
    /// frame. A safe no-op when no document is open or nothing changed — the draft
    /// can only be mutated by the user typing into a visible Metadata tab.
    fn apply_metadata_edits_if_changed(&mut self) {
        if self.editor.open_path.is_none() {
            return;
        }
        let current = self.metadata_draft.to_meta();
        if current == self.metadata_last_applied {
            return;
        }
        self.editor.buffer = crate::frontmatter::write_back(&self.editor.buffer, &current);
        self.editor.mark_dirty();
        self.metadata_last_applied = current;
    }

    /// Refresh `backlinks` from the project whenever the open document has changed
    /// since the last scan — a no-op most frames. Called before the dock renders
    /// each frame; recomputing regardless of whether the Backlinks tab happens to
    /// be visible right now is simplest, since the scan itself is cheap (see
    /// `Project::backlinks`).
    fn refresh_backlinks_if_needed(&mut self) {
        if self.editor.open_path == self.backlinks_computed_for {
            return;
        }
        self.recompute_backlinks();
    }

    fn recompute_backlinks(&mut self) {
        self.backlinks = match (&self.project, &self.editor.open_path) {
            (Some(project), Some(path)) => project.backlinks(path),
            _ => Vec::new(),
        };
        self.backlinks_computed_for = self.editor.open_path.clone();
    }

    /// Open or close a dock tab: present → removed, absent → opened in whichever
    /// leaf currently has focus.
    fn toggle_dock_tab(&mut self, tab: DockTab) {
        if let Some(path) = self.dock_state.find_tab(&tab) {
            self.dock_state.remove_tab(path);
        } else {
            self.dock_state.push_to_focused_leaf(tab);
        }
    }

    /// Opens `tab` as a new tab in the same dock node as `anchor` — e.g. next to
    /// the editor — rather than wherever `push_to_focused_leaf` would land it
    /// (whichever leaf last had focus, which could be Binder's or anything else).
    /// Falls back to `push_to_focused_leaf` if `anchor` isn't currently open.
    fn open_tab_next_to(&mut self, tab: DockTab, anchor: DockTab) {
        match self.dock_state.find_tab(&anchor) {
            Some(path) => self.dock_state[path.surface][path.node].append_tab(tab),
            None => self.dock_state.push_to_focused_leaf(tab),
        }
    }

    /// Like `toggle_dock_tab`, but opens `tab` next to `anchor` (see
    /// `open_tab_next_to`) instead of wherever's focused — for tabs that
    /// conceptually pair with the editor (Preview, Corkboard), so toggling them
    /// doesn't land somewhere surprising depending on what the user last clicked.
    fn toggle_dock_tab_near(&mut self, tab: DockTab, anchor: DockTab) {
        if let Some(path) = self.dock_state.find_tab(&tab) {
            self.dock_state.remove_tab(path);
        } else {
            self.open_tab_next_to(tab, anchor);
        }
    }

    /// Enable/disable Focus Mode, keeping the OS window maximized in lock-step
    /// with it — Scrivener's Composition Mode works the same way (entering
    /// always goes fullscreen, leaving always leaves it), so there's no
    /// separate "was already maximized" state to track. Refuses to *enter*
    /// with no document open — there'd be nothing to show and no Binder to
    /// pick one from, since Focus Mode hides everything else.
    ///
    /// Uses `Maximized`, not `Fullscreen`: on Wayland compositors with patchy
    /// `xdg-shell` fullscreen support (e.g. niri, a scrollable-tiling
    /// compositor where "fullscreen" is a less-trodden path than the
    /// tiling-native "maximize") `Fullscreen` can report a viewport size that
    /// doesn't match what's actually visible — a real winit/niri interaction
    /// bug, not something fixable from egui's side of the layout. `Maximized`
    /// is what a tiling compositor already handles constantly and reliably.
    fn set_focus_mode(&mut self, ctx: &egui::Context, enabled: bool) {
        if enabled && self.editor.open_path.is_none() {
            self.status_message = Some("Open a document before entering Focus Mode.".to_string());
            return;
        }
        self.focus_mode = enabled;
        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(enabled));
    }

    /// Resolve a `[[wikilink]]` activated (clicked in the preview, or Ctrl+Enter in
    /// the editor) to a document in the current project (matched by filename,
    /// case-insensitively) and open it. If it doesn't exist and `force_create` was
    /// requested (Ctrl/Cmd was held), create it in the same folder as the document
    /// the link was activated from.
    fn activate_wikilink(&mut self, activation: WikilinkActivation) {
        let WikilinkActivation {
            target,
            force_create,
        } = activation;
        let Some(project) = &self.project else {
            self.status_message = Some(format!("No project open — can't resolve [[{target}]]"));
            return;
        };
        if let Some(node) = project.tree.find_document_by_stem(&target) {
            let path = node.path.clone();
            self.open_document(&path);
            return;
        }
        if !force_create {
            self.status_message = Some(format!("No note found for [[{target}]]"));
            return;
        }
        self.create_wikilink_target(&target);
    }

    /// Create a new document named `target` in the same folder as the document
    /// currently open (i.e. the one containing the wikilink that was activated), then
    /// open it.
    fn create_wikilink_target(&mut self, target: &str) {
        let Some(parent) = self
            .editor
            .open_path
            .as_ref()
            .and_then(|p| p.parent())
            .map(Path::to_path_buf)
        else {
            self.status_message = Some(format!(
                "Couldn't create a note for [[{target}]]: no document is open"
            ));
            return;
        };
        self.create_document(&parent, target);
    }

    fn handle_binder_event(&mut self, event: BinderEvent) {
        match event {
            BinderEvent::Selected(path) => self.open_document(&path),
            BinderEvent::NewFile { parent } => self.prompt_new_file(parent),
            BinderEvent::NewFolder { parent } => self.prompt_new_folder(parent),
            BinderEvent::NewFileFromTemplate {
                parent,
                template_path,
            } => self.prompt_new_file_from_template(parent, template_path),
            BinderEvent::Rename { path } => self.prompt_rename(path),
            BinderEvent::Delete { path } => self.delete_node(&path),
            BinderEvent::Restore { path } => self.restore_node(&path),
            BinderEvent::SetFolderRole { path, role } => self.set_folder_role(&path, role),
            BinderEvent::EmptyTrash { path } => self.empty_trash_folder(&path),
            BinderEvent::MoveItem { path, new_parent } => self.move_item(&path, &new_parent),
            BinderEvent::MoveItemBefore { path, before } => self.move_item_before(&path, &before),
            BinderEvent::Export { path } => self.open_export(path),
        }
    }

    /// Open the Export dialog for `path` (a binder folder) — pre-fills the
    /// Title/Author fields from `ProjectMeta::book_title`/`book_author` and
    /// the Style choice from `ProjectMeta::book_style`, falling back to the
    /// first loaded style (built-in "Manuscript") if unset or no longer
    /// resolves.
    fn open_export(&mut self, path: PathBuf) {
        let Some(project) = &self.project else {
            return;
        };
        let source_label = project
            .tree
            .find_by_path(&path)
            .map(|node| node.name.clone())
            .unwrap_or_else(|| path.display().to_string());
        let style_id = project
            .meta
            .book_style
            .clone()
            .filter(|id| crate::export::style::find(&self.typeset_styles, id).is_some())
            .or_else(|| self.typeset_styles.first().map(|s| s.id.clone()))
            .unwrap_or_default();
        self.export = Some(ExportState {
            source: path,
            source_label,
            meta: crate::export::BookMeta {
                title: project.meta.book_title.clone().unwrap_or_default(),
                author: project.meta.book_author.clone().unwrap_or_default(),
            },
            style_id,
        });
    }

    /// Handle an outcome from the export dialog: Docx/Epub/Pdf opens a native
    /// save dialog and runs the export; Reload re-scans custom styles; Close
    /// dismisses it. Title/Author/Style edits are persisted to the project
    /// regardless of which button was pressed, since the fields may have
    /// changed even if the user just closes the dialog.
    fn finish_export(&mut self, action: ui::export_panel::ExportAction) {
        let Some(state) = &self.export else {
            return;
        };
        let source = state.source.clone();
        let meta = state.meta.clone();
        let style_id = state.style_id.clone();

        if let Some(project) = &mut self.project
            && let Err(err) =
                project.set_book_meta(meta.title.clone(), meta.author.clone(), style_id.clone())
        {
            self.status_message = Some(format!("Couldn't save settings: {err}"));
        }

        let Some(style) = crate::export::style::find(&self.typeset_styles, &style_id).cloned()
        else {
            match action {
                ui::export_panel::ExportAction::Close => self.export = None,
                ui::export_panel::ExportAction::ReloadStyles => self.reload_typeset_styles(),
                _ => self.status_message = Some("No typesetting style selected".to_string()),
            }
            return;
        };

        match action {
            ui::export_panel::ExportAction::Close => {
                self.export = None;
            }
            ui::export_panel::ExportAction::ReloadStyles => {
                self.reload_typeset_styles();
            }
            ui::export_panel::ExportAction::Docx => {
                if let Some(out_path) = rfd::FileDialog::new()
                    .set_file_name("manuscript.docx")
                    .add_filter("Word Document", &["docx"])
                    .save_file()
                {
                    self.run_export(&source, &meta, &style, &out_path);
                }
            }
            ui::export_panel::ExportAction::Epub => {
                if let Some(out_path) = rfd::FileDialog::new()
                    .set_file_name("manuscript.epub")
                    .add_filter("EPUB", &["epub"])
                    .save_file()
                {
                    self.run_export_epub(&source, &meta, &style, &out_path);
                }
            }
            ui::export_panel::ExportAction::Pdf => {
                if let Some(out_path) = rfd::FileDialog::new()
                    .set_file_name("manuscript.pdf")
                    .add_filter("PDF", &["pdf"])
                    .save_file()
                {
                    self.run_export_pdf(&source, &meta, &style, &out_path);
                }
            }
        }
    }

    fn run_export(
        &mut self,
        source: &Path,
        meta: &crate::export::BookMeta,
        style: &crate::export::style::TypesetStyle,
        out_path: &Path,
    ) {
        let Some(project) = &self.project else {
            return;
        };
        let Some(folder) = project.tree.find_by_path(source) else {
            return;
        };
        let docs = crate::export::gather(project, folder);
        match crate::export::docx::export_docx(&docs, meta, style, &project.root, out_path) {
            Ok(()) => {
                self.status_message = Some(format!("Exported to {}", out_path.display()));
            }
            Err(err) => {
                self.status_message = Some(format!("Export failed: {err}"));
            }
        }
    }

    fn run_export_epub(
        &mut self,
        source: &Path,
        meta: &crate::export::BookMeta,
        style: &crate::export::style::TypesetStyle,
        out_path: &Path,
    ) {
        let Some(project) = &self.project else {
            return;
        };
        let Some(folder) = project.tree.find_by_path(source) else {
            return;
        };
        let docs = crate::export::gather(project, folder);
        match crate::export::epub::export_epub(&docs, meta, style, &project.root, out_path) {
            Ok(()) => {
                self.status_message = Some(format!("Exported to {}", out_path.display()));
            }
            Err(err) => {
                self.status_message = Some(format!("Export failed: {err}"));
            }
        }
    }

    fn run_export_pdf(
        &mut self,
        source: &Path,
        meta: &crate::export::BookMeta,
        style: &crate::export::style::TypesetStyle,
        out_path: &Path,
    ) {
        let Some(project) = &self.project else {
            return;
        };
        let Some(folder) = project.tree.find_by_path(source) else {
            return;
        };
        let docs = crate::export::gather(project, folder);
        match crate::export::pdf::export_pdf(&docs, meta, style, &project.root, out_path) {
            Ok(spine_width_in) => {
                self.status_message = Some(format!(
                    "Exported to {} — estimated spine width: {spine_width_in:.2}in",
                    out_path.display()
                ));
            }
            Err(err) => {
                self.status_message = Some(format!("Export failed: {err}"));
            }
        }
    }

    /// Move a file or folder into `new_parent` (a drag-and-drop in the binder). Keeps
    /// `selected_path`/the open editor's `open_path` following along if either was
    /// pointing at the moved item *or* something inside it (moving a folder relocates
    /// its whole subtree) — the buffer's content is untouched by a plain filesystem
    /// move, so there's nothing to save or reload, just retarget where Save will
    /// write.
    fn move_item(&mut self, path: &Path, new_parent: &Path) {
        let Some(project) = &mut self.project else {
            return;
        };
        match project.move_item(path, new_parent) {
            Ok(new_path) => {
                let rebase = |p: &Path| -> Option<PathBuf> {
                    p.strip_prefix(path).ok().map(|rest| new_path.join(rest))
                };
                if let Some(rebased) = self.selected_path.as_deref().and_then(rebase) {
                    self.selected_path = Some(rebased);
                }
                if let Some(rebased) = self.editor.open_path.as_deref().and_then(rebase) {
                    self.editor.open_path = Some(rebased);
                }
            }
            Err(err) => {
                self.status_message = Some(format!("Couldn't move {}: {err}", path.display()));
            }
        }
    }

    /// Same rebase-selected/open-path logic as `move_item`, for a document or
    /// folder dropped directly onto another document row (see
    /// `Project::move_item_before`).
    fn move_item_before(&mut self, path: &Path, before: &Path) {
        let Some(project) = &mut self.project else {
            return;
        };
        match project.move_item_before(path, before) {
            Ok(new_path) => {
                let rebase = |p: &Path| -> Option<PathBuf> {
                    p.strip_prefix(path).ok().map(|rest| new_path.join(rest))
                };
                if let Some(rebased) = self.selected_path.as_deref().and_then(rebase) {
                    self.selected_path = Some(rebased);
                }
                if let Some(rebased) = self.editor.open_path.as_deref().and_then(rebase) {
                    self.editor.open_path = Some(rebased);
                }
            }
            Err(err) => {
                self.status_message = Some(format!("Couldn't move {}: {err}", path.display()));
            }
        }
    }

    fn handle_corkboard_event(&mut self, event: CorkboardEvent) {
        match event {
            CorkboardEvent::CreateCard => self.card_draft = Some(CardDraft::new()),
            CorkboardEvent::EditCard(id) => {
                if let Some(project) = &self.project
                    && let Some(card) = project.story_card(id)
                {
                    self.card_draft = Some(CardDraft::from_card(card));
                }
            }
            CorkboardEvent::DeleteCard(id) => {
                if let Some(project) = &mut self.project
                    && let Err(err) = project.delete_story_card(id)
                {
                    self.status_message = Some(format!("Couldn't delete card: {err}"));
                }
            }
            CorkboardEvent::MoveCard { id, new_index } => {
                if let Some(project) = &mut self.project
                    && let Err(err) = project.move_story_card(id, new_index)
                {
                    self.status_message = Some(format!("Couldn't reorder card: {err}"));
                }
            }
            CorkboardEvent::OpenLinkedDocument(path) => {
                self.open_document(&path);
                match self.dock_state.find_tab(&DockTab::Editor) {
                    Some(tab_path) => {
                        let _ = self.dock_state.set_active_tab(tab_path);
                    }
                    None => self.open_tab_next_to(DockTab::Editor, DockTab::Corkboard),
                }
            }
            CorkboardEvent::SetProtagonistDesire(desire) => {
                if let Some(project) = &mut self.project
                    && let Err(err) = project.set_protagonist_desire(desire)
                {
                    self.status_message = Some(format!("Couldn't save desire: {err}"));
                }
            }
            CorkboardEvent::SetProtagonistMisbelief(misbelief) => {
                if let Some(project) = &mut self.project
                    && let Err(err) = project.set_protagonist_misbelief(misbelief)
                {
                    self.status_message = Some(format!("Couldn't save misbelief: {err}"));
                }
            }
        }
    }

    /// Handle the card-editor modal closing this frame, whether by Save, Delete, or
    /// Cancel — always clears `card_draft` either way, since the modal is done either
    /// way once an outcome is produced.
    fn finish_card_editor(&mut self, outcome: CardEditorOutcome) {
        let Some(draft) = self.card_draft.take() else {
            return;
        };
        let Some(project) = &mut self.project else {
            return;
        };
        match outcome {
            CardEditorOutcome::Save => {
                if let Err(err) = project.upsert_story_card(draft.finalize()) {
                    self.status_message = Some(format!("Couldn't save card: {err}"));
                }
            }
            CardEditorOutcome::Delete(id) => {
                if let Err(err) = project.delete_story_card(id) {
                    self.status_message = Some(format!("Couldn't delete card: {err}"));
                }
            }
            CardEditorOutcome::Cancel => {}
        }
    }

    /// Open the "New File" name-prompt modal for a file to be created inside
    /// `parent`. Shared by the binder's right-click menu and the New File keyboard
    /// shortcut.
    fn prompt_new_file(&mut self, parent: PathBuf) {
        self.prompt = Some(PendingPrompt {
            action: PromptAction::NewFile { parent },
            state: NamePromptState::new("New File", "Create", ""),
        });
    }

    /// Open the "New Folder" name-prompt modal for a folder to be created inside
    /// `parent`. Shared by the binder's right-click menu and the New Folder keyboard
    /// shortcut.
    fn prompt_new_folder(&mut self, parent: PathBuf) {
        self.prompt = Some(PendingPrompt {
            action: PromptAction::NewFolder { parent },
            state: NamePromptState::new("New Folder", "Create", ""),
        });
    }

    /// Open the "New From Template" name-prompt modal for a document to be created
    /// inside `parent`, copying `template_path`'s content — pre-filled with the
    /// template's own stem, same as `prompt_rename` pre-fills from the renamed
    /// item's current name.
    fn prompt_new_file_from_template(&mut self, parent: PathBuf, template_path: PathBuf) {
        let name = template_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();
        self.prompt = Some(PendingPrompt {
            action: PromptAction::NewFileFromTemplate {
                parent,
                template_path,
            },
            state: NamePromptState::new("New From Template", "Create", name),
        });
    }

    /// Open the "Rename" name-prompt modal, pre-filled with `path`'s current stem.
    /// Shared by the binder's right-click menu and the Rename keyboard shortcut.
    fn prompt_rename(&mut self, path: PathBuf) {
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();
        self.prompt = Some(PendingPrompt {
            action: PromptAction::Rename { path },
            state: NamePromptState::new("Rename", "Rename", name),
        });
    }

    /// New File/Folder triggered by keyboard shortcut: targets the currently
    /// selected document's parent folder, or the project root if nothing's
    /// selected. No-op if no project is open (rather than popping up a modal that
    /// would silently go nowhere on confirm).
    fn keyboard_new_file(&mut self) {
        let Some(project) = &self.project else {
            return;
        };
        let parent = self
            .selected_path
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .unwrap_or_else(|| project.root.clone());
        self.prompt_new_file(parent);
    }

    /// See `keyboard_new_file`.
    fn keyboard_new_folder(&mut self) {
        let Some(project) = &self.project else {
            return;
        };
        let parent = self
            .selected_path
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .unwrap_or_else(|| project.root.clone());
        self.prompt_new_folder(parent);
    }

    /// Run the action a keyboard shortcut just triggered. Contextual actions
    /// (New File/Folder, Rename, Delete, Restore) act on `selected_path` — the
    /// currently open document — and no-op if nothing's selected (or, for Restore,
    /// if what's selected isn't actually trashed), matching how the equivalent
    /// binder right-click item simply wouldn't be there.
    fn dispatch_shortcut_action(&mut self, ctx: &egui::Context, action: ShortcutAction) {
        match action {
            ShortcutAction::NewProject => self.start_new_project(),
            ShortcutAction::OpenProject => self.browse_for_project(),
            ShortcutAction::OpenSettings => self.show_settings = true,
            ShortcutAction::Exit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            ShortcutAction::TogglePreview => {
                self.toggle_dock_tab_near(DockTab::Preview, DockTab::Editor)
            }
            ShortcutAction::ToggleCorkboard => {
                self.toggle_dock_tab_near(DockTab::Corkboard, DockTab::Editor)
            }
            ShortcutAction::Save => {
                if let Err(err) = self.save_editor() {
                    self.status_message = Some(format!("Save failed: {err}"));
                }
            }
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
                    self.status_message = Some("No project open".to_string());
                }
            }
            ShortcutAction::CloseDocument => self.close_document(ctx),
            ShortcutAction::FindReplace => self.find_replace.request_open(),
            ShortcutAction::CommandPrompt => self.command_prompt.request_open(),
            ShortcutAction::GitCommit => self.prompt_git_commit(false),
            ShortcutAction::GitPush => self.run_git_push(ctx),
            ShortcutAction::ToggleBacklinks => self.toggle_dock_tab(DockTab::Backlinks),
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
        }
    }

    /// Save the open document, first running every loaded plugin's `on_save` hook
    /// over the buffer (see `plugins::PluginEngine::run_on_save`) — a hook that
    /// errors just leaves the text as-is rather than blocking the save, so a
    /// broken plugin can never stop the user from saving. Used by the explicit
    /// save actions (`:w`/`Ctrl+S`, `:wq`) only; the focus-loss autosave in
    /// `editor_panel.rs` and the save-before-switching-documents path inside
    /// `EditorState::open` both stay plugin-agnostic (see `plugins.rs`'s v1 scope
    /// note) rather than threading plugin awareness into those lower layers.
    fn save_editor(&mut self) -> std::io::Result<()> {
        let (transformed, errors) = self.plugin_engine.run_on_save(&self.editor.buffer);
        if !errors.is_empty() {
            self.status_message = Some(errors.join("; "));
        }
        if transformed != self.editor.buffer {
            self.editor.buffer = transformed;
            self.editor.mark_dirty();
        }
        self.editor.save()
    }

    /// Run a command parsed from the `:` command prompt.
    fn execute_command(&mut self, ctx: &egui::Context, command: Command) {
        match command {
            Command::Save => {
                if let Err(err) = self.save_editor() {
                    self.status_message = Some(format!("Save failed: {err}"));
                }
            }
            Command::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Command::SaveAndQuit => {
                if let Err(err) = self.save_editor() {
                    self.status_message = Some(format!("Save failed: {err}"));
                    return;
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Command::Open(title) => {
                let Some(project) = &self.project else {
                    self.status_message = Some("No project open".to_string());
                    return;
                };
                match project.tree.find_document_by_stem(&title) {
                    Some(node) => {
                        let path = node.path.clone();
                        self.open_document(&path);
                    }
                    None => self.status_message = Some(format!("No note found for \"{title}\"")),
                }
            }
            Command::New(title) => {
                let Some(project) = &self.project else {
                    self.status_message = Some("No project open".to_string());
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
            Command::Git(git_command) => match git_command {
                GitCommand::Enable => self.enable_git_support_manually(),
                GitCommand::Commit(Some(message)) => self.run_git_commit(ctx, &message, false),
                GitCommand::Commit(None) => self.prompt_git_commit(false),
                GitCommand::Push => self.run_git_push(ctx),
                GitCommand::Pull => self.run_git_pull(ctx),
                GitCommand::Backup(Some(message)) => self.run_git_commit(ctx, &message, true),
                GitCommand::Backup(None) => self.prompt_git_commit(true),
            },
            Command::Find(query) => {
                if !query.is_empty() {
                    self.find_replace.query = query;
                }
                self.find_replace.request_open();
            }
            Command::Plugin(name, arg) => self.run_plugin_command(&name, &arg),
        }
    }

    /// Run a plugin-registered `:` command, giving it the open document's live
    /// buffer to read via `tachylite_document_text()`, its file name (minus
    /// `.md`) via `tachylite_document_basename()`, and its path relative to the
    /// project root (`.md` included) via `tachylite_document_filename()`, and
    /// applying whatever effects it produced (a status message, and/or a new
    /// buffer if it called `tachylite_set_document_text`) back onto real app
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
            self.status_message = Some(err);
            return;
        }
        // Only meaningful with a document open — a plugin can't fabricate one.
        if document_open && let Some(text) = effects.set_document_text {
            self.editor.buffer = text;
            self.editor.mark_dirty();
        }
        if let Some(message) = effects.status_message {
            self.status_message = Some(message);
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
                    self.status_message = Some(format!("Unknown theme: {id}"));
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

    /// The documents a find/replace `scope` covers right now. Empty if no project is
    /// open, or (for the file-relative scopes) if nothing's open in the editor.
    fn search_scope_paths(&self, scope: SearchScope) -> Vec<PathBuf> {
        let Some(project) = &self.project else {
            return Vec::new();
        };
        match scope {
            SearchScope::CurrentFile => self.editor.open_path.clone().into_iter().collect(),
            SearchScope::CurrentDirectory => {
                let Some(dir) = self.editor.open_path.as_deref().and_then(Path::parent) else {
                    return Vec::new();
                };
                project
                    .tree
                    .document_paths()
                    .into_iter()
                    .filter(|path| path.parent() == Some(dir))
                    .collect()
            }
            SearchScope::ModifiedFiles => self.editor.modified_paths.iter().cloned().collect(),
            SearchScope::AllFiles => project.tree.document_paths(),
        }
    }

    fn handle_find_replace_event(&mut self, ctx: &egui::Context, event: FindReplaceEvent) {
        match event {
            FindReplaceEvent::Search => self.run_search(),
            FindReplaceEvent::ReplaceAll => self.run_replace_all(),
            FindReplaceEvent::OpenResult(index) => self.open_search_result(ctx, index),
        }
    }

    fn run_search(&mut self) {
        let paths = self.search_scope_paths(self.find_replace.scope);
        let live = self
            .editor
            .open_path
            .as_deref()
            .map(|path| (path, self.editor.buffer.as_str()));
        self.find_replace.results = search::search_paths(
            &paths,
            &self.find_replace.query,
            self.find_replace.case_sensitive,
            live,
        );
    }

    /// Replace every match in scope. The currently open document (if in scope) is
    /// updated in its live buffer and marked dirty, matching how the rest of the app
    /// treats it as unsaved until focus leaves the editor or the user saves
    /// explicitly; every other file in scope is read, replaced, and written back to
    /// disk immediately, since there's no in-memory buffer for it to land in.
    fn run_replace_all(&mut self) {
        let paths = self.search_scope_paths(self.find_replace.scope);
        let mut total = 0usize;
        for path in paths {
            let is_open = self.editor.open_path.as_deref() == Some(path.as_path());
            let content = if is_open {
                self.editor.buffer.clone()
            } else {
                match fs::read_to_string(&path) {
                    Ok(content) => content,
                    Err(_) => continue,
                }
            };

            let (new_content, count) = search::replace_all(
                &content,
                &self.find_replace.query,
                &self.find_replace.replacement,
                self.find_replace.case_sensitive,
            );
            if count == 0 {
                continue;
            }
            total += count;

            if is_open {
                self.editor.buffer = new_content;
                self.editor.mark_dirty();
            } else if let Err(err) = fs::write(&path, &new_content) {
                self.status_message = Some(format!("Couldn't update {}: {err}", path.display()));
            }
        }
        self.status_message = Some(format!("Replaced {total} occurrence(s)"));
        self.run_search();
    }

    /// Open the result's document (if it isn't already open) and move the editor
    /// cursor to where the match starts.
    fn open_search_result(&mut self, ctx: &egui::Context, index: usize) {
        let Some(result) = self.find_replace.results.get(index).cloned() else {
            return;
        };
        if self.editor.open_path.as_deref() != Some(result.path.as_path()) {
            self.open_document(&result.path);
        }
        if self.editor.open_path.as_deref() == Some(result.path.as_path()) {
            ui::editor_panel::move_cursor_to(
                ctx,
                ui::editor_panel::editor_text_edit_id(),
                &self.editor.buffer,
                result.byte_start,
            );
        }
    }

    /// Move a trashed item back to the folder it was deleted from. If that folder no
    /// longer exists, offers via a native Yes/No dialog to recreate it — matching
    /// `open_project_or_offer_to_adopt`'s "try, then offer, then retry" shape —
    /// leaving the item in Trash if declined.
    fn restore_node(&mut self, path: &Path) {
        let Some(project) = &mut self.project else {
            return;
        };
        match project.restore_from_trash(path, false) {
            Ok(_) => {}
            Err(RestoreError::OriginalFolderMissing(folder)) => {
                let recreate = rfd::MessageDialog::new()
                    .set_title("Restore")
                    .set_description(format!(
                        "\"{}\" no longer exists. Recreate it and restore here?",
                        folder.display()
                    ))
                    .set_level(rfd::MessageLevel::Info)
                    .set_buttons(rfd::MessageButtons::YesNo)
                    .show();
                if recreate == rfd::MessageDialogResult::Yes
                    && let Some(project) = &mut self.project
                    && let Err(err) = project.restore_from_trash(path, true)
                {
                    self.status_message = Some(format!("Couldn't restore: {err}"));
                }
            }
            Err(err) => self.status_message = Some(format!("Couldn't restore: {err}")),
        }
    }

    fn set_folder_role(&mut self, path: &Path, role: Option<crate::project::FolderRole>) {
        let Some(project) = &mut self.project else {
            return;
        };
        if let Err(err) = project.set_folder_role(path, role) {
            self.status_message = Some(format!("Couldn't set folder role: {err}"));
        }
    }

    /// Ask for confirmation, then permanently delete everything inside the
    /// designated Trash folder at `path`.
    fn empty_trash_folder(&mut self, path: &Path) {
        let confirmed = rfd::MessageDialog::new()
            .set_title("Empty Trash")
            .set_description("Permanently delete everything in Trash? This cannot be undone.")
            .set_level(rfd::MessageLevel::Warning)
            .set_buttons(rfd::MessageButtons::YesNo)
            .show();
        if confirmed != rfd::MessageDialogResult::Yes {
            return;
        }

        let Some(project) = &mut self.project else {
            return;
        };
        if let Err(err) = project.empty_trash() {
            self.status_message = Some(format!("Couldn't empty Trash: {err}"));
            return;
        }
        if self
            .selected_path
            .as_deref()
            .is_some_and(|selected| selected.starts_with(path))
        {
            self.editor = EditorState::default();
            self.selected_path = None;
        }
    }

    fn finish_prompt(&mut self, ctx: &egui::Context, outcome: NamePromptOutcome) {
        let Some(pending) = self.prompt.take() else {
            return;
        };
        let NamePromptOutcome::Confirmed(name) = outcome else {
            return;
        };
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        match pending.action {
            PromptAction::NewFile { parent } => self.create_document(&parent, name),
            PromptAction::NewFolder { parent } => self.create_folder(&parent, name),
            PromptAction::NewFileFromTemplate {
                parent,
                template_path,
            } => self.create_document_from_template(&parent, name, &template_path),
            PromptAction::Rename { path } => self.rename_node(&path, name),
            PromptAction::NewProject { location } => self.create_project(&location, name),
            PromptAction::GitCommit { push_after } => self.run_git_commit(ctx, name, push_after),
            PromptAction::SaveLayout => self.save_named_layout(ctx, name),
        }
    }

    fn rename_node(&mut self, path: &Path, new_name: &str) {
        // If `path` is the open document and it's dirty, save it *before* the
        // physical rename below — `project.rename` does an immediate `fs::rename`,
        // and letting `EditorState::open`'s own save-if-dirty run afterward (from
        // `open_document`, once the item's reopened under its new name) would try to
        // save to `editor.open_path`, which is still the pre-rename path and no
        // longer exists — silently resurrecting a stray file there with the unsaved
        // content while the visible buffer quietly reverts to the pre-edit version.
        // Saving first means the rename carries the up-to-date content along.
        if self.editor.open_path.as_deref() == Some(path)
            && self.editor.dirty
            && let Err(err) = self.editor.save()
        {
            self.status_message = Some(format!("Couldn't save before renaming: {err}"));
            return;
        }

        let Some(project) = &mut self.project else {
            return;
        };
        match project.rename(path, new_name) {
            Ok(new_path) => {
                if self.selected_path.as_deref() == Some(path) {
                    self.open_document(&new_path);
                } else if !self.editor.dirty
                    && let Some(open_path) = self.editor.open_path.clone()
                {
                    // The rename may have rewritten a `[[wikilink]]` to this document
                    // on disk; reload it so the editor reflects that. Skipped while
                    // dirty so we don't clobber unsaved edits with the disk version.
                    let _ = self.editor.open(&open_path);
                }
            }
            Err(err) => {
                self.status_message = Some(format!("Couldn't rename: {err}"));
            }
        }
    }

    /// Ask for confirmation via a native dialog, then delete the file or folder at
    /// `path`, closing it in the editor first if it (or its containing folder) was
    /// open. Worded accurately depending on whether this will move `path` into a
    /// designated Trash folder or remove it from disk outright.
    fn delete_node(&mut self, path: &Path) {
        let to_trash = self
            .project
            .as_ref()
            .is_some_and(|project| project.deletes_to_trash(path));
        let confirmed = if to_trash {
            rfd::MessageDialog::new()
                .set_title("Move to Trash")
                .set_description(format!("Move \"{}\" to Trash?", path.display()))
                .set_level(rfd::MessageLevel::Info)
                .set_buttons(rfd::MessageButtons::YesNo)
                .show()
        } else {
            rfd::MessageDialog::new()
                .set_title("Delete")
                .set_description(format!(
                    "Delete \"{}\"? This cannot be undone.",
                    path.display()
                ))
                .set_level(rfd::MessageLevel::Warning)
                .set_buttons(rfd::MessageButtons::YesNo)
                .show()
        };
        if confirmed != rfd::MessageDialogResult::Yes {
            return;
        }

        let Some(project) = &mut self.project else {
            return;
        };
        match project.delete(path) {
            Ok(()) => {
                if self
                    .selected_path
                    .as_deref()
                    .is_some_and(|selected| selected == path || selected.starts_with(path))
                {
                    self.editor = EditorState::default();
                    self.selected_path = None;
                }
            }
            Err(err) => {
                self.status_message = Some(format!("Couldn't delete {}: {err}", path.display()));
            }
        }
    }

    fn create_document(&mut self, parent: &Path, name: &str) {
        let Some(project) = &mut self.project else {
            return;
        };
        match project.create_document(parent, name) {
            Ok(path) => self.open_document(&path),
            Err(err) => self.status_message = Some(format!("Couldn't create file: {err}")),
        }
    }

    fn create_folder(&mut self, parent: &Path, name: &str) {
        let Some(project) = &mut self.project else {
            return;
        };
        if let Err(err) = project.create_folder(parent, name) {
            self.status_message = Some(format!("Couldn't create folder: {err}"));
        }
    }

    fn create_document_from_template(&mut self, parent: &Path, name: &str, template_path: &Path) {
        let Some(project) = &mut self.project else {
            return;
        };
        match project.create_document_from_template(
            parent,
            name,
            template_path,
            &self.settings.template_date_format,
        ) {
            Ok(path) => self.open_document(&path),
            Err(err) => self.status_message = Some(format!("Couldn't create file: {err}")),
        }
    }
}

/// Requests raised by `AppTabViewer::ui` for the caller to apply once the dock has
/// finished rendering for the frame — `egui_dock::TabViewer::ui` only gets `&mut
/// self` on the *viewer*, not on `TachyliteApp`, so it can't call `&mut self`
/// methods like `open_document` directly; it collects what it wants done instead.
enum DockAction {
    OpenDocument(PathBuf),
    Binder(BinderEvent),
    RefreshBacklinks,
    EditorSaveError(String),
    Wikilink(WikilinkActivation),
    Corkboard(CorkboardEvent),
}

/// A short-lived `egui_dock::TabViewer` impl, constructed fresh each frame right
/// before `DockArea::show_inside` and drained right after (see `DockAction`).
/// Borrows exactly what each tab's content needs to render; `metadata_draft` and
/// `editor` are the two `&mut` fields since the Metadata and Editor tabs mutate
/// them directly (live editing, no event needed for Metadata — see
/// `apply_metadata_edits_if_changed` — while Editor's own internal edits don't
/// need to round-trip through a `DockAction` either, only its save/wikilink
/// outcomes do).
struct AppTabViewer<'a> {
    project: Option<&'a Project>,
    selected_path: Option<&'a Path>,
    /// Owned (not `&'a Path`) because `editor` below is a `&'a mut EditorState`
    /// borrowed at the same time — an `&'a Path` still pointing into
    /// `editor.open_path` would alias it.
    open_path: Option<PathBuf>,
    backlinks: &'a [BacklinkEntry],
    metadata_draft: &'a mut MetadataDraft,
    editor: &'a mut EditorState,
    settings: &'a Settings,
    color_themes: &'a [crate::color_theme::ColorTheme],
    actions: Vec<DockAction>,
    /// See `TachyliteApp::focus_binder_requested`.
    focus_binder_requested: bool,
}

impl egui_dock::TabViewer for AppTabViewer<'_> {
    type Tab = DockTab;

    fn title(&mut self, tab: &mut DockTab) -> egui::WidgetText {
        match tab {
            DockTab::Binder => "Binder".into(),
            DockTab::Backlinks => "Backlinks".into(),
            DockTab::Metadata => "Metadata".into(),
            DockTab::Editor => "Editor".into(),
            DockTab::Preview => "Preview".into(),
            DockTab::Corkboard => "Corkboard".into(),
        }
    }

    /// The Editor tab can't be closed: unlike every other tab here, closing it
    /// would stop `editor_panel::show` from rendering that frame, which means its
    /// "save on lost-focus" path never runs — a silent way to lose unsaved edits
    /// that has no precedent before this tab existed (the editor was never
    /// closeable at all).
    fn closeable(&mut self, tab: &mut DockTab) -> bool {
        !matches!(tab, DockTab::Editor)
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut DockTab) {
        match tab {
            DockTab::Binder => match self.project {
                Some(project) => {
                    if let Some(event) = ui::binder_panel::show(
                        ui,
                        project,
                        self.selected_path,
                        self.focus_binder_requested,
                    ) {
                        self.actions.push(DockAction::Binder(event));
                    }
                }
                None => {
                    ui.label("Open a project folder to get started.");
                }
            },
            DockTab::Backlinks => {
                if let Some(event) =
                    ui::backlinks_panel::show(ui, self.open_path.as_deref(), self.backlinks)
                {
                    match event {
                        BacklinksEvent::OpenDocument(path) => {
                            self.actions.push(DockAction::OpenDocument(path));
                        }
                        BacklinksEvent::Refresh => self.actions.push(DockAction::RefreshBacklinks),
                    }
                }
            }
            DockTab::Metadata => {
                ui::metadata_panel::show(ui, self.open_path.as_deref(), self.metadata_draft);
            }
            DockTab::Editor => {
                let note_titles = self
                    .project
                    .map(|project| project.tree.document_names())
                    .unwrap_or_default();
                let activate_wikilink_shortcut = self
                    .settings
                    .shortcuts
                    .get(ShortcutAction::ActivateWikilink);
                match ui::editor_panel::show(
                    ui,
                    self.editor,
                    &note_titles,
                    activate_wikilink_shortcut,
                    false,
                    self.settings.editor_font,
                    crate::editor_font::resolve_size(self.settings.editor_font_size),
                ) {
                    Some(EditorEvent::SaveError(err)) => {
                        self.actions.push(DockAction::EditorSaveError(err));
                    }
                    Some(EditorEvent::Wikilink(activation)) => {
                        self.actions.push(DockAction::Wikilink(activation));
                    }
                    None => {}
                }
            }
            DockTab::Preview => {
                if self.editor.open_path.is_some() {
                    let base_dir = self.editor.open_path.as_deref().and_then(Path::parent);
                    let project_root = self.project.map(|project| project.root.as_path());
                    let active_theme = self
                        .settings
                        .color_theme
                        .as_deref()
                        .and_then(|id| crate::color_theme::find(self.color_themes, id));
                    if let Some(activation) = ui::markdown_preview::show(
                        ui,
                        &self.editor.buffer,
                        base_dir,
                        project_root,
                        active_theme,
                        self.settings.editor_font,
                        crate::editor_font::resolve_size(self.settings.editor_font_size),
                    ) {
                        self.actions.push(DockAction::Wikilink(activation));
                    }
                } else {
                    ui.label("Select a file from the binder to preview.");
                }
            }
            DockTab::Corkboard => match self.project {
                Some(project) => {
                    if let Some(event) = ui::corkboard_panel::show(ui, project) {
                        self.actions.push(DockAction::Corkboard(event));
                    }
                }
                None => {
                    ui.label("Open a project folder to get started.");
                }
            },
        }
    }
}

impl eframe::App for TachyliteApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll_git_operation();

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
        }

        if self.recording_shortcut.is_none() {
            let ctx = ui.ctx().clone();
            let mut pairs: Vec<(ShortcutTarget, egui::KeyboardShortcut)> = self
                .settings
                .shortcuts
                .bindings()
                .into_iter()
                // `ActivateWikilink` is consumed inline in `editor_panel::show`
                // instead (see its doc comment) — including it here too would let
                // this pass steal the key event first, so `editor_panel::show`
                // would never see it.
                .filter(|(action, _)| *action != ShortcutAction::ActivateWikilink)
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

        if !self.focus_mode {
            egui::Panel::top("menu_bar").show(ui, |ui| {
                egui::MenuBar::new().ui(ui, |ui| {
                    egui::containers::menu::MenuButton::new("File").ui(ui, |ui| {
                        let new_project_shortcut =
                            self.settings.shortcuts.get(ShortcutAction::NewProject);
                        let open_project_shortcut =
                            self.settings.shortcuts.get(ShortcutAction::OpenProject);
                        let open_settings_shortcut =
                            self.settings.shortcuts.get(ShortcutAction::OpenSettings);
                        let exit_shortcut = self.settings.shortcuts.get(ShortcutAction::Exit);
                        let open_document_shortcut =
                            self.settings.shortcuts.get(ShortcutAction::OpenDocument);
                        let close_document_shortcut =
                            self.settings.shortcuts.get(ShortcutAction::CloseDocument);

                        if menu_button_with_shortcut(ui, "New Project", new_project_shortcut)
                            .clicked()
                        {
                            self.start_new_project();
                        }
                        if menu_button_with_shortcut(ui, "Open Project", open_project_shortcut)
                            .clicked()
                        {
                            self.browse_for_project();
                        }
                        ui.add_enabled(false, egui::Button::new("Close Project"));
                        ui.separator();
                        if menu_button_with_shortcut(ui, "Open Document…", open_document_shortcut)
                            .clicked()
                        {
                            if self.project.is_some() {
                                self.open_document_prompt.request_open();
                            } else {
                                self.status_message = Some("No project open".to_string());
                            }
                        }
                        if menu_button_with_shortcut(ui, "Close Document", close_document_shortcut)
                            .clicked()
                        {
                            let ctx = ui.ctx().clone();
                            self.close_document(&ctx);
                        }
                        ui.separator();
                        if menu_button_with_shortcut(ui, "Settings", open_settings_shortcut)
                            .clicked()
                        {
                            self.show_settings = true;
                        }
                        ui.separator();
                        if menu_button_with_shortcut(ui, "Exit", exit_shortcut).clicked() {
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });
                    egui::containers::menu::MenuButton::new("Edit").ui(ui, |ui| {
                        if menu_button_with_shortcut(
                            ui,
                            "Cut",
                            Some(egui::KeyboardShortcut::new(
                                egui::Modifiers::COMMAND,
                                egui::Key::X,
                            )),
                        )
                        .clicked()
                        {
                            ui.ctx()
                                .send_viewport_cmd(egui::ViewportCommand::RequestCut);
                        }
                        if menu_button_with_shortcut(
                            ui,
                            "Copy",
                            Some(egui::KeyboardShortcut::new(
                                egui::Modifiers::COMMAND,
                                egui::Key::C,
                            )),
                        )
                        .clicked()
                        {
                            ui.ctx()
                                .send_viewport_cmd(egui::ViewportCommand::RequestCopy);
                        }
                        if menu_button_with_shortcut(
                            ui,
                            "Paste",
                            Some(egui::KeyboardShortcut::new(
                                egui::Modifiers::COMMAND,
                                egui::Key::V,
                            )),
                        )
                        .clicked()
                        {
                            ui.ctx()
                                .send_viewport_cmd(egui::ViewportCommand::RequestPaste);
                        }
                        ui.separator();
                        let find_replace_shortcut =
                            self.settings.shortcuts.get(ShortcutAction::FindReplace);
                        if menu_button_with_shortcut(ui, "Find and Replace", find_replace_shortcut)
                            .clicked()
                        {
                            self.find_replace.request_open();
                        }
                        let metadata_shortcut =
                            self.settings.shortcuts.get(ShortcutAction::EditMetadata);
                        if menu_button_with_shortcut(ui, "Document Metadata", metadata_shortcut)
                            .clicked()
                        {
                            self.toggle_dock_tab(DockTab::Metadata);
                        }
                    });
                    egui::containers::menu::MenuButton::new("View").ui(ui, |ui| {
                        if ui.button("Focus Mode").clicked() {
                            let ctx = ui.ctx().clone();
                            self.set_focus_mode(&ctx, !self.focus_mode);
                        }
                        ui.separator();
                        if ui.button("Editor").clicked() {
                            self.toggle_dock_tab(DockTab::Editor);
                        }
                        if ui.button("Preview").clicked() {
                            self.toggle_dock_tab_near(DockTab::Preview, DockTab::Editor);
                        }
                        if ui.button("Corkboard").clicked() {
                            self.toggle_dock_tab_near(DockTab::Corkboard, DockTab::Editor);
                        }
                        ui.separator();
                        if ui.button("Binder").clicked() {
                            self.toggle_dock_tab(DockTab::Binder);
                        }
                        if ui.button("Backlinks").clicked() {
                            self.toggle_dock_tab(DockTab::Backlinks);
                        }
                        ui.separator();
                        // `SubMenuButton`, not `MenuButton`: this is nested *inside* the
                        // View menu, and `MenuButton` is for top-level, click-to-open menu
                        // bar buttons. Using it here meant clicking "Theme" behaved like
                        // opening a second, independent top-level menu rather than a
                        // proper submenu — items inside never got a chance to run, since
                        // the parent popup's own close-on-click handling collapsed it out
                        // from under `SubMenuButton`'s (hover-to-open, keeps parents open)
                        // dedicated handling for exactly this case.
                        egui::containers::menu::SubMenuButton::new("Theme").ui(ui, |ui| {
                            if ui.button("Reload Custom Themes").clicked() {
                                let ctx = ui.ctx().clone();
                                self.reload_color_themes(&ctx);
                            }
                            ui.separator();
                            // Cloned rather than borrowed: `set_color_theme` below needs
                            // `&mut self`, which a live borrow of `self.settings`/
                            // `self.color_themes` here would conflict with across loop
                            // iterations.
                            let current = self.settings.color_theme.clone();
                            let themes = self.color_themes.clone();
                            if ui.radio(current.is_none(), "Default").clicked() {
                                self.set_color_theme(ui.ctx(), None);
                            }
                            for theme in &themes {
                                if ui
                                    .radio(
                                        current.as_deref() == Some(theme.id.as_str()),
                                        &theme.label,
                                    )
                                    .clicked()
                                {
                                    self.set_color_theme(ui.ctx(), Some(&theme.id));
                                }
                            }
                        });
                    });
                    egui::containers::menu::MenuButton::new("Tools").ui(ui, |ui| {
                        let command_prompt_shortcut =
                            self.settings.shortcuts.get(ShortcutAction::CommandPrompt);
                        if menu_button_with_shortcut(ui, "Command Prompt", command_prompt_shortcut)
                            .clicked()
                        {
                            self.command_prompt.request_open();
                        }
                        ui.separator();
                        if ui.button("Reload Plugins").clicked() {
                            self.reload_plugins();
                        }
                        let project_plugins_enabled = self
                            .project
                            .as_ref()
                            .is_some_and(|project| project.meta.plugins_enabled);
                        if self.project.is_some()
                            && !project_plugins_enabled
                            && ui.button("Enable Project Plugins").clicked()
                        {
                            if let Some(project) = &mut self.project
                                && let Err(err) = project.set_plugins_enabled(true)
                            {
                                self.status_message =
                                    Some(format!("Couldn't enable project plugins: {err}"));
                            }
                            self.reload_plugins();
                        }
                    });
                    egui::containers::menu::MenuButton::new("Versions").ui(ui, |ui| {
                        let git_enabled = self
                            .project
                            .as_ref()
                            .is_some_and(|project| project.meta.git_enabled);
                        if !git_enabled {
                            if ui.button("Enable Git Support").clicked() {
                                self.enable_git_support_manually();
                            }
                        } else {
                            let commit_shortcut =
                                self.settings.shortcuts.get(ShortcutAction::GitCommit);
                            if menu_button_with_shortcut(ui, "Commit", commit_shortcut).clicked() {
                                self.prompt_git_commit(false);
                            }
                            // Push/pull run on a background thread (see `spawn_git_operation`);
                            // disabled while one is already in flight rather than letting a
                            // second click queue up or race it.
                            let git_busy = self.pending_git.is_some();
                            ui.add_enabled_ui(!git_busy, |ui| {
                                if ui.button("Commit and Push").clicked() {
                                    self.prompt_git_commit(true);
                                }
                                let push_shortcut =
                                    self.settings.shortcuts.get(ShortcutAction::GitPush);
                                if menu_button_with_shortcut(ui, "Push", push_shortcut).clicked() {
                                    self.run_git_push(ui.ctx());
                                }
                                if ui.button("Pull").clicked() {
                                    self.run_git_pull(ui.ctx());
                                }
                            });
                        }
                    });
                    egui::containers::menu::MenuButton::new("Window").ui(ui, |ui| {
                        if ui.button("Save Current Layout…").clicked() {
                            self.prompt_save_layout();
                        }
                        // `SubMenuButton`, not `MenuButton` — see the matching comment on
                        // View's "Theme" submenu for why.
                        egui::containers::menu::SubMenuButton::new("Layouts").ui(ui, |ui| {
                            if self.saved_layouts.is_empty() {
                                ui.add_enabled(false, egui::Button::new("No saved layouts"));
                            } else {
                                // Collected up front rather than iterating
                                // `self.saved_layouts` directly: clicking an entry needs
                                // `&mut self.dock_state`, which an active immutable borrow
                                // of `self.saved_layouts` (the loop) would conflict with.
                                let names: Vec<String> =
                                    self.saved_layouts.keys().cloned().collect();
                                for name in names {
                                    if ui.button(&name).clicked()
                                        && let Some(layout) = self.saved_layouts.get(&name)
                                    {
                                        self.dock_state = layout.clone();
                                    }
                                }
                            }
                        });
                        ui.separator();
                        if ui.button("Restore Default Layout").clicked() {
                            self.dock_state = default_dock_state();
                        }
                    });
                    egui::containers::menu::MenuButton::new("Help").ui(ui, |ui| {
                        if ui.button("About").clicked() {
                            self.show_about = true;
                        }
                    });
                });
            });
        }

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

        if ui::settings_panel::show(
            ui.ctx(),
            &mut self.show_settings,
            &mut self.settings,
            &mut self.recording_shortcut,
            &plugin_shortcut_rows,
        ) {
            self.persist_settings();
            self.plugin_shortcuts = self.compute_effective_plugin_shortcuts();
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
            if let Some(event) = ui::command_prompt::show(
                ui.ctx(),
                &mut self.command_prompt,
                &note_titles,
                &plugin_commands,
                &theme_ids,
            ) {
                let ctx = ui.ctx().clone();
                match event {
                    CommandPromptEvent::Run(command) => self.execute_command(&ctx, command),
                    CommandPromptEvent::Error(err) => self.status_message = Some(err),
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
                                ui::binder_panel::document_label(&relative.to_string_lossy())
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

        if self.show_about && ui::about_panel::show(ui.ctx()) {
            self.show_about = false;
        }

        if !self.focus_mode {
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
                        ui.colored_label(egui::Color32::from_rgb(200, 60, 60), msg);
                    }
                });
            });
        }

        self.refresh_backlinks_if_needed();
        self.refresh_metadata_if_needed();

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
                let activate_wikilink_shortcut = self
                    .settings
                    .shortcuts
                    .get(ShortcutAction::ActivateWikilink);
                match ui::editor_panel::show(
                    &mut column_ui,
                    &mut self.editor,
                    &note_titles,
                    activate_wikilink_shortcut,
                    true,
                    self.settings.editor_font,
                    crate::editor_font::resolve_size(self.settings.editor_font_size),
                ) {
                    Some(EditorEvent::SaveError(err)) => self.status_message = Some(err),
                    Some(EditorEvent::Wikilink(activation)) => self.activate_wikilink(activation),
                    None => {}
                }
            });
        } else {
            egui::CentralPanel::default().show(ui, |ui| {
                let mut viewer = AppTabViewer {
                    project: self.project.as_ref(),
                    selected_path: self.selected_path.as_deref(),
                    open_path: self.editor.open_path.clone(),
                    backlinks: &self.backlinks,
                    metadata_draft: &mut self.metadata_draft,
                    editor: &mut self.editor,
                    settings: &self.settings,
                    color_themes: &self.color_themes,
                    actions: Vec::new(),
                    focus_binder_requested: std::mem::take(&mut self.focus_binder_requested),
                };
                egui_dock::DockArea::new(&mut self.dock_state)
                    .style(egui_dock::Style::from_egui(ui.style().as_ref()))
                    .show_inside(ui, &mut viewer);
                for action in viewer.actions {
                    match action {
                        DockAction::OpenDocument(path) => self.open_document(&path),
                        DockAction::Binder(event) => self.handle_binder_event(event),
                        DockAction::RefreshBacklinks => self.recompute_backlinks(),
                        DockAction::EditorSaveError(err) => self.status_message = Some(err),
                        DockAction::Wikilink(activation) => self.activate_wikilink(activation),
                        DockAction::Corkboard(event) => self.handle_corkboard_event(event),
                    }
                }
            });
        }

        self.apply_metadata_edits_if_changed();

        if let Some(draft) = &mut self.card_draft {
            // Only walk the document tree for titles while the card editor (and its
            // linked-document completion) is actually open, rather than every frame.
            let note_titles = self
                .project
                .as_ref()
                .map(|project| project.tree.document_names())
                .unwrap_or_default();
            if let Some(outcome) =
                ui::corkboard_panel::show_card_editor(ui.ctx(), draft, &note_titles)
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
                self.finish_export(action);
            }
        }
    }
}

#[cfg(test)]
mod dock_layout_persistence_tests {
    use super::DockTab;

    /// `egui_dock::DockState` only derives `Clone`/`Debug`, not `PartialEq` — so
    /// round-tripping is checked by comparing the set of open tabs (and, since
    /// `iter_all_tabs` walks every surface/node, this also exercises a split
    /// layout's extra surface, not just the default single-surface case).
    fn tab_set(state: &egui_dock::DockState<DockTab>) -> Vec<DockTab> {
        let mut tabs: Vec<DockTab> = state.iter_all_tabs().map(|(_, tab)| *tab).collect();
        tabs.sort_by_key(|tab| format!("{tab:?}"));
        tabs
    }

    struct NoopViewer;

    impl egui_dock::TabViewer for NoopViewer {
        type Tab = DockTab;

        fn title(&mut self, tab: &mut DockTab) -> egui::WidgetText {
            format!("{tab:?}").into()
        }

        fn ui(&mut self, ui: &mut egui::Ui, _tab: &mut DockTab) {
            ui.label("test");
        }
    }

    /// Actually renders `state` through a real `DockArea` for one frame before
    /// handing it back — every `Node`'s `rect` starts out as `Rect::NOTHING`
    /// (`{+inf, +inf} .. {-inf, -inf}`, see `egui_dock`'s `LeafNode::new`), which
    /// JSON can't represent (`serde_json` silently emits `null` for an infinite
    /// f32, then fails to deserialize that `null` back into a plain, non-`Option`
    /// f32 field) — a real freshly-*un-rendered* `DockState` would hit this same
    /// trap, but `persist_dock_layout` only ever runs once the dock has already
    /// been shown every frame the app was open, so its rects are always concrete
    /// real numbers. Rendering once here first is what makes these tests
    /// representative of that, rather than of a state no code path actually ever
    /// tries to persist.
    fn rendered(mut state: egui_dock::DockState<DockTab>) -> egui_dock::DockState<DockTab> {
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            egui_dock::DockArea::new(&mut state).show_inside(ui, &mut NoopViewer);
        });
        state
    }

    #[test]
    fn default_single_tab_layout_round_trips_through_json() {
        let state = rendered(egui_dock::DockState::new(vec![DockTab::Binder]));

        let json = serde_json::to_string(&state).unwrap();
        let restored: egui_dock::DockState<DockTab> = serde_json::from_str(&json).unwrap();

        assert_eq!(tab_set(&restored), vec![DockTab::Binder]);
    }

    #[test]
    fn a_split_layout_with_multiple_tabs_round_trips_through_json() {
        let mut state = egui_dock::DockState::new(vec![DockTab::Binder]);
        state.push_to_focused_leaf(DockTab::Backlinks);
        state.push_to_focused_leaf(DockTab::Metadata);
        let state = rendered(state);

        let json = serde_json::to_string(&state).unwrap();
        let restored: egui_dock::DockState<DockTab> = serde_json::from_str(&json).unwrap();

        assert_eq!(
            tab_set(&restored),
            vec![DockTab::Backlinks, DockTab::Binder, DockTab::Metadata]
        );
    }

    /// The exact scenario from the bug report: a tab dragged out into its own
    /// floating window (here simulated with `detach_tab` rather than an actual
    /// drag) reopens with the right tabs, but at the wrong on-screen position.
    #[test]
    fn a_floating_windows_position_survives_a_save_and_reload_round_trip() {
        let mut state = egui_dock::DockState::new(vec![DockTab::Binder]);
        let detach_rect =
            egui::Rect::from_min_size(egui::pos2(400.0, 50.0), egui::vec2(200.0, 300.0));
        let window_index = state.detach_tab(
            egui_dock::TabPath::new(
                egui_dock::SurfaceIndex::main(),
                egui_dock::NodeIndex::root(),
                egui_dock::TabIndex(0),
            ),
            detach_rect,
        );

        // Render with one persistent `Context` (unlike `rendered` above, which
        // uses a fresh throwaway one per call) so the floating window's actual
        // placement lands in egui's own Area memory, the same way it would
        // across real frames in one running session.
        let live_ctx = egui::Context::default();
        let _ = live_ctx.run_ui(egui::RawInput::default(), |ui| {
            egui_dock::DockArea::new(&mut state).show_inside(ui, &mut NoopViewer);
        });

        super::capture_floating_window_positions(&mut state, &live_ctx);

        // Round-trip through JSON exactly like `persist_dock_layout`/`load_dock_state`.
        let json = serde_json::to_string(&state).unwrap();
        let mut restored: egui_dock::DockState<DockTab> = serde_json::from_str(&json).unwrap();

        // A brand-new `Context` — no memory of the previous session's Area
        // positions at all — mirrors an actual app restart.
        let fresh_ctx = egui::Context::default();
        let _ = fresh_ctx.run_ui(egui::RawInput::default(), |ui| {
            egui_dock::DockArea::new(&mut restored).show_inside(ui, &mut NoopViewer);
        });
        let restored_rect = fresh_ctx
            .memory(|mem| mem.area_rect(super::floating_window_id(window_index)))
            .expect("the floating window should have rendered at some rect");

        assert!(
            (restored_rect.min - detach_rect.min).length() < 1.0,
            "expected the floating window to reopen near {:?}, but it reopened at {:?}",
            detach_rect.min,
            restored_rect.min
        );
    }

    #[test]
    fn default_dock_state_has_exactly_binder_and_editor() {
        let state = super::default_dock_state();

        assert_eq!(tab_set(&state), vec![DockTab::Binder, DockTab::Editor]);
    }

    #[test]
    fn default_dock_state_gives_the_editor_the_majority_of_the_width() {
        // Regression test: `split_left`'s `fraction` turned out to be the *new*
        // (left/Binder) node's share, not the old node's as its own doc comment
        // claims — a `fraction` of 0.78 was previously giving Binder 78% of the
        // width and Editor only 22%, backwards from the intent of a narrow
        // Binder column with Editor filling the rest.
        let state = rendered(super::default_dock_state());

        let mut binder_width = None;
        let mut editor_width = None;
        for node in state.main_surface().iter() {
            let Some(rect) = node.rect() else { continue };
            match node.tabs() {
                Some(tabs) if tabs.contains(&DockTab::Binder) => binder_width = Some(rect.width()),
                Some(tabs) if tabs.contains(&DockTab::Editor) => editor_width = Some(rect.width()),
                _ => {}
            }
        }
        let binder_width = binder_width.expect("Binder should be a leaf with a rect");
        let editor_width = editor_width.expect("Editor should be a leaf with a rect");

        assert!(
            editor_width > binder_width,
            "expected Editor ({editor_width}) to occupy the majority of the width, \
             not Binder ({binder_width})"
        );
    }

    #[test]
    fn ensure_editor_tab_present_adds_editor_when_missing() {
        // Simulates a `dock_layout.json` persisted before the editor became a dock
        // tab (or a hand-edited file) — deserializes fine, but has no Editor tab.
        let mut state = egui_dock::DockState::new(vec![DockTab::Binder]);

        super::ensure_editor_tab_present(&mut state);

        assert!(
            tab_set(&state).contains(&DockTab::Editor),
            "expected an Editor tab to be added when one wasn't already present"
        );
    }

    #[test]
    fn ensure_editor_tab_present_is_a_no_op_when_editor_already_exists() {
        let mut state = super::default_dock_state();

        super::ensure_editor_tab_present(&mut state);

        assert_eq!(tab_set(&state), vec![DockTab::Binder, DockTab::Editor]);
    }

    #[test]
    fn named_saved_layouts_round_trip_through_json() {
        let mut saved: std::collections::BTreeMap<String, egui_dock::DockState<DockTab>> =
            std::collections::BTreeMap::new();
        saved.insert(
            "Writing".to_string(),
            rendered(egui_dock::DockState::new(vec![DockTab::Editor])),
        );
        let mut research = egui_dock::DockState::new(vec![DockTab::Binder]);
        research.push_to_focused_leaf(DockTab::Corkboard);
        saved.insert("Research".to_string(), rendered(research));

        let json = serde_json::to_string(&saved).unwrap();
        let restored: std::collections::BTreeMap<String, egui_dock::DockState<DockTab>> =
            serde_json::from_str(&json).unwrap();

        assert_eq!(
            restored.keys().cloned().collect::<Vec<_>>(),
            vec!["Research".to_string(), "Writing".to_string()],
            "BTreeMap should keep saved layouts sorted by name"
        );
        assert_eq!(tab_set(&restored["Writing"]), vec![DockTab::Editor]);
        assert_eq!(
            tab_set(&restored["Research"]),
            vec![DockTab::Binder, DockTab::Corkboard]
        );
    }
}
