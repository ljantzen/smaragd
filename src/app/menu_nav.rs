/// Shows `label` as a menu-bar button, with `shortcut`'s formatted text (if any)
/// dimmed on the right, matching `egui::Button::shortcut_text`'s intended use.
fn menu_button_with_shortcut(
    ui: &mut egui::Ui,
    label: &str,
    shortcut: Option<egui::KeyboardShortcut>,
) -> egui::Response {
    let mut button = egui::Button::new(label);
    if let Some(shortcut) = shortcut {
        button = button.shortcut_text(ui.ctx().format_shortcut(&shortcut));
    }
    ui.add(button)
}

/// Accumulates the ordered list of keyboard-focusable item `Id`s a top-level
/// dropdown's content renders this frame, in visual order — the list Up/Down
/// navigates over (see `handle_dropdown_arrows`). Rebuilt fresh every frame
/// (immediate mode), mirroring `binder_panel.rs`'s own `visible_rows`
/// accumulator for its file tree. Disabled rows are simply never pushed
/// (checked via `ui.is_enabled()`), so a temporarily-disabled group (e.g.
/// Versions' git actions while a git operation is running, via
/// `add_enabled_ui`) is transparently skipped without touching that call site;
/// `ui.separator()` calls are never routed through here at all, so they're
/// excluded the same way.
#[derive(Default)]
pub(super) struct MenuNav {
    items: Vec<egui::Id>,
}

impl MenuNav {
    pub(super) fn track(&mut self, ui: &egui::Ui, response: &egui::Response) {
        if ui.is_enabled() {
            self.items.push(response.id);
        }
    }

    pub(super) fn button(&mut self, ui: &mut egui::Ui, label: &str) -> egui::Response {
        let response = ui.button(label);
        self.track(ui, &response);
        response
    }

    pub(super) fn shortcut_button(
        &mut self,
        ui: &mut egui::Ui,
        label: &str,
        shortcut: Option<egui::KeyboardShortcut>,
    ) -> egui::Response {
        let response = menu_button_with_shortcut(ui, label, shortcut);
        self.track(ui, &response);
        response
    }

    /// Like `button`, for a tracked radio item inside a submenu (Theme/Color
    /// Binder By's items are `ui.radio`, not `ui.button`).
    pub(super) fn radio(
        &mut self,
        ui: &mut egui::Ui,
        selected: bool,
        label: &str,
    ) -> egui::Response {
        let response = ui.radio(selected, label);
        self.track(ui, &response);
        response
    }
}

/// The 8 top-level menus, in menu-bar order — also the fixed cycle Left/Right
/// switch through in `handle_dropdown_arrows` (wrapping: Help's Right goes to
/// File, File's Left goes to Help).
const TOP_MENUS: [(&str, egui::Key); 8] = [
    ("File", egui::Key::F),
    ("Edit", egui::Key::E),
    ("View", egui::Key::V),
    ("Tools", egui::Key::T),
    ("Versions", egui::Key::S),
    ("Collaborate", egui::Key::C),
    ("Window", egui::Key::W),
    ("Help", egui::Key::H),
];

/// A stable popup `Id` derived purely from a top-level menu's label, rather than
/// `Popup::menu`'s own default (`response.id.with("popup")`, see
/// `Popup::default_response_id`) — needed so `handle_dropdown_arrows` can address
/// a *sibling* menu's popup by label alone on a Left/Right press, without having
/// that sibling's `Response` (it may not have rendered yet this frame).
fn top_menu_popup_id(label: &str) -> egui::Id {
    egui::Id::new("top_menu_popup").with(label)
}

