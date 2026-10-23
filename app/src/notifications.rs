//! OS-level desktop notifications (currently just the Pomodoro timer's phase
//! changes — see `app::pomodoro::tick_pomodoro`). A thin wrapper over
//! `notify-rust` so callers don't need that crate's own API surface, and so
//! this is the one place a future caller/test would swap in a fake backend.
//!
//! Linux uses `notify-rust`'s vendored-libdbus backend (the `dbus`/`d_vendored`
//! feature, not `zbus`): `eframe`'s own Linux accessibility stack already pulls
//! in a `zbus` version incompatible with the one `notify-rust` needs, and the
//! vendored classic `dbus` crate sidesteps that clash entirely — it also means
//! no system `libdbus-1-dev` is required to build, or `libdbus` itself to run,
//! since the vendored copy is compiled in. macOS/Windows use `notify-rust`'s
//! native backends unconditionally (no such conflict there).

/// Show a desktop notification via the OS's native notification center.
/// Best-effort: a failure (no notification daemon running, permission denied,
/// etc.) is returned as a string for the caller to decide whether it's worth
/// surfacing, rather than panicking — a missed notification is never worth
/// blocking on.
pub fn show(summary: &str, body: &str) -> Result<(), String> {
    notify_rust::Notification::new()
        .summary(summary)
        .body(body)
        .show()
        .map(|_| ())
        .map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Not run in CI/`cargo test`'s default set: needs a real desktop
    /// notification daemon (D-Bus session on Linux, Notification Center on
    /// macOS, the Action Center on Windows) — exactly what a sandboxed CI
    /// runner or headless dev environment doesn't have. Run manually with
    /// `cargo test --lib notifications::tests -- --ignored` on a real
    /// desktop to confirm a notification actually appears.
    #[test]
    #[ignore = "requires a real desktop notification daemon"]
    fn show_succeeds_on_a_real_desktop() {
        show("Smaragd", "This is a test notification.").unwrap();
    }
}
