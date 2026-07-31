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

#[cfg(test)]
mod tests {
    use super::{TOP_MENUS, top_menu_button, top_menu_popup_id};

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
}