/// A top-level menu-bar button (File/Edit/View/...) that also drops down when
/// `mnemonic` is pressed with Alt held, in addition to the normal click-to-toggle
/// behavior — a fixed Alt+letter menu-bar accelerator, matching the classic
/// Windows/GTK convention. Deliberately *not* part of the user-configurable
/// `ShortcutAction`/`shortcuts.rs` system: these are positional (whichever menu
/// happens to be first gets Alt+F, etc.) and never meant to be rebound.
///
/// Reimplements `egui::containers::menu::MenuButton::ui` rather than calling it,
/// since that helper always ties the dropdown's open state to the button's own
/// click (`Popup::menu`'s built-in toggle) with no hook to force it open from an
/// unrelated keypress or to switch it from a sibling menu (see
/// `handle_dropdown_arrows`'s Left/Right handling). Open/close is driven
/// explicitly via `Popup::open_id`/`toggle_id` against a stable, label-derived
/// popup id (`top_menu_popup_id`) instead — `Popup::open_id`'s own doc comment
/// ("Open the given popup and close all others") is exactly the mutual-exclusion
/// this app wants across the 7 top-level menus, and is safe to rely on here
/// specifically because this simple Memory-backed popup mechanism is never used
/// for the nested submenus (Theme/Layouts/multi-folder Export Manuscript), which
/// need independent, simultaneously-open parent+child state instead. Safe to
/// skip `MenuConfig::find`/`MenuBar::config`, which `MenuButton::ui` normally
/// consults, since nothing in this app ever calls `MenuBar::config`/
/// `MenuButton::config` to override the ambient default.
pub(super) fn top_menu_button(
    ui: &mut egui::Ui,
    label: &str,
    mnemonic: egui::Key,
    content: impl FnOnce(&mut egui::Ui, &mut MenuNav),
) {
    let popup_id = top_menu_popup_id(label);
    if ui.input_mut(|i| i.consume_key(egui::Modifiers::ALT, mnemonic)) {
        egui::Popup::open_id(ui.ctx(), popup_id);
    }
    let response = ui.button(label);
    if response.clicked() {
        egui::Popup::toggle_id(ui.ctx(), popup_id);
    }
    let mut nav = MenuNav::default();
    egui::Popup::menu(&response)
        .id(popup_id)
        .open_memory(None)
        .show(|ui| content(ui, &mut nav));
    handle_dropdown_arrows(ui.ctx(), &nav, popup_id, label);
}

/// Up/Down/Left/Right handling for whichever top-level dropdown is currently
/// open, called once per dropdown right after its content has rendered (so
/// `nav.items` is complete for the frame) — mirrors `binder_panel.rs`'s own
/// "handle Up/Down once, after the whole list is built" structure.
///
/// Up/Down move the highlighted item within `nav.items`, wrapping at the ends.
/// Left/Right switch to the previous/next of `TOP_MENUS`, wrapping there too.
/// Opening with nothing yet focused inside (however it was opened — click, Alt
/// mnemonic, or a Left/Right switch from a sibling) lands on the first item, so
/// Down always has a defined starting point.
///
/// That auto-focus only fires the first time `nav.items` is non-empty after the
/// popup opens — tracked via a small per-popup flag in `ctx.data`, cleared
/// whenever the popup isn't open — rather than on every later frame the
/// dropdown merely happens to still be nominally open with nothing of its own
/// focused. That distinction matters because no menu item anywhere in this file
/// calls `ui.close()` after acting (clicking "Open Document…", say, doesn't
/// close the File dropdown it lives in) — harmless before this function
/// existed, since the dropdown just sat there open and inert, but actively
/// disruptive once this function started auto-focusing: without this guard, a
/// still-technically-open File menu would re-steal focus from whatever dialog
/// "Open Document…" just opened, every single frame, before that dialog's own
/// text field could ever process so much as an Enter press. (A simpler-looking
/// "was the popup already open last frame" check, via `Context::read_response`,
/// doesn't work here: egui registers that a frame *before* `nav.items` actually
/// becomes non-empty — a popup's content only starts rendering the frame after
/// it's marked open, its own settling delay — so that signal is one frame out
/// of step with the one this function actually needs.)
fn handle_dropdown_arrows(ctx: &egui::Context, nav: &MenuNav, popup_id: egui::Id, label: &str) {
    if !egui::Popup::is_id_open(ctx, popup_id) {
        ctx.data_mut(|d| d.remove::<bool>(popup_id));
        return;
    }
    if nav.items.is_empty() {
        return;
    }
    let had_items_last_time = ctx.data(|d| d.get_temp::<bool>(popup_id)).unwrap_or(false);
    ctx.data_mut(|d| d.insert_temp(popup_id, true));

    let focused = ctx.memory(|m| m.focused());
    let Some(current) = focused.and_then(|id| nav.items.iter().position(|i| *i == id)) else {
        if !had_items_last_time {
            ctx.memory_mut(|m| m.request_focus(nav.items[0]));
        }
        return;
    };

    // Claim vertical+horizontal arrows so egui's own geometric "nearest widget in
    // that screen direction" focus jump (see `Focus::end_pass`/
    // `find_widget_in_direction` in egui's own source) never fires instead — the
    // same technique `binder_panel.rs`'s `ARROW_KEYS_FILTER` and egui's own
    // `Slider` arrow-key handling both use.
    ctx.memory_mut(|m| {
        m.set_focus_lock_filter(
            nav.items[current],
            egui::EventFilter {
                tab: false,
                horizontal_arrows: true,
                vertical_arrows: true,
                escape: false,
            },
        )
    });

    let len = nav.items.len();
    let next = if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
        Some((current + 1) % len)
    } else if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
        Some((current + len - 1) % len)
    } else {
        None
    };
    // Guard against a redundant `request_focus` call when already at the target
    // index — it unconditionally resets the focus-lock filter set just above,
    // which would otherwise reopen a one-frame gap every frame at the ends.
    if let Some(next) = next
        && next != current
    {
        ctx.memory_mut(|m| m.request_focus(nav.items[next]));
    }

    let right = ctx.input(|i| i.key_pressed(egui::Key::ArrowRight));
    let left = ctx.input(|i| i.key_pressed(egui::Key::ArrowLeft));
    if right || left {
        let my_index = TOP_MENUS
            .iter()
            .position(|(l, _)| *l == label)
            .expect("label is always one of TOP_MENUS");
        let delta = if right { 1 } else { TOP_MENUS.len() - 1 };
        let target = (my_index + delta) % TOP_MENUS.len();
        egui::Popup::open_id(ctx, top_menu_popup_id(TOP_MENUS[target].0));
    }
}

