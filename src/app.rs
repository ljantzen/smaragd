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

/// Accumulates the ordered list of keyboard-focusable item `Id`s a top-level
/// dropdown's content renders this frame, in visual order — the list Up/Down
/// navigates over (see `handle_dropdown_arrows`). Rebuilt fresh every frame
/// (immediate mode), mirroring `binder_panel.rs`'s own `visible_rows`
/// accumulator for its file tree. Disabled rows are simply never pushed
/// (checked via `ui.is_enabled()`), so a temporarily-disabled group (e.g.
/// Versions' git actions while a git operation is running, via
/// `add_enabled_ui`) is transparently skipped without touching that call site;
/// `ui.separator()` calls are never routed through here at all, so they're
/// excluded the same way.
#[derive(Default)]
struct MenuNav {
    items: Vec<egui::Id>,
}

impl MenuNav {
    fn track(&mut self, ui: &egui::Ui, response: &egui::Response) {
        if ui.is_enabled() {
            self.items.push(response.id);
        }
    }

    fn button(&mut self, ui: &mut egui::Ui, label: &str) -> egui::Response {
        let response = ui.button(label);
        self.track(ui, &response);
        response
    }

    fn shortcut_button(
        &mut self,
        ui: &mut egui::Ui,
        label: &str,
        shortcut: Option<egui::KeyboardShortcut>,
    ) -> egui::Response {
        let response = menu_button_with_shortcut(ui, label, shortcut);
        self.track(ui, &response);
        response
    }
}

/// The 7 top-level menus, in menu-bar order — also the fixed cycle Left/Right
/// switch through in `handle_dropdown_arrows` (wrapping: Help's Right goes to
/// File, File's Left goes to Help).
const TOP_MENUS: [(&str, egui::Key); 7] = [
    ("File", egui::Key::F),
    ("Edit", egui::Key::E),
    ("View", egui::Key::V),
    ("Tools", egui::Key::T),
    ("Versions", egui::Key::S),
    ("Window", egui::Key::W),
    ("Help", egui::Key::H),
];

/// A stable popup `Id` derived purely from a top-level menu's label, rather than
/// `Popup::menu`'s own default (`response.id.with("popup")`, see
/// `Popup::default_response_id`) — needed so `handle_dropdown_arrows` can address
/// a *sibling* menu's popup by label alone on a Left/Right press, without having
/// that sibling's `Response` (it may not have rendered yet this frame).
fn top_menu_popup_id(label: &str) -> egui::Id {
    egui::Id::new("top_menu_popup").with(label)
}

/// A top-level menu-bar button (File/Edit/View/...) that also drops down when
/// `mnemonic` is pressed with Alt held, in addition to the normal click-to-toggle
/// behavior — a fixed Alt+letter menu-bar accelerator, matching the classic
/// Windows/GTK convention. Deliberately *not* part of the user-configurable
/// `ShortcutAction`/`shortcuts.rs` system: these are positional (whichever menu
/// happens to be first gets Alt+F, etc.) and never meant to be rebound.
///
/// Reimplements `egui::containers::menu::MenuButton::ui` rather than calling it,
/// since that helper always ties the dropdown's open state to the button's own
/// click (`Popup::menu`'s built-in toggle) with no hook to force it open from an
/// unrelated keypress or to switch it from a sibling menu (see
/// `handle_dropdown_arrows`'s Left/Right handling). Open/close is driven
/// explicitly via `Popup::open_id`/`toggle_id` against a stable, label-derived
/// popup id (`top_menu_popup_id`) instead — `Popup::open_id`'s own doc comment
/// ("Open the given popup and close all others") is exactly the mutual-exclusion
/// this app wants across the 7 top-level menus, and is safe to rely on here
/// specifically because this simple Memory-backed popup mechanism is never used
/// for the nested submenus (Theme/Layouts/multi-folder Export Manuscript), which
/// need independent, simultaneously-open parent+child state instead. Safe to
/// skip `MenuConfig::find`/`MenuBar::config`, which `MenuButton::ui` normally
/// consults, since nothing in this app ever calls `MenuBar::config`/
/// `MenuButton::config` to override the ambient default.
fn top_menu_button(
    ui: &mut egui::Ui,
    label: &str,
    mnemonic: egui::Key,
    content: impl FnOnce(&mut egui::Ui, &mut MenuNav),
) {
    let popup_id = top_menu_popup_id(label);
    if ui.input_mut(|i| i.consume_key(egui::Modifiers::ALT, mnemonic)) {
        egui::Popup::open_id(ui.ctx(), popup_id);
    }
    let response = ui.button(label);
    if response.clicked() {
        egui::Popup::toggle_id(ui.ctx(), popup_id);
    }
    let mut nav = MenuNav::default();
    egui::Popup::menu(&response)
        .id(popup_id)
        .open_memory(None)
        .show(|ui| content(ui, &mut nav));
    handle_dropdown_arrows(ui.ctx(), &nav, popup_id, label);
}

/// Up/Down/Left/Right handling for whichever top-level dropdown is currently
/// open, called once per dropdown right after its content has rendered (so
/// `nav.items` is complete for the frame) — mirrors `binder_panel.rs`'s own
/// "handle Up/Down once, after the whole list is built" structure.
///
/// Up/Down move the highlighted item within `nav.items`, wrapping at the ends.
/// Left/Right switch to the previous/next of `TOP_MENUS`, wrapping there too.
/// Opening with nothing yet focused inside (however it was opened — click, Alt
/// mnemonic, or a Left/Right switch from a sibling) lands on the first item, so
/// Down always has a defined starting point.
///
/// That auto-focus only fires the first time `nav.items` is non-empty after the
/// popup opens — tracked via a small per-popup flag in `ctx.data`, cleared
/// whenever the popup isn't open — rather than on every later frame the
/// dropdown merely happens to still be nominally open with nothing of its own
/// focused. That distinction matters because no menu item anywhere in this file
/// calls `ui.close()` after acting (clicking "Open Document…", say, doesn't
/// close the File dropdown it lives in) — harmless before this function
/// existed, since the dropdown just sat there open and inert, but actively
/// disruptive once this function started auto-focusing: without this guard, a
/// still-technically-open File menu would re-steal focus from whatever dialog
/// "Open Document…" just opened, every single frame, before that dialog's own
/// text field could ever process so much as an Enter press. (A simpler-looking
/// "was the popup already open last frame" check, via `Context::read_response`,
/// doesn't work here: egui registers that a frame *before* `nav.items` actually
/// becomes non-empty — a popup's content only starts rendering the frame after
/// it's marked open, its own settling delay — so that signal is one frame out
/// of step with the one this function actually needs.)
fn handle_dropdown_arrows(ctx: &egui::Context, nav: &MenuNav, popup_id: egui::Id, label: &str) {
    if !egui::Popup::is_id_open(ctx, popup_id) {
        ctx.data_mut(|d| d.remove::<bool>(popup_id));
        return;
    }
    if nav.items.is_empty() {
        return;
    }
    let had_items_last_time = ctx.data(|d| d.get_temp::<bool>(popup_id)).unwrap_or(false);
    ctx.data_mut(|d| d.insert_temp(popup_id, true));

    let focused = ctx.memory(|m| m.focused());
    let Some(current) = focused.and_then(|id| nav.items.iter().position(|i| *i == id)) else {
        if !had_items_last_time {
            ctx.memory_mut(|m| m.request_focus(nav.items[0]));
        }
        return;
    };

    // Claim vertical+horizontal arrows so egui's own geometric "nearest widget in
    // that screen direction" focus jump (see `Focus::end_pass`/
    // `find_widget_in_direction` in egui's own source) never fires instead — the
    // same technique `binder_panel.rs`'s `ARROW_KEYS_FILTER` and egui's own
    // `Slider` arrow-key handling both use.
    ctx.memory_mut(|m| {
        m.set_focus_lock_filter(
            nav.items[current],
            egui::EventFilter {
                tab: false,
                horizontal_arrows: true,
                vertical_arrows: true,
                escape: false,
            },
        )
    });

    let len = nav.items.len();
    let next = if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
        Some((current + 1) % len)
    } else if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
        Some((current + len - 1) % len)
    } else {
        None
    };
    // Guard against a redundant `request_focus` call when already at the target
    // index — it unconditionally resets the focus-lock filter set just above,
    // which would otherwise reopen a one-frame gap every frame at the ends.
    if let Some(next) = next
        && next != current
    {
        ctx.memory_mut(|m| m.request_focus(nav.items[next]));
    }

    let right = ctx.input(|i| i.key_pressed(egui::Key::ArrowRight));
    let left = ctx.input(|i| i.key_pressed(egui::Key::ArrowLeft));
    if right || left {
        let my_index = TOP_MENUS
            .iter()
            .position(|(l, _)| *l == label)
            .expect("label is always one of TOP_MENUS");
        let delta = if right { 1 } else { TOP_MENUS.len() - 1 };
        let target = (my_index + delta) % TOP_MENUS.len();
        egui::Popup::open_id(ctx, top_menu_popup_id(TOP_MENUS[target].0));
    }
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
        template_id: String,
    },
    /// Commit with the (editable) message the prompt was confirmed with; `push_after`
    /// carries through whether this was "Commit" or "Commit and Push".
    GitCommit {
        push_after: bool,
    },
    /// Save the current dock layout under the confirmed name (see
    /// `save_named_layout`).
    SaveLayout,
    /// Save the current project's structure as a new custom template under the
    /// confirmed name (see `save_project_as_template`).
    SaveProjectAsTemplate,
}

struct PendingPrompt {
    action: PromptAction,
    state: NamePromptState,
}

/// An error-severity notification, shown as a floating, auto-dismissing box
/// stacked in the corner of the window rather than as status-bar text — see
/// `SmaragdApp::push_error_toast`/`show_toasts`.
struct Toast {
    message: String,
    shown_at: std::time::Instant,
}

/// Built-in toast duration used when `Settings::toast_duration_secs` is
/// unconfigured (`0`) — long enough to read a short sentence without having to
/// rush, short enough that several errors in a row don't pile up into a
/// permanent wall of boxes.
const DEFAULT_TOAST_DURATION: std::time::Duration = std::time::Duration::from_secs(6);

/// Built-in status-bar auto-clear duration used when
/// `Settings::status_message_duration_secs` is unconfigured (`0`) — a little
/// more generous than the toast default, since a routine confirmation has no
/// manual dismiss button of its own to cut that wait short.
const DEFAULT_STATUS_MESSAGE_DURATION: std::time::Duration = std::time::Duration::from_secs(8);

/// Resolve `Settings::toast_duration_secs`'s blank-means-unset (`0`) convention
/// to an actual `Duration` — same shape as `editor_font::resolve_size`/
/// `pomodoro::resolve_durations`.
fn resolve_toast_duration(settings: &Settings) -> std::time::Duration {
    match settings.toast_duration_secs {
        0 => DEFAULT_TOAST_DURATION,
        secs => std::time::Duration::from_secs(secs as u64),
    }
}

