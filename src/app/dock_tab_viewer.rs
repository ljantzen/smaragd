use super::*;

/// Requests raised by `AppTabViewer::ui` for the caller to apply once the dock has
/// finished rendering for the frame — `egui_dock::TabViewer::ui` only gets `&mut
/// self` on the *viewer*, not on `SmaragdApp`, so it can't call `&mut self`
/// methods like `open_document` directly; it collects what it wants done instead.
pub(super) enum DockAction {
    OpenDocument(PathBuf),
    Binder(BinderEvent),
    ProjectMeta(crate::ui::metadata_panel::ProjectMetaEvent),
    RefreshBacklinks,
    RefreshTags,
    EditorSaveError(String),
    Wikilink(WikilinkActivation),
    Corkboard(CorkboardEvent),
    StoryGrid(crate::ui::story_grid_panel::StoryGridEvent),
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
    /// See `SmaragdApp`'s `MetadataState::project_selected` — when true, the
    /// Metadata dock shows `ui::metadata_panel::show_project` (the binder's
    /// root project row) instead of the open document's frontmatter.
    pub(super) project_metadata_selected: bool,
    pub(super) editor: &'a mut EditorState,
    pub(super) settings: &'a Settings,
    pub(super) color_themes: &'a [crate::color_theme::ColorTheme],
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
            DockTab::Pomodoro => "Pomodoro".into(),
            DockTab::WordCount => "Word Count".into(),
            DockTab::Collab => "Collaborate".into(),
            DockTab::Streak => "Streak".into(),
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
                        self.project_metadata_selected,
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
                    }
                }
            }
            DockTab::Metadata if self.project_metadata_selected => match self.project {
                Some(project) => {
                    if let Some(event) = ui::metadata_panel::show_project(ui, project) {
                        self.actions.push(DockAction::ProjectMeta(event));
                    }
                }
                None => {
                    ui.label("Open a project to edit its metadata.");
                }
            },
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
                    self.collaborating,
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
            DockTab::StoryGrid => match self.project {
                Some(project) => {
                    if let Some(event) = ui::story_grid_panel::show(
                        ui,
                        project,
                        self.settings.unplaced_story_cards_position,
                    ) {
                        self.actions.push(DockAction::StoryGrid(event));
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
