use super::*;
use std::collections::{HashMap, HashSet};

/// Requests raised by `AppTabViewer::ui` for the caller to apply once the dock has
/// finished rendering for the frame — `egui_dock::TabViewer::ui` only gets `&mut
/// self` on the *viewer*, not on `SmaragdApp`, so it can't call `&mut self`
/// methods like `open_document` directly; it collects what it wants done instead.
pub(super) enum DockAction {
    OpenDocument(PathBuf),
    Binder(BinderEvent),
    ProjectMeta(crate::ui::metadata_panel::ProjectMetaEvent),
    /// A status color was assigned/edited from the Status field's swatch —
    /// see `ui::metadata_panel::MetadataFormEvent`. Raised by both `show`
    /// (document metadata) and `show_folder` (folder metadata), so it isn't
    /// specific to either `DockAction::Binder` nor `ProjectMeta`.
    Metadata(crate::ui::metadata_panel::MetadataFormEvent),
    RefreshBacklinks,
    RefreshTags,
    /// The Tags dock's "Rename…" button was clicked for the given tag — see
    /// `ui::tags_panel::TagsEvent::RenameTag`.
    RenameTag(String),
    /// A `#tag` was clicked in the Preview tab — see
    /// `ui::markdown_preview::PreviewClick::Tag`.
    PreviewTagClicked(String),
    EditorSaveError(String),
    Wikilink(WikilinkActivation),
    /// The Editor's gutter (a click on a bookmarked/unbookmarked line's
    /// diamond slot) or its `ShortcutAction::ToggleBookmark` shortcut fired
    /// — see `ui::editor_panel::EditorEvent::ToggleBookmark`. Carries the
    /// 1-based logical line to toggle at whichever document is currently
    /// open.
    ToggleBookmark(usize),
    /// A row action in the Bookmarks dock — see
    /// `ui::bookmarks_panel::BookmarksEvent`.
    Bookmarks(crate::ui::bookmarks_panel::BookmarksEvent),
    /// The Preview tab's inline Style picker (`ui::markdown_preview::show`)
    /// was switched to a different style this frame — persisted onto
    /// `ProjectMeta::book_style` via `Project::set_book_style`, the same
    /// field the Export dialog reads/writes, so the two always agree.
    SetBookStyle(String),
    Corkboard(CorkboardEvent),
    StoryGrid(crate::ui::story_grid_panel::StoryGridEvent),
    BeliefTimeline(crate::ui::belief_timeline_panel::BeliefTimelineEvent),
    Pomodoro(crate::ui::pomodoro_panel::PomodoroEvent),
    WordCount(crate::ui::word_count_panel::WordCountEvent),
    Collab(CollabPanelEvent),
    Streak(crate::ui::streak_panel::StreakEvent),
    /// Raised by the Binder tab's empty state (no project open) when the user
    /// clicks "New Project" / "Open Project" — routed through `DockAction`
    /// like every other tab-originated request rather than reaching for
    /// `self.app` directly, since `AppTabViewer` only ever borrows app state,
    /// it doesn't own a way to mutate it (see this struct's own doc comment).
    RequestNewProject,
    RequestOpenProject,
}