/// Resolve `Settings::status_message_duration_secs`'s blank-means-unset (`0`)
/// convention to an actual `Duration`.
fn resolve_status_message_duration(settings: &Settings) -> std::time::Duration {
    match settings.status_message_duration_secs {
        0 => DEFAULT_STATUS_MESSAGE_DURATION,
        secs => std::time::Duration::from_secs(secs as u64),
    }
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
    Tags,
    Metadata,
    Editor,
    Preview,
    Corkboard,
    Pomodoro,
    WordCount,
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

/// Live editing buffers for the open document's frontmatter, and the bookkeeping
/// needed to keep them in sync with whichever document is open — grouped out of
/// `SmaragdApp` (2026-07-31 code-quality review) since all three always change
/// together, driven by `refresh_metadata_if_needed`/`apply_metadata_edits_if_changed`.
#[derive(Default)]
struct MetadataState {
    /// There's no "closed" state to represent here, since the Metadata dock
    /// tab's own presence in `dock_state` is what tracks visibility.
    draft: MetadataDraft,
    /// Which document `draft`/`last_applied` were last computed for, so a later
    /// frame can tell whether `editor.open_path` has since changed.
    computed_for: Option<PathBuf>,
    /// The `DocumentMeta` last written into `editor.buffer` — compared against
    /// `draft.to_meta()` each frame to notice a live edit without re-writing the
    /// buffer when nothing changed.
    last_applied: DocumentMeta,
}

/// Every `[[wikilink]]` elsewhere in the project pointing at the open document,
/// and which document it was computed for — grouped out of `SmaragdApp`
/// (2026-07-31 code-quality review) since both always change together, driven
/// by `refresh_backlinks_if_needed`.
#[derive(Default)]
struct BacklinksState {
    entries: Vec<BacklinkEntry>,
    /// Which document `entries` was last computed for.
    computed_for: Option<PathBuf>,
}

/// The open document's tags, the project-wide tag search box, and the
/// bookkeeping needed to keep both in sync — grouped out of `SmaragdApp`
/// (2026-07-31 code-quality review) since all five always change together,
/// driven by `refresh_tags_if_needed`/`refresh_tag_search_if_needed`.
#[derive(Default)]
struct TagsState {
    /// The open document's tags (frontmatter `tags:` merged with inline `#tag`
    /// mentions), each paired with the other project documents sharing it.
    entries: Vec<crate::project::TagGroup>,
    /// Which document `entries` was last computed for.
    computed_for: Option<PathBuf>,
    /// Live text of the Tags dock's search box — typing here (or clicking one
    /// of `entries`' tag headings, which fills it in) requests a vault-wide
    /// lookup of every document carrying that tag.
    search_text: String,
    /// Vault-wide search results for `search_text` (see
    /// `Project::documents_with_tag`), recomputed only when it changes.
    search_results: Vec<(PathBuf, String)>,
    /// Which `search_text` value `search_results` was last computed for.
    search_computed_for: String,
}

/// The open project's word count, its background-recompute machinery, and the
/// characters-typed activity counter — grouped out of `SmaragdApp` (2026-07-31
/// code-quality review) since all six always change together, driven by
/// `spawn_word_count_recompute`/`track_char_activity`.
#[derive(Default)]
struct WordCountState {
    /// The open project's word count under `ProjectMeta::word_count_scope`,
    /// shown in the Word Count dock tab and the status bar. Recomputed only on
    /// a handful of triggers (see `spawn_word_count_recompute`'s callers) via a
    /// background thread, never every frame — unlike `metadata_panel`'s live
    /// per-document count from the open buffer, which is unrelated to this.
    cache: usize,
    /// A word-count recompute currently running on a background thread, if any —
    /// walking every tracked document's content from disk could be slow for a
    /// large project, so (like `pending_git`'s push/pull) this never runs
    /// synchronously on the UI thread. `None` once `poll_word_count` has picked
    /// up its result.
    pending: Option<std::sync::mpsc::Receiver<usize>>,
    /// `editor.dirty` as of the previous frame — compared against its current
    /// value in `refresh_word_count_if_needed` to edge-detect "a save just
    /// happened" (dirty went `true` -> `false`), the moment any of the three
    /// existing save paths (explicit save, focus-loss autosave, closing the
    /// document) commits bytes to disk, without needing a call at each site.
    last_dirty: bool,
    /// Characters typed *and* deleted this session, in tracked documents only
    /// (see `Project::is_path_tracked`) — an activity counter, not a net delta:
    /// typing 100 characters then deleting them all reads 200, not 0. Purely
    /// informational (no target), kept only in memory — not persisted to
    /// `project.json` — and reset when a project is opened (see `set_project`)
    /// or the panel's "Reset Session" is clicked (see `handle_word_count_event`).
    /// Updated by `track_char_activity`, called after the dock renders each frame.
    char_activity: u64,
    /// The open document's buffer length (in `chars()`), as of the last frame
    /// `track_char_activity` ran — compared against the current frame's length
    /// to find how many characters just changed. `None` right after opening or
    /// switching documents, so the jump between two different documents' lengths
    /// is never miscounted as characters typed.
    char_activity_last_len: Option<usize>,
    /// Which document `char_activity_last_len` was captured for — lets
    /// `track_char_activity` notice a document switch (vs. an edit to the same
    /// document) and reset the baseline instead of diffing across two unrelated
    /// buffers.
    char_activity_tracked_path: Option<PathBuf>,
}

pub struct SmaragdApp {
    project: Option<Project>,
    editor: EditorState,
    selected_path: Option<PathBuf>,
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
    /// Every selectable project template: the 4 built-ins plus whatever custom
    /// templates are in `project_template::global_project_templates_dir()`.
    /// Rebuilt by `reload_project_templates`.
    project_templates: Vec<crate::project_template::ProjectTemplate>,
    /// The open Export dialog, if any — see `ExportState`.
    export: Option<ExportState>,
    /// Every selectable typesetting style: the 2 built-ins plus whatever
    /// `*.toml` files are in `export::style::global_styles_dir()`. Rebuilt by
    /// `reload_typeset_styles`.
    typeset_styles: Vec<crate::export::style::TypesetStyle>,
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

impl SmaragdApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        crate::editor_font::install(&cc.egui_ctx);

        let settings = crate::settings::config_file_path()
            .map(|path| Settings::load_from_path(&path))
            .unwrap_or_default();
        cc.egui_ctx.set_theme(settings.theme_preference);
        cc.egui_ctx.set_zoom_factor(settings.resolve_ui_scale());
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
            status_message: None,
            status_message_set_at: None,
            toasts: Vec::new(),
            settings,
            show_settings: false,
            show_about: false,
            prompt: None,
            recording_shortcut: None,
            settings_category: ui::settings_panel::SettingsCategory::General,
            find_replace: FindReplaceState::default(),
            card_draft: None,
            command_prompt: CommandPromptState::default(),
            open_document_prompt: ui::open_document_prompt::OpenDocumentPromptState::default(),
            new_project_template_prompt:
                ui::new_project_template_prompt::NewProjectTemplatePromptState::default(),
            metadata: MetadataState::default(),
            backlinks: BacklinksState::default(),
            tags: TagsState::default(),
            word_count: WordCountState::default(),
            dock_state: Self::load_dock_state(),
            saved_layouts: Self::load_saved_layouts(),
            pending_git: None,
            plugin_engine: crate::plugins::PluginEngine::default(),
            plugin_shortcuts: Vec::new(),
            color_themes: Vec::new(),
            project_templates: Vec::new(),
            export: None,
            typeset_styles: Vec::new(),
            pomodoro: crate::pomodoro::PomodoroState::new(&initial_pomodoro_durations),
            focus_binder_requested: false,
            focus_mode: false,
        };
        app.reload_typeset_styles();
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
            status_message: None,
            status_message_set_at: None,
            toasts: Vec::new(),
            settings,
            show_settings: false,
            show_about: false,
            prompt: None,
            recording_shortcut: None,
            settings_category: ui::settings_panel::SettingsCategory::General,
            find_replace: FindReplaceState::default(),
            card_draft: None,
            command_prompt: CommandPromptState::default(),
            open_document_prompt: ui::open_document_prompt::OpenDocumentPromptState::default(),
            new_project_template_prompt:
                ui::new_project_template_prompt::NewProjectTemplatePromptState::default(),
            metadata: MetadataState::default(),
            backlinks: BacklinksState::default(),
            tags: TagsState::default(),
            word_count: WordCountState::default(),
            dock_state: default_dock_state(),
            saved_layouts: std::collections::BTreeMap::new(),
            pending_git: None,
            plugin_engine: crate::plugins::PluginEngine::default(),
            plugin_shortcuts: Vec::new(),
            color_themes: Vec::new(),
            project_templates: Vec::new(),
            export: None,
            typeset_styles: Vec::new(),
            pomodoro: crate::pomodoro::PomodoroState::new(&pomodoro_durations),
            focus_binder_requested: false,
            focus_mode: false,
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

    /// Rebuild `project_templates` from the 4 built-ins plus whatever custom
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

    /// Reload the selectable typesetting styles (`self.typeset_styles`): the 2
    /// built-ins plus every `*.toml` file in `export::style::global_styles_dir()`.
    /// Called at startup and from the Export dialog's "Reload Styles" action.
    fn reload_typeset_styles(&mut self) {
        let styles_dir = crate::export::style::global_styles_dir();
        let dirs: Vec<&Path> = styles_dir.as_deref().into_iter().collect();
        let (styles, errors) = crate::export::style::load(&dirs);
        self.typeset_styles = styles;
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

    /// Open `path` as a project. Used for the automatic "reopen last project" path at
    /// startup, where a missing `.smaragd` marker must just be reported (not
    /// interactively resolved) — the user didn't just explicitly ask to open this
    /// folder, so an unprompted modal dialog on launch would be wrong.
    fn open_project(&mut self, ctx: &egui::Context, path: &Path) {
        match Project::load_from_folder(path) {
            Ok(project) => self.set_project(ctx, project, path),
            Err(err) => {
                self.push_error_toast(format!("Couldn't open {}: {err}", path.display()));
            }
        }
    }

    fn set_project(&mut self, ctx: &egui::Context, mut project: Project, path: &Path) {
        if self.settings.create_starter_folders {
            Self::ensure_starter_folders(&mut project);
        }
        self.project = Some(project);
        self.editor = EditorState::default();
        self.selected_path = None;
        self.clear_status_message();
        self.settings.last_project_path = Some(path.to_path_buf());
        self.persist_settings();
        self.maybe_offer_git_support();
        if let Some(project) = &self.project
            && let Err(err) = Self::ensure_git_repo(project)
        {
            self.push_error_toast(format!("Couldn't initialize git: {err}"));
        }
        self.reload_plugins();
        self.word_count.last_dirty = false;
        self.word_count.char_activity = 0;
        self.word_count.char_activity_last_len = None;
        self.word_count.char_activity_tracked_path = None;
        self.spawn_word_count_recompute(ctx);
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
            "This project is already a git repository. Enable Smaragd's git integration (commit/push/pull from the Versions menu)?"
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
                self.push_error_toast(format!("Couldn't initialize git: {err}"));
                return;
            }
            if let Err(err) = project.enable_git_support() {
                self.push_error_toast(format!("Couldn't save settings: {err}"));
            }
        } else if let Err(err) = project.decline_git_support() {
            self.push_error_toast(format!("Couldn't save settings: {err}"));
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
            self.push_error_toast("No project open");
            return;
        };
        if !crate::git::is_available() {
            self.push_error_toast("git was not found on this system");
            return;
        }
        if let Err(err) = Self::init_repo_if_needed(&project.root) {
            self.push_error_toast(format!("Couldn't initialize git: {err}"));
            return;
        }

        let Some(project) = &mut self.project else {
            return;
        };
        match project.enable_git_support() {
            Ok(()) => self.set_status_message("Git support enabled"),
            Err(err) => self.push_error_toast(format!("Couldn't save settings: {err}")),
        }
    }

    /// Open the commit-message prompt (the existing name-prompt modal, reused),
    /// pre-filled with a default message. Shared by the Versions menu, the
    /// `GitCommit` shortcut, and `:git commit`/`:git backup` with no inline message.
    fn prompt_git_commit(&mut self, push_after: bool) {
        let Some(project) = &self.project else {
            self.push_error_toast("No project open");
            return;
        };
        if !project.meta.git_enabled {
            self.push_error_toast("Git support isn't enabled for this project");
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
                "Smaragd backup",
            ),
        });
    }

    fn run_git_commit(&mut self, ctx: &egui::Context, message: &str, push_after: bool) {
        let Some(project) = &self.project else {
            self.push_error_toast("No project open");
            return;
        };
        if !project.meta.git_enabled {
            self.push_error_toast("Git support isn't enabled for this project");
            return;
        }
        if let Err(err) = Self::ensure_git_repo(project) {
            self.push_error_toast(format!("Couldn't initialize git: {err}"));
            return;
        }
        match crate::git::commit_all(&project.root, message) {
            Ok(()) => {
                self.set_status_message("Committed");
                if push_after {
                    self.run_git_push(ctx);
                }
            }
            Err(crate::git::GitError::NothingToCommit) => {
                self.set_status_message("Nothing to commit");
            }
            Err(err) => self.push_error_toast(format!("Commit failed: {err}")),
        }
    }

    fn run_git_push(&mut self, ctx: &egui::Context) {
        let Some(project) = &self.project else {
            self.push_error_toast("No project open");
            return;
        };
        if !project.meta.git_enabled {
            self.push_error_toast("Git support isn't enabled for this project");
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
            self.push_error_toast("No project open");
            return;
        };
        if !project.meta.git_enabled {
            self.push_error_toast("Git support isn't enabled for this project");
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
            self.push_error_toast("A git operation is already in progress");
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
        self.set_status_message(format!("{}ing…", operation.label()));
        self.pending_git = Some((operation, receiver));
    }

    /// Check whether the in-flight `pending_git` operation (if any) has finished, and
    /// apply its result — a status message, plus a binder rescan on a successful pull.
    /// Called every frame; a no-op whenever nothing is pending or the background
    /// thread hasn't sent its result yet.
    fn poll_git_operation(&mut self, ctx: &egui::Context) {
        let Some((_, receiver)) = &self.pending_git else {
            return;
        };
        let result = match receiver.try_recv() {
            Ok(result) => result,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                let (operation, _) = self.pending_git.take().expect("checked above");
                self.push_error_toast(format!(
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
                    self.spawn_word_count_recompute(ctx);
                }
                self.set_status_message(format!("{}ed", operation.label()));
            }
            Err(err) => {
                self.push_error_toast(format!("{} failed: {err}", operation.label()));
            }
        }
    }

    fn persist_settings(&mut self) {
        let Some(path) = crate::settings::config_file_path() else {
            return;
        };
        if let Err(err) = self.settings.save_to_path(&path) {
            self.push_error_toast(format!("Couldn't save settings: {err}"));
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
    /// Project" menu item). If `path` has never been opened by smaragd before (no
    /// `.smaragd/project.json`), offers via a native Yes/No dialog to set it up in
    /// place, matching `delete_node`'s confirmation pattern.
    fn open_project_or_offer_to_adopt(&mut self, ctx: &egui::Context, path: &Path) {
        match Project::load_from_folder(path) {
            Ok(project) => self.set_project(ctx, project, path),
            Err(LoadError::NotInitialized(_)) => {
                let adopt = rfd::MessageDialog::new()
                    .set_title("Set Up Project")
                    .set_description(format!(
                        "\"{}\" hasn't been opened in smaragd before. Set it up as a smaragd project here?",
                        path.display()
                    ))
                    .set_level(rfd::MessageLevel::Info)
                    .set_buttons(rfd::MessageButtons::YesNo)
                    .show();
                if adopt == rfd::MessageDialogResult::Yes {
                    match Project::initialize(path) {
                        Ok(project) => self.set_project(ctx, project, path),
                        Err(err) => {
                            self.push_error_toast(format!(
                                "Couldn't set up {}: {err}",
                                path.display()
                            ));
                        }
                    }
                }
            }
            Err(err) => {
                self.push_error_toast(format!("Couldn't open {}: {err}", path.display()));
            }
        }
    }

    /// Open the OS's native folder-picker dialog and, if the user selects a folder,
    /// open it as a project immediately (offering to adopt it if needed).
    fn browse_for_project(&mut self, ctx: &egui::Context) {
        if let Some(path) = rfd::FileDialog::new().pick_folder() {
            self.open_project_or_offer_to_adopt(ctx, &path);
        }
    }

    /// Start the "New Project" flow: first the template-choice modal (see
    /// `start_new_project_with_template` for the rest of the flow, once a template's
    /// been chosen).
    fn start_new_project(&mut self) {
        self.new_project_template_prompt.request_open();
    }

    /// Continue "New Project" once `template_id` has been chosen: pick a parent
    /// folder via the native folder picker, then prompt for the new project's name
    /// via the existing name-prompt modal.
    fn start_new_project_with_template(&mut self, template_id: String) {
        let Some(location) = rfd::FileDialog::new().pick_folder() else {
            return;
        };
        self.prompt = Some(PendingPrompt {
            action: PromptAction::NewProject {
                location,
                template_id,
            },
            state: NamePromptState::new("New Project", "Create", ""),
        });
    }

    fn create_project(
        &mut self,
        ctx: &egui::Context,
        location: &Path,
        name: &str,
        template_id: &str,
    ) {
        let root = location.join(name);
        if root.exists() {
            // Unlike the adopt flow, "New Project" should only ever create a fresh
            // folder — silently folding an unrelated existing folder in as a project
            // would be surprising.
            self.push_error_toast(format!("{} already exists", root.display()));
            return;
        }
        match Project::initialize(&root) {
            Ok(mut project) => {
                // An id that no longer resolves (e.g. a custom template deleted
                // between picker and confirm) is treated as "no scaffolding" rather
                // than a hard error — the same fallback Blank itself produces.
                let template_error =
                    crate::project_template::find(&self.project_templates, template_id)
                        .and_then(|template| template.apply(&mut project).err());
                // `set_project` unconditionally clears `status_message`, so a
                // template-apply error must be recorded after it runs, not before.
                self.set_project(ctx, project, &root);
                if let Some(err) = template_error {
                    self.push_error_toast(format!("Couldn't apply template: {err}"));
                }
            }
            Err(err) => {
                self.push_error_toast(format!("Couldn't create project: {err}"));
            }
        }
    }

    /// Open the "Save Project as Template" name-prompt modal.
    fn prompt_save_project_as_template(&mut self) {
        self.prompt = Some(PendingPrompt {
            action: PromptAction::SaveProjectAsTemplate,
            state: NamePromptState::new("Save Project as Template", "Save", ""),
        });
    }

    fn save_project_as_template(&mut self, name: &str) {
        let Some(project) = &self.project else {
            self.push_error_toast("No project open");
            return;
        };
        let Some(dir) = crate::project_template::global_project_templates_dir() else {
            self.push_error_toast("Couldn't determine templates directory");
            return;
        };
        match crate::project_template::save_from_project(&dir, name, project) {
            Ok(_) => {
                self.set_status_message(format!("Saved template \"{name}\""));
                self.reload_project_templates();
            }
            Err(err) => {
                self.push_error_toast(format!("Couldn't save template: {err}"));
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
                self.push_error_toast(format!("Couldn't open {}: {err}", path.display()));
            }
        }
    }

    /// Close the currently open document (silently autosaving first if dirty — same
    /// convention as `open_document`/`rename_node`, no discard/cancel prompt).
    fn close_document(&mut self, ctx: &egui::Context) {
        if let Err(err) = self.editor.close() {
            self.push_error_toast(format!("Couldn't save before closing: {err}"));
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

    /// Refresh `metadata.draft` from the open document's current frontmatter
    /// (parsed from the live buffer, not necessarily what's on disk yet, so it
    /// reflects any unsaved edits to the block itself) whenever the open document
    /// has changed since the last computation — a no-op most frames. Called before
    /// the dock renders each frame, alongside `refresh_backlinks_if_needed`.
    fn refresh_metadata_if_needed(&mut self) {
        if self.editor.open_path == self.metadata.computed_for {
            return;
        }
        let meta = match &self.editor.open_path {
            Some(_) => crate::frontmatter::parse(&self.editor.buffer),
            None => DocumentMeta::default(),
        };
        self.metadata.draft = MetadataDraft::from_meta(&meta);
        self.metadata.last_applied = meta;
        self.metadata.computed_for = self.editor.open_path.clone();
        // Only set — never clear — the status message here: most document switches
        // have nothing wrong with their frontmatter, and blanking whatever the
        // status bar was already showing (e.g. a just-completed git operation) on
        // every single switch would be far noisier than useful.
        if self.editor.open_path.is_some()
            && let Some(err) = crate::frontmatter::validate(&self.editor.buffer)
        {
            self.push_error_toast(err.to_string());
        }
    }

    /// Notice a live edit to `metadata.draft` (typed into the Metadata dock tab this
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
        let current = self.metadata.draft.to_meta();
        if current == self.metadata.last_applied {
            return;
        }
        self.editor.buffer = crate::frontmatter::write_back(&self.editor.buffer, &current);
        self.editor.mark_dirty();
        self.metadata.last_applied = current;
    }

    /// Refresh `backlinks` from the project whenever the open document has changed
    /// since the last scan — a no-op most frames. Called before the dock renders
    /// each frame; recomputing regardless of whether the Backlinks tab happens to
    /// be visible right now is simplest, since the scan itself is cheap (see
    /// `Project::backlinks`).
    fn refresh_backlinks_if_needed(&mut self) {
        if self.editor.open_path == self.backlinks.computed_for {
            return;
        }
        self.recompute_backlinks();
    }

    fn recompute_backlinks(&mut self) {
        self.backlinks.entries = match (&self.project, &self.editor.open_path) {
            (Some(project), Some(path)) => project.backlinks(path),
            _ => Vec::new(),
        };
        self.backlinks.computed_for = self.editor.open_path.clone();
    }

    /// Refresh `tags` from the project whenever the open document has changed
    /// since the last scan — a no-op most frames. Called before the dock
    /// renders each frame, alongside `refresh_backlinks_if_needed`.
    fn refresh_tags_if_needed(&mut self) {
        if self.editor.open_path == self.tags.computed_for {
            return;
        }
        self.recompute_tags();
    }

    fn recompute_tags(&mut self) {
        self.tags.entries = match (&self.project, &self.editor.open_path) {
            (Some(project), Some(path)) => project.related_by_tag(path),
            _ => Vec::new(),
        };
        self.tags.computed_for = self.editor.open_path.clone();
    }

    /// Refresh `tags.search_results` whenever `tags.search_text` has changed
    /// since the last scan — a no-op most frames. Called *after* the dock
    /// renders each frame (unlike `refresh_tags_if_needed`), since the search
    /// box is a live text field the user may have just edited that same
    /// frame — same reasoning as why `apply_metadata_edits_if_changed` runs
    /// after rendering rather than before.
    fn refresh_tag_search_if_needed(&mut self) {
        if self.tags.search_text == self.tags.search_computed_for {
            return;
        }
        let query = self.tags.search_text.trim();
        self.tags.search_results = match (&self.project, query.is_empty()) {
            (Some(project), false) => project.documents_with_tag(query),
            _ => Vec::new(),
        };
        self.tags.search_computed_for = self.tags.search_text.clone();
    }

    /// Recompute `word_count.cache` whenever a save just completed — edge-detects
    /// `editor.dirty` going `true` -> `false` this frame, the moment any of the
    /// three existing save paths (explicit `Ctrl+S`, focus-loss autosave inside
    /// `editor_panel::show`, or `close_document`) commits bytes to disk, without
    /// needing a call at each of those sites. Unlike `refresh_backlinks_if_needed`/
    /// `refresh_tags_if_needed` (keyed on which document is open), word count
    /// doesn't depend on the open document at all, so a dirty-edge check is the
    /// right trigger here instead. A no-op most frames.
    fn refresh_word_count_if_needed(&mut self, ctx: &egui::Context) {
        let just_saved = self.word_count.last_dirty && !self.editor.dirty;
        self.word_count.last_dirty = self.editor.dirty;
        if just_saved {
            self.spawn_word_count_recompute(ctx);
        }
    }

    /// Update `word_count.char_activity` from however much the open document's
    /// buffer length changed since the last frame — both growing (typing) and
    /// shrinking (deleting) count toward the total, so typing 100 characters
    /// then deleting them all adds 200, not 0 (see that field's doc
    /// comment). Cheap (no disk I/O, no full-project walk — see
    /// `Project::is_path_tracked`), so unlike `word_count.cache` this runs every
    /// frame. Called after the dock renders each frame, alongside
    /// `apply_metadata_edits_if_changed`.
    fn track_char_activity(&mut self) {
        let Some(open_path) = self.editor.open_path.clone() else {
            self.word_count.char_activity_last_len = None;
            self.word_count.char_activity_tracked_path = None;
            return;
        };
        if self.word_count.char_activity_tracked_path.as_deref() != Some(open_path.as_path()) {
            // Just opened or switched to this document — the previous frame's
            // length (if any) belonged to a different buffer, so there's
            // nothing meaningful to diff against yet.
            self.word_count.char_activity_tracked_path = Some(open_path.clone());
            self.word_count.char_activity_last_len = Some(self.editor.buffer.chars().count());
            return;
        }
        let current_len = self.editor.buffer.chars().count();
        if let Some(previous_len) = self.word_count.char_activity_last_len {
            let is_tracked = self.project.as_ref().is_some_and(|project| {
                project.is_path_tracked(&open_path, project.meta.word_count_scope)
            });
            if is_tracked {
                self.word_count.char_activity += current_len.abs_diff(previous_len) as u64;
            }
        }
        self.word_count.char_activity_last_len = Some(current_len);
    }

    /// Kick off a word-count recompute on a background thread for the current
    /// project (a no-op with none open) — walking every tracked document's
    /// content from disk could be slow for a large project, so (like `git push`/
    /// `pull`, see `spawn_git_operation`) this never runs synchronously on the UI
    /// thread. Refuses to start a second computation while one's already in
    /// flight rather than queuing or racing it. The spawned thread requests a
    /// repaint once it has a result, so `poll_word_count` (called every frame)
    /// picks it up promptly.
    fn spawn_word_count_recompute(&mut self, ctx: &egui::Context) {
        let Some(project) = &self.project else {
            self.word_count.cache = 0;
            return;
        };
        if self.word_count.pending.is_some() {
            return;
        }
        // Clone just the data the walk needs rather than requiring `Project`
        // itself to be `Send` — `root`/`tree`/`meta` are all plain, cloneable
        // data (see their derives in `project/mod.rs`).
        let root = project.root.clone();
        let tree = project.tree.clone();
        let meta = project.meta.clone();
        let scope = meta.word_count_scope;
        let (sender, receiver) = std::sync::mpsc::channel();
        let repaint_ctx = ctx.clone();
        std::thread::spawn(move || {
            let snapshot = crate::project::Project { root, tree, meta };
            let total = snapshot.word_count(scope);
            let _ = sender.send(total);
            repaint_ctx.request_repaint();
        });
        self.word_count.pending = Some(receiver);
    }

    /// Check whether an in-flight `word_count.pending` recompute has finished,
    /// and if so apply it to `word_count.cache` and roll the Session Target's
    /// baseline forward if the calendar day has changed (see
    /// `Project::maybe_roll_over_session`). Called every frame, mirroring
    /// `poll_git_operation`; a no-op whenever nothing is pending or the
    /// background thread hasn't sent its result yet.
    fn poll_word_count(&mut self) {
        let Some(receiver) = &self.word_count.pending else {
            return;
        };
        let total = match receiver.try_recv() {
            Ok(total) => total,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.word_count.pending = None;
                return;
            }
        };
        self.word_count.pending = None;
        self.word_count.cache = total;
        if let Some(project) = &mut self.project {
            let _ = project.maybe_roll_over_session(total);
        }
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
            self.push_error_toast("Open a document before entering Focus Mode.");
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
            self.push_error_toast(format!("No project open — can't resolve [[{target}]]"));
            return;
        };
        if let Some(node) = project.tree.find_document_by_stem(&target) {
            let path = node.path.clone();
            self.open_document(&path);
            return;
        }
        if !force_create {
            self.push_error_toast(format!("No note found for [[{target}]]"));
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
            self.push_error_toast(format!(
                "Couldn't create a note for [[{target}]]: no document is open"
            ));
            return;
        };
        self.create_document(&parent, target);
    }

    fn handle_binder_event(&mut self, ctx: &egui::Context, event: BinderEvent) {
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
            BinderEvent::SetFolderRole { path, role } => self.set_folder_role(ctx, &path, role),
            BinderEvent::SetPicklistFolder { field, path } => self.set_picklist_folder(field, path),
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
            self.push_error_toast(format!("Couldn't save settings: {err}"));
        }

        let Some(style) = crate::export::style::find(&self.typeset_styles, &style_id).cloned()
        else {
            match action {
                ui::export_panel::ExportAction::Close => self.export = None,
                ui::export_panel::ExportAction::ReloadStyles => self.reload_typeset_styles(),
                _ => self.push_error_toast("No typesetting style selected"),
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
        let docs = crate::export::gather(project, folder, self.settings.typewriter_quotes);
        match crate::export::docx::export_docx(&docs, meta, style, &project.root, out_path) {
            Ok(()) => {
                self.set_status_message(format!("Exported to {}", out_path.display()));
            }
            Err(err) => {
                self.push_error_toast(format!("Export failed: {err}"));
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
        let docs = crate::export::gather(project, folder, self.settings.typewriter_quotes);
        match crate::export::epub::export_epub(&docs, meta, style, &project.root, out_path) {
            Ok(()) => {
                self.set_status_message(format!("Exported to {}", out_path.display()));
            }
            Err(err) => {
                self.push_error_toast(format!("Export failed: {err}"));
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
        let docs = crate::export::gather(project, folder, self.settings.typewriter_quotes);
        match crate::export::pdf::export_pdf(&docs, meta, style, &project.root, out_path) {
            Ok(spine_width_in) => {
                self.set_status_message(format!(
                    "Exported to {} — estimated spine width: {spine_width_in:.2}in",
                    out_path.display()
                ));
            }
            Err(err) => {
                self.push_error_toast(format!("Export failed: {err}"));
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
                self.push_error_toast(format!("Couldn't move {}: {err}", path.display()));
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
                self.push_error_toast(format!("Couldn't move {}: {err}", path.display()));
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
                    self.push_error_toast(format!("Couldn't delete card: {err}"));
                }
            }
            CorkboardEvent::MoveCard { id, new_index } => {
                if let Some(project) = &mut self.project
                    && let Err(err) = project.move_story_card(id, new_index)
                {
                    self.push_error_toast(format!("Couldn't reorder card: {err}"));
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
                    self.push_error_toast(format!("Couldn't save desire: {err}"));
                }
            }
            CorkboardEvent::SetProtagonistMisbelief(misbelief) => {
                if let Some(project) = &mut self.project
                    && let Err(err) = project.set_protagonist_misbelief(misbelief)
                {
                    self.push_error_toast(format!("Couldn't save misbelief: {err}"));
                }
            }
        }
    }

    fn handle_pomodoro_event(&mut self, event: ui::pomodoro_panel::PomodoroEvent) {
        let durations = crate::pomodoro::resolve_durations(&self.settings);
        match event {
            ui::pomodoro_panel::PomodoroEvent::Start => {
                self.pomodoro.start(std::time::Instant::now());
            }
            ui::pomodoro_panel::PomodoroEvent::Pause => self.pomodoro.pause(),
            ui::pomodoro_panel::PomodoroEvent::Reset => self.pomodoro.reset(&durations),
            ui::pomodoro_panel::PomodoroEvent::Skip => self.pomodoro.skip(&durations),
        }
    }

    fn handle_word_count_event(
        &mut self,
        ctx: &egui::Context,
        event: ui::word_count_panel::WordCountEvent,
    ) {
        use ui::word_count_panel::WordCountEvent;
        match event {
            WordCountEvent::SetDraftTarget(target) => {
                if let Some(project) = &mut self.project
                    && let Err(err) = project.set_draft_target_words(target)
                {
                    self.push_error_toast(format!("Couldn't save draft target: {err}"));
                }
            }
            WordCountEvent::SetSessionTarget(target) => {
                if let Some(project) = &mut self.project
                    && let Err(err) = project.set_session_target_words(target)
                {
                    self.push_error_toast(format!("Couldn't save session target: {err}"));
                }
            }
            WordCountEvent::SetScope(scope) => {
                if let Some(project) = &mut self.project
                    && let Err(err) = project.set_word_count_scope(scope)
                {
                    self.push_error_toast(format!("Couldn't save tracking scope: {err}"));
                }
                self.spawn_word_count_recompute(ctx);
            }
            WordCountEvent::Refresh => self.spawn_word_count_recompute(ctx),
            WordCountEvent::ResetSession => {
                let total = self.word_count.cache;
                if let Some(project) = &mut self.project
                    && let Err(err) = project.reset_session(total)
                {
                    self.push_error_toast(format!("Couldn't reset session: {err}"));
                }
                self.word_count.char_activity = 0;
            }
        }
    }

    /// Advances the Pomodoro timer by however much wall-clock time passed since
    /// the last frame, regardless of whether its dock tab is currently open —
    /// its state (and the status bar's countdown segment) needs to keep moving
    /// even while the tab is closed. Schedules another repaint a second out
    /// while running, since egui's default reactive mode otherwise only
    /// repaints on input/events and the countdown would freeze on screen.
    fn tick_pomodoro(&mut self, ctx: &egui::Context) {
        let durations = crate::pomodoro::resolve_durations(&self.settings);
        self.pomodoro.tick(std::time::Instant::now(), &durations);
        if self.pomodoro.is_running() {
            ctx.request_repaint_after(std::time::Duration::from_secs(1));
        }
    }

    /// Show `message` as an error-severity toast — see `Toast`'s doc comment for
    /// when to reach for this instead of `status_message`.
    fn push_error_toast(&mut self, message: impl Into<String>) {
        self.toasts.push(Toast {
            message: message.into(),
            shown_at: std::time::Instant::now(),
        });
    }

    /// Drop any toast that's outlived `Settings::toast_duration_secs` (see
    /// `resolve_toast_duration`), then render whatever's left stacked down the
    /// top-right corner of the window (oldest at top,
    /// each independently dismissible via its own × button) — called every
    /// frame regardless of Focus Mode or which dock tabs are open, since an
    /// error is exactly the kind of thing that shouldn't go unnoticed just
    /// because of what else happens to be on screen. Schedules a short
    /// repaint interval while any toast is showing so it actually
    /// disappears on its own once its time is up, the same reasoning as
    /// `tick_pomodoro`'s own `request_repaint_after`.
    fn show_toasts(&mut self, ctx: &egui::Context) {
        let duration = resolve_toast_duration(&self.settings);
        let now = std::time::Instant::now();
        self.toasts
            .retain(|toast| now.duration_since(toast.shown_at) < duration);
        if self.toasts.is_empty() {
            return;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(200));

        let mut dismiss = None;
        for (index, toast) in self.toasts.iter().enumerate() {
            egui::Area::new(egui::Id::new("toast").with(index))
                .anchor(
                    egui::Align2::RIGHT_TOP,
                    egui::vec2(-12.0, 12.0 + index as f32 * 56.0),
                )
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    egui::Frame::popup(ui.style())
                        .fill(egui::Color32::from_rgb(140, 30, 30))
                        .show(ui, |ui| {
                            ui.set_max_width(360.0);
                            ui.horizontal(|ui| {
                                ui.colored_label(egui::Color32::WHITE, &toast.message);
                                if ui.small_button("×").clicked() {
                                    dismiss = Some(index);
                                }
                            });
                        });
                });
        }
        if let Some(index) = dismiss {
            self.toasts.remove(index);
        }
    }

    /// Set the status-bar confirmation (see `status_message`'s doc comment for
    /// when to use this instead of `push_error_toast`) and record when, so
    /// `clear_status_message_if_expired` can time it out on its own.
    fn set_status_message(&mut self, message: impl Into<String>) {
        self.status_message = Some(message.into());
        self.status_message_set_at = Some(std::time::Instant::now());
    }

    /// Clear `status_message` (and its timestamp) immediately, rather than
    /// waiting for `clear_status_message_if_expired` to time it out on its own
    /// — for the rare case that wants a clean slate right away (e.g.
    /// `set_project`, switching to a different project).
    fn clear_status_message(&mut self) {
        self.status_message = None;
        self.status_message_set_at = None;
    }

    /// Auto-clear `status_message` once it's been showing longer than
    /// `Settings::status_message_duration_secs` (see
    /// `resolve_status_message_duration`) — called every frame, mirroring
    /// `show_toasts`' own expiry check and for the same reason: status-bar
    /// text that just sits there until the next unrelated update happens to
    /// overwrite it is easy to mistake for something still current.
    fn clear_status_message_if_expired(&mut self, ctx: &egui::Context) {
        let Some(set_at) = self.status_message_set_at else {
            return;
        };
        let duration = resolve_status_message_duration(&self.settings);
        let elapsed = set_at.elapsed();
        if elapsed >= duration {
            self.clear_status_message();
        } else {
            ctx.request_repaint_after(duration - elapsed);
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
                    self.push_error_toast(format!("Couldn't save card: {err}"));
                }
            }
            CardEditorOutcome::Delete(id) => {
                if let Err(err) = project.delete_story_card(id) {
                    self.push_error_toast(format!("Couldn't delete card: {err}"));
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
            ShortcutAction::OpenProject => self.browse_for_project(ctx),
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
                    self.push_error_toast(format!("Save failed: {err}"));
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
                    self.push_error_toast("No project open");
                }
            }
            ShortcutAction::CloseDocument => self.close_document(ctx),
            ShortcutAction::FindReplace => self.find_replace.request_open(),
            ShortcutAction::CommandPrompt => self.command_prompt.request_open(),
            ShortcutAction::GitCommit => self.prompt_git_commit(false),
            ShortcutAction::GitPush => self.run_git_push(ctx),
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
        }
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
                self.push_error_toast(format!("Couldn't update {}: {err}", path.display()));
            }
        }
        self.set_status_message(format!("Replaced {total} occurrence(s)"));
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
                    self.push_error_toast(format!("Couldn't restore: {err}"));
                }
            }
            Err(err) => self.push_error_toast(format!("Couldn't restore: {err}")),
        }
    }

    fn set_folder_role(
        &mut self,
        ctx: &egui::Context,
        path: &Path,
        role: Option<crate::project::FolderRole>,
    ) {
        let Some(project) = &mut self.project else {
            return;
        };
        match project.set_folder_role(path, role) {
            Ok(()) => self.spawn_word_count_recompute(ctx),
            Err(err) => self.push_error_toast(format!("Couldn't set folder role: {err}")),
        }
    }

    fn set_picklist_folder(&mut self, field: crate::project::PicklistField, path: Option<PathBuf>) {
        let Some(project) = &mut self.project else {
            return;
        };
        if let Err(err) = project.set_picklist_folder(field, path.as_deref()) {
            self.push_error_toast(format!("Couldn't set dropdown source: {err}"));
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
            self.push_error_toast(format!("Couldn't empty Trash: {err}"));
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
            PromptAction::NewProject {
                location,
                template_id,
            } => self.create_project(ctx, &location, name, &template_id),
            PromptAction::GitCommit { push_after } => self.run_git_commit(ctx, name, push_after),
            PromptAction::SaveLayout => self.save_named_layout(ctx, name),
            PromptAction::SaveProjectAsTemplate => self.save_project_as_template(name),
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
            self.push_error_toast(format!("Couldn't save before renaming: {err}"));
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
                self.push_error_toast(format!("Couldn't rename: {err}"));
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
                self.push_error_toast(format!("Couldn't delete {}: {err}", path.display()));
            }
        }
    }

    fn create_document(&mut self, parent: &Path, name: &str) {
        let Some(project) = &mut self.project else {
            return;
        };
        match project.create_document(parent, name) {
            Ok(path) => self.open_document(&path),
            Err(err) => self.push_error_toast(format!("Couldn't create file: {err}")),
        }
    }

    fn create_folder(&mut self, parent: &Path, name: &str) {
        let Some(project) = &mut self.project else {
            return;
        };
        if let Err(err) = project.create_folder(parent, name) {
            self.push_error_toast(format!("Couldn't create folder: {err}"));
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
            Err(err) => self.push_error_toast(format!("Couldn't create file: {err}")),
        }
    }

    /// Renders the top menu bar (File/Edit/View/Tools/Versions/Window/Help) —
    /// hidden during Focus Mode. Extracted from `ui()` verbatim (2026-07-31
    /// code-quality review: that function was 766 lines).
    fn show_menu_bar(&mut self, ui: &mut egui::Ui) {
        if !self.focus_mode {
            egui::Panel::top("menu_bar").show(ui, |ui| {
                egui::MenuBar::new().ui(ui, |ui| {
                    top_menu_button(ui, "File", egui::Key::F, |ui, nav| {
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

                        if nav
                            .shortcut_button(ui, "New Project", new_project_shortcut)
                            .clicked()
                        {
                            self.start_new_project();
                        }
                        if nav
                            .shortcut_button(ui, "Open Project", open_project_shortcut)
                            .clicked()
                        {
                            self.browse_for_project(ui.ctx());
                        }
                        ui.add_enabled(false, egui::Button::new("Close Project"));
                        ui.separator();
                        if nav
                            .shortcut_button(ui, "Open Document…", open_document_shortcut)
                            .clicked()
                        {
                            if self.project.is_some() {
                                self.open_document_prompt.request_open();
                            } else {
                                self.push_error_toast("No project open");
                            }
                        }
                        if nav
                            .shortcut_button(ui, "Close Document", close_document_shortcut)
                            .clicked()
                        {
                            let ctx = ui.ctx().clone();
                            self.close_document(&ctx);
                        }
                        if nav
                            .shortcut_button(ui, "Save Project as Template…", None)
                            .clicked()
                        {
                            if self.project.is_some() {
                                self.prompt_save_project_as_template();
                            } else {
                                self.push_error_toast("No project open");
                            }
                        }
                        // Manuscript isn't an exclusive role — a project can have
                        // several Manuscript folders at once (see
                        // `FolderRole::is_exclusive`) — so this offers a submenu to
                        // choose among them once there's more than one, rather than
                        // silently picking just the first.
                        let manuscript_folders = self
                            .project
                            .as_ref()
                            .map(|project| {
                                project.folder_role_paths(crate::project::FolderRole::Manuscript)
                            })
                            .unwrap_or_default();
                        match manuscript_folders.as_slice() {
                            [] => {
                                if nav
                                    .shortcut_button(ui, "Export Manuscript…", None)
                                    .clicked()
                                {
                                    if let Some(project) = &self.project {
                                        self.open_export(project.root.clone());
                                    } else {
                                        self.push_error_toast("No project open");
                                    }
                                }
                            }
                            [only] => {
                                if nav
                                    .shortcut_button(ui, "Export Manuscript…", None)
                                    .clicked()
                                {
                                    self.open_export(only.clone());
                                }
                            }
                            many => {
                                // Not keyboard-navigable past its own trigger row — see
                                // the plan's scope note on nested submenus (Theme/
                                // Layouts/this) staying mouse/hover-only for now.
                                let outer = ui.menu_button("Export Manuscript", |ui| {
                                    for path in many {
                                        let label = self
                                            .project
                                            .as_ref()
                                            .and_then(|project| project.tree.find_by_path(path))
                                            .map(|node| node.name.clone())
                                            .unwrap_or_else(|| path.display().to_string());
                                        if ui.button(format!("{label}…")).clicked() {
                                            self.open_export(path.clone());
                                            ui.close();
                                        }
                                    }
                                });
                                nav.track(ui, &outer.response);
                            }
                        }
                        ui.separator();
                        if nav
                            .shortcut_button(ui, "Settings", open_settings_shortcut)
                            .clicked()
                        {
                            self.show_settings = true;
                        }
                        ui.separator();
                        if nav.shortcut_button(ui, "Exit", exit_shortcut).clicked() {
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });
                    top_menu_button(ui, "Edit", egui::Key::E, |ui, nav| {
                        if nav
                            .shortcut_button(
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
                        if nav
                            .shortcut_button(
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
                        if nav
                            .shortcut_button(
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
                        if nav
                            .shortcut_button(ui, "Find and Replace", find_replace_shortcut)
                            .clicked()
                        {
                            self.find_replace.request_open();
                        }
                        let metadata_shortcut =
                            self.settings.shortcuts.get(ShortcutAction::EditMetadata);
                        if nav
                            .shortcut_button(ui, "Document Metadata", metadata_shortcut)
                            .clicked()
                        {
                            self.toggle_dock_tab(DockTab::Metadata);
                        }
                    });
                    top_menu_button(ui, "View", egui::Key::V, |ui, nav| {
                        if nav.button(ui, "Focus Mode").clicked() {
                            let ctx = ui.ctx().clone();
                            self.set_focus_mode(&ctx, !self.focus_mode);
                        }
                        ui.separator();
                        if nav.button(ui, "Editor").clicked() {
                            self.toggle_dock_tab(DockTab::Editor);
                        }
                        if nav.button(ui, "Preview").clicked() {
                            self.toggle_dock_tab_near(DockTab::Preview, DockTab::Editor);
                        }
                        if nav.button(ui, "Corkboard").clicked() {
                            self.toggle_dock_tab_near(DockTab::Corkboard, DockTab::Editor);
                        }
                        ui.separator();
                        if nav.button(ui, "Binder").clicked() {
                            self.toggle_dock_tab(DockTab::Binder);
                        }
                        if nav.button(ui, "Backlinks").clicked() {
                            self.toggle_dock_tab(DockTab::Backlinks);
                        }
                        if nav.button(ui, "Tags").clicked() {
                            self.toggle_dock_tab(DockTab::Tags);
                        }
                        ui.separator();
                        // `SubMenuButton`, not `MenuButton`: this is nested *inside* the
                        // View menu, and `MenuButton` is for top-level, click-to-open menu
                        // bar buttons. Using it here meant clicking "Theme" behaved like
                        // opening a second, independent top-level menu rather than a
                        // proper submenu — items inside never got a chance to run, since
                        // the parent popup's own close-on-click handling collapsed it out
                        // from under `SubMenuButton`'s (hover-to-open, keeps parents open)
                        // dedicated handling for exactly this case. Its trigger row is
                        // still tracked (so Up/Down/Left/Right can reach it like any other
                        // row), but not arrow-navigable past it — the flyout stays
                        // mouse/hover-only for now (see the arrow-nav plan's scope note).
                        let (theme_trigger, _) =
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
                        nav.track(ui, &theme_trigger);
                    });
                    top_menu_button(ui, "Tools", egui::Key::T, |ui, nav| {
                        let command_prompt_shortcut =
                            self.settings.shortcuts.get(ShortcutAction::CommandPrompt);
                        if nav
                            .shortcut_button(ui, "Command Prompt", command_prompt_shortcut)
                            .clicked()
                        {
                            self.command_prompt.request_open();
                        }
                        let pomodoro_shortcut =
                            self.settings.shortcuts.get(ShortcutAction::TogglePomodoro);
                        if nav
                            .shortcut_button(ui, "Pomodoro Timer", pomodoro_shortcut)
                            .clicked()
                        {
                            self.toggle_dock_tab(DockTab::Pomodoro);
                        }
                        let word_count_shortcut =
                            self.settings.shortcuts.get(ShortcutAction::ToggleWordCount);
                        if nav
                            .shortcut_button(ui, "Word Count", word_count_shortcut)
                            .clicked()
                        {
                            self.toggle_dock_tab(DockTab::WordCount);
                        }
                        let refresh_word_count_shortcut = self
                            .settings
                            .shortcuts
                            .get(ShortcutAction::RefreshWordCount);
                        if nav
                            .shortcut_button(ui, "Refresh Word Count", refresh_word_count_shortcut)
                            .clicked()
                        {
                            self.spawn_word_count_recompute(ui.ctx());
                        }
                        ui.separator();
                        if nav.button(ui, "Reload Plugins").clicked() {
                            self.reload_plugins();
                        }
                        let project_plugins_enabled = self
                            .project
                            .as_ref()
                            .is_some_and(|project| project.meta.plugins_enabled);
                        if self.project.is_some()
                            && !project_plugins_enabled
                            && nav.button(ui, "Enable Project Plugins").clicked()
                        {
                            if let Some(project) = &mut self.project
                                && let Err(err) = project.set_plugins_enabled(true)
                            {
                                self.push_error_toast(format!(
                                    "Couldn't enable project plugins: {err}"
                                ));
                            }
                            self.reload_plugins();
                        }
                    });
                    // "S" rather than "V" (Versions' first letter) since View already
                    // claims Alt+V — matches the classic Windows-mnemonic convention
                    // of falling back to a distinguishing later letter on collision.
                    top_menu_button(ui, "Versions", egui::Key::S, |ui, nav| {
                        let git_enabled = self
                            .project
                            .as_ref()
                            .is_some_and(|project| project.meta.git_enabled);
                        if !git_enabled {
                            if nav.button(ui, "Enable Git Support").clicked() {
                                self.enable_git_support_manually();
                            }
                        } else {
                            let commit_shortcut =
                                self.settings.shortcuts.get(ShortcutAction::GitCommit);
                            if nav.shortcut_button(ui, "Commit", commit_shortcut).clicked() {
                                self.prompt_git_commit(false);
                            }
                            // Push/pull run on a background thread (see `spawn_git_operation`);
                            // disabled while one is already in flight rather than letting a
                            // second click queue up or race it. `MenuNav::track`'s
                            // `ui.is_enabled()` check means this trio is automatically
                            // skipped by arrow-key navigation while busy, with no separate
                            // bookkeeping needed.
                            let git_busy = self.pending_git.is_some();
                            ui.add_enabled_ui(!git_busy, |ui| {
                                if nav.button(ui, "Commit and Push").clicked() {
                                    self.prompt_git_commit(true);
                                }
                                let push_shortcut =
                                    self.settings.shortcuts.get(ShortcutAction::GitPush);
                                if nav.shortcut_button(ui, "Push", push_shortcut).clicked() {
                                    self.run_git_push(ui.ctx());
                                }
                                if nav.button(ui, "Pull").clicked() {
                                    self.run_git_pull(ui.ctx());
                                }
                            });
                        }
                    });
                    top_menu_button(ui, "Window", egui::Key::W, |ui, nav| {
                        if nav.button(ui, "Save Current Layout…").clicked() {
                            self.prompt_save_layout();
                        }
                        // `SubMenuButton`, not `MenuButton` — see the matching comment on
                        // View's "Theme" submenu for why. Trigger row tracked the same way.
                        let (layouts_trigger, _) =
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
                        nav.track(ui, &layouts_trigger);
                        ui.separator();
                        if nav.button(ui, "Restore Default Layout").clicked() {
                            self.dock_state = default_dock_state();
                        }
                    });
                    top_menu_button(ui, "Help", egui::Key::H, |ui, nav| {
                        if nav.button(ui, "About").clicked() {
                            self.show_about = true;
                        }
                    });
                });
            });
        }
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

        if ui::settings_panel::show(
            ui.ctx(),
            &mut self.show_settings,
            &mut self.settings,
            &mut self.settings_category,
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

        if self.new_project_template_prompt.open
            && let Some(template_id) = ui::new_project_template_prompt::show(
                ui.ctx(),
                &mut self.new_project_template_prompt,
                &self.project_templates,
            )
        {
            self.start_new_project_with_template(template_id);
        }

        if self.show_about && ui::about_panel::show(ui.ctx()) {
            self.show_about = false;
        }
    }

    /// Renders the bottom status bar — hidden during Focus Mode. Extracted
    /// from `ui()` verbatim (2026-07-31 code-quality review).
    fn show_status_bar(&mut self, ui: &mut egui::Ui) {
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
                        // No special color: now that error-severity messages go
                        // through `push_error_toast` instead (see its doc
                        // comment), everything left here is a routine
                        // confirmation, not a problem — coloring it red the way
                        // this label used to unconditionally do would misread as
                        // an error for messages like "Committed".
                        ui.label(msg);
                    }
                    // Independent of `status_message` above, same rationale as
                    // the Pomodoro segment below — the Draft Target's progress
                    // should be visible at a glance regardless of whether the
                    // Word Count dock tab is open. Placed before Pomodoro so it
                    // renders as the segment just left of it (right_to_left
                    // layouts stack in call order), keeping Pomodoro anchored as
                    // the rightmost item users are already used to. Only shown
                    // once a Draft Target is actually set — Session Target
                    // progress is dock-panel-only, not surfaced here.
                    if let Some(project) = &self.project
                        && let Some(target) = project.meta.draft_target_words
                    {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(format!(
                                "{} : {} / {} words",
                                self.word_count.char_activity, self.word_count.cache, target
                            ));
                        });
                    }
                    // Independent of `status_message` above (which ~40 other call
                    // sites overwrite freely) — a running/paused-mid-session
                    // Pomodoro timer needs a segment of its own that survives
                    // those, so it's genuinely visible at a glance regardless of
                    // whether its dock tab is open (see `tick_pomodoro`). Not
                    // shown once nothing's ever been started this session.
                    if self.pomodoro.has_started() {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let remaining = self.pomodoro.remaining().as_secs();
                            ui.label(format!(
                                "⏱ {} {:02}:{:02}",
                                self.pomodoro.phase().label(),
                                remaining / 60,
                                remaining % 60
                            ));
                        });
                    }
                });
            });
        }
    }
}