/// A submenu nested inside a top-level dropdown (Theme/Layouts/Recent
/// Projects/Import/Color Binder By/multi-folder Export Manuscript), with the
/// same Up/Down navigation `top_menu_button`/`handle_dropdown_arrows` give
/// the dropdown itself, plus Right to open it from its (focused) trigger row
/// and Left to back out of it without closing the parent dropdown.
///
/// Enter/Space already open a focused `SubMenuButton`'s trigger today, via
/// egui's own built-in focus+Enter=click accessibility path (`Response::
/// clicked()` includes keyboard/accessibility activation). Right reuses that
/// *exact* path — synthesizing an Enter key event for `SubMenuButton::ui`'s
/// own click handling to pick up — rather than writing `MenuState.open_item`
/// directly: writing it directly was tried first and doesn't work for a
/// submenu's *first-ever* open. `MenuState::from_id`'s staleness-reset logic
/// (egui's own `containers/menu.rs`) resets `open_item` back to `None` unless
/// the *target* item already has its own "recently shown" bookkeeping entry —
/// which a submenu only gets from successfully rendering at least once via
/// `Popup::show`. A direct external write immediately gets wiped by
/// `SubMenuButton::ui`'s own very next (unavoidable, internal, cosmetic
/// "is this open, for styling" ) read of the same state, every single frame,
/// since content never gets the chance to render and establish that entry —
/// confirmed empirically (a real test failure, not a guess) before switching
/// to this approach. Going through egui's own click path sidesteps the whole
/// problem: it computes and writes that state once, itself, in a single pass
/// that doesn't re-read after writing.
///
/// Escape already closes *both* this submenu and its parent dropdown in the
/// same keypress, natively (every open `Popup::show`, at every nesting
/// level, independently checks the same unconsumed Escape event) —
/// deliberately left alone rather than fought: Escape means "close
/// everything," Left means "back up one level, keep the parent open," two
/// different, both useful, behaviors. Left's handler (in
/// `handle_submenu_arrows`) *can* write `MenuState.open_item` directly
/// (unlike opening) — by the time it runs, the submenu has already rendered
/// this frame, so its "recently shown" entry already exists and the same
/// reset hazard doesn't apply; it also must never go through `ui.close()`
/// (what `SubMenuButton`'s own close path uses), which cascades to close the
/// parent too — exactly the Escape behavior, and exactly what Left must
/// *not* do.
pub(super) fn nav_submenu(
    ui: &mut egui::Ui,
    parent_nav: &mut MenuNav,
    label: &str,
    content: impl FnOnce(&mut egui::Ui, &mut MenuNav),
) -> egui::Response {
    // Must match the id `SubMenuButton::ui` predicts internally (also via
    // `ui.next_auto_id()`, before rendering) — `next_auto_id` is a pure read
    // of `next_auto_id_salt` with no side effect, so calling it here first
    // doesn't shift what `SubMenuButton::ui`'s own later call returns.
    let my_id = ui.next_auto_id();
    let submenu_id = egui::containers::menu::SubMenu::id_from_widget_id(my_id);
    let already_open = egui::containers::menu::MenuState::from_ui(ui, |state, _stack| {
        state.open_item == Some(submenu_id)
    });
    let trigger_focused = ui.ctx().memory(|m| m.focused()) == Some(my_id);
    if trigger_focused
        && !already_open
        && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight))
    {
        ui.input_mut(|i| {
            i.events.push(egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            });
        });
    }

    let mut child_nav = MenuNav::default();
    let (trigger, _) =
        egui::containers::menu::SubMenuButton::new(label).ui(ui, |ui| content(ui, &mut child_nav));
    parent_nav.track(ui, &trigger);
    handle_submenu_arrows(ui, &child_nav, submenu_id, my_id);
    trigger
}

