use std::path::{Path, PathBuf};

use crate::editor::EditorState;
use crate::project::Project;
use crate::ui;
use crate::ui::binder_panel::BinderEvent;

pub struct TachyliteApp {
    project: Option<Project>,
    editor: EditorState,
    selected_path: Option<PathBuf>,
    status_message: Option<String>,
    preview_mode: bool,
}

impl TachyliteApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self {
            project: None,
            editor: EditorState::default(),
            selected_path: None,
            status_message: None,
            preview_mode: false,
        }
    }

    fn open_project(&mut self, path: &Path) {
        match Project::load_from_folder(path) {
            Ok(project) => {
                self.project = Some(project);
                self.selected_path = None;
                self.status_message = None;
            }
            Err(err) => {
                self.status_message = Some(format!("Couldn't open {}: {err}", path.display()));
            }
        }
    }

    /// Open the OS's native folder-picker dialog and, if the user selects a folder,
    /// open it as a project immediately.
    fn browse_for_project(&mut self) {
        if let Some(path) = rfd::FileDialog::new().pick_folder() {
            self.open_project(&path);
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

    /// Resolve a `[[wikilink]]` target clicked in the preview to a document in the
    /// current project (matched by filename, case-insensitively) and open it.
    fn open_wikilink(&mut self, target: &str) {
        let Some(project) = &self.project else {
            self.status_message = Some(format!("No project open — can't resolve [[{target}]]"));
            return;
        };
        match project.tree.find_document_by_stem(target) {
            Some(node) => {
                let path = node.path.clone();
                self.open_document(&path);
            }
            None => {
                self.status_message = Some(format!("No note found for [[{target}]]"));
            }
        }
    }

    fn handle_binder_event(&mut self, event: BinderEvent) {
        match event {
            BinderEvent::Selected(path) => self.open_document(&path),
            BinderEvent::NewFile { parent } => self.create_document(&parent),
            BinderEvent::NewFolder { parent } => self.create_folder(&parent),
        }
    }

    fn create_document(&mut self, parent: &Path) {
        let Some(project) = &mut self.project else {
            return;
        };
        let name = unique_name(parent, "New Document", ".md");
        match project.create_document(parent, &name) {
            Ok(path) => self.open_document(&path),
            Err(err) => self.status_message = Some(format!("Couldn't create file: {err}")),
        }
    }

    fn create_folder(&mut self, parent: &Path) {
        let Some(project) = &mut self.project else {
            return;
        };
        let name = unique_name(parent, "New Folder", "");
        if let Err(err) = project.create_folder(parent, &name) {
            self.status_message = Some(format!("Couldn't create folder: {err}"));
        }
    }
}

/// Find an unused filename in `parent` starting from `{base}{ext}`, falling back to
/// `{base} 2{ext}`, `{base} 3{ext}`, etc. to avoid clobbering an existing file.
fn unique_name(parent: &Path, base: &str, ext: &str) -> String {
    let candidate = format!("{base}{ext}");
    if !parent.join(&candidate).exists() {
        return candidate;
    }
    let mut n = 2;
    loop {
        let candidate = format!("{base} {n}{ext}");
        if !parent.join(&candidate).exists() {
            return candidate;
        }
        n += 1;
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
                    if ui.button("Open Project").clicked() {
                        self.browse_for_project();
                    }
                    ui.add_enabled(false, egui::Button::new("Close Project"));
                    ui.add_enabled(false, egui::Button::new("Settings"));
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
                    if let Some(target) = ui::markdown_preview::show(ui, &self.editor.buffer) {
                        self.open_wikilink(&target);
                    }
                } else {
                    ui.label("Select a file from the binder to preview.");
                }
            } else if let Some(err) = ui::editor_panel::show(ui, &mut self.editor) {
                self.status_message = Some(err);
            }
        });
    }
}