/// Requests raised by `AppTabViewer::ui` for the caller to apply once the dock has
/// finished rendering for the frame — `egui_dock::TabViewer::ui` only gets `&mut
/// self` on the *viewer*, not on `SmaragdApp`, so it can't call `&mut self`
/// methods like `open_document` directly; it collects what it wants done instead.
enum DockAction {
    OpenDocument(PathBuf),
    Binder(BinderEvent),
    RefreshBacklinks,
    RefreshTags,
    EditorSaveError(String),
    Wikilink(WikilinkActivation),
    Corkboard(CorkboardEvent),
    Pomodoro(crate::ui::pomodoro_panel::PomodoroEvent),
    WordCount(crate::ui::word_count_panel::WordCountEvent),
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
    tags: &'a [crate::project::TagGroup],
    tags_search_text: &'a mut String,
    tag_search_results: &'a [(PathBuf, String)],
    metadata_draft: &'a mut MetadataDraft,
    editor: &'a mut EditorState,
    settings: &'a Settings,
    color_themes: &'a [crate::color_theme::ColorTheme],
    pomodoro: &'a crate::pomodoro::PomodoroState,
    pomodoro_durations: crate::pomodoro::PomodoroDurations,
    /// See `SmaragdApp::word_count_cache`.
    word_count_cache: usize,
    /// See `SmaragdApp::char_activity`.
    char_activity: u64,
    actions: Vec<DockAction>,
    /// See `SmaragdApp::focus_binder_requested`.
    focus_binder_requested: bool,
}