/// Up/Down/Left handling for an open submenu, called once right after its
/// content has rendered — mirrors `handle_dropdown_arrows`'s no-op-if-
/// closed/had-items-bookkeeping/auto-focus-first-item/claim-arrow-focus-lock
/// structure (kept as a separate, small duplication rather than a forced
/// shared abstraction: the two differ in which id drives bookkeeping
/// (predicted `submenu_id` here vs. label-derived `popup_id` there),
/// consuming vs. non-consuming key reads, and what the "back" direction
/// does — a submenu has no siblings to switch to the way top-level menus
/// do). Every arrow key read here uses `consume_key`, not `key_pressed`:
/// `handle_dropdown_arrows` runs *after* this (it's the caller's caller),
/// reading the *same* Up/Down/Left/Right via non-consuming reads — without
/// consuming here first, one Right press would open this submenu *and*
/// switch the top-level menu to its sibling in the same frame, and one Left
/// press would back out of this submenu *and* trigger a sibling-switch.
fn handle_submenu_arrows(ui: &egui::Ui, nav: &MenuNav, submenu_id: egui::Id, trigger_id: egui::Id) {
    let ctx = ui.ctx();
    let is_open = egui::containers::menu::MenuState::from_ui(ui, |state, _stack| {
        state.open_item == Some(submenu_id)
    });
    if !is_open {
        ctx.data_mut(|d| d.remove::<bool>(submenu_id));
        return;
    }
    if nav.items.is_empty() {
        return;
    }
    let had_items_last_time = ctx
        .data(|d| d.get_temp::<bool>(submenu_id))
        .unwrap_or(false);
    ctx.data_mut(|d| d.insert_temp(submenu_id, true));

    let focused = ctx.memory(|m| m.focused());
    let Some(current) = focused.and_then(|id| nav.items.iter().position(|i| *i == id)) else {
        if !had_items_last_time {
            ctx.memory_mut(|m| m.request_focus(nav.items[0]));
        }
        return;
    };

    ctx.memory_mut(|m| {
        m.set_focus_lock_filter(
            nav.items[current],
            egui::EventFilter {
                tab: false,
                horizontal_arrows: true,
                vertical_arrows: true,
                escape: false,
            },
        )
    });

    let len = nav.items.len();
    let next = if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown)) {
        Some((current + 1) % len)
    } else if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp)) {
        Some((current + len - 1) % len)
    } else {
        None
    };
    if let Some(next) = next
        && next != current
    {
        ctx.memory_mut(|m| m.request_focus(nav.items[next]));
    }

    if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft)) {
        egui::containers::menu::MenuState::from_ui(ui, |state, _stack| {
            state.open_item = None;
        });
        ctx.memory_mut(|m| m.request_focus(trigger_id));
    }
}

#[cfg(test)]
mod tests {
    use super::{TOP_MENUS, nav_submenu, top_menu_button, top_menu_popup_id};

    /// Drives the real `top_menu_button` for `label` with three plain stand-in
    /// items ("Alpha"/"Beta"/"Gamma"), for one frame with the given key
    /// `events`, and returns the resulting item ids — going through
    /// `top_menu_button` itself (rather than calling `MenuNav`/
    /// `handle_dropdown_arrows` directly) matters: a `Popup`'s "still open"
    /// bookkeeping (`keep_popup_open`, via `open_memory(None)`) is refreshed by
    /// `Popup::show` every frame it's actually shown, so skipping that call
    /// would make `Popup::is_id_open` silently go false after the first frame.
    fn frame(
        ctx: &egui::Context,
        label: &'static str,
        mnemonic: egui::Key,
        events: Vec<egui::Event>,
    ) -> Vec<egui::Id> {
        let input = egui::RawInput {
            events,
            ..Default::default()
        };
        let mut items = Vec::new();
        let _ = ctx.run_ui(input, |ui| {
            top_menu_button(ui, label, mnemonic, |ui, nav| {
                nav.button(ui, "Alpha");
                nav.button(ui, "Beta");
                nav.button(ui, "Gamma");
                items = nav.items.clone();
            });
        });
        items
    }

    /// Like `frame`, but renders *every* top-level menu (each with the same
    /// three stand-in items), matching how the real menu bar renders all 7
    /// every frame regardless of which one is open — needed for any test that
    /// exercises Left/Right switching to a *different* menu, since
    /// `handle_dropdown_arrows` only clears its per-popup "had items" bookkeeping
    /// (see its doc comment) for menus that actually get rendered that frame.
    /// Rendering only the "currently relevant" one, like `frame` does, would
    /// leave a just-closed menu's stale bookkeeping around indefinitely.
    fn frame_all(ctx: &egui::Context, events: Vec<egui::Event>) {
        let input = egui::RawInput {
            events,
            ..Default::default()
        };
        let _ = ctx.run_ui(input, |ui| {
            for (label, mnemonic) in TOP_MENUS {
                top_menu_button(ui, label, mnemonic, |ui, nav| {
                    nav.button(ui, "Alpha");
                    nav.button(ui, "Beta");
                    nav.button(ui, "Gamma");
                });
            }
        });
    }

