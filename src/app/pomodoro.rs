use super::*;

impl SmaragdApp {
    pub(super) fn handle_pomodoro_event(&mut self, event: ui::pomodoro_panel::PomodoroEvent) {
        let durations = crate::pomodoro::resolve_durations(&self.settings);
        match event {
            ui::pomodoro_panel::PomodoroEvent::Start => {
                self.pomodoro.start(std::time::Instant::now());
            }
            ui::pomodoro_panel::PomodoroEvent::Pause => self.pomodoro.pause(),
            ui::pomodoro_panel::PomodoroEvent::Reset => self.pomodoro.reset(&durations),
            ui::pomodoro_panel::PomodoroEvent::Skip => self.pomodoro.skip(&durations),
        }
    }

    /// Advances the Pomodoro timer by however much wall-clock time passed since
    /// the last frame, regardless of whether its dock tab is currently open —
    /// its state (and the status bar's countdown segment) needs to keep moving
    /// even while the tab is closed. Schedules another repaint a second out
    /// while running, since egui's default reactive mode otherwise only
    /// repaints on input/events and the countdown would freeze on screen.
    pub(super) fn tick_pomodoro(&mut self, ctx: &egui::Context) {
        let durations = crate::pomodoro::resolve_durations(&self.settings);
        self.pomodoro.tick(std::time::Instant::now(), &durations);
        if self.pomodoro.is_running() {
            ctx.request_repaint_after(std::time::Duration::from_secs(1));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pomodoro_start_event_starts_the_timer() {
        let mut app = SmaragdApp::test_fixture();
        assert!(!app.pomodoro.is_running());

        app.handle_pomodoro_event(ui::pomodoro_panel::PomodoroEvent::Start);

        assert!(app.pomodoro.is_running());
    }

    #[test]
    fn pomodoro_pause_event_stops_a_running_timer() {
        let mut app = SmaragdApp::test_fixture();
        app.handle_pomodoro_event(ui::pomodoro_panel::PomodoroEvent::Start);
        assert!(app.pomodoro.is_running());

        app.handle_pomodoro_event(ui::pomodoro_panel::PomodoroEvent::Pause);

        assert!(!app.pomodoro.is_running());
    }
}
