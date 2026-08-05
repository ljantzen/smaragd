//! Pomodoro work/break interval timer: pure state machine, no egui dependency
//! — same "logic separate from rendering" split as `search.rs`/`shortcuts.rs`.
//! `ui::pomodoro_panel` renders it; `app.rs` owns a `PomodoroState` and ticks
//! it once per frame regardless of whether that panel is currently visible,
//! so the timer keeps running while its dock tab is closed.

use std::time::{Duration, Instant};

use crate::settings::Settings;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PomodoroPhase {
    Work,
    ShortBreak,
    LongBreak,
}

impl PomodoroPhase {
    pub fn label(self) -> &'static str {
        match self {
            PomodoroPhase::Work => "Work",
            PomodoroPhase::ShortBreak => "Short Break",
            PomodoroPhase::LongBreak => "Long Break",
        }
    }
}

/// Reported by [`PomodoroState::tick`] the one frame a phase actually
/// completes on its own — never on `skip` (a deliberate user action, not
/// something they need to be told about) — so a caller can react (e.g. an OS
/// notification, see `notifications`/`app::pomodoro::tick_pomodoro`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhaseTransition {
    pub completed: PomodoroPhase,
    pub next: PomodoroPhase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PomodoroDurations {
    pub work: Duration,
    pub short_break: Duration,
    pub long_break: Duration,
    pub cycles_before_long_break: u32,
}

const DEFAULT_WORK_MINUTES: u32 = 25;
const DEFAULT_SHORT_BREAK_MINUTES: u32 = 5;
const DEFAULT_LONG_BREAK_MINUTES: u32 = 15;
const DEFAULT_CYCLES_BEFORE_LONG_BREAK: u32 = 4;