impl egui_dock::TabViewer for AppTabViewer<'_> {
    type Tab = DockTab;

    fn title(&mut self, tab: &mut DockTab) -> egui::WidgetText {
        match tab {
            DockTab::Binder => "Binder".into(),
            DockTab::Backlinks => "Backlinks".into(),
            DockTab::Tags => "Tags".into(),
            DockTab::Metadata => "Metadata".into(),
            DockTab::Editor => "Editor".into(),
            DockTab::Preview => "Preview".into(),
            DockTab::Corkboard => "Corkboard".into(),
            DockTab::Pomodoro => "Pomodoro".into(),
            DockTab::WordCount => "Word Count".into(),
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
            DockTab::Tags => {
                if let Some(event) = ui::tags_panel::show(
                    ui,
                    self.open_path.as_deref(),
                    self.tags,
                    self.tags_search_text,
                    self.tag_search_results,
                ) {
                    match event {
                        ui::tags_panel::TagsEvent::OpenDocument(path) => {
                            self.actions.push(DockAction::OpenDocument(path));
                        }
                        ui::tags_panel::TagsEvent::Refresh => {
                            self.actions.push(DockAction::RefreshTags)
                        }
                    }
                }
            }
            DockTab::Metadata => {
                let project = self.project;
                let picklist_titles = |field: crate::project::PicklistField| -> Vec<String> {
                    project
                        .map(|project| {
                            project
                                .picklist_documents(field)
                                .iter()
                                .map(|node| {
                                    ui::binder_panel::document_label(&node.name).to_string()
                                })
                                .collect()
                        })
                        .unwrap_or_default()
                };
                let types = picklist_titles(crate::project::PicklistField::Type);
                let statuses = picklist_titles(crate::project::PicklistField::Status);
                let povs = picklist_titles(crate::project::PicklistField::Pov);
                let picklists = ui::metadata_panel::MetadataPicklists {
                    types: &types,
                    statuses: &statuses,
                    povs: &povs,
                };
                let word_count = crate::frontmatter::count_words(&self.editor.buffer);
                ui::metadata_panel::show(
                    ui,
                    self.open_path.as_deref(),
                    self.metadata_draft,
                    &picklists,
                    word_count,
                );
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
                        self.settings.typewriter_quotes,
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
            DockTab::Pomodoro => {
                if let Some(event) =
                    ui::pomodoro_panel::show(ui, self.pomodoro, &self.pomodoro_durations)
                {
                    self.actions.push(DockAction::Pomodoro(event));
                }
            }
            DockTab::WordCount => match self.project {
                Some(project) => {
                    if let Some(event) = ui::word_count_panel::show(
                        ui,
                        project,
                        self.word_count_cache,
                        self.char_activity,
                    ) {
                        self.actions.push(DockAction::WordCount(event));
                    }
                }
                None => {
                    ui.label("Open a project folder to get started.");
                }
            },
        }
    }
}

