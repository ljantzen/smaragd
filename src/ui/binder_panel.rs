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
    /// A "New From Template" menu entry was picked: create a document under
    /// `parent` whose initial content copies `template_path`.
    NewFileFromTemplate {
        parent: PathBuf,
        template_path: PathBuf,
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
    /// A file or folder was dragged onto a different folder and dropped there.
    MoveItem {
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
        Some(FolderRole::Templates) => " (Templates)",
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

/// A hand-built stand-in for `egui::CollapsingHeader::new(label).id_salt(id).show(...)`'s
/// header half — closely mirroring that widget's own internals (see egui's
/// `containers/collapsing_header.rs`) — because `CollapsingHeader` hardcodes its header
/// to `Sense::click()` with no way to also make it sense drags, which a folder row needs
/// (so it can be both clicked to expand/collapse and dragged to move, matching how a
/// document row's `Sense::click_and_drag()` button already works).
///
/// An earlier version of this worked around that by layering a second, drag-only
/// `ui.interact()` over the header's rect *after* building it. That doesn't actually
/// work: egui's hit-test resolves two interactive widgets sharing the same rect by
/// which was added last ("topmost"), and a drag-only widget necessarily has to be added
/// after the header (it needs the header's already-resolved rect) — so it always won
/// the hit test and swallowed every click before the header's own `Sense::click()`
/// widget ever saw it, silently breaking expand/collapse by mouse. Building the header
/// ourselves with a single `Sense::click_and_drag()` interact avoids the ambiguity
/// entirely: there's only ever one widget over that rect.
///
/// Returns the header's response (click/drag/focus, same as `CollapsingHeader`'s own
/// `header_response`) and the `CollapsingState` the caller drives (toggle it, then pass
/// the response to `CollapsingState::show_body_indented` to render the children).
fn folder_header(
    ui: &mut egui::Ui,
    id: egui::Id,
    label: &str,
    default_open: bool,
) -> (
    egui::Response,
    egui::containers::collapsing_header::CollapsingState,
) {
    use egui::NumExt as _;
    use egui::containers::collapsing_header::{CollapsingState, paint_default_icon};

    let button_padding = ui.spacing().button_padding;
    let available = ui.available_rect_before_wrap();
    let text_pos = available.min + egui::vec2(ui.spacing().indent, 0.0);
    let wrap_width = available.right() - text_pos.x;
    let galley = egui::WidgetText::from(label).into_galley(
        ui,
        Some(egui::TextWrapMode::Extend),
        wrap_width,
        egui::TextStyle::Button,
    );
    let desired_width = text_pos.x + galley.size().x + button_padding.x - available.left();
    let desired_size = egui::vec2(desired_width, galley.size().y + 2.0 * button_padding.y)
        .at_least(ui.spacing().interact_size);
    let (_, rect) = ui.allocate_space(desired_size);

    let header_response = ui.interact(rect, id, egui::Sense::click_and_drag());
    let text_pos = egui::pos2(
        text_pos.x,
        header_response.rect.center().y - galley.size().y / 2.0,
    );

    let state = CollapsingState::load_with_default_open(ui.ctx(), id, default_open);
    let openness = state.openness(ui.ctx());

    if ui.is_rect_visible(rect) {
        let visuals = ui.style().interact_selectable(&header_response, false);
        let (mut icon_rect, _) = ui.spacing().icon_rectangles(header_response.rect);
        icon_rect.set_center(egui::pos2(
            header_response.rect.left() + ui.spacing().indent / 2.0,
            header_response.rect.center().y,
        ));
        let icon_response = header_response.clone().with_new_rect(icon_rect);
        paint_default_icon(ui, openness, &icon_response);
        ui.painter().galley(text_pos, galley, visuals.text_color());
    }

    (header_response, state)
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
            let id = ui.make_persistent_id(&node.path);
            let (header_response, mut state) = folder_header(ui, id, &label, true);

            if header_response.clicked() {
                state.toggle(ui);
                header_response.request_focus();
            }
            // Keyboard expand/collapse (Left/Right arrow), only while this row has
            // focus — otherwise every folder in the tree would react to one keypress.
            if header_response.has_focus() {
                let collapse = state.is_open()
                    && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft));
                let expand = !state.is_open()
                    && ui
                        .input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight));
                if collapse {
                    state.set_open(false);
                } else if expand {
                    state.set_open(true);
                }
            }

            // Drag source: a folder (with everything under it) can be dragged onto a
            // different folder, same as a document — except the project root, which
            // isn't a real, movable node in the tree. `Project::move_item` catches
            // (with a clear error) dropping a folder into itself or one of its own
            // subfolders, so no need to filter that out here.
            if !is_root {
                header_response.dnd_set_drag_payload(node.path.clone());
            }

            // Drop target: a file or folder being dragged, released over this
            // folder's header. `is_root` doesn't need special-casing here — the
            // project root is itself rendered as a (non-collapsible-in-spirit)
            // folder header, so "drop onto root" already falls out of the same
            // handling.
            if let Some(dragged_path) = header_response.dnd_release_payload::<PathBuf>() {
                *event = Some(BinderEvent::MoveItem {
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
                let templates = project.template_documents();
                if !templates.is_empty() {
                    ui.menu_button("New From Template", |ui| {
                        for template in templates {
                            if ui.button(document_label(&template.name)).clicked() {
                                *event = Some(BinderEvent::NewFileFromTemplate {
                                    parent: node.path.clone(),
                                    template_path: template.path.clone(),
                                });
                                ui.close();
                            }
                        }
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
                        if ui
                            .radio(role == Some(FolderRole::Templates), "Templates")
                            .clicked()
                        {
                            *event = Some(BinderEvent::SetFolderRole {
                                path: node.path.clone(),
                                role: Some(FolderRole::Templates),
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

            state.show_body_indented(&header_response, ui, |ui| {
                for child in children {
                    show_node(ui, project, child, selected, event, false);
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
            // Drag source: see the matching folder-header handling above for the
            // other draggable case.
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
