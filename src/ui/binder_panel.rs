use std::path::{Path, PathBuf};

use crate::project::model::{BinderNode, BinderNodeKind, BinderTree};

/// Outcomes of user interaction with the binder tree, handled by the caller (`app.rs`)
/// rather than mutated here — keeps this module a pure rendering layer over `&BinderTree`.
pub enum BinderEvent {
    Selected(PathBuf),
    NewFile { parent: PathBuf },
    NewFolder { parent: PathBuf },
    Rename { path: PathBuf },
    Delete { path: PathBuf },
}

pub fn show(ui: &mut egui::Ui, tree: &BinderTree, selected: Option<&Path>) -> Option<BinderEvent> {
    let mut event = None;
    show_node(ui, &tree.root, selected, &mut event, true);
    event
}

fn show_node(
    ui: &mut egui::Ui,
    node: &BinderNode,
    selected: Option<&Path>,
    event: &mut Option<BinderEvent>,
    is_root: bool,
) {
    match &node.kind {
        BinderNodeKind::Folder { children } => {
            let response = egui::CollapsingHeader::new(&node.name)
                .id_salt(node.id)
                .default_open(true)
                .show(ui, |ui| {
                    for child in children {
                        show_node(ui, child, selected, event, false);
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
                // Renaming or deleting the project's root folder isn't something the
                // binder should offer — that's the project folder itself, currently open.
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
            });
        }
    }
}
