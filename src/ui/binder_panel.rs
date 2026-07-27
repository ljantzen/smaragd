use std::path::{Path, PathBuf};

use crate::project::model::{BinderNode, BinderNodeKind};
use crate::project::{FolderRole, Project};

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

pub fn show(ui: &mut egui::Ui, project: &Project, selected: Option<&Path>) -> Option<BinderEvent> {
    let mut event = None;
    let mut visible_rows = Vec::new();
    show_node(
        ui,
        project,
        &project.tree.root,
        selected,
        &mut event,
        true,
        &mut visible_rows,
    );

    // Up/Down move the keyboard cursor between rows, in the same top-to-bottom order
    // they were just rendered in — which already skips the children of any collapsed
    // folder, since `show_node` never recurses into those in the first place. Only
    // acts when a binder row actually has focus, so this can't steal Up/Down from,
    // say, the main editor's `TextEdit` while the user is typing there.
    if let Some(focused_id) = ui.ctx().memory(|mem| mem.focused())
        && let Some(current) = visible_rows.iter().position(|id| *id == focused_id)
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
                .memory_mut(|mem| mem.request_focus(visible_rows[next]));
        }
    }

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
        // Unlike `CollapsingHeader` (which only paints this when explicitly made
        // `.selectable(true)`, which we never do), always show it on hover/focus —
        // otherwise there'd be no visual sign of which row the Up/Down keyboard
        // cursor is currently on.
        if header_response.hovered() || header_response.has_focus() {
            ui.painter().rect(
                header_response.rect.expand(visuals.expansion),
                visuals.corner_radius,
                visuals.weak_bg_fill,
                visuals.bg_stroke,
                egui::StrokeKind::Inside,
            );
        }
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
    visible_rows: &mut Vec<egui::Id>,
) {
    match &node.kind {
        BinderNodeKind::Folder { children } => {
            let role = project.folder_role(&node.path);
            let label = format!("{}{}", node.name, role_suffix(role));
            let id = ui.make_persistent_id(&node.path);
            let (header_response, mut state) = folder_header(ui, id, &label, true);
            visible_rows.push(header_response.id);

            if header_response.clicked() {
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
                    show_node(ui, project, child, selected, event, false, visible_rows);
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
            visible_rows.push(response.id);
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
            let input = egui::RawInput {
                events,
                ..Default::default()
            };
            let mut event = None;
            let _ = self.ctx.run_ui(input, |ui| {
                event = show(ui, project, None);
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