impl eframe::App for SmaragdApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll_git_operation(ui.ctx());
        self.poll_word_count();
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

        self.show_menu_bar(ui);

        self.show_modals(ui);

        self.show_status_bar(ui);

        self.refresh_backlinks_if_needed();
        self.refresh_tags_if_needed();
        self.refresh_metadata_if_needed();
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
                    Some(EditorEvent::SaveError(err)) => self.push_error_toast(err),
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
                    backlinks: &self.backlinks.entries,
                    tags: &self.tags.entries,
                    tags_search_text: &mut self.tags.search_text,
                    tag_search_results: &self.tags.search_results,
                    metadata_draft: &mut self.metadata.draft,
                    editor: &mut self.editor,
                    settings: &self.settings,
                    color_themes: &self.color_themes,
                    pomodoro: &self.pomodoro,
                    pomodoro_durations: crate::pomodoro::resolve_durations(&self.settings),
                    word_count_cache: self.word_count.cache,
                    char_activity: self.word_count.char_activity,
                    actions: Vec::new(),
                    focus_binder_requested: std::mem::take(&mut self.focus_binder_requested),
                };
                egui_dock::DockArea::new(&mut self.dock_state)
                    .style(egui_dock::Style::from_egui(ui.style().as_ref()))
                    .show_inside(ui, &mut viewer);
                for action in viewer.actions {
                    match action {
                        DockAction::OpenDocument(path) => self.open_document(&path),
                        DockAction::Binder(event) => self.handle_binder_event(ui.ctx(), event),
                        DockAction::RefreshBacklinks => self.recompute_backlinks(),
                        DockAction::RefreshTags => self.recompute_tags(),
                        DockAction::EditorSaveError(err) => self.push_error_toast(err),
                        DockAction::Wikilink(activation) => self.activate_wikilink(activation),
                        DockAction::Corkboard(event) => self.handle_corkboard_event(event),
                        DockAction::Pomodoro(event) => self.handle_pomodoro_event(event),
                        DockAction::WordCount(event) => {
                            self.handle_word_count_event(ui.ctx(), event)
                        }
                    }
                }
            });
        }

        self.apply_metadata_edits_if_changed();
        self.track_char_activity();
        self.refresh_tag_search_if_needed();

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
mod duration_resolution_tests {
    use super::{
        DEFAULT_STATUS_MESSAGE_DURATION, DEFAULT_TOAST_DURATION, resolve_status_message_duration,
        resolve_toast_duration,
    };
    use crate::settings::Settings;

