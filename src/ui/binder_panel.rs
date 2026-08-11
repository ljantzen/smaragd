use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::project::model::{BinderNode, BinderNodeKind, document_label};
use crate::project::{BinderColorMode, FolderRole, PicklistField, Project};

/// Outcomes of user interaction with the binder tree, handled by the caller (`app.rs`)
/// rather than mutated here — keeps this module a pure rendering layer over `&Project`.
#[derive(Debug, PartialEq)]
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
    /// A file or folder was dragged onto a different folder's header and
    /// dropped there — always lands at the end of that folder.
    MoveItem {
        path: PathBuf,
        new_parent: PathBuf,
    },
    /// A file or folder was dragged and dropped directly onto a document row —
    /// moved to that document's parent folder (which may be the dragged item's
    /// own current parent — a pure reorder) and positioned immediately before
    /// it, unlike `MoveItem`'s always-append-at-the-end.
    MoveItemBefore {
        path: PathBuf,
        before: PathBuf,
    },
    SetFolderRole {
        path: PathBuf,
        role: Option<FolderRole>,
    },
    /// `path`'s "Dropdown Source" checkbox for `field` was toggled: `Some(path)`
    /// assigns `path` as `field`'s picklist folder, `None` clears it (only
    /// meaningful when `path` was the one currently assigned).
    SetPicklistFolder {
        field: PicklistField,
        path: Option<PathBuf>,
    },
    EmptyTrash {
        path: PathBuf,
    },
    /// "Export…" was picked for a folder — compile it and its subfolders to a
    /// document format (DOCX/EPUB), handled by opening the export dialog.
    Export {
        path: PathBuf,
    },
    /// The project's own root row was clicked — show project-wide metadata
    /// (title/logline/synopsis/etc., see `ui::metadata_panel::show_project`)
    /// in the Metadata dock instead of a document's frontmatter. Only ever
    /// raised for the root folder, matching how `Selected` is only ever
    /// raised for a document.
    SelectProject,
    /// A non-root folder row was clicked — show its own metadata (Type/
    /// Status/POV/Word Count Target/Tags, see `ui::metadata_panel::show_folder`)
    /// in the Metadata dock. Kept separate from `SelectProject`: the Project
    /// form and a plain folder's metadata form have entirely different fields.
    SelectFolder(PathBuf),
}

/// Keyboard filter claimed on every focused binder row: all four arrow keys are ours
/// (Up/Down move the tree cursor, Left/Right expand/collapse), so egui's own built-in
/// "arrow keys move focus to the nearest widget in that direction" behavior never
/// fires and fights with — or unpredictably jumps focus out of the tree instead of —
/// our own handling. This is the same mechanism `TextEdit` uses to claim arrow keys
/// for cursor movement instead of focus navigation.
const ARROW_KEYS_FILTER: egui::EventFilter = egui::EventFilter {
    tab: false,
    horizontal_arrows: true,
    vertical_arrows: true,
    escape: false,
};

#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut egui::Ui,
    project: &Project,
    selected: Option<&Path>,
    focus_requested: bool,
    project_selected: bool,
    selected_folder: Option<&Path>,
    document_row_color: &dyn Fn(&Path) -> Option<egui::Color32>,
    folder_word_counts: &HashMap<PathBuf, usize>,
    git_dirty: &std::collections::HashSet<PathBuf>,
) -> Option<BinderEvent> {
    let mut event = None;
    let mut visible_rows: Vec<(PathBuf, egui::Id)> = Vec::new();
    show_node(
        ui,
        project,
        &project.tree.root,
        selected,
        &mut event,
        true,
        &mut visible_rows,
        project_selected,
        selected_folder,
        document_row_color,
        folder_word_counts,
        git_dirty,
    );

    // Up/Down move the keyboard cursor between rows, in the same top-to-bottom order
    // they were just rendered in — which already skips the children of any collapsed
    // folder, since `show_node` never recurses into those in the first place. Only
    // acts when a binder row actually has focus, so this can't steal Up/Down from,
    // say, the main editor's `TextEdit` while the user is typing there.
    if let Some(focused_id) = ui.ctx().memory(|mem| mem.focused())
        && let Some(current) = visible_rows.iter().position(|(_, id)| *id == focused_id)
    {
        let move_down =
            ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown));
        let move_up = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp));
        let next = if move_down {
            Some((current + 1).min(visible_rows.len() - 1))
        } else if move_up {
            Some(current.saturating_sub(1))
        } else {
            None
        };
        // Skip the `request_focus` call entirely when clamped at either end (`next
        // == current`): `Memory::request_focus` unconditionally resets the target's
        // focus-lock filter to its default (unclaimed) state, even when it's
        // already the focused widget — calling it every frame while pinned at the
        // last/first row would repeatedly wipe `ARROW_KEYS_FILTER` right back off,
        // reopening the one-frame gap where egui's own built-in arrow-key focus
        // navigation can step in (see `Harness::press`'s doc comment in the tests
        // below for the full mechanism).
        if let Some(next) = next
            && next != current
        {
            ui.ctx()
                .memory_mut(|mem| mem.request_focus(visible_rows[next].1));
        }
    }

    // `ShortcutAction::ToggleBinderFocus` asking us to take keyboard focus this
    // frame — land on the currently selected document's row if it's visible (not
    // hidden inside a collapsed folder), otherwise fall back to the first row so
    // the shortcut always does *something* useful.
    if focus_requested
        && let Some(&(_, id)) = visible_rows
            .iter()
            .find(|(path, _)| Some(path.as_path()) == selected)
            .or_else(|| visible_rows.first())
    {
        ui.ctx().memory_mut(|mem| mem.request_focus(id));
    }

    event
}

