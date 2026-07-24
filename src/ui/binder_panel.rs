use std::path::{Path, PathBuf};

use crate::project::model::{BinderNode, BinderNodeKind};
use crate::project::{FolderRole, Project};

/// Outcomes of user interaction with the binder tree, handled by the caller (`app.rs`)
/// rather than mutated here — keeps this module a pure rendering layer over `&Project`.
pub enum BinderEvent {
    Selected(PathBuf),
    NewFile {
        parent: PathBuf,
    },
    NewFolder {
        parent: PathBuf,
    },
    Rename {
        path: PathBuf,
    },
    Delete {
        path: PathBuf,
    },
    Restore {
        path: PathBuf,
    },
    SetFolderRole {
        path: PathBuf,
        role: Option<FolderRole>,
    },
    EmptyTrash {
        path: PathBuf,
    },
}

pub fn show(ui: &mut egui::Ui, project: &Project, selected: Option<&Path>) -> Option<BinderEvent> {
    let mut event = None;
    show_node(ui, project, &project.tree.root, selected, &mut event, true);
    event
}

fn role_suffix(role: Option<FolderRole>) -> &'static str {
    match role {
        Some(FolderRole::Research) => " (Research)",
        Some(FolderRole::Trash) => " (Trash)",
        None => "",
    }
}

fn show_node(
    ui: &mut egui::Ui,
    project: &Project,
    node: &BinderNode,
    selected: Option<&Path>,
    event: &mut Option<BinderEvent>,
    is_root: bool,
) {
    match &node.kind {
        BinderNodeKind::Folder { children } => {
            let role = project.folder_role(&node.path);
            let label = format!("{}{}", node.name, role_suffix(role));
            let response = egui::CollapsingHeader::new(label)
                .id_salt(node.id)
                .default_open(true)
                .show(ui, |ui| {
                    for child in children {
                        show_node(ui, project, child, selected, event, false);
                    }
                });

            response.header_response.context_menu(|ui| {
                if ui.button("New File").clicked() {
                    *event = Some(BinderEvent::NewFile {
                        parent: node.path.clone(),
                    });
                }
                if ui.button("New Folder").clicked() {
                    *event = Some(BinderEvent::NewFolder {
                        parent: node.path.clone(),
                    });
                }
                // Renaming, deleting, or assigning a role to the project's root
                // folder isn't something the binder should offer — that's the
                // project folder itself, currently open.
                if !is_root {
                    ui.separator();
                    if ui.button("Rename").clicked() {
                        *event = Some(BinderEvent::Rename {
                            path: node.path.clone(),
                        });
                    }
                    if ui.button("Delete").clicked() {
                        *event = Some(BinderEvent::Delete {
                            path: node.path.clone(),
                        });
                    }
                    if project.trashed_origin(&node.path).is_some()
                        && ui.button("Restore").clicked()
                    {
                        *event = Some(BinderEvent::Restore {
                            path: node.path.clone(),
                        });
                    }
                    ui.separator();
                    ui.menu_button("Folder Role", |ui| {
                        if ui
                            .radio(role == Some(FolderRole::Research), "Research")
                            .clicked()
                        {
                            *event = Some(BinderEvent::SetFolderRole {
                                path: node.path.clone(),
                                role: Some(FolderRole::Research),
                            });
                            ui.close();
                        }
                        if ui.radio(role == Some(FolderRole::Trash), "Trash").clicked() {
                            *event = Some(BinderEvent::SetFolderRole {
                                path: node.path.clone(),
                                role: Some(FolderRole::Trash),
                            });
                            ui.close();
                        }
                        if ui.radio(role.is_none(), "None").clicked() {
                            *event = Some(BinderEvent::SetFolderRole {
                                path: node.path.clone(),
                                role: None,
                            });
                            ui.close();
                        }
                    });
                    if role == Some(FolderRole::Trash) && ui.button("Empty Trash").clicked() {
                        *event = Some(BinderEvent::EmptyTrash {
                            path: node.path.clone(),
                        });
                    }
                }
            });
        }
        BinderNodeKind::Document => {
            let is_selected = selected == Some(node.path.as_path());
            let response = ui.selectable_label(is_selected, &node.name);
            if response.clicked() {
                *event = Some(BinderEvent::Selected(node.path.clone()));
            }
            response.context_menu(|ui| {
                if ui.button("Rename").clicked() {
                    *event = Some(BinderEvent::Rename {
                        path: node.path.clone(),
                    });
                }
                if ui.button("Delete").clicked() {
                    *event = Some(BinderEvent::Delete {
                        path: node.path.clone(),
                    });
                }
                if project.trashed_origin(&node.path).is_some() && ui.button("Restore").clicked() {
                    *event = Some(BinderEvent::Restore {
                        path: node.path.clone(),
                    });
                }
            });
        }
    }
}
