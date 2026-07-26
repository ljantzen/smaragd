use std::fs;
use std::path::{Path, PathBuf};

use crate::editor::EditorState;
use crate::project::{LoadError, Project, RestoreError};
use crate::search::{self, SearchScope};
use crate::settings::Settings;
use crate::shortcuts::{ShortcutAction, sorted_by_specificity};
use crate::ui;
use crate::ui::WikilinkActivation;
use crate::ui::binder_panel::BinderEvent;
use crate::ui::command_prompt::{
    Command, CommandPromptEvent, CommandPromptState, DarkModeChoice, GitCommand,
};
use crate::ui::corkboard_panel::{CardDraft, CardEditorOutcome, CorkboardEvent};
use crate::ui::editor_panel::EditorEvent;
use crate::ui::find_replace_panel::{FindReplaceEvent, FindReplaceState};
use crate::ui::metadata_panel::{MetadataDraft, MetadataOutcome};
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
}

struct PendingPrompt {
    action: PromptAction,
    state: NamePromptState,
}

/// Which of the three mutually exclusive ways of looking at the project is currently
/// shown in the `CentralPanel`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ViewMode {
    Editor,
    Preview,
    Corkboard,
}

pub struct TachyliteApp {
    project: Option<Project>,
    editor: EditorState,
    selected_path: Option<PathBuf>,
    status_message: Option<String>,
    view_mode: ViewMode,
    settings: Settings,
    show_settings: bool,
    prompt: Option<PendingPrompt>,
    recording_shortcut: Option<ShortcutAction>,
    find_replace: FindReplaceState,
    card_draft: Option<CardDraft>,
    command_prompt: CommandPromptState,
    metadata_draft: Option<MetadataDraft>,
}

