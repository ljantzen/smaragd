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
    /// A document was dragged onto a different folder and dropped there.
    MoveDocument {
        path: PathBuf,
        new_parent: PathBuf,
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

/// Display label for a document node: `node.name` itself stays the full filename
/// (with `.md`) since it's matched against on-disk names and `ProjectMeta::node_order`
/// entries elsewhere (see `apply_order`) — only the binder's rendering trims the
/// extension, Scrivener/Ulysses-style.
fn document_label(name: &str) -> &str {
    name.strip_suffix(".md").unwrap_or(name)
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
            let header_response = response.header_response;

            // Drop target: a document being dragged, released over this folder's
            // header. `is_root` doesn't need special-casing here — the project root
            // is itself rendered as a (non-collapsible-in-spirit) folder header, so
            // "drop onto root" already falls out of the same handling.
            if let Some(dragged_path) = header_response.dnd_release_payload::<PathBuf>() {
                *event = Some(BinderEvent::MoveDocument {
                    path: (*dragged_path).clone(),
                    new_parent: node.path.clone(),
                });
            }
            if header_response.dnd_hover_payload::<PathBuf>().is_some() {
                ui.painter().rect_stroke(
                    header_response.rect,
                    2.0,
                    egui::Stroke::new(2.0, ui.visuals().selection.stroke.color),
                    egui::StrokeKind::Inside,
                );
            }

            header_response.context_menu(|ui| {
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
            // `ui.selectable_label` (a `Button` under the hood) only senses clicks by
            // default; `dnd_set_drag_payload` needs the widget itself to sense drags
            // too (`Response::drag_started` — and thus this — is only ever true for a
            // widget built with drag sensing), so a plain `selectable_label` never
            // actually starts a drag no matter how it's dragged. `click_and_drag`
            // keeps the exact same clickable/selectable look and behavior.
            let response = ui.add(
                egui::Button::selectable(is_selected, document_label(&node.name))
                    .sense(egui::Sense::click_and_drag()),
            );
            if response.clicked() {
                *event = Some(BinderEvent::Selected(node.path.clone()));
            }
            // Drag source: only markdown documents can be dragged, never folders —
            // moving a folder would also need to rewrite its nested order keys,
            // which `Project::move_document` (the drop side of this) deliberately
            // doesn't handle.
            response.dnd_set_drag_payload(node.path.clone());
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_label_strips_the_md_extension() {
        assert_eq!(document_label("01-opening.md"), "01-opening");
    }

    #[test]
    fn document_label_leaves_names_without_the_md_extension_unchanged() {
        assert_eq!(document_label("README"), "README");
    }

    #[test]
    fn document_label_only_strips_a_trailing_md_extension() {
        assert_eq!(document_label("notes.md.bak"), "notes.md.bak");
    }
}