/// Whether a folder row should paint as persistently selected: the root row
/// selects on `project_selected` (the Project form is showing), any other
/// folder on whether `selected_folder` names it — see
/// `BinderEvent::SelectProject`/`SelectFolder`. Its own pure function (rather
/// than inlined at the one call site) so the two-way branch is directly unit
/// testable without driving a full `egui::Ui` frame.
fn folder_row_is_selected(
    is_root: bool,
    project_selected: bool,
    selected_folder: Option<&Path>,
    node_path: &Path,
) -> bool {
    if is_root {
        project_selected
    } else {
        selected_folder == Some(node_path)
    }
}

/// A folder row's background color under whichever `BinderColorMode` is
/// currently active — see `ProjectMeta::binder_color_mode`. Status/Pov each
/// look up the folder's own `folder_meta` value against the matching
/// project-wide color map; WordCountProgress needs `folder_word_counts`
/// (computed off the UI thread — see `app::spawn_word_count_recompute`)
/// rather than anything `Project` alone can answer, since summing a folder's
/// descendant word counts would mean a full subtree disk read on every
/// frame otherwise.
fn folder_row_color(
    project: &Project,
    path: &Path,
    folder_word_counts: &HashMap<PathBuf, usize>,
) -> Option<egui::Color32> {
    let folder_meta = project.folder_meta(path);
    match project.meta.binder_color_mode {
        BinderColorMode::Off => None,
        BinderColorMode::Status => folder_meta
            .status
            .as_deref()
            .and_then(|status| project.status_color_hex(status))
            .and_then(crate::color_theme::parse_hex_color),
        BinderColorMode::Pov => folder_meta
            .pov
            .as_deref()
            .and_then(|pov| project.pov_color_hex(pov))
            .and_then(crate::color_theme::parse_hex_color),
        BinderColorMode::WordCountProgress => {
            let target = folder_meta.word_count_target.filter(|&t| t > 0)?;
            let count = *folder_word_counts.get(path)?;
            Some(crate::color_theme::word_count_progress_color(
                count as f32 / target as f32,
            ))
        }
    }
}

fn role_prefix(role: Option<FolderRole>) -> &'static str {
    match role {
        Some(FolderRole::Research) => "🔍 ",
        Some(FolderRole::Trash) => "🗑 ",
        Some(FolderRole::Templates) => "📋 ",
        Some(FolderRole::Manuscript) => "📖 ",
        None => "",
    }
}

/// A trailing marker (`•`, verified present in the `Ubuntu-Light` fallback
/// every UI font keeps in its chain — see `editor_font::install`'s doc
/// comment) appended to a row's label when it has uncommitted git changes.
/// Kept as a plain text suffix rather than a full-row recolor so it composes
/// with any `BinderColorMode` instead of competing with it — see
/// `folder_row_color`'s doc comment on why only one color mode can be active
/// at a time.
const GIT_DIRTY_MARKER: &str = " •";

/// A folder counts as dirty if *any* path under it (at any depth) has
/// uncommitted changes — cheap in practice since `git_dirty` is normally
/// tiny (only files actually changed since the last commit), so a linear
/// scan per folder row beats maintaining a second precomputed set.
fn folder_is_dirty(git_dirty: &std::collections::HashSet<PathBuf>, folder_path: &Path) -> bool {
    git_dirty.iter().any(|path| path.starts_with(folder_path))
}

/// A document row's label — `document_label`'s extension-stripped name, plus
/// `GIT_DIRTY_MARKER` when `dirty`.
fn document_display_label(name: &str, dirty: bool) -> String {
    let base = document_label(name);
    if dirty {
        format!("{base}{GIT_DIRTY_MARKER}")
    } else {
        base.to_string()
    }
}

