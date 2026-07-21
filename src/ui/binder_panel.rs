use std::path::{Path, PathBuf};

use crate::project::model::{BinderNode, BinderNodeKind, BinderTree};

/// Outcomes of user interaction with the binder tree, handled by the caller (`app.rs`)
/// rather than mutated here — keeps this module a pure rendering layer over `&BinderTree`.
pub enum BinderEvent {
    Selected(PathBuf),
    NewFile { parent: PathBuf },
    NewFolder { parent: PathBuf },
}

pub fn show(ui: &mut egui::Ui, tree: &BinderTree, selected: Option<&Path>) -> Option<BinderEvent> {
    let mut event = None;
    show_node(ui, &tree.root, selected, &mut event);
    event
}

fn show_node(
    ui: &mut egui::Ui,
    node: &BinderNode,
    selected: Option<&Path>,
    event: &mut Option<BinderEvent>,
) {
    match &node.kind {
        BinderNodeKind::Folder { children } => {
            egui::CollapsingHeader::new(&node.name)
                .id_salt(node.id)
                .default_open(true)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if ui.small_button("+ file").clicked() {
                            *event = Some(BinderEvent::NewFile {
                                parent: node.path.clone(),
                            });
                        }
                        if ui.small_button("+ folder").clicked() {
                            *event = Some(BinderEvent::NewFolder {
                                parent: node.path.clone(),
                            });
                        }
                    });
                    for child in children {
                        show_node(ui, child, selected, event);
                    }
                });
        }
        BinderNodeKind::Document => {
            let is_selected = selected == Some(node.path.as_path());
            if ui.selectable_label(is_selected, &node.name).clicked() {
                *event = Some(BinderEvent::Selected(node.path.clone()));
            }
        }
    }
}