/// `0` in any of `Settings`' four `pomodoro_*` fields means "unset" (a fresh
/// `Settings::default()`), resolved to a real default here — same convention
/// `editor_font::resolve_size` uses for `editor_font_size`, rather than
/// `#[serde(default = "...")]` on the fields themselves.
pub fn resolve_durations(settings: &Settings) -> PomodoroDurations {
    let minutes = |configured: u32, default: u32| {
        Duration::from_secs(u64::from(if configured > 0 { configured } else { default }) * 60)
    };
    PomodoroDurations {
        work: minutes(settings.pomodoro_work_minutes, DEFAULT_WORK_MINUTES),
        short_break: minutes(
            settings.pomodoro_short_break_minutes,
            DEFAULT_SHORT_BREAK_MINUTES,
        ),
        long_break: minutes(
            settings.pomodoro_long_break_minutes,
            DEFAULT_LONG_BREAK_MINUTES,
        ),
        cycles_before_long_break: if settings.pomodoro_cycles_before_long_break > 0 {
            settings.pomodoro_cycles_before_long_break
        } else {
            DEFAULT_CYCLES_BEFORE_LONG_BREAK
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PomodoroState {
    phase: PomodoroPhase,
    remaining: Duration,
    running: bool,
    completed_work_sessions: u32,
    last_tick: Option<Instant>,
}

impl PomodoroState {
    pub fn new(durations: &PomodoroDurations) -> Self {
        PomodoroState {
            phase: PomodoroPhase::Work,
            remaining: durations.work,
            running: false,
            completed_work_sessions: 0,
            last_tick: None,
        }
    }

    pub fn phase(&self) -> PomodoroPhase {
        self.phase
    }

    pub fn remaining(&self) -> Duration {
        self.remaining
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    pub fn completed_work_sessions(&self) -> u32 {
        self.completed_work_sessions
    }

    /// Whether the timer has ever been started — used to decide whether the
    /// status bar's countdown segment should show at all (a never-started
    /// timer has nothing worth glancing at).
    pub fn has_started(&self) -> bool {
        self.running || self.last_tick.is_some() || self.completed_work_sessions > 0
    }

    pub fn start(&mut self, now: Instant) {
        self.running = true;
        self.last_tick = Some(now);
    }

    pub fn pause(&mut self) {
        self.running = false;
        self.last_tick = None;
    }

    pub fn reset(&mut self, durations: &PomodoroDurations) {
        self.phase = PomodoroPhase::Work;
        self.remaining = durations.work;
        self.running = false;
        self.last_tick = None;
    }

    /// Manually advance to the next phase right now, regardless of how much
    /// time remains — pauses afterward, same as a phase completing on its own.
    pub fn skip(&mut self, durations: &PomodoroDurations) {
        self.advance_phase(durations);
        self.running = false;
        self.last_tick = None;
    }

    /// Advances elapsed wall-clock time since the last tick (or since
    /// `start`, for the first tick of a run) into `remaining`. A no-op while
    /// paused. When `remaining` reaches zero, advances to the next phase and
    /// pauses — each completed phase is an explicit checkpoint the user
    /// acknowledges with Start, not an unattended auto-cycling loop. Returns
    /// the transition on the one frame that happens, `None` every other
    /// frame (including every no-op case above).
    pub fn tick(&mut self, now: Instant, durations: &PomodoroDurations) -> Option<PhaseTransition> {
        if !self.running {
            return None;
        }
        let Some(last_tick) = self.last_tick else {
            self.last_tick = Some(now);
            return None;
        };
        let elapsed = now.saturating_duration_since(last_tick);
        self.last_tick = Some(now);
        if elapsed >= self.remaining {
            let completed = self.phase;
            self.advance_phase(durations);
            self.running = false;
            self.last_tick = None;
            Some(PhaseTransition {
                completed,
                next: self.phase,
            })
        } else {
            self.remaining -= elapsed;
            None
        }
    }

    fn advance_phase(&mut self, durations: &PomodoroDurations) {
        self.phase = match self.phase {
            PomodoroPhase::Work => {
                self.completed_work_sessions += 1;
                if self
                    .completed_work_sessions
                    .is_multiple_of(durations.cycles_before_long_break.max(1))
                {
                    PomodoroPhase::LongBreak
                } else {
                    PomodoroPhase::ShortBreak
                }
            }
            PomodoroPhase::ShortBreak | PomodoroPhase::LongBreak => PomodoroPhase::Work,
        };
        self.remaining = match self.phase {
            PomodoroPhase::Work => durations.work,
            PomodoroPhase::ShortBreak => durations.short_break,
            PomodoroPhase::LongBreak => durations.long_break,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn durations() -> PomodoroDurations {
        PomodoroDurations {
            work: Duration::from_secs(25 * 60),
            short_break: Duration::from_secs(5 * 60),
            long_break: Duration::from_secs(15 * 60),
            cycles_before_long_break: 4,
        }
    }

    #[test]
    fn new_state_starts_paused_in_work_phase_at_full_duration() {
        let d = durations();
        let state = PomodoroState::new(&d);
        assert_eq!(state.phase(), PomodoroPhase::Work);
        assert_eq!(state.remaining(), d.work);
        assert!(!state.is_running());
        assert!(!state.has_started());
    }

    #[test]
    fn ticking_while_paused_does_nothing() {
        let d = durations();
        let mut state = PomodoroState::new(&d);
        let now = Instant::now();
        state.tick(now + Duration::from_secs(60), &d);
        assert_eq!(state.remaining(), d.work);
    }

    #[test]
    fn ticking_while_running_reduces_remaining_time() {
        let d = durations();
        let mut state = PomodoroState::new(&d);
        let t0 = Instant::now();
        state.start(t0);
        state.tick(t0 + Duration::from_secs(60), &d);
        assert_eq!(state.remaining(), d.work - Duration::from_secs(60));
        assert!(state.is_running());
        assert!(state.has_started());
    }

    #[test]
    fn a_completed_work_phase_advances_to_short_break_and_pauses() {
        let d = durations();
        let mut state = PomodoroState::new(&d);
        let t0 = Instant::now();
        state.start(t0);
        state.tick(t0 + d.work + Duration::from_secs(1), &d);
        assert_eq!(state.phase(), PomodoroPhase::ShortBreak);
        assert_eq!(state.remaining(), d.short_break);
        assert!(!state.is_running());
        assert_eq!(state.completed_work_sessions(), 1);
    }

    #[test]
    fn tick_reports_a_phase_transition_only_the_frame_it_completes() {
        let d = durations();
        let mut state = PomodoroState::new(&d);
        let t0 = Instant::now();
        state.start(t0);

        assert_eq!(state.tick(t0 + Duration::from_secs(60), &d), None);

        let transition = state.tick(t0 + d.work + Duration::from_secs(1), &d);
        assert_eq!(
            transition,
            Some(PhaseTransition {
                completed: PomodoroPhase::Work,
                next: PomodoroPhase::ShortBreak,
            })
        );
    }

    #[test]
    fn every_fourth_work_session_is_followed_by_a_long_break() {
        let d = durations();
        let mut state = PomodoroState::new(&d);
        let mut t = Instant::now();

        // Work -> ShortBreak -> Work -> ShortBreak -> Work -> ShortBreak -> Work -> LongBreak
        for expected_after in [
            PomodoroPhase::ShortBreak,
            PomodoroPhase::Work,
            PomodoroPhase::ShortBreak,
            PomodoroPhase::Work,
            PomodoroPhase::ShortBreak,
            PomodoroPhase::Work,
            PomodoroPhase::LongBreak,
        ] {
            let full = state.remaining();
            state.start(t);
            t += full + Duration::from_secs(1);
            state.tick(t, &d);
            assert_eq!(state.phase(), expected_after);
        }
        assert_eq!(state.completed_work_sessions(), 4);
    }

    #[test]
    fn skip_advances_immediately_regardless_of_remaining_time() {
        let d = durations();
        let mut state = PomodoroState::new(&d);
        state.start(Instant::now());
        state.skip(&d);
        assert_eq!(state.phase(), PomodoroPhase::ShortBreak);
        assert_eq!(state.remaining(), d.short_break);
        assert!(!state.is_running());
    }

    #[test]
    fn reset_returns_to_a_fresh_work_phase_but_keeps_session_count() {
        let d = durations();
        let mut state = PomodoroState::new(&d);
        state.start(Instant::now());
        state.skip(&d); // now in ShortBreak, completed_work_sessions == 1
        state.reset(&d);
        assert_eq!(state.phase(), PomodoroPhase::Work);
        assert_eq!(state.remaining(), d.work);
        assert!(!state.is_running());
        assert_eq!(state.completed_work_sessions(), 1);
    }

    #[test]
    fn pause_then_resume_does_not_lose_or_double_count_elapsed_time() {
        let d = durations();
        let mut state = PomodoroState::new(&d);
        let t0 = Instant::now();
        state.start(t0);
        state.tick(t0 + Duration::from_secs(30), &d);
        state.pause();
        // Time passes while paused; ticking during the pause must not move
        // `remaining` at all.
        state.tick(t0 + Duration::from_secs(9999), &d);
        assert_eq!(state.remaining(), d.work - Duration::from_secs(30));

        let t1 = t0 + Duration::from_secs(9999);
        state.start(t1);
        state.tick(t1 + Duration::from_secs(30), &d);
        assert_eq!(state.remaining(), d.work - Duration::from_secs(60));
    }

    #[test]
    fn resolve_durations_falls_back_to_defaults_when_settings_are_unset() {
        let settings = Settings::default();
        let d = resolve_durations(&settings);
        assert_eq!(d.work, Duration::from_secs(25 * 60));
        assert_eq!(d.short_break, Duration::from_secs(5 * 60));
        assert_eq!(d.long_break, Duration::from_secs(15 * 60));
        assert_eq!(d.cycles_before_long_break, 4);
    }

    #[test]
    fn resolve_durations_uses_configured_values_when_set() {
        let settings = Settings {
            pomodoro_work_minutes: 50,
            pomodoro_short_break_minutes: 10,
            pomodoro_long_break_minutes: 30,
            pomodoro_cycles_before_long_break: 3,
            ..Default::default()
        };
        let d = resolve_durations(&settings);
        assert_eq!(d.work, Duration::from_secs(50 * 60));
        assert_eq!(d.short_break, Duration::from_secs(10 * 60));
        assert_eq!(d.long_break, Duration::from_secs(30 * 60));
        assert_eq!(d.cycles_before_long_break, 3);
    }
}
