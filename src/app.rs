use std::path::{Path, PathBuf};

use crate::editor::EditorState;
use crate::project::Project;
use crate::ui;
use crate::ui::binder_panel::BinderEvent;

pub struct TachyliteApp {
    project: Option<Project>,
    editor: EditorState,
    selected_path: Option<PathBuf>,
    project_path_input: String,
    status_message: Option<String>,
}

impl TachyliteApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self {
            project: None,
            editor: EditorState::default(),
            selected_path: None,
            project_path_input: ".".to_string(),
            status_message: None,
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

    fn open_document(&mut self, path: &Path) {
        match self.editor.open(path) {
            Ok(()) => self.selected_path = Some(path.to_path_buf()),
            Err(err) => {
                self.status_message = Some(format!("Couldn't open {}: {err}", path.display()));
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

        egui::Panel::top("toolbar").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.text_edit_singleline(&mut self.project_path_input);
                if ui.button("Open Project").clicked() {
                    let path = PathBuf::from(self.project_path_input.trim());
                    self.open_project(&path);
                }
                if ui.button("Save").clicked()
                    && let Err(err) = self.editor.save()
                {
                    self.status_message = Some(format!("Save failed: {err}"));
                }
                if let Some(msg) = &self.status_message {
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
            if let Some(err) = ui::editor_panel::show(ui, &mut self.editor) {
                self.status_message = Some(err);
            }
        });
    }
}