/// A short-lived `egui_dock::TabViewer` impl, constructed fresh each frame right
/// before `DockArea::show_inside` and drained right after (see `DockAction`).
/// Borrows exactly what each tab's content needs to render; `metadata_draft` and
/// `editor` are the two `&mut` fields since the Metadata and Editor tabs mutate
/// them directly (live editing, no event needed for Metadata — see
/// `apply_metadata_edits_if_changed` — while Editor's own internal edits don't
/// need to round-trip through a `DockAction` either, only its save/wikilink
/// outcomes do).
pub(super) struct AppTabViewer<'a> {
    pub(super) project: Option<&'a Project>,
    pub(super) selected_path: Option<&'a Path>,
    /// Owned (not `&'a Path`) because `editor` below is a `&'a mut EditorState`
    /// borrowed at the same time — an `&'a Path` still pointing into
    /// `editor.open_path` would alias it.
    pub(super) open_path: Option<PathBuf>,
    pub(super) backlinks: &'a [BacklinkEntry],
    pub(super) tags: &'a [crate::project::TagGroup],
    pub(super) tags_search_text: &'a mut String,
    pub(super) tag_search_results: &'a [(PathBuf, String)],
    pub(super) metadata_draft: &'a mut MetadataDraft,
    /// See `MetadataTarget` — which of Document/Project/Folder(path) the
    /// Metadata dock currently shows.
    pub(super) metadata_target: MetadataTarget,
    /// The draft for `metadata_target`'s `Folder(path)`, when that's what it
    /// is — see `MetadataState::folder_draft`.
    pub(super) folder_metadata_draft: &'a mut MetadataDraft,
    /// See `SmaragdApp::document_status_cache`.
    pub(super) document_status_cache: &'a DocumentStatusCache,
    /// See `WordCountState::folder_totals` — the data source for
    /// `BinderColorMode::WordCountProgress`'s folder rows.
    pub(super) folder_word_counts: &'a HashMap<PathBuf, usize>,
    /// See `SmaragdApp::git_dirty_paths`.
    pub(super) git_dirty_paths: &'a HashSet<PathBuf>,
    pub(super) editor: &'a mut EditorState,
    pub(super) settings: &'a Settings,
    /// The selectable typesetting styles (see `SmaragdApp::typeset_styles`) —
    /// used by the Preview tab's inline Style picker, the same list the
    /// Export dialog's own Style combo box draws from.
    pub(super) typeset_styles: &'a [crate::export::style::TypesetStyle],
    /// The project's currently effective export style id, resolved the same
    /// way `SmaragdApp::open_export` resolves it (`ProjectMeta::book_style`
    /// if it still exists in `typeset_styles`, else the first loaded style).
    pub(super) book_style_id: &'a str,
    /// Every custom font name successfully registered with egui via a loaded
    /// style's `font_file` (see `SmaragdApp::custom_font_names`) — the Preview
    /// tab passes this to `ui::markdown_preview::show` so it knows which
    /// non-bundled font names are actually safe to render with, rather than
    /// assuming a style's `font_file` succeeded just because it was set.
    pub(super) custom_fonts: &'a [String],
    pub(super) pomodoro: &'a crate::pomodoro::PomodoroState,
    pub(super) pomodoro_durations: crate::pomodoro::PomodoroDurations,
    /// See `SmaragdApp::word_count_cache`.
    pub(super) word_count_cache: usize,
    /// See `SmaragdApp::char_activity`.
    pub(super) char_activity: u64,
    /// The same "session words" quantity `word_count_panel::show` computes
    /// (`word_count_cache` minus `session_baseline_words`), hoisted up here
    /// since both the Word Count and Streak tabs need it.
    pub(super) today_words_so_far: u32,
    /// Which of the Streak tab's two inner tabs is showing — see
    /// `SmaragdApp::streak_sub_tab`.
    pub(super) streak_sub_tab: &'a mut crate::ui::streak_panel::StreakSubTab,
    /// The Belief Timeline tab's currently selected POV character — see
    /// `SmaragdApp::belief_timeline_character`. Mutated in place by
    /// `ui::belief_timeline_panel::show`, the same direct-mutation convention
    /// `streak_sub_tab` above uses, rather than round-tripping through a
    /// `DockAction` for something that's pure tab-local UI state.
    pub(super) belief_timeline_character: &'a mut String,
    pub(super) actions: Vec<DockAction>,
    /// See `SmaragdApp::focus_binder_requested`.
    pub(super) focus_binder_requested: bool,
    /// Derived from `SmaragdApp::collab` — see `CollabStatus`.
    pub(super) collab_status: CollabStatus<'a>,
    /// Whether a collaboration session is active — see `editor_panel::show`'s
    /// `collaborating` parameter.
    pub(super) collaborating: bool,
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
            DockTab::StoryGrid => "Story Grid".into(),
            DockTab::BeliefTimeline => "Belief Timeline".into(),
            DockTab::Pomodoro => "Pomodoro".into(),
            DockTab::WordCount => "Word Count".into(),
            DockTab::Collab => "Collaborate".into(),
            DockTab::Streak => "Streak".into(),
            DockTab::Bookmarks => "Bookmarks".into(),
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
                    let project_selected = self.metadata_target == MetadataTarget::Project;
                    let selected_folder = match &self.metadata_target {
                        MetadataTarget::Folder(path) => Some(path.as_path()),
                        _ => None,
                    };
                    // The currently-open document's fields come straight from
                    // `metadata_draft`/the live editor buffer (already in
                    // memory, reflects unsaved edits) rather than the cache,
                    // which is only for every *other* document — see
                    // `DocumentStatusCache`'s doc comment.
                    let document_row_color = |path: &Path| -> Option<egui::Color32> {
                        let is_open = Some(path) == self.open_path.as_deref();
                        match project.meta.binder_color_mode {
                            BinderColorMode::Off => None,
                            BinderColorMode::Status => {
                                let status = if is_open {
                                    non_empty(&self.metadata_draft.status)
                                } else {
                                    self.document_status_cache.status(path)
                                };
                                status
                                    .and_then(|s| project.status_color_hex(&s))
                                    .and_then(crate::color_theme::parse_hex_color)
                            }
                            BinderColorMode::Pov => {
                                let pov = if is_open {
                                    non_empty(&self.metadata_draft.pov)
                                } else {
                                    self.document_status_cache.pov(path)
                                };
                                pov.and_then(|p| project.pov_color_hex(&p))
                                    .and_then(crate::color_theme::parse_hex_color)
                            }
                            BinderColorMode::WordCountProgress => {
                                let (word_count, target) = if is_open {
                                    (
                                        crate::frontmatter::count_words(&self.editor.buffer),
                                        self.metadata_draft
                                            .word_count_target_text
                                            .trim()
                                            .parse()
                                            .ok(),
                                    )
                                } else {
                                    self.document_status_cache.word_count_progress(path)
                                };
                                target.filter(|&t| t > 0).map(|target| {
                                    crate::color_theme::word_count_progress_color(
                                        word_count as f32 / target as f32,
                                    )
                                })
                            }
                        }
                    };
                    // Same open-vs-cached split `document_row_color` above uses:
                    // the open document's stats come from the live buffer (may
                    // include unsaved edits), every other document's from
                    // `document_status_cache` (a single disk read, reused across
                    // frames). Returns `None` entirely when the setting is off,
                    // so `binder_panel::document_display_label` skips the
                    // stats suffix rather than showing a hidden zeroed one.
                    let document_stats = |path: &Path| -> Option<(usize, usize, usize)> {
                        if !self.settings.show_document_stats_in_binder {
                            return None;
                        }
                        let is_open = Some(path) == self.open_path.as_deref();
                        Some(if is_open {
                            (
                                crate::frontmatter::count_lines(&self.editor.buffer),
                                crate::frontmatter::count_words(&self.editor.buffer),
                                crate::frontmatter::count_chars(&self.editor.buffer),
                            )
                        } else {
                            self.document_status_cache.document_stats(path)
                        })
                    };
                    if let Some(event) = ui::binder_panel::show(
                        ui,
                        project,
                        self.selected_path,
                        self.focus_binder_requested,
                        project_selected,
                        selected_folder,
                        &document_row_color,
                        self.folder_word_counts,
                        self.git_dirty_paths,
                        &document_stats,
                    ) {
                        self.actions.push(DockAction::Binder(event));
                    }
                }
                None => {
                    ui.add_space(12.0);
                    ui.vertical_centered(|ui| {
                        ui.label("No project open.");
                        ui.add_space(8.0);
                        if ui.button("New Project…").clicked() {
                            self.actions.push(DockAction::RequestNewProject);
                        }
                        ui.add_space(4.0);
                        if ui.button("Open Project…").clicked() {
                            self.actions.push(DockAction::RequestOpenProject);
                        }
                    });
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
                        ui::tags_panel::TagsEvent::RenameTag(tag) => {
                            self.actions.push(DockAction::RenameTag(tag));
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
                                    crate::project::model::document_label(&node.name).to_string()
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
                let known_tags = project
                    .map(|project| project.all_tags())
                    .unwrap_or_default();
                match &self.metadata_target {
                    MetadataTarget::Project => match project {
                        Some(project) => {
                            if let Some(event) = ui::metadata_panel::show_project(ui, project) {
                                self.actions.push(DockAction::ProjectMeta(event));
                            }
                        }
                        None => {
                            ui.label("Open a project to edit its metadata.");
                        }
                    },
                    MetadataTarget::Folder(_) => match project {
                        Some(project) => {
                            let status_color = project
                                .status_color_hex(&self.folder_metadata_draft.status)
                                .and_then(crate::color_theme::parse_hex_color);
                            let pov_color = project
                                .pov_color_hex(&self.folder_metadata_draft.pov)
                                .and_then(crate::color_theme::parse_hex_color);
                            if let Some(event) = ui::metadata_panel::show_folder(
                                ui,
                                self.folder_metadata_draft,
                                &picklists,
                                status_color,
                                pov_color,
                                &known_tags,
                            ) {
                                self.actions.push(DockAction::Metadata(event));
                            }
                        }
                        None => {
                            ui.label("Open a project to edit folder metadata.");
                        }
                    },
                    MetadataTarget::Document => {
                        let word_count = crate::frontmatter::count_words(&self.editor.buffer);
                        let status_color = project
                            .and_then(|p| p.status_color_hex(&self.metadata_draft.status))
                            .and_then(crate::color_theme::parse_hex_color);
                        let pov_color = project
                            .and_then(|p| p.pov_color_hex(&self.metadata_draft.pov))
                            .and_then(crate::color_theme::parse_hex_color);
                        if let Some(event) = ui::metadata_panel::show(
                            ui,
                            self.open_path.as_deref(),
                            self.metadata_draft,
                            &picklists,
                            word_count,
                            status_color,
                            pov_color,
                            &known_tags,
                        ) {
                            self.actions.push(DockAction::Metadata(event));
                        }
                    }
                }
            }
            DockTab::Editor => {
                let note_titles = self
                    .project
                    .map(|project| project.tree.document_names())
                    .unwrap_or_default();
                let tag_names = self
                    .project
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
                    .zip(self.project)
                    .map(|(path, project)| project.bookmarked_lines_for(path))
                    .unwrap_or_default();
                match ui::editor_panel::show(
                    ui,
                    self.editor,
                    &note_titles,
                    &tag_names,
                    activate_wikilink_shortcut,
                    false,
                    self.settings.editor_font,
                    crate::editor_font::resolve_size(self.settings.editor_font_size),
                    self.collaborating,
                    self.settings.spell_check_language,
                    self.settings.show_editor_gutter,
                    &bookmarked_lines,
                    toggle_bookmark_shortcut,
                ) {
                    Some(EditorEvent::SaveError(err)) => {
                        self.actions.push(DockAction::EditorSaveError(err));
                    }
                    Some(EditorEvent::Wikilink(activation)) => {
                        self.actions.push(DockAction::Wikilink(activation));
                    }
                    Some(EditorEvent::ToggleBookmark(line)) => {
                        self.actions.push(DockAction::ToggleBookmark(line));
                    }
                    None => {}
                }
            }
            DockTab::Preview => {
                if self.editor.open_path.is_some() {
                    let base_dir = self.editor.open_path.as_deref().and_then(Path::parent);
                    let project_root = self.project.map(|project| project.root.as_path());
                    let note_titles = self
                        .project
                        .map(|project| project.tree.document_names())
                        .unwrap_or_default();
                    let document_title = self
                        .editor
                        .open_path
                        .as_deref()
                        .and_then(|path| path.file_stem())
                        .and_then(|stem| stem.to_str());
                    let outcome = ui::markdown_preview::show(
                        ui,
                        &self.editor.buffer,
                        base_dir,
                        project_root,
                        self.typeset_styles,
                        self.book_style_id,
                        self.custom_fonts,
                        self.settings.typewriter_quotes,
                        &note_titles,
                        document_title,
                    );
                    match outcome.click {
                        Some(ui::markdown_preview::PreviewClick::Wikilink(activation)) => {
                            self.actions.push(DockAction::Wikilink(activation));
                        }
                        Some(ui::markdown_preview::PreviewClick::Tag(tag)) => {
                            self.actions.push(DockAction::PreviewTagClicked(tag));
                        }
                        None => {}
                    }
                    if let Some(style_id) = outcome.style_changed {
                        self.actions.push(DockAction::SetBookStyle(style_id));
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
            DockTab::Bookmarks => match self.project {
                Some(project) => {
                    if let Some(event) = ui::bookmarks_panel::show(ui, project) {
                        self.actions.push(DockAction::Bookmarks(event));
                    }
                }
                None => {
                    ui.label("Open a project folder to get started.");
                }
            },
            DockTab::StoryGrid => match self.project {
                Some(project) => {
                    if let Some(event) = ui::story_grid_panel::show(
                        ui,
                        project,
                        self.settings.unplaced_story_cards_position,
                        &self.settings.resolved_story_grid_column_order(),
                        &self.settings.story_grid_hidden_columns,
                    ) {
                        self.actions.push(DockAction::StoryGrid(event));
                    }
                }
                None => {
                    ui.label("Open a project folder to get started.");
                }
            },
            DockTab::BeliefTimeline => match self.project {
                Some(project) => {
                    if let Some(event) =
                        ui::belief_timeline_panel::show(ui, project, self.belief_timeline_character)
                    {
                        self.actions.push(DockAction::BeliefTimeline(event));
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
            DockTab::Collab => {
                if let Some(event) = ui::collab_panel::show(ui, self.collab_status) {
                    self.actions.push(DockAction::Collab(event));
                }
            }
            DockTab::Streak => match self.project {
                Some(project) => {
                    if let Some(event) = ui::streak_panel::show(
                        ui,
                        project,
                        self.today_words_so_far,
                        self.streak_sub_tab,
                    ) {
                        self.actions.push(DockAction::Streak(event));
                    }
                }
                None => {
                    ui.label("Open a project folder to get started.");
                }
            },
        }
    }
}

/// `Some(s.trim())` unless that's empty — the same "blank field means unset,
/// not an empty string" convention `ui::metadata_panel::MetadataDraft::to_meta`
/// already uses, needed here too since `document_row_color`'s live (open-
/// document) branch reads straight from the draft's raw text buffers instead
/// of a parsed `DocumentMeta`.
fn non_empty(s: &str) -> Option<String> {
    let trimmed = s.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}