/// Paint a row's background: `status_color` (if any — see `ProjectMeta::status_colors`)
/// as an always-on base fill, then the usual hover/focus/selection highlight
/// layered on top when any of those apply. Interaction feedback always wins
/// over the status tint (painted after, not blended) rather than being hidden
/// by it, so a hovered/selected/focused row is never harder to spot than an
/// unpainted one — the status color just stays visible whenever the row isn't
/// being interacted with, which is the common case. Shared by `folder_header`
/// and `document_row`, which need identical layering: this is also the reason
/// `document_row` can't just be `egui::Button::selectable(...).fill(color)` —
/// `Button::fill`'s override is unconditional (its own doc comment: "this
/// will override any on-hover effects"), so it can't layer a base color under
/// a separate highlight the way this does.
fn paint_row_background(
    ui: &egui::Ui,
    response: &egui::Response,
    is_selected: bool,
    status_color: Option<egui::Color32>,
) -> egui::style::WidgetVisuals {
    let visuals = ui.style().interact_selectable(response, is_selected);
    if ui.is_rect_visible(response.rect) {
        if let Some(color) = status_color {
            ui.painter()
                .rect_filled(response.rect, visuals.corner_radius, color);
        }
        if is_selected || response.hovered() || response.has_focus() {
            ui.painter().rect(
                response.rect.expand(visuals.expansion),
                visuals.corner_radius,
                visuals.weak_bg_fill,
                visuals.bg_stroke,
                egui::StrokeKind::Inside,
            );
        }
    }
    visuals
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
    is_selected: bool,
    status_color: Option<egui::Color32>,
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
        // `is_selected` (true for the project root once its metadata is
        // showing in the Metadata dock, or for whichever other folder's own
        // metadata is — see `BinderEvent::SelectProject`/`SelectFolder`)
        // paints the same way regardless of hover/focus, matching a document
        // row's persistent selection highlight. Unlike `CollapsingHeader`
        // (which only paints hover/focus when explicitly made
        // `.selectable(true)`, which we never do), always show it — otherwise
        // there'd be no visual sign of which row the Up/Down keyboard cursor
        // is currently on.
        let visuals = paint_row_background(ui, &header_response, is_selected, status_color);
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

/// A hand-built stand-in for `ui.add(egui::Button::selectable(is_selected,
/// label).sense(Sense::click_and_drag()))`, needed for the same reason
/// `folder_header` is hand-built: `Button::fill` can't layer a status color
/// underneath the usual hover/selection highlight — see
/// `paint_row_background`'s doc comment. Matches `Button::selectable`'s
/// size/padding so switching to this doesn't shift the binder's layout.
fn document_row(
    ui: &mut egui::Ui,
    label: &str,
    is_selected: bool,
    status_color: Option<egui::Color32>,
) -> egui::Response {
    use egui::NumExt as _;

    let button_padding = ui.spacing().button_padding;
    let wrap_width = ui.available_width() - 2.0 * button_padding.x;
    let galley = egui::WidgetText::from(label).into_galley(
        ui,
        Some(egui::TextWrapMode::Extend),
        wrap_width,
        egui::TextStyle::Button,
    );
    let desired_size = egui::vec2(
        ui.available_width(),
        galley.size().y + 2.0 * button_padding.y,
    )
    .at_least(ui.spacing().interact_size);
    let (rect, response) = ui.allocate_exact_size(desired_size, egui::Sense::click_and_drag());

    if ui.is_rect_visible(rect) {
        let visuals = paint_row_background(ui, &response, is_selected, status_color);
        let text_pos = egui::pos2(
            rect.min.x + button_padding.x,
            rect.center().y - galley.size().y / 2.0,
        );
        ui.painter().galley(text_pos, galley, visuals.text_color());
    }

    response
}

#[allow(clippy::too_many_arguments)]
fn show_node(
    ui: &mut egui::Ui,
    project: &Project,
    node: &BinderNode,
    selected: Option<&Path>,
    event: &mut Option<BinderEvent>,
    is_root: bool,
    visible_rows: &mut Vec<(PathBuf, egui::Id)>,
    project_selected: bool,
    selected_folder: Option<&Path>,
    document_row_color: &dyn Fn(&Path) -> Option<egui::Color32>,
    folder_word_counts: &HashMap<PathBuf, usize>,
    git_dirty: &std::collections::HashSet<PathBuf>,
) {
    match &node.kind {
        BinderNodeKind::Folder { children } => {
            let role = project.folder_role(&node.path);
            let dirty_marker = if folder_is_dirty(git_dirty, &node.path) {
                GIT_DIRTY_MARKER
            } else {
                ""
            };
            let label = format!("{}{}{}", role_prefix(role), node.name, dirty_marker);
            let id = ui.make_persistent_id(&node.path);
            let is_selected =
                folder_row_is_selected(is_root, project_selected, selected_folder, &node.path);
            let status_color = folder_row_color(project, &node.path, folder_word_counts);
            let (header_response, mut state) =
                folder_header(ui, id, &label, true, is_selected, status_color);
            visible_rows.push((node.path.clone(), header_response.id));

            if header_response.clicked() {
                *event = Some(if is_root {
                    BinderEvent::SelectProject
                } else {
                    BinderEvent::SelectFolder(node.path.clone())
                });
                state.toggle(ui);
                // `request_focus` unconditionally resets the target's focus-lock
                // filter (see the comment on the equivalent guard in `show`) — skip
                // it when this row is already focused, so re-clicking an
                // already-focused folder to toggle it doesn't wipe the filter and
                // reopen the one-frame gap for its very next keypress.
                if !header_response.has_focus() {
                    header_response.request_focus();
                }
            }
            if header_response.has_focus() {
                ui.ctx().memory_mut(|mem| {
                    mem.set_focus_lock_filter(header_response.id, ARROW_KEYS_FILTER)
                });
                // Left/Right expand/collapse. Up/Down are handled once, after the
                // whole tree has been rendered, by `show` — see its doc comment.
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
                ui.separator();
                if ui.button("Export…").clicked() {
                    *event = Some(BinderEvent::Export {
                        path: node.path.clone(),
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
                        if ui
                            .radio(role == Some(FolderRole::Manuscript), "Manuscript")
                            .clicked()
                        {
                            *event = Some(BinderEvent::SetFolderRole {
                                path: node.path.clone(),
                                role: Some(FolderRole::Manuscript),
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
                    // Independent of "Folder Role" above: a folder can be a picklist
                    // source for any combination of Type/POV/Status regardless of
                    // whatever role (or none) it also holds — e.g. a Research folder
                    // of character bios can double as the POV source without
                    // becoming exempt from export the way Trash/Templates are.
                    ui.menu_button("Dropdown Source", |ui| {
                        for (field, label) in [
                            (PicklistField::Type, "Type"),
                            (PicklistField::Pov, "POV"),
                            (PicklistField::Status, "Status"),
                        ] {
                            let mut checked = project.is_picklist_folder(field, &node.path);
                            if ui.checkbox(&mut checked, label).changed() {
                                *event = Some(BinderEvent::SetPicklistFolder {
                                    field,
                                    path: checked.then(|| node.path.clone()),
                                });
                                ui.close();
                            }
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
                    show_node(
                        ui,
                        project,
                        child,
                        selected,
                        event,
                        false,
                        visible_rows,
                        project_selected,
                        selected_folder,
                        document_row_color,
                        folder_word_counts,
                        git_dirty,
                    );
                }
            });
        }
        BinderNodeKind::Document => {
            let is_selected = selected == Some(node.path.as_path());
            let status_color = document_row_color(&node.path);
            let label = document_display_label(&node.name, git_dirty.contains(&node.path));
            let response = document_row(ui, &label, is_selected, status_color);
            visible_rows.push((node.path.clone(), response.id));
            if response.clicked() {
                *event = Some(BinderEvent::Selected(node.path.clone()));
                // Unlike `folder_header`, `Button` doesn't request focus on click by
                // itself — without this, clicking a document (the most common first
                // interaction with the binder) would leave nothing focused, and
                // arrow keys would have no row to act on at all. This also makes
                // Enter/Space open the focused document "for free": egui already
                // treats Space/Enter on a focused click-sensing widget as a click
                // (`Response::clicked`'s doc comment), so no extra handling is
                // needed for that once focus itself works. Guarded on `has_focus`
                // for the same reason `folder_header`'s click handler is: repeatedly
                // clicking an already-focused row would otherwise keep resetting
                // its focus-lock filter (`request_focus` always does that, even
                // when the target is already focused), reopening the one-frame gap
                // for its very next keypress every time.
                if !response.has_focus() {
                    response.request_focus();
                }
            }
            // Claimed so Left/Right don't trigger egui's built-in spatial
            // focus-jump while a document row has focus — see `ARROW_KEYS_FILTER`.
            // Documents have no children to expand/collapse, so Left/Right are
            // otherwise inert here; only Up/Down (handled once by `show`) do
            // anything.
            if response.has_focus() {
                ui.ctx()
                    .memory_mut(|mem| mem.set_focus_lock_filter(response.id, ARROW_KEYS_FILTER));
            }
            // Drag source: see the matching folder-header handling above for the
            // other draggable case.
            response.dnd_set_drag_payload(node.path.clone());
            // Drop target: unlike a folder header (which always appends to the
            // end), dropping directly onto a document row reorders — the dragged
            // item lands immediately before this one, whether it's a sibling
            // (a pure reorder) or from a different folder (a move, positioned
            // rather than appended) — see `Project::move_item_before`.
            if let Some(dragged_path) = response.dnd_release_payload::<PathBuf>() {
                *event = Some(BinderEvent::MoveItemBefore {
                    path: (*dragged_path).clone(),
                    before: node.path.clone(),
                });
            }
            if response.dnd_hover_payload::<PathBuf>().is_some() {
                ui.painter().rect_stroke(
                    response.rect,
                    2.0,
                    egui::Stroke::new(2.0, ui.visuals().selection.stroke.color),
                    egui::StrokeKind::Inside,
                );
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_display_label_appends_the_marker_only_when_dirty() {
        assert_eq!(document_display_label("01-opening.md", false), "01-opening");
        assert_eq!(
            document_display_label("01-opening.md", true),
            format!("01-opening{GIT_DIRTY_MARKER}")
        );
    }

    #[test]
    fn folder_is_dirty_true_for_a_direct_or_nested_dirty_file_false_otherwise() {
        let folder = Path::new("/project/Chapter 1");
        let mut dirty = std::collections::HashSet::new();
        assert!(!folder_is_dirty(&dirty, folder));

        dirty.insert(Path::new("/project/Chapter 1/Scene 1.md").to_path_buf());
        assert!(folder_is_dirty(&dirty, folder));

        dirty.clear();
        dirty.insert(Path::new("/project/Chapter 1/Sub/Scene 2.md").to_path_buf());
        assert!(folder_is_dirty(&dirty, folder));

        dirty.clear();
        dirty.insert(Path::new("/project/Other Chapter/Scene 1.md").to_path_buf());
        assert!(!folder_is_dirty(&dirty, folder));
    }

    #[test]
    fn folder_row_is_selected_for_root_follows_project_selected_only() {
        let unrelated = Path::new("/project/Chapter 1");
        assert!(folder_row_is_selected(true, true, None, unrelated));
        assert!(folder_row_is_selected(
            true,
            true,
            Some(Path::new("/project/Other")),
            unrelated
        ));
        assert!(!folder_row_is_selected(true, false, None, unrelated));
    }

    #[test]
    fn folder_row_is_selected_for_a_non_root_folder_follows_selected_folder_only() {
        let chapter = Path::new("/project/Chapter 1");
        let other = Path::new("/project/Other");
        assert!(folder_row_is_selected(false, true, Some(chapter), chapter));
        assert!(!folder_row_is_selected(false, true, Some(other), chapter));
        assert!(!folder_row_is_selected(false, true, None, chapter));
        // `project_selected` is irrelevant for a non-root row.
        assert!(folder_row_is_selected(false, false, Some(chapter), chapter));
    }

    #[test]
    fn folder_row_color_reads_status_or_pov_or_word_count_progress_depending_on_mode() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = crate::project::Project::initialize(dir.path()).unwrap();
        let chapter = project.create_folder(dir.path(), "Chapter 1").unwrap();
        project
            .set_folder_meta(
                &chapter,
                crate::frontmatter::DocumentMeta {
                    status: Some("draft".to_string()),
                    pov: Some("Alice".to_string()),
                    word_count_target: Some(100),
                    ..Default::default()
                },
            )
            .unwrap();
        project
            .set_status_color_hex("draft", "#ff0000".to_string())
            .unwrap();
        project
            .set_pov_color_hex("Alice", "#00ff00".to_string())
            .unwrap();
        let mut folder_word_counts = HashMap::new();
        folder_word_counts.insert(chapter.clone(), 50);

        project
            .set_binder_color_mode(crate::project::BinderColorMode::Status)
            .unwrap();
        assert_eq!(
            folder_row_color(&project, &chapter, &folder_word_counts),
            Some(egui::Color32::from_rgb(0xff, 0x00, 0x00))
        );

        project
            .set_binder_color_mode(crate::project::BinderColorMode::Pov)
            .unwrap();
        assert_eq!(
            folder_row_color(&project, &chapter, &folder_word_counts),
            Some(egui::Color32::from_rgb(0x00, 0xff, 0x00))
        );

        project
            .set_binder_color_mode(crate::project::BinderColorMode::WordCountProgress)
            .unwrap();
        assert_eq!(
            folder_row_color(&project, &chapter, &folder_word_counts),
            Some(crate::color_theme::word_count_progress_color(0.5))
        );

        project
            .set_binder_color_mode(crate::project::BinderColorMode::Off)
            .unwrap();
        assert_eq!(
            folder_row_color(&project, &chapter, &folder_word_counts),
            None
        );
    }

    #[test]
    fn folder_row_color_is_none_for_word_count_progress_without_a_target_or_a_cached_total() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = crate::project::Project::initialize(dir.path()).unwrap();
        let chapter = project.create_folder(dir.path(), "Chapter 1").unwrap();
        project
            .set_binder_color_mode(crate::project::BinderColorMode::WordCountProgress)
            .unwrap();

        // No word_count_target set on the folder at all.
        assert_eq!(folder_row_color(&project, &chapter, &HashMap::new()), None);

        // A target is set, but `folder_word_counts` has no entry for this path
        // (e.g. the background recompute hasn't completed yet).
        project
            .set_folder_meta(
                &chapter,
                crate::frontmatter::DocumentMeta {
                    word_count_target: Some(100),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(folder_row_color(&project, &chapter, &HashMap::new()), None);
    }

    /// Drives `show` with synthetic input across frames, so keyboard-navigation
    /// behavior can be checked without a running window — worth the extra machinery
    /// specifically because this exact class of bug (a click that silently fails to
    /// grant keyboard focus, breaking every arrow-key interaction downstream of it)
    /// already shipped once undetected by build/clippy/fmt and a crash-only manual
    /// smoke test.
    #[derive(Default)]
    struct Harness {
        ctx: egui::Context,
    }

    impl Harness {
        fn frame(&self, project: &Project, events: Vec<egui::Event>) -> Option<BinderEvent> {
            self.frame_with(project, None, false, events)
        }

        /// Like `frame`, but exposes `selected`/`focus_requested` — the two `show`
        /// parameters `frame` otherwise hardcodes to `None`/`false` — for testing
        /// `ShortcutAction::ToggleBinderFocus`'s "land on the selected row, or the
        /// first row if nothing's selected" behavior.
        fn frame_with(
            &self,
            project: &Project,
            selected: Option<&Path>,
            focus_requested: bool,
            events: Vec<egui::Event>,
        ) -> Option<BinderEvent> {
            let input = egui::RawInput {
                events,
                ..Default::default()
            };
            let mut event = None;
            let _ = self.ctx.run_ui(input, |ui| {
                event = show(
                    ui,
                    project,
                    selected,
                    focus_requested,
                    false,
                    None,
                    &|_| None,
                    &HashMap::new(),
                    &std::collections::HashSet::new(),
                );
            });
            event
        }

        /// Zero animation time, so a folder's open/closed `openness` snaps
        /// immediately instead of taking many real frames to tween — otherwise a
        /// freshly-collapsed folder's children would still be "visible" (mid-fade)
        /// for a while and this test would need to wait out that animation, rather
        /// than being able to assert on the state right after the keypress that
        /// caused it. Then a couple of empty frames so any other first-render
        /// transients settle before rendered rects/order are relied on to stay put.
        fn settle(&self, project: &Project) {
            self.ctx.all_styles_mut(|style| style.animation_time = 0.0);
            self.frame(project, vec![]);
            self.frame(project, vec![]);
        }

        fn click(&self, project: &Project, pos: egui::Pos2) -> Option<BinderEvent> {
            self.frame(
                project,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    },
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            )
        }

        /// One frame with no input at all — see `press`'s doc comment for why tests
        /// need this between a focus change and the next key press.
        fn idle(&self, project: &Project) {
            self.frame(project, vec![]);
        }

        /// Press `key` and return the resulting event. Always call `idle` first if
        /// the previously-focused row *just* gained focus this same test step (e.g.
        /// right after `click_first_row`, or right after a previous `press` moved
        /// focus to a new row): egui's own focus-lock filter — the mechanism
        /// `set_focus_lock_filter`/`ARROW_KEYS_FILTER` uses to stop it from treating
        /// arrow keys as its own built-in "jump focus to the nearest widget"
        /// shortcut — only takes effect starting the *second* frame a widget has
        /// focus (see `Memory::set_focus_lock_filter`'s doc comment: "You must first
        /// give focus to the widget before calling this"). A key pressed on the very
        /// same frame focus lands somewhere new can therefore still be treated as a
        /// focus-direction request by egui itself, racing our own handling. This
        /// isn't reachable from an actual keyboard — a real click and a real
        /// subsequent keypress are always several frames apart — and it's the exact
        /// same one-frame gap `TextEdit` itself has, but our synthetic frames need
        /// an explicit `idle` to reproduce that natural gap.
        fn press(&self, project: &Project, key: egui::Key) -> Option<BinderEvent> {
            self.frame(
                project,
                vec![egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            )
        }

        fn focused(&self) -> Option<egui::Id> {
            self.ctx.memory(|mem| mem.focused())
        }

        /// Click down the left column, row by row, until keyboard focus actually
        /// changes — reaches the first row without hand-computing pixel offsets,
        /// which shift depending on how many rows are above it. The first row is
        /// always the project root's own folder header, so this doubles as the
        /// "click a folder to focus it" case: a second click at the same spot
        /// immediately undoes the open/closed toggle that click also causes,
        /// since tests using this just want focus, not a collapsed root hiding
        /// everything under it.
        fn click_first_row(&self, project: &Project) -> egui::Id {
            let before = self.focused();
            for y in (8..2000).step_by(4) {
                let pos = egui::pos2(20.0, y as f32);
                self.click(project, pos);
                if let Some(after) = self.focused()
                    && Some(after) != before
                {
                    self.click(project, pos);
                    return after;
                }
            }
            panic!("clicking down the column never changed keyboard focus");
        }

        /// Click down the left column until `target` is the one that gets selected
        /// — unlike `click_first_row`, this specifically reaches a *document* row
        /// (not whichever folder header happens to render above it), since that's
        /// the exact row kind whose click handler was missing `request_focus`.
        fn click_document(&self, project: &Project, target: &Path) {
            for y in (8..2000).step_by(4) {
                if let Some(BinderEvent::Selected(path)) =
                    self.click(project, egui::pos2(20.0, y as f32))
                    && path == target
                {
                    return;
                }
            }
            panic!(
                "clicking down the column never selected {}",
                target.display()
            );
        }
    }

    #[test]
    fn clicking_a_document_row_grants_it_keyboard_focus() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = crate::project::Project::initialize(dir.path()).unwrap();
        let doc = project.create_document(dir.path(), "Doc A").unwrap();

        let harness = Harness::default();
        harness.settle(&project);
        assert_eq!(harness.focused(), None);

        harness.click_document(&project, &doc);
        assert!(
            harness.focused().is_some(),
            "clicking a document row should grant it keyboard focus, the same way \
             clicking a folder header already does"
        );
    }

    #[test]
    fn clicking_a_row_grants_it_keyboard_focus() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = crate::project::Project::initialize(dir.path()).unwrap();
        project.create_document(dir.path(), "Doc A").unwrap();

        let harness = Harness::default();
        harness.settle(&project);
        assert_eq!(harness.focused(), None);

        harness.click_first_row(&project);
        assert!(harness.focused().is_some());
    }

    #[test]
    fn arrow_down_moves_focus_to_the_next_row_and_arrow_up_moves_back() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = crate::project::Project::initialize(dir.path()).unwrap();
        project.create_document(dir.path(), "Doc A").unwrap();
        project.create_document(dir.path(), "Doc B").unwrap();

        let harness = Harness::default();
        harness.settle(&project);
        let first = harness.click_first_row(&project);
        harness.idle(&project);

        harness.press(&project, egui::Key::ArrowDown);
        let second = harness.focused().unwrap();
        assert_ne!(
            first, second,
            "ArrowDown should move focus to a different row"
        );
        harness.idle(&project);

        harness.press(&project, egui::Key::ArrowUp);
        assert_eq!(
            harness.focused(),
            Some(first),
            "ArrowUp should move focus back to the previous row"
        );
    }

    #[test]
    fn arrow_down_does_not_move_past_the_last_row() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = crate::project::Project::initialize(dir.path()).unwrap();
        project.create_document(dir.path(), "Doc A").unwrap();

        let harness = Harness::default();
        harness.settle(&project);
        harness.click_first_row(&project);

        // root + Doc A = 2 rows; pressing down far more than that should just stay
        // on the last row instead of panicking (an out-of-bounds index) or wrapping.
        for _ in 0..10 {
            harness.idle(&project);
            harness.press(&project, egui::Key::ArrowDown);
        }
        assert!(harness.focused().is_some());
    }

    #[test]
    fn focus_requested_lands_on_the_selected_documents_row() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = crate::project::Project::initialize(dir.path()).unwrap();
        project.create_document(dir.path(), "Doc A").unwrap();
        let doc_b = project.create_document(dir.path(), "Doc B").unwrap();

        // A separate harness (fresh `egui::Context`) clicks Doc B directly, so its
        // row id can be captured for comparison — widget ids are derived purely
        // from the (deterministic) tree structure, not from anything specific to
        // one `egui::Context`, so this id is exactly what a focus request in a
        // different harness should land on too.
        let reference = Harness::default();
        reference.settle(&project);
        reference.click_document(&project, &doc_b);
        let expected = reference.focused().unwrap();

        let harness = Harness::default();
        harness.settle(&project);
        assert_eq!(harness.focused(), None);
        harness.frame_with(&project, Some(&doc_b), true, vec![]);

        assert_eq!(
            harness.focused(),
            Some(expected),
            "ShortcutAction::ToggleBinderFocus should focus the currently selected \
             document's row"
        );
    }

    #[test]
    fn focus_requested_falls_back_to_the_first_row_when_nothing_is_selected() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = crate::project::Project::initialize(dir.path()).unwrap();
        project.create_document(dir.path(), "Doc A").unwrap();

        let reference = Harness::default();
        reference.settle(&project);
        let expected = reference.click_first_row(&project);

        let harness = Harness::default();
        harness.settle(&project);
        harness.frame_with(&project, None, true, vec![]);

        assert_eq!(
            harness.focused(),
            Some(expected),
            "with nothing selected, ToggleBinderFocus should fall back to the first row"
        );
    }

    #[test]
    fn enter_opens_the_focused_document() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = crate::project::Project::initialize(dir.path()).unwrap();
        let doc = project.create_document(dir.path(), "Doc A").unwrap();

        let harness = Harness::default();
        harness.settle(&project);
        harness.click_first_row(&project); // focuses the root header
        harness.idle(&project);
        harness.press(&project, egui::Key::ArrowDown); // -> Doc A
        harness.idle(&project);

        let event = harness.press(&project, egui::Key::Enter);
        assert_eq!(event, Some(BinderEvent::Selected(doc)));
    }

    #[test]
    fn clicking_the_root_row_raises_select_project() {
        let dir = tempfile::tempdir().unwrap();
        let project = crate::project::Project::initialize(dir.path()).unwrap();

        let harness = Harness::default();
        harness.settle(&project);

        // The root row is always the very first row (see `click_first_row`'s doc
        // comment) — clicking it should raise `SelectProject` on top of its
        // existing expand/collapse toggle, the same way clicking a document row
        // raises `Selected` on top of granting it focus.
        let event = harness.click(&project, egui::pos2(20.0, 8.0));
        assert_eq!(event, Some(BinderEvent::SelectProject));
    }

    #[test]
    fn activating_a_non_root_folder_row_raises_select_folder_not_select_project() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = crate::project::Project::initialize(dir.path()).unwrap();
        let chapter = project.create_folder(dir.path(), "Chapter 1").unwrap();

        let harness = Harness::default();
        harness.settle(&project);
        harness.click_first_row(&project); // focuses the root header
        harness.idle(&project);
        harness.press(&project, egui::Key::ArrowDown); // -> Chapter 1
        harness.idle(&project);

        // Activating (Enter) a non-root folder toggles expand/collapse (same as
        // before) and now also raises `SelectFolder` — never `SelectProject`,
        // which is reserved for the root row.
        let event = harness.press(&project, egui::Key::Enter);
        assert_eq!(event, Some(BinderEvent::SelectFolder(chapter)));
    }

    #[test]
    fn left_and_right_arrows_collapse_and_expand_a_focused_folder() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = crate::project::Project::initialize(dir.path()).unwrap();
        let chapter = project.create_folder(dir.path(), "Chapter 1").unwrap();
        project.create_document(&chapter, "Scene 1").unwrap();

        let harness = Harness::default();
        harness.settle(&project);

        harness.click_first_row(&project); // focuses the root header
        harness.idle(&project);
        harness.press(&project, egui::Key::ArrowDown); // -> Chapter 1 (open by default)
        let chapter_focus = harness.focused().unwrap();
        harness.idle(&project);

        // Sanity check: with Chapter 1 open, ArrowDown should reach "Scene 1" — a
        // third, distinct row.
        harness.press(&project, egui::Key::ArrowDown);
        let scene_focus = harness.focused().unwrap();
        assert_ne!(chapter_focus, scene_focus);
        harness.idle(&project);

        // Refocus Chapter 1, collapse it, and confirm ArrowDown no longer descends
        // into it — there's nothing else below it, so focus should just stay put.
        harness.press(&project, egui::Key::ArrowUp);
        assert_eq!(harness.focused(), Some(chapter_focus));
        harness.idle(&project);
        harness.press(&project, egui::Key::ArrowLeft);
        harness.idle(&project);
        harness.press(&project, egui::Key::ArrowDown);
        assert_eq!(
            harness.focused(),
            Some(chapter_focus),
            "a collapsed folder has no visible children to move down into"
        );

        // Expand it again and confirm the child is reachable once more.
        harness.idle(&project);
        harness.press(&project, egui::Key::ArrowRight);
        harness.idle(&project);
        harness.press(&project, egui::Key::ArrowDown);
        assert_eq!(
            harness.focused(),
            Some(scene_focus),
            "expanding the folder again should make its child reachable"
        );
    }
}
