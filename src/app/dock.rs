use super::*;

/// A dockable tab in `dock_state`. Binder/Backlinks/Metadata used to be
/// (respectively) a fixed left panel or a blocking modal; Editor/Preview/
/// Corkboard used to be the three mutually-exclusive `ViewMode`s of a separate
/// `CentralPanel`, entirely outside the dock. All six now live in one shared
/// `egui_dock::DockState`, so any of them can be freely dragged, split, and
/// resized against any other — see `AppTabViewer` and the single
/// `DockArea::show_inside` call in `eframe::App::ui`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum DockTab {
    Binder,
    Backlinks,
    Tags,
    Metadata,
    Editor,
    Preview,
    Corkboard,
    StoryGrid,
    Pomodoro,
    WordCount,
    Collab,
    Streak,
}

/// The initial dock layout for a fresh install (no persisted `dock_layout.json`
/// yet), and the "Restore Default Layout" Window-menu action: a narrow Binder
/// column on the left, Editor filling the middle, and a Metadata/Backlinks
/// column on the right — enough for a first-time user to see what the app can
/// do without opening any menus, while staying short of the full tab roster
/// (Tags/Pomodoro/WordCount/Collab/Streak all still start closed, reachable
/// from the Window menu).
pub(super) fn default_dock_state() -> egui_dock::DockState<DockTab> {
    let mut state = egui_dock::DockState::new(vec![DockTab::Editor]);
    // `fraction` is always the *first* (left/top) child's share, regardless of
    // which side gets the newly-split-off node — confirmed empirically.
    // `split_left` puts the *new* node on the left (child 0), so 0.22 there
    // gives Binder a narrow column and leaves Editor the majority. `split_right`
    // instead puts the *old* node on the left (child 0) and the new one on the
    // right, so the fraction has to be inverted: 0.75 keeps Editor's share and
    // leaves Metadata the narrow remainder — passing the same 0.25 as above
    // would have swapped their proportions (Metadata mistakenly getting the
    // majority) rather than mirroring it.
    let [editor, _binder] = state
        .main_surface_mut()
        .split_left(egui_dock::NodeIndex::root(), 0.22, vec![DockTab::Binder]);
    let [_editor, metadata] = state
        .main_surface_mut()
        .split_right(editor, 0.75, vec![DockTab::Metadata]);
    state
        .main_surface_mut()
        .split_below(metadata, 0.5, vec![DockTab::Backlinks]);
    state
}

/// Guards against a layout that deserializes fine but has no `Editor` tab
/// anywhere — e.g. one persisted by a build from before the editor became a
/// dock tab, or a hand-edited file — which would otherwise leave no way to
/// edit any document at all. A no-op if `Editor` is already present.
pub(super) fn ensure_editor_tab_present(state: &mut egui_dock::DockState<DockTab>) {
    if state
        .iter_all_tabs()
        .all(|(_, tab)| *tab != DockTab::Editor)
    {
        state.push_to_focused_leaf(DockTab::Editor);
    }
}

/// The `egui::Id` `egui_dock` renders a floating surface's `egui::Window` under —
/// duplicated here (rather than exposed by the crate) because nothing public
/// exposes it; see `capture_floating_window_positions`'s doc comment for why this
/// is needed at all. Matches `show_window_surface`'s own `id` exactly (egui_dock
/// 0.20.1, `src/widgets/dock_area/show/window_surface.rs`): `format!("window
/// {surf_index:?}").into()`, i.e. `Id::new` of that same formatted string.
pub(super) fn floating_window_id(surface: egui_dock::SurfaceIndex) -> egui::Id {
    egui::Id::new(format!("window {surface:?}"))
}

/// `DockState`'s tree structure (tabs, splits, which surface each lives on)
/// round-trips through our JSON persistence just fine, but a floating surface's
/// on-screen *position* isn't actually part of that tree at all: `WindowState`
/// (the part of `DockState` that records it) only ever gets its `next_position`/
/// `next_size` populated once, right when a tab is first dragged out live (see
/// `DockState::detach_tab`) — `egui_dock` 0.20.1 never writes back to those
/// fields afterward (nor to `WindowState`'s `screen_rect`, which is otherwise
/// dead code in this version). The window's actual current position instead
/// lives only in egui's own per-session `Memory` (keyed by `floating_window_id`),
/// which starts out empty on every fresh launch — so a restored floating panel
/// would otherwise land wherever egui's built-in cascade default is, rather than
/// where it was left (exactly the bug reported: tabs reopened correctly, but
/// stacked at the top-left instead of docked to the right edge).
///
/// Called right before persisting (see `persist_dock_layout`), with the live
/// `ctx` still available: reads each floating surface's actual current rect out
/// of egui's `Memory` and writes it into that surface's `next_position`/
/// `next_size` — fields that *do* round-trip through our JSON serialization, and
/// that `WindowState::create_window` picks up automatically (exactly as it would
/// for a freshly-detached tab) the very first time this restored layout is shown.
pub(super) fn capture_floating_window_positions<Tab>(
    state: &mut egui_dock::DockState<Tab>,
    ctx: &egui::Context,
) {
    let floating_surfaces: Vec<egui_dock::SurfaceIndex> = state
        .iter_surfaces_indexed()
        .filter(|(_, surface)| matches!(surface, egui_dock::Surface::Window(..)))
        .map(|(index, _)| index)
        .collect();
    for index in floating_surfaces {
        let Some(rect) = ctx.memory(|mem| mem.area_rect(floating_window_id(index))) else {
            continue;
        };
        if !rect.is_finite() {
            continue;
        }
        if let Some(window_state) = state.get_window_state_mut(index) {
            window_state.set_position(rect.min);
            window_state.set_size(rect.size());
        }
    }
}
