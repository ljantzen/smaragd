use super::*;

/// An error-severity notification, shown as a floating, auto-dismissing box
/// stacked in the corner of the window rather than as status-bar text — see
/// `SmaragdApp::push_error_toast`/`show_toasts`.
pub(super) struct Toast {
    pub(super) message: String,
    shown_at: std::time::Instant,
}

/// Built-in toast duration used when `Settings::toast_duration_secs` is
/// unconfigured (`0`) — long enough to read a short sentence without having to
/// rush, short enough that several errors in a row don't pile up into a
/// permanent wall of boxes.
pub(super) const DEFAULT_TOAST_DURATION: std::time::Duration = std::time::Duration::from_secs(6);

/// Built-in status-bar auto-clear duration used when
/// `Settings::status_message_duration_secs` is unconfigured (`0`) — a little
/// more generous than the toast default, since a routine confirmation has no
/// manual dismiss button of its own to cut that wait short.
pub(super) const DEFAULT_STATUS_MESSAGE_DURATION: std::time::Duration =
    std::time::Duration::from_secs(8);

/// Resolve `Settings::toast_duration_secs`'s blank-means-unset (`0`) convention
/// to an actual `Duration` — same shape as `editor_font::resolve_size`/
/// `pomodoro::resolve_durations`.
pub(super) fn resolve_toast_duration(settings: &Settings) -> std::time::Duration {
    match settings.toast_duration_secs {
        0 => DEFAULT_TOAST_DURATION,
        secs => std::time::Duration::from_secs(secs as u64),
    }
}

/// Resolve `Settings::status_message_duration_secs`'s blank-means-unset (`0`)
/// convention to an actual `Duration`.
pub(super) fn resolve_status_message_duration(settings: &Settings) -> std::time::Duration {
    match settings.status_message_duration_secs {
        0 => DEFAULT_STATUS_MESSAGE_DURATION,
        secs => std::time::Duration::from_secs(secs as u64),
    }
}

impl SmaragdApp {
    /// Show `message` as an error-severity toast — see `Toast`'s doc comment for
    /// when to reach for this instead of `status_message`.
    pub(super) fn push_error_toast(&mut self, message: impl Into<String>) {
        self.toasts.push(Toast {
            message: message.into(),
            shown_at: std::time::Instant::now(),
        });
    }

    /// Drop any toast that's outlived `Settings::toast_duration_secs` (see
    /// `resolve_toast_duration`), then render whatever's left stacked down the
    /// top-right corner of the window (oldest at top,
    /// each independently dismissible via its own × button) — called every
    /// frame regardless of Focus Mode or which dock tabs are open, since an
    /// error is exactly the kind of thing that shouldn't go unnoticed just
    /// because of what else happens to be on screen. Schedules a short
    /// repaint interval while any toast is showing so it actually
    /// disappears on its own once its time is up, the same reasoning as
    /// `tick_pomodoro`'s own `request_repaint_after`.
    pub(super) fn show_toasts(&mut self, ctx: &egui::Context) {
        let duration = resolve_toast_duration(&self.settings);
        let now = std::time::Instant::now();
        self.toasts
            .retain(|toast| now.duration_since(toast.shown_at) < duration);
        if self.toasts.is_empty() {
            return;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(200));

        let mut dismiss = None;
        for (index, toast) in self.toasts.iter().enumerate() {
            egui::Area::new(egui::Id::new("toast").with(index))
                .anchor(
                    egui::Align2::RIGHT_TOP,
                    egui::vec2(-12.0, 12.0 + index as f32 * 56.0),
                )
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    egui::Frame::popup(ui.style())
                        .fill(egui::Color32::from_rgb(140, 30, 30))
                        .show(ui, |ui| {
                            ui.set_max_width(360.0);
                            ui.horizontal(|ui| {
                                ui.colored_label(egui::Color32::WHITE, &toast.message);
                                if ui.small_button("×").clicked() {
                                    dismiss = Some(index);
                                }
                            });
                        });
                });
        }
        if let Some(index) = dismiss {
            self.toasts.remove(index);
        }
    }

    /// Set the status-bar confirmation (see `status_message`'s doc comment for
    /// when to use this instead of `push_error_toast`) and record when, so
    /// `clear_status_message_if_expired` can time it out on its own.
    pub(super) fn set_status_message(&mut self, message: impl Into<String>) {
        self.status_message = Some(message.into());
        self.status_message_set_at = Some(std::time::Instant::now());
    }

    /// Clear `status_message` (and its timestamp) immediately, rather than
    /// waiting for `clear_status_message_if_expired` to time it out on its own
    /// — for the rare case that wants a clean slate right away (e.g.
    /// `set_project`, switching to a different project).
    pub(super) fn clear_status_message(&mut self) {
        self.status_message = None;
        self.status_message_set_at = None;
    }

    /// Auto-clear `status_message` once it's been showing longer than
    /// `Settings::status_message_duration_secs` (see
    /// `resolve_status_message_duration`) — called every frame, mirroring
    /// `show_toasts`' own expiry check and for the same reason: status-bar
    /// text that just sits there until the next unrelated update happens to
    /// overwrite it is easy to mistake for something still current.
    pub(super) fn clear_status_message_if_expired(&mut self, ctx: &egui::Context) {
        let Some(set_at) = self.status_message_set_at else {
            return;
        };
        let duration = resolve_status_message_duration(&self.settings);
        let elapsed = set_at.elapsed();
        if elapsed >= duration {
            self.clear_status_message();
        } else {
            ctx.request_repaint_after(duration - elapsed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_STATUS_MESSAGE_DURATION, DEFAULT_TOAST_DURATION, resolve_status_message_duration,
        resolve_toast_duration,
    };
    use crate::settings::Settings;

    #[test]
    fn resolve_toast_duration_falls_back_to_the_default_when_unconfigured() {
        let settings = Settings {
            toast_duration_secs: 0,
            ..Default::default()
        };
        assert_eq!(resolve_toast_duration(&settings), DEFAULT_TOAST_DURATION);
    }

    #[test]
    fn resolve_toast_duration_uses_the_configured_value() {
        let settings = Settings {
            toast_duration_secs: 20,
            ..Default::default()
        };
        assert_eq!(
            resolve_toast_duration(&settings),
            std::time::Duration::from_secs(20)
        );
    }

    #[test]
    fn resolve_status_message_duration_falls_back_to_the_default_when_unconfigured() {
        let settings = Settings {
            status_message_duration_secs: 0,
            ..Default::default()
        };
        assert_eq!(
            resolve_status_message_duration(&settings),
            DEFAULT_STATUS_MESSAGE_DURATION
        );
    }

    #[test]
    fn resolve_status_message_duration_uses_the_configured_value() {
        let settings = Settings {
            status_message_duration_secs: 30,
            ..Default::default()
        };
        assert_eq!(
            resolve_status_message_duration(&settings),
            std::time::Duration::from_secs(30)
        );
    }
}
