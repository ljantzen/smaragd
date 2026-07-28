use crate::pomodoro::{PomodoroDurations, PomodoroState};

/// Outcomes of user interaction with the Pomodoro panel, handled by the caller
/// (`app.rs`) rather than mutated here — same pure-rendering-layer split as
/// `corkboard_panel`/`CorkboardEvent`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PomodoroEvent {
    Start,
    Pause,
    Reset,
    Skip,
}

/// Renders the Pomodoro dock tab: a large `MM:SS` countdown, the current
/// phase, completed work-session count, and Start/Pause/Reset/Skip buttons
/// (Start and Pause are mutually exclusive, shown depending on
/// `PomodoroState::is_running`). `durations` isn't used to render the
/// countdown itself (`state.remaining()` already reflects it) — only to show
/// what the *next* phase's length will be, so changing Settings mid-session
/// is visible before it takes effect.
pub fn show(
    ui: &mut egui::Ui,
    state: &PomodoroState,
    durations: &PomodoroDurations,
) -> Option<PomodoroEvent> {
    let mut event = None;
    ui.vertical_centered(|ui| {
        ui.add_space(16.0);
        ui.label(egui::RichText::new(state.phase().label()).size(20.0));
        ui.add_space(8.0);

        let remaining = state.remaining().as_secs();
        ui.label(
            egui::RichText::new(format!("{:02}:{:02}", remaining / 60, remaining % 60))
                .size(36.0)
                .strong(),
        );
        ui.add_space(4.0);
        ui.weak(format!(
            "{} work session{} completed",
            state.completed_work_sessions(),
            if state.completed_work_sessions() == 1 {
                ""
            } else {
                "s"
            }
        ));
        ui.add_space(16.0);

        ui.horizontal(|ui| {
            if state.is_running() {
                if ui.button("Pause").clicked() {
                    event = Some(PomodoroEvent::Pause);
                }
            } else if ui.button("Start").clicked() {
                event = Some(PomodoroEvent::Start);
            }
            if ui.button("Skip").clicked() {
                event = Some(PomodoroEvent::Skip);
            }
            if ui.button("Reset").clicked() {
                event = Some(PomodoroEvent::Reset);
            }
        });

        ui.add_space(16.0);
        ui.weak(format!(
            "Next: {} min work / {} min short break / {} min long break every {} sessions",
            durations.work.as_secs() / 60,
            durations.short_break.as_secs() / 60,
            durations.long_break.as_secs() / 60,
            durations.cycles_before_long_break,
        ));
    });
    event
}
