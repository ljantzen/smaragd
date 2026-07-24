use std::path::{Path, PathBuf};

use crate::editor::EditorState;
use crate::project::{LoadError, Project};
use crate::settings::Settings;
use crate::ui;
use crate::ui::WikilinkActivation;
use crate::ui::binder_panel::BinderEvent;
use crate::ui::editor_panel::EditorEvent;
use crate::ui::name_prompt::{NamePromptOutcome, NamePromptState};

/// What a `NamePromptState` modal should do with the name once confirmed.
enum PromptAction {
    NewFile { parent: PathBuf },
    NewFolder { parent: PathBuf },
    Rename { path: PathBuf },
    NewProject { location: PathBuf },
}

struct PendingPrompt {
    action: PromptAction,
    state: NamePromptState,
}

pub struct TachyliteApp {
    project: Option<Project>,
    editor: EditorState,
    selected_path: Option<PathBuf>,
    status_message: Option<String>,
    preview_mode: bool,
    settings: Settings,
    show_settings: bool,
    prompt: Option<PendingPrompt>,
}

impl TachyliteApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);

        let settings = crate::settings::config_file_path()
            .map(|path| Settings::load_from_path(&path))
            .unwrap_or_default();

        let mut app = Self {
            project: None,
            editor: EditorState::default(),
            selected_path: None,
            status_message: None,
            preview_mode: false,
            settings,
            show_settings: false,
            prompt: None,
        };

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

    fn set_project(&mut self, project: Project, path: &Path) {
        self.project = Some(project);
        self.editor = EditorState::default();
        self.selected_path = None;
        self.status_message = None;
        self.settings.last_project_path = Some(path.to_path_buf());
        self.persist_settings();
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

    fn open_document(&mut self, path: &Path) {
        match self.editor.open(path) {
            Ok(()) => self.selected_path = Some(path.to_path_buf()),
            Err(err) => {
                self.status_message = Some(format!("Couldn't open {}: {err}", path.display()));
            }
        }
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
            BinderEvent::NewFile { parent } => {
                self.prompt = Some(PendingPrompt {
                    action: PromptAction::NewFile { parent },
                    state: NamePromptState {
                        title: "New File".to_string(),
                        confirm_label: "Create".to_string(),
                        name: String::new(),
                    },
                });
            }
            BinderEvent::NewFolder { parent } => {
                self.prompt = Some(PendingPrompt {
                    action: PromptAction::NewFolder { parent },
                    state: NamePromptState {
                        title: "New Folder".to_string(),
                        confirm_label: "Create".to_string(),
                        name: String::new(),
                    },
                });
            }
            BinderEvent::Rename { path } => {
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
            BinderEvent::Delete { path } => self.delete_node(&path),
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
        }
    }

    fn rename_node(&mut self, path: &Path, new_name: &str) {
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
    /// open.
    fn delete_node(&mut self, path: &Path) {
        let confirmed = rfd::MessageDialog::new()
            .set_title("Delete")
            .set_description(format!(
                "Delete \"{}\"? This cannot be undone.",
                path.display()
            ))
            .set_level(rfd::MessageLevel::Warning)
            .set_buttons(rfd::MessageButtons::YesNo)
            .show();
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
        let save_shortcut = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::S);
        if ui.ctx().input_mut(|i| i.consume_shortcut(&save_shortcut))
            && let Err(err) = self.editor.save()
        {
            self.status_message = Some(format!("Save failed: {err}"));
        }

        egui::Panel::top("menu_bar").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                egui::containers::menu::MenuButton::new("File").ui(ui, |ui| {
                    if ui.button("New Project").clicked() {
                        self.start_new_project();
                    }
                    if ui.button("Open Project").clicked() {
                        self.browse_for_project();
                    }
                    ui.add_enabled(false, egui::Button::new("Close Project"));
                    if ui.button("Settings").clicked() {
                        self.show_settings = true;
                    }
                    ui.separator();
                    if ui.button("Exit").clicked() {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                egui::containers::menu::MenuButton::new("Edit").ui(ui, |ui| {
                    ui.add_enabled(false, egui::Button::new("Cut"));
                    ui.add_enabled(false, egui::Button::new("Copy"));
                    ui.add_enabled(false, egui::Button::new("Paste"));
                });
                egui::containers::menu::MenuButton::new("View").ui(ui, |ui| {
                    ui.checkbox(&mut self.preview_mode, "Toggle preview");
                });
                ui.add_enabled(false, egui::Button::new("Tools"));
                egui::containers::menu::MenuButton::new("Help").ui(ui, |ui| {
                    ui.add_enabled(false, egui::Button::new("About"));
                });
            });
        });

        if ui::settings_panel::show(ui.ctx(), &mut self.show_settings, &mut self.settings) {
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

        if let Some(msg) = self.status_message.clone() {
            egui::Panel::bottom("status_bar").show(ui, |ui| {
                ui.colored_label(egui::Color32::from_rgb(200, 60, 60), msg);
            });
        }

        egui::Panel::left("binder_panel")
            .resizable(true)
            .default_size(220.0)
            .show(ui, |ui| match &self.project {
                Some(project) => {
                    if let Some(event) =
                        ui::binder_panel::show(ui, &project.tree, self.selected_path.as_deref())
                    {
                        self.handle_binder_event(event);
                    }
                }
                None => {
                    ui.label("Open a project folder to get started.");
                }
            });

        egui::CentralPanel::default().show(ui, |ui| {
            if self.preview_mode {
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
            } else {
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
        });
    }
}