    fn key_event(key: egui::Key) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }
    }

    #[test]
    fn down_and_up_wrap_at_the_ends_of_the_dropdown() {
        let ctx = egui::Context::default();
        ctx.all_styles_mut(|style| style.animation_time = 0.0);
        let popup_id = top_menu_popup_id("File");
        egui::Popup::open_id(&ctx, popup_id);
        // A popup marked open via `open_id` outside any pass doesn't actually
        // render its content until the following frame (`Popup::show`'s own
        // first-frame settling) — one warm-up frame before relying on its
        // content/`nav.items` having been populated at all.
        frame(&ctx, "File", egui::Key::F, vec![]);

        // Nothing focused yet — should land on the first item.
        let items = frame(&ctx, "File", egui::Key::F, vec![]);
        assert_eq!(ctx.memory(|m| m.focused()), Some(items[0]));

        // Let focus settle a frame (mirrors `binder_panel.rs`'s test harness: a
        // widget's focus-lock filter only takes effect starting the frame after
        // it gains focus) before pressing Up — from the first item, Up should
        // wrap to the last.
        frame(&ctx, "File", egui::Key::F, vec![]);
        let items = frame(
            &ctx,
            "File",
            egui::Key::F,
            vec![key_event(egui::Key::ArrowUp)],
        );
        assert_eq!(ctx.memory(|m| m.focused()), Some(items[2]));

        // From the last item, Down should wrap back to the first.
        frame(&ctx, "File", egui::Key::F, vec![]);
        let items = frame(
            &ctx,
            "File",
            egui::Key::F,
            vec![key_event(egui::Key::ArrowDown)],
        );
        assert_eq!(ctx.memory(|m| m.focused()), Some(items[0]));
    }

    #[test]
    fn right_and_left_cycle_through_top_menus_with_wraparound() {
        let ctx = egui::Context::default();
        ctx.all_styles_mut(|style| style.animation_time = 0.0);
        let file_id = top_menu_popup_id("File");
        egui::Popup::open_id(&ctx, file_id);
        frame_all(&ctx, vec![]); // focus lands on the first item
        frame_all(&ctx, vec![]); // let focus settle

        frame_all(&ctx, vec![key_event(egui::Key::ArrowRight)]);
        let edit_id = top_menu_popup_id("Edit");
        assert!(
            egui::Popup::is_id_open(&ctx, edit_id),
            "Right from File should open Edit (the next menu in TOP_MENUS)"
        );
        assert!(!egui::Popup::is_id_open(&ctx, file_id));

        // From Edit, Left should go back to File, and Left again should wrap
        // around to the last menu (Help).
        frame_all(&ctx, vec![]);
        frame_all(&ctx, vec![]);
        frame_all(&ctx, vec![key_event(egui::Key::ArrowLeft)]);
        assert!(egui::Popup::is_id_open(&ctx, file_id));

        frame_all(&ctx, vec![]);
        frame_all(&ctx, vec![]);
        frame_all(&ctx, vec![key_event(egui::Key::ArrowLeft)]);
        let help_id = top_menu_popup_id(TOP_MENUS[TOP_MENUS.len() - 1].0);
        assert!(
            egui::Popup::is_id_open(&ctx, help_id),
            "Left from File should wrap around to Help (the last menu in TOP_MENUS)"
        );
    }

    /// Regression test for a real bug: a menu item's own click handler doesn't
    /// call `ui.close()` anywhere in this codebase (clicking "Open Document…",
    /// say, leaves the File dropdown nominally still "open" in egui's Memory,
    /// even once the dialog it opened takes over) — which used to be harmless,
    /// since the dropdown just sat there unfocused and inert. Once
    /// `handle_dropdown_arrows` started auto-focusing the first item whenever
    /// nothing in *its own* list has focus, that harmless leftover "still open"
    /// state became actively disruptive: it kept re-stealing focus back onto
    /// the dropdown's first item every single frame, away from whatever dialog
    /// had opened on top of it, on every frame *after* the one where it
    /// legitimately first opened.
    #[test]
    fn does_not_steal_focus_back_once_something_else_has_it() {
        let ctx = egui::Context::default();
        ctx.all_styles_mut(|style| style.animation_time = 0.0);
        let popup_id = top_menu_popup_id("File");
        egui::Popup::open_id(&ctx, popup_id);
        frame(&ctx, "File", egui::Key::F, vec![]); // warm-up (see the other tests)
        frame(&ctx, "File", egui::Key::F, vec![]); // legitimate first-open auto-focus

        // A stand-in for e.g. the Open Document modal's own text field, rendered
        // (like a real dialog would be) in the same pass as the still-open File
        // dropdown, and given focus — without the File dropdown ever being
        // closed. A bare `request_focus` on an id nothing ever renders wouldn't
        // do: egui drops focus from a widget that isn't shown in a pass, which
        // would trivially "pass" this test for the wrong reason.
        let mut other_id = None;
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            let other_response = ui.button("Other Dialog");
            other_id = Some(other_response.id);
            other_response.request_focus();
            top_menu_button(ui, "File", egui::Key::F, |ui, nav| {
                nav.button(ui, "Alpha");
                nav.button(ui, "Beta");
                nav.button(ui, "Gamma");
            });
        });
        let other_id = other_id.unwrap();
        assert_eq!(ctx.memory(|m| m.focused()), Some(other_id));

        // Rendering both again — the still-nominally-open File dropdown must
        // not claw focus back to its own first item.
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            let _ = ui.button("Other Dialog");
            top_menu_button(ui, "File", egui::Key::F, |ui, nav| {
                nav.button(ui, "Alpha");
                nav.button(ui, "Beta");
                nav.button(ui, "Gamma");
            });
        });
        assert_eq!(
            ctx.memory(|m| m.focused()),
            Some(other_id),
            "File's dropdown re-stole focus even though it didn't just open this frame"
        );
    }

    /// Drives a "File" dropdown containing a single nested submenu ("Sub",
    /// three stand-in items "Alpha"/"Beta"/"Gamma", none of which call
    /// `ui.close()` — matching Theme/Color Binder By/Layouts, the submenus
    /// whose items never close on click). Returns the submenu's item ids
    /// (empty unless it's open and rendered this frame) and the submenu
    /// trigger's own `Response` (focus should land back on `.id` when the
    /// submenu closes).
    fn submenu_frame(
        ctx: &egui::Context,
        events: Vec<egui::Event>,
    ) -> (Vec<egui::Id>, egui::Response) {
        let input = egui::RawInput {
            events,
            ..Default::default()
        };
        let mut items = Vec::new();
        let mut trigger = None;
        let _ = ctx.run_ui(input, |ui| {
            top_menu_button(ui, "File", egui::Key::F, |ui, nav| {
                let t = nav_submenu(ui, nav, "Sub", |ui, sub_nav| {
                    sub_nav.button(ui, "Alpha");
                    sub_nav.button(ui, "Beta");
                    sub_nav.button(ui, "Gamma");
                    items = sub_nav.items.clone();
                });
                trigger = Some(t);
            });
        });
        (items, trigger.unwrap())
    }

    /// Like `submenu_frame`, but also renders a plain "Edit" top-level menu
    /// alongside "File" — needed for any test checking that a keypress inside
    /// the submenu doesn't *also* get read by the top-level Left/Right
    /// sibling-switch logic (see `frame_all`'s own doc comment for why
    /// rendering every relevant top-level menu, not just "File", matters).
    fn submenu_frame_all(ctx: &egui::Context, events: Vec<egui::Event>) {
        let input = egui::RawInput {
            events,
            ..Default::default()
        };
        let _ = ctx.run_ui(input, |ui| {
            top_menu_button(ui, "File", egui::Key::F, |ui, nav| {
                nav_submenu(ui, nav, "Sub", |ui, sub_nav| {
                    sub_nav.button(ui, "Alpha");
                    sub_nav.button(ui, "Beta");
                    sub_nav.button(ui, "Gamma");
                });
            });
            top_menu_button(ui, "Edit", egui::Key::E, |ui, nav| {
                nav.button(ui, "Alpha");
                nav.button(ui, "Beta");
                nav.button(ui, "Gamma");
            });
        });
    }

    #[test]
    fn right_arrow_opens_a_focused_submenu_and_focuses_its_first_item() {
        let ctx = egui::Context::default();
        ctx.all_styles_mut(|style| style.animation_time = 0.0);
        let popup_id = top_menu_popup_id("File");
        egui::Popup::open_id(&ctx, popup_id);
        submenu_frame(&ctx, vec![]); // warm-up: File's content (just the trigger) renders
        let (_, trigger) = submenu_frame(&ctx, vec![]); // nothing focused -> lands on the trigger (File's only item)
        assert_eq!(ctx.memory(|m| m.focused()), Some(trigger.id));

        submenu_frame(&ctx, vec![]); // let focus settle (mirrors the top-level tests' own pattern)
        submenu_frame(&ctx, vec![key_event(egui::Key::ArrowRight)]); // consumed: opens, but (like a
        // top-level `Popup::open_id`) the submenu's own content doesn't
        // actually render until the following frame — `Popup::show`'s own
        // first-frame settling, the same reason `down_and_up_wrap_at_the_
        // ends_of_the_dropdown` above needs its own warm-up frame.
        let (items, _) = submenu_frame(&ctx, vec![]);
        assert_eq!(
            ctx.memory(|m| m.focused()),
            Some(items[0]),
            "Right should open the submenu and focus its first item"
        );
    }

    #[test]
    fn up_and_down_wrap_at_the_ends_of_an_open_submenu() {
        let ctx = egui::Context::default();
        ctx.all_styles_mut(|style| style.animation_time = 0.0);
        let popup_id = top_menu_popup_id("File");
        egui::Popup::open_id(&ctx, popup_id);
        submenu_frame(&ctx, vec![]);
        submenu_frame(&ctx, vec![]); // focuses the trigger
        submenu_frame(&ctx, vec![]); // let focus settle
        submenu_frame(&ctx, vec![key_event(egui::Key::ArrowRight)]); // consumed: opens
        submenu_frame(&ctx, vec![]); // content renders, auto-focuses items[0]

        submenu_frame(&ctx, vec![]); // let the freshly-focused item's lock filter settle
        let (items, _) = submenu_frame(&ctx, vec![key_event(egui::Key::ArrowUp)]);
        assert_eq!(
            ctx.memory(|m| m.focused()),
            Some(items[2]),
            "Up from the first item should wrap to the last"
        );

        submenu_frame(&ctx, vec![]);
        let (items, _) = submenu_frame(&ctx, vec![key_event(egui::Key::ArrowDown)]);
        assert_eq!(
            ctx.memory(|m| m.focused()),
            Some(items[0]),
            "Down from the last item should wrap to the first"
        );
    }

    #[test]
    fn left_closes_the_submenu_and_returns_focus_to_the_trigger_without_closing_the_parent() {
        let ctx = egui::Context::default();
        ctx.all_styles_mut(|style| style.animation_time = 0.0);
        let popup_id = top_menu_popup_id("File");
        egui::Popup::open_id(&ctx, popup_id);
        submenu_frame(&ctx, vec![]);
        let (_, trigger) = submenu_frame(&ctx, vec![]); // focuses the trigger
        submenu_frame(&ctx, vec![]); // let focus settle
        submenu_frame(&ctx, vec![key_event(egui::Key::ArrowRight)]); // consumed: opens
        submenu_frame(&ctx, vec![]); // content renders, auto-focuses items[0]
        submenu_frame(&ctx, vec![]); // let focus settle

        submenu_frame(&ctx, vec![key_event(egui::Key::ArrowLeft)]);

        assert_eq!(
            ctx.memory(|m| m.focused()),
            Some(trigger.id),
            "Left should return focus to the trigger"
        );
        assert!(
            egui::Popup::is_id_open(&ctx, popup_id),
            "Left should not close the parent File dropdown"
        );
    }

    #[test]
    fn right_arrow_opening_a_submenu_does_not_also_switch_to_the_sibling_top_level_menu() {
        let ctx = egui::Context::default();
        ctx.all_styles_mut(|style| style.animation_time = 0.0);
        let file_id = top_menu_popup_id("File");
        egui::Popup::open_id(&ctx, file_id);
        submenu_frame_all(&ctx, vec![]);
        submenu_frame_all(&ctx, vec![]); // focuses the submenu trigger (File's only item)
        submenu_frame_all(&ctx, vec![]); // let focus settle

        submenu_frame_all(&ctx, vec![key_event(egui::Key::ArrowRight)]);

        let edit_id = top_menu_popup_id("Edit");
        assert!(
            !egui::Popup::is_id_open(&ctx, edit_id),
            "Right opening a submenu must not also switch the top-level menu to Edit"
        );
        assert!(egui::Popup::is_id_open(&ctx, file_id));
    }

    /// Regression test analogous to the top-level `does_not_steal_focus_back_...`
    /// above, for a submenu whose items never call `ui.close()` (Theme/Color
    /// Binder By/Layouts) — there's no live bug for the submenus that *do*
    /// close on click (Recent Projects/Import/multi-folder Export
    /// Manuscript), since closing on click already cascades to close the
    /// parent too, but this still matters as a defensive test for any future
    /// submenu item that opens a modal without closing.
    #[test]
    fn submenu_does_not_steal_focus_back_once_something_else_has_it() {
        let ctx = egui::Context::default();
        ctx.all_styles_mut(|style| style.animation_time = 0.0);
        let popup_id = top_menu_popup_id("File");
        egui::Popup::open_id(&ctx, popup_id);
        submenu_frame(&ctx, vec![]);
        submenu_frame(&ctx, vec![]); // focuses the trigger
        submenu_frame(&ctx, vec![]); // let focus settle
        submenu_frame(&ctx, vec![key_event(egui::Key::ArrowRight)]); // open, focuses items[0]
        submenu_frame(&ctx, vec![]); // legitimate first-open auto-focus settle

        let mut other_id = None;
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            let other_response = ui.button("Other Dialog");
            other_id = Some(other_response.id);
            other_response.request_focus();
            top_menu_button(ui, "File", egui::Key::F, |ui, nav| {
                nav_submenu(ui, nav, "Sub", |ui, sub_nav| {
                    sub_nav.button(ui, "Alpha");
                    sub_nav.button(ui, "Beta");
                    sub_nav.button(ui, "Gamma");
                });
            });
        });
        let other_id = other_id.unwrap();
        assert_eq!(ctx.memory(|m| m.focused()), Some(other_id));

        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            let _ = ui.button("Other Dialog");
            top_menu_button(ui, "File", egui::Key::F, |ui, nav| {
                nav_submenu(ui, nav, "Sub", |ui, sub_nav| {
                    sub_nav.button(ui, "Alpha");
                    sub_nav.button(ui, "Beta");
                    sub_nav.button(ui, "Gamma");
                });
            });
        });
        assert_eq!(
            ctx.memory(|m| m.focused()),
            Some(other_id),
            "the submenu re-stole focus even though it didn't just open this frame"
        );
    }

    /// Drives a "File" dropdown containing *two* nested submenus ("Sub1"/
    /// "Sub2"), each with one stand-in item — for `opening_a_sibling_submenu_
    /// closes_the_previously_open_one`, which needs to navigate from one
    /// submenu to the other and confirm the shared `MenuState` (one
    /// `open_item` slot per dropdown, holding at most one submenu open at a
    /// time) behaves as documented.
    fn two_submenu_frame(
        ctx: &egui::Context,
        events: Vec<egui::Event>,
    ) -> (egui::Response, egui::Response, Vec<egui::Id>, Vec<egui::Id>) {
        let input = egui::RawInput {
            events,
            ..Default::default()
        };
        let mut trigger1 = None;
        let mut trigger2 = None;
        let mut items1 = Vec::new();
        let mut items2 = Vec::new();
        let _ = ctx.run_ui(input, |ui| {
            top_menu_button(ui, "File", egui::Key::F, |ui, nav| {
                let t1 = nav_submenu(ui, nav, "Sub1", |ui, sub_nav| {
                    sub_nav.button(ui, "Alpha");
                    items1 = sub_nav.items.clone();
                });
                let t2 = nav_submenu(ui, nav, "Sub2", |ui, sub_nav| {
                    sub_nav.button(ui, "Beta");
                    items2 = sub_nav.items.clone();
                });
                trigger1 = Some(t1);
                trigger2 = Some(t2);
            });
        });
        (trigger1.unwrap(), trigger2.unwrap(), items1, items2)
    }

    #[test]
    fn opening_a_sibling_submenu_closes_the_previously_open_one() {
        let ctx = egui::Context::default();
        ctx.all_styles_mut(|style| style.animation_time = 0.0);
        let popup_id = top_menu_popup_id("File");
        egui::Popup::open_id(&ctx, popup_id);
        two_submenu_frame(&ctx, vec![]);
        let (t1, _, _, _) = two_submenu_frame(&ctx, vec![]); // focuses Sub1's trigger
        two_submenu_frame(&ctx, vec![]); // let focus settle
        two_submenu_frame(&ctx, vec![key_event(egui::Key::ArrowRight)]); // consumed: opens Sub1
        let (_, _, items1, _) = two_submenu_frame(&ctx, vec![]); // content renders, auto-focuses items1[0]
        assert_eq!(ctx.memory(|m| m.focused()), Some(items1[0]));

        two_submenu_frame(&ctx, vec![]); // let focus settle
        two_submenu_frame(&ctx, vec![key_event(egui::Key::ArrowLeft)]); // close Sub1, back to its trigger
        assert_eq!(ctx.memory(|m| m.focused()), Some(t1.id));

        two_submenu_frame(&ctx, vec![]); // let File's own focus-lock filter settle
        let (_, t2, _, _) = two_submenu_frame(&ctx, vec![key_event(egui::Key::ArrowDown)]); // move to Sub2's trigger
        assert_eq!(ctx.memory(|m| m.focused()), Some(t2.id));

        two_submenu_frame(&ctx, vec![]); // let focus settle
        two_submenu_frame(&ctx, vec![key_event(egui::Key::ArrowRight)]); // consumed: opens Sub2
        let (_, _, _, items2) = two_submenu_frame(&ctx, vec![]); // content renders, auto-focuses items2[0]
        assert_eq!(
            ctx.memory(|m| m.focused()),
            Some(items2[0]),
            "Right on Sub2's trigger should open Sub2, proving the shared MenuState \
             correctly switched away from Sub1 (one open_item slot per dropdown)"
        );
    }
}