impl TachyliteApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);

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
            view_mode: ViewMode::Editor,
            settings,
            show_settings: false,
            prompt: None,
            recording_shortcut: None,
            find_replace: FindReplaceState::default(),
            card_draft: None,
            command_prompt: CommandPromptState::default(),
            metadata_draft: None,
        };

        if let Some(id) = &app.settings.color_theme
            && let Some(theme) = crate::color_theme::find(id)
        {
            crate::color_theme::apply(&cc.egui_ctx, theme);
        }

        if app.settings.reopen_last_project
            && let Some(path) = app.settings.last_project_path.clone()
        {
            app.open_project(&path);
        }

        app
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
            state: NamePromptState {
                title: "Commit".to_string(),
                confirm_label: if push_after {
                    "Commit and Push".to_string()
                } else {
                    "Commit".to_string()
                },
                name: "Tachylite backup".to_string(),
            },
        });
    }

    fn run_git_commit(&mut self, message: &str, push_after: bool) {
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
                    self.run_git_push();
                }
            }
            Err(crate::git::GitError::NothingToCommit) => {
                self.status_message = Some("Nothing to commit".to_string());
            }
            Err(err) => self.status_message = Some(format!("Commit failed: {err}")),
        }
    }

    fn run_git_push(&mut self) {
        let Some(project) = &self.project else {
            self.status_message = Some("No project open".to_string());
            return;
        };
        if !project.meta.git_enabled {
            self.status_message = Some("Git support isn't enabled for this project".to_string());
            return;
        }
        match crate::git::push(&project.root) {
            Ok(()) => self.status_message = Some("Pushed".to_string()),
            Err(err) => self.status_message = Some(format!("Push failed: {err}")),
        }
    }

    /// Pulls, then rescans the binder tree so any files the pull added/removed show
    /// up. Deliberately doesn't reload the currently open document even if its
    /// on-disk content changed — that could silently clobber unsaved local edits;
    /// the user can reopen it themselves if they want the pulled version.
    fn run_git_pull(&mut self) {
        let Some(project) = &mut self.project else {
            self.status_message = Some("No project open".to_string());
            return;
        };
        if !project.meta.git_enabled {
            self.status_message = Some("Git support isn't enabled for this project".to_string());
            return;
        }
        match crate::git::pull(&project.root) {
            Ok(()) => {
                project.rescan();
                self.status_message = Some("Pulled".to_string());
            }
            Err(err) => self.status_message = Some(format!("Pull failed: {err}")),
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
            state: NamePromptState {
                title: "New Project".to_string(),
                confirm_label: "Create".to_string(),
                name: String::new(),
            },
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

    /// Open the document metadata modal, pre-filled from the open document's current
    /// frontmatter (parsed from the live buffer, not necessarily what's on disk yet,
    /// so it reflects any unsaved edits to the block itself).
    fn open_metadata_editor(&mut self) {
        if self.editor.open_path.is_none() {
            self.status_message = Some("No document open".to_string());
            return;
        }
        let meta = crate::frontmatter::parse(&self.editor.buffer);
        self.metadata_draft = Some(MetadataDraft::from_meta(&meta));
    }

    /// Handle the metadata modal closing this frame. On save, rewrites the editor
    /// buffer's frontmatter block in place (preserving any keys the form doesn't
    /// expose — see `frontmatter::write_back`) and marks it dirty, same as any other
    /// in-buffer edit; the existing save path (explicit Save, autosave on focus loss,
    /// etc.) takes it from there.
    fn finish_metadata_editor(&mut self, outcome: MetadataOutcome) {
        self.metadata_draft = None;
        let MetadataOutcome::Save(meta) = outcome else {
            return;
        };
        self.editor.buffer = crate::frontmatter::write_back(&self.editor.buffer, &meta);
        self.editor.mark_dirty();
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
            BinderEvent::Rename { path } => self.prompt_rename(path),
            BinderEvent::Delete { path } => self.delete_node(&path),
            BinderEvent::Restore { path } => self.restore_node(&path),
            BinderEvent::SetFolderRole { path, role } => self.set_folder_role(&path, role),
            BinderEvent::EmptyTrash { path } => self.empty_trash_folder(&path),
            BinderEvent::MoveItem { path, new_parent } => self.move_item(&path, &new_parent),
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
                self.view_mode = ViewMode::Editor;
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
            state: NamePromptState {
                title: "New File".to_string(),
                confirm_label: "Create".to_string(),
                name: String::new(),
            },
        });
    }

    /// Open the "New Folder" name-prompt modal for a folder to be created inside
    /// `parent`. Shared by the binder's right-click menu and the New Folder keyboard
    /// shortcut.
    fn prompt_new_folder(&mut self, parent: PathBuf) {
        self.prompt = Some(PendingPrompt {
            action: PromptAction::NewFolder { parent },
            state: NamePromptState {
                title: "New Folder".to_string(),
                confirm_label: "Create".to_string(),
                name: String::new(),
            },
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
            state: NamePromptState {
                title: "Rename".to_string(),
                confirm_label: "Rename".to_string(),
                name,
            },
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
                self.view_mode = match self.view_mode {
                    ViewMode::Preview => ViewMode::Editor,
                    _ => ViewMode::Preview,
                };
            }
            ShortcutAction::ToggleCorkboard => {
                self.view_mode = match self.view_mode {
                    ViewMode::Corkboard => ViewMode::Editor,
                    _ => ViewMode::Corkboard,
                };
            }
            ShortcutAction::Save => {
                if let Err(err) = self.editor.save() {
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
            ShortcutAction::FindReplace => self.find_replace.request_open(),
            ShortcutAction::CommandPrompt => self.command_prompt.request_open(),
            ShortcutAction::GitCommit => self.prompt_git_commit(false),
            ShortcutAction::GitPush => self.run_git_push(),
            ShortcutAction::EditMetadata => self.open_metadata_editor(),
        }
    }

    /// Run a command parsed from the `:` command prompt.
    fn execute_command(&mut self, ctx: &egui::Context, command: Command) {
        match command {
            Command::Save => {
                if let Err(err) = self.editor.save() {
                    self.status_message = Some(format!("Save failed: {err}"));
                }
            }
            Command::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Command::SaveAndQuit => {
                if let Err(err) = self.editor.save() {
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
                GitCommand::Commit(Some(message)) => self.run_git_commit(&message, false),
                GitCommand::Commit(None) => self.prompt_git_commit(false),
                GitCommand::Push => self.run_git_push(),
                GitCommand::Pull => self.run_git_pull(),
                GitCommand::Backup(Some(message)) => self.run_git_commit(&message, true),
                GitCommand::Backup(None) => self.prompt_git_commit(true),
            },
            Command::Find(query) => {
                if !query.is_empty() {
                    self.find_replace.query = query;
                }
                self.find_replace.request_open();
            }
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
                let Some(theme) = crate::color_theme::find(id) else {
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

    fn finish_prompt(&mut self, outcome: NamePromptOutcome) {
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
            PromptAction::Rename { path } => self.rename_node(&path, name),
            PromptAction::NewProject { location } => self.create_project(&location, name),
            PromptAction::GitCommit { push_after } => self.run_git_commit(name, push_after),
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
}

impl eframe::App for TachyliteApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if self.recording_shortcut.is_none() {
            let ctx = ui.ctx().clone();
            let bindings = sorted_by_specificity(self.settings.shortcuts.bindings());
            let triggered: Vec<ShortcutAction> = bindings
                .into_iter()
                .filter(|(_, shortcut)| ctx.input_mut(|i| i.consume_shortcut(shortcut)))
                .map(|(action, _)| action)
                .collect();
            for action in triggered {
                self.dispatch_shortcut_action(&ctx, action);
            }
        }

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

                    if menu_button_with_shortcut(ui, "New Project", new_project_shortcut).clicked()
                    {
                        self.start_new_project();
                    }
                    if menu_button_with_shortcut(ui, "Open Project", open_project_shortcut)
                        .clicked()
                    {
                        self.browse_for_project();
                    }
                    ui.add_enabled(false, egui::Button::new("Close Project"));
                    if menu_button_with_shortcut(ui, "Settings", open_settings_shortcut).clicked() {
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
                        self.open_metadata_editor();
                    }
                });
                egui::containers::menu::MenuButton::new("View").ui(ui, |ui| {
                    ui.radio_value(&mut self.view_mode, ViewMode::Editor, "Editor");
                    ui.radio_value(&mut self.view_mode, ViewMode::Preview, "Preview");
                    ui.radio_value(&mut self.view_mode, ViewMode::Corkboard, "Corkboard");
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
                        // Cloned rather than borrowed: `set_color_theme` below needs
                        // `&mut self`, which a live borrow of `self.settings` here
                        // would conflict with across loop iterations.
                        let current = self.settings.color_theme.clone();
                        if ui.radio(current.is_none(), "Default").clicked() {
                            self.set_color_theme(ui.ctx(), None);
                        }
                        for theme in crate::color_theme::THEMES {
                            if ui
                                .radio(current.as_deref() == Some(theme.id), theme.label)
                                .clicked()
                            {
                                self.set_color_theme(ui.ctx(), Some(theme.id));
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
                        if ui.button("Commit and Push").clicked() {
                            self.prompt_git_commit(true);
                        }
                        let push_shortcut = self.settings.shortcuts.get(ShortcutAction::GitPush);
                        if menu_button_with_shortcut(ui, "Push", push_shortcut).clicked() {
                            self.run_git_push();
                        }
                        if ui.button("Pull").clicked() {
                            self.run_git_pull();
                        }
                    }
                });
                egui::containers::menu::MenuButton::new("Help").ui(ui, |ui| {
                    ui.add_enabled(false, egui::Button::new("About"));
                });
            });
        });

        if ui::settings_panel::show(
            ui.ctx(),
            &mut self.show_settings,
            &mut self.settings,
            &mut self.recording_shortcut,
        ) {
            self.persist_settings();
        }

        if self.prompt.is_some() {
            let outcome = {
                let pending = self.prompt.as_mut().expect("checked above");
                ui::name_prompt::show(ui.ctx(), &mut pending.state)
            };
            if let Some(outcome) = outcome {
                self.finish_prompt(outcome);
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
            if let Some(event) =
                ui::command_prompt::show(ui.ctx(), &mut self.command_prompt, &note_titles)
            {
                let ctx = ui.ctx().clone();
                match event {
                    CommandPromptEvent::Run(command) => self.execute_command(&ctx, command),
                    CommandPromptEvent::Error(err) => self.status_message = Some(err),
                }
            }
        }

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

        egui::Panel::left("binder_panel")
            .resizable(true)
            .default_size(220.0)
            .show(ui, |ui| match &self.project {
                Some(project) => {
                    if let Some(event) =
                        ui::binder_panel::show(ui, project, self.selected_path.as_deref())
                    {
                        self.handle_binder_event(event);
                    }
                }
                None => {
                    ui.label("Open a project folder to get started.");
                }
            });

        egui::CentralPanel::default().show(ui, |ui| match self.view_mode {
            ViewMode::Preview => {
                if self.editor.open_path.is_some() {
                    let base_dir = self.editor.open_path.as_deref().and_then(Path::parent);
                    if let Some(activation) =
                        ui::markdown_preview::show(ui, &self.editor.buffer, base_dir)
                    {
                        self.activate_wikilink(activation);
                    }
                } else {
                    ui.label("Select a file from the binder to preview.");
                }
            }
            ViewMode::Editor => {
                let note_titles = self
                    .project
                    .as_ref()
                    .map(|project| project.tree.document_names())
                    .unwrap_or_default();
                match ui::editor_panel::show(ui, &mut self.editor, &note_titles) {
                    Some(EditorEvent::SaveError(err)) => self.status_message = Some(err),
                    Some(EditorEvent::Wikilink(activation)) => self.activate_wikilink(activation),
                    None => {}
                }
            }
            ViewMode::Corkboard => match &self.project {
                Some(project) => {
                    if let Some(event) = ui::corkboard_panel::show(ui, project) {
                        self.handle_corkboard_event(event);
                    }
                }
                None => {
                    ui.label("Open a project folder to get started.");
                }
            },
        });

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

        if let Some(draft) = &mut self.metadata_draft
            && let Some(outcome) = ui::metadata_panel::show(ui.ctx(), draft)
        {
            self.finish_metadata_editor(outcome);
        }
    }
}