    #[test]
    fn resolve_toast_duration_falls_back_to_the_default_when_unconfigured() {
        let settings = Settings {
            toast_duration_secs: 0,
            ..Default::default()
        };
        assert_eq!(resolve_toast_duration(&settings), DEFAULT_TOAST_DURATION);
    }

    #[test]
    fn resolve_toast_duration_uses_the_configured_value() {
        let settings = Settings {
            toast_duration_secs: 20,
            ..Default::default()
        };
        assert_eq!(
            resolve_toast_duration(&settings),
            std::time::Duration::from_secs(20)
        );
    }

    #[test]
    fn resolve_status_message_duration_falls_back_to_the_default_when_unconfigured() {
        let settings = Settings {
            status_message_duration_secs: 0,
            ..Default::default()
        };
        assert_eq!(
            resolve_status_message_duration(&settings),
            DEFAULT_STATUS_MESSAGE_DURATION
        );
    }

    #[test]
    fn resolve_status_message_duration_uses_the_configured_value() {
        let settings = Settings {
            status_message_duration_secs: 30,
            ..Default::default()
        };
        assert_eq!(
            resolve_status_message_duration(&settings),
            std::time::Duration::from_secs(30)
        );
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

#[cfg(test)]
mod menu_nav_tests {
    use super::{TOP_MENUS, top_menu_button, top_menu_popup_id};

    /// Drives the real `top_menu_button` for `label` with three plain stand-in
    /// items ("Alpha"/"Beta"/"Gamma"), for one frame with the given key
    /// `events`, and returns the resulting item ids — going through
    /// `top_menu_button` itself (rather than calling `MenuNav`/
    /// `handle_dropdown_arrows` directly) matters: a `Popup`'s "still open"
    /// bookkeeping (`keep_popup_open`, via `open_memory(None)`) is refreshed by
    /// `Popup::show` every frame it's actually shown, so skipping that call
    /// would make `Popup::is_id_open` silently go false after the first frame.
    fn frame(
        ctx: &egui::Context,
        label: &'static str,
        mnemonic: egui::Key,
        events: Vec<egui::Event>,
    ) -> Vec<egui::Id> {
        let input = egui::RawInput {
            events,
            ..Default::default()
        };
        let mut items = Vec::new();
        let _ = ctx.run_ui(input, |ui| {
            top_menu_button(ui, label, mnemonic, |ui, nav| {
                nav.button(ui, "Alpha");
                nav.button(ui, "Beta");
                nav.button(ui, "Gamma");
                items = nav.items.clone();
            });
        });
        items
    }

    /// Like `frame`, but renders *every* top-level menu (each with the same
    /// three stand-in items), matching how the real menu bar renders all 7
    /// every frame regardless of which one is open — needed for any test that
    /// exercises Left/Right switching to a *different* menu, since
    /// `handle_dropdown_arrows` only clears its per-popup "had items" bookkeeping
    /// (see its doc comment) for menus that actually get rendered that frame.
    /// Rendering only the "currently relevant" one, like `frame` does, would
    /// leave a just-closed menu's stale bookkeeping around indefinitely.
    fn frame_all(ctx: &egui::Context, events: Vec<egui::Event>) {
        let input = egui::RawInput {
            events,
            ..Default::default()
        };
        let _ = ctx.run_ui(input, |ui| {
            for (label, mnemonic) in TOP_MENUS {
                top_menu_button(ui, label, mnemonic, |ui, nav| {
                    nav.button(ui, "Alpha");
                    nav.button(ui, "Beta");
                    nav.button(ui, "Gamma");
                });
            }
        });
    }

    fn key_event(key: egui::Key) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }
    }

