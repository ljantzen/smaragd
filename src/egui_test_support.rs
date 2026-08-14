//! Shared helper for the `ctx.run_ui(...)` pattern every UI test module uses to
//! drive a widget with synthetic input and inspect what it does, without caring
//! about the frame's actual paint output.

/// Runs one egui frame and discards its `FullOutput`, same as the old
/// `let _ = ctx.run_ui(...)` every UI test module used to write directly — except
/// `epaint` 0.36 added a `Drop` guard on `TexturesDelta` that panics (in debug
/// builds) if it's dropped with unapplied deltas rather than explicitly
/// `clear`ed, which a bare `let _ =` no longer satisfies. Centralized here
/// instead of calling `.textures_delta.clear()` at each of this pattern's ~45
/// call sites.
pub(crate) fn run_ui_and_discard(
    ctx: &egui::Context,
    input: egui::RawInput,
    add_contents: impl FnMut(&mut egui::Ui),
) {
    let mut output = ctx.run_ui(input, add_contents);
    output.textures_delta.clear();
}