    #[test]
    fn down_and_up_wrap_at_the_ends_of_the_dropdown() {
        let ctx = egui::Context::default();
        ctx.all_styles_mut(|style| style.animation_time = 0.0);
        let popup_id = top_menu_popup_id("File");
        egui::Popup::open_id(&ctx, popup_id);
        // A popup marked open via `open_id` outside any pass doesn't actually
        // render its content until the following frame (`Popup::show`'s own
        // first-frame settling) — one warm-up frame before relying on its
        // content/`nav.items` having been populated at all.
        frame(&ctx, "File", egui::Key::F, vec![]);

        // Nothing focused yet — should land on the first item.
        let items = frame(&ctx, "File", egui::Key::F, vec![]);
        assert_eq!(ctx.memory(|m| m.focused()), Some(items[0]));

        // Let focus settle a frame (mirrors `binder_panel.rs`'s test harness: a
        // widget's focus-lock filter only takes effect starting the frame after
        // it gains focus) before pressing Up — from the first item, Up should
        // wrap to the last.
        frame(&ctx, "File", egui::Key::F, vec![]);
        let items = frame(
            &ctx,
            "File",
            egui::Key::F,
            vec![key_event(egui::Key::ArrowUp)],
        );
        assert_eq!(ctx.memory(|m| m.focused()), Some(items[2]));

        // From the last item, Down should wrap back to the first.
        frame(&ctx, "File", egui::Key::F, vec![]);
        let items = frame(
            &ctx,
            "File",
            egui::Key::F,
            vec![key_event(egui::Key::ArrowDown)],
        );
        assert_eq!(ctx.memory(|m| m.focused()), Some(items[0]));
    }

    #[test]
    fn right_and_left_cycle_through_top_menus_with_wraparound() {
        let ctx = egui::Context::default();
        ctx.all_styles_mut(|style| style.animation_time = 0.0);
        let file_id = top_menu_popup_id("File");
        egui::Popup::open_id(&ctx, file_id);
        frame_all(&ctx, vec![]); // focus lands on the first item
        frame_all(&ctx, vec![]); // let focus settle

        frame_all(&ctx, vec![key_event(egui::Key::ArrowRight)]);
        let edit_id = top_menu_popup_id("Edit");
        assert!(
            egui::Popup::is_id_open(&ctx, edit_id),
            "Right from File should open Edit (the next menu in TOP_MENUS)"
        );
        assert!(!egui::Popup::is_id_open(&ctx, file_id));

        // From Edit, Left should go back to File, and Left again should wrap
        // around to the last menu (Help).
        frame_all(&ctx, vec![]);
        frame_all(&ctx, vec![]);
        frame_all(&ctx, vec![key_event(egui::Key::ArrowLeft)]);
        assert!(egui::Popup::is_id_open(&ctx, file_id));

        frame_all(&ctx, vec![]);
        frame_all(&ctx, vec![]);
        frame_all(&ctx, vec![key_event(egui::Key::ArrowLeft)]);
        let help_id = top_menu_popup_id(TOP_MENUS[TOP_MENUS.len() - 1].0);
        assert!(
            egui::Popup::is_id_open(&ctx, help_id),
            "Left from File should wrap around to Help (the last menu in TOP_MENUS)"
        );
    }

    /// Regression test for a real bug: a menu item's own click handler doesn't
    /// call `ui.close()` anywhere in this codebase (clicking "Open Document…",
    /// say, leaves the File dropdown nominally still "open" in egui's Memory,
    /// even once the dialog it opened takes over) — which used to be harmless,
    /// since the dropdown just sat there unfocused and inert. Once
    /// `handle_dropdown_arrows` started auto-focusing the first item whenever
    /// nothing in *its own* list has focus, that harmless leftover "still open"
    /// state became actively disruptive: it kept re-stealing focus back onto
    /// the dropdown's first item every single frame, away from whatever dialog
    /// had opened on top of it, on every frame *after* the one where it
    /// legitimately first opened.
    #[test]
    fn does_not_steal_focus_back_once_something_else_has_it() {
        let ctx = egui::Context::default();
        ctx.all_styles_mut(|style| style.animation_time = 0.0);
        let popup_id = top_menu_popup_id("File");
        egui::Popup::open_id(&ctx, popup_id);
        frame(&ctx, "File", egui::Key::F, vec![]); // warm-up (see the other tests)
        frame(&ctx, "File", egui::Key::F, vec![]); // legitimate first-open auto-focus

        // A stand-in for e.g. the Open Document modal's own text field, rendered
        // (like a real dialog would be) in the same pass as the still-open File
        // dropdown, and given focus — without the File dropdown ever being
        // closed. A bare `request_focus` on an id nothing ever renders wouldn't
        // do: egui drops focus from a widget that isn't shown in a pass, which
        // would trivially "pass" this test for the wrong reason.
        let mut other_id = None;
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            let other_response = ui.button("Other Dialog");
            other_id = Some(other_response.id);
            other_response.request_focus();
            top_menu_button(ui, "File", egui::Key::F, |ui, nav| {
                nav.button(ui, "Alpha");
                nav.button(ui, "Beta");
                nav.button(ui, "Gamma");
            });
        });
        let other_id = other_id.unwrap();
        assert_eq!(ctx.memory(|m| m.focused()), Some(other_id));

        // Rendering both again — the still-nominally-open File dropdown must
        // not claw focus back to its own first item.
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            let _ = ui.button("Other Dialog");
            top_menu_button(ui, "File", egui::Key::F, |ui, nav| {
                nav.button(ui, "Alpha");
                nav.button(ui, "Beta");
                nav.button(ui, "Gamma");
            });
        });
        assert_eq!(
            ctx.memory(|m| m.focused()),
            Some(other_id),
            "File's dropdown re-stole focus even though it didn't just open this frame"
        );
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
}

#[cfg(test)]
mod event_routing_tests {
    use super::*;
    use crate::project::WordCountScope;
    use crate::ui::word_count_panel::WordCountEvent;

    #[test]
    fn pomodoro_start_event_starts_the_timer() {
        let mut app = SmaragdApp::test_fixture();
        assert!(!app.pomodoro.is_running());

        app.handle_pomodoro_event(ui::pomodoro_panel::PomodoroEvent::Start);

        assert!(app.pomodoro.is_running());
    }

    #[test]
    fn pomodoro_pause_event_stops_a_running_timer() {
        let mut app = SmaragdApp::test_fixture();
        app.handle_pomodoro_event(ui::pomodoro_panel::PomodoroEvent::Start);
        assert!(app.pomodoro.is_running());

        app.handle_pomodoro_event(ui::pomodoro_panel::PomodoroEvent::Pause);

        assert!(!app.pomodoro.is_running());
    }

    #[test]
    fn corkboard_event_updates_protagonist_desire_on_the_open_project() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::initialize(dir.path()).unwrap();
        let mut app = SmaragdApp::test_fixture();
        app.project = Some(project);

        app.handle_corkboard_event(CorkboardEvent::SetProtagonistDesire(
            "Reclaim the throne".to_string(),
        ));

        assert_eq!(
            app.project.as_ref().unwrap().meta.protagonist_desire,
            "Reclaim the throne"
        );
    }

    #[test]
    fn corkboard_event_is_a_no_op_without_an_open_project() {
        let mut app = SmaragdApp::test_fixture();

        // Must not panic when there's nothing to apply the edit to.
        app.handle_corkboard_event(CorkboardEvent::SetProtagonistMisbelief(
            "Unworthy of the crown".to_string(),
        ));

        assert!(app.project.is_none());
    }

    #[test]
    fn word_count_event_set_draft_target_persists_on_the_open_project() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::initialize(dir.path()).unwrap();
        let mut app = SmaragdApp::test_fixture();
        app.project = Some(project);
        let ctx = egui::Context::default();

        app.handle_word_count_event(&ctx, WordCountEvent::SetDraftTarget(Some(50_000)));

        assert_eq!(
            app.project.as_ref().unwrap().meta.draft_target_words,
            Some(50_000)
        );
    }

    #[test]
    fn word_count_event_set_scope_persists_and_triggers_a_recompute() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::initialize(dir.path()).unwrap();
        let mut app = SmaragdApp::test_fixture();
        app.project = Some(project);
        let ctx = egui::Context::default();

        app.handle_word_count_event(
            &ctx,
            WordCountEvent::SetScope(WordCountScope::EverythingExceptTrash),
        );

        assert_eq!(
            app.project.as_ref().unwrap().meta.word_count_scope,
            WordCountScope::EverythingExceptTrash
        );
        assert!(app.word_count.pending.is_some());
    }

    #[test]
    fn word_count_event_reset_session_zeroes_char_activity() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::initialize(dir.path()).unwrap();
        let mut app = SmaragdApp::test_fixture();
        app.project = Some(project);
        app.word_count.char_activity = 42;
        let ctx = egui::Context::default();

        app.handle_word_count_event(&ctx, WordCountEvent::ResetSession);

        assert_eq!(app.word_count.char_activity, 0);
    }
}

#[cfg(test)]
mod char_activity_tests {
    use super::*;

    #[test]
    fn no_project_and_no_open_document_never_panics_or_accumulates() {
        let mut app = SmaragdApp::test_fixture();

        app.track_char_activity();
        app.track_char_activity();

        assert_eq!(app.word_count.char_activity, 0);
    }

    #[test]
    fn typing_then_deleting_counts_both_directions_not_the_net_change() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let path = project.create_document(dir.path(), "Scene").unwrap();
        let mut app = SmaragdApp::test_fixture();
        app.project = Some(project);
        app.editor.open_path = Some(path);

        // First frame with this document open: only establishes the baseline,
        // the initial (empty) length isn't itself counted as "typed."
        app.track_char_activity();
        assert_eq!(app.word_count.char_activity, 0);

        // "Type" 100 characters.
        app.editor.buffer = "a".repeat(100);
        app.track_char_activity();
        assert_eq!(app.word_count.char_activity, 100);

        // "Delete" them all back to empty — the example from the bug report:
        // 100 typed + 100 deleted reads 200, not a net 0.
        app.editor.buffer.clear();
        app.track_char_activity();
        assert_eq!(app.word_count.char_activity, 200);
    }

    #[test]
    fn switching_documents_does_not_count_the_length_jump_between_them() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let first = project.create_document(dir.path(), "First").unwrap();
        let second = project.create_document(dir.path(), "Second").unwrap();
        let mut app = SmaragdApp::test_fixture();
        app.project = Some(project);

        app.editor.open_path = Some(first);
        app.track_char_activity(); // establishes the baseline (empty) length
        app.editor.buffer = "a".repeat(50);
        app.track_char_activity();
        assert_eq!(app.word_count.char_activity, 50);

        // Switch to a different, much longer document.
        app.editor.open_path = Some(second);
        app.editor.buffer = "b".repeat(500);
        app.track_char_activity();

        // The 450-character jump between the two unrelated buffers must not
        // be counted as characters typed.
        assert_eq!(app.word_count.char_activity, 50);
    }

    #[test]
    fn editing_a_document_outside_the_tracked_scope_does_not_count() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let manuscript = project.create_folder(dir.path(), "Manuscript").unwrap();
        project
            .set_folder_role(&manuscript, Some(crate::project::FolderRole::Manuscript))
            .unwrap();
        // ManuscriptOnly is the default scope, and this document lives
        // outside the one Manuscript-role folder.
        let outtake = project.create_document(dir.path(), "Outtake").unwrap();
        let mut app = SmaragdApp::test_fixture();
        app.project = Some(project);
        app.editor.open_path = Some(outtake);

        app.track_char_activity();
        app.editor.buffer = "a".repeat(100);
        app.track_char_activity();

        assert_eq!(app.word_count.char_activity, 0);
    }
}

#[cfg(test)]
mod word_count_refresh_tests {
    use super::*;

    #[test]
    fn a_save_edge_triggers_a_recompute() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::initialize(dir.path()).unwrap();
        let mut app = SmaragdApp::test_fixture();
        app.project = Some(project);
        let ctx = egui::Context::default();

        app.editor.dirty = true;
        app.refresh_word_count_if_needed(&ctx);
        assert!(
            app.word_count.pending.is_none(),
            "becoming dirty is not a save"
        );

        app.editor.dirty = false;
        app.refresh_word_count_if_needed(&ctx);
        assert!(
            app.word_count.pending.is_some(),
            "dirty->clean transition is a save and should trigger a recompute"
        );
    }

    #[test]
    fn staying_clean_never_triggers_a_recompute() {
        let mut app = SmaragdApp::test_fixture();
        let ctx = egui::Context::default();

        app.refresh_word_count_if_needed(&ctx);
        app.refresh_word_count_if_needed(&ctx);

        assert!(app.word_count.pending.is_none());
    }
}
