//! Phase 1 proof-of-concept: the first real command+event round trip through
//! Tauri IPC, using the actual `smaragd_core::pomodoro` state machine (not
//! throwaway demo code) so this exercises the real pattern later panels will
//! follow. See core/src/pomodoro.rs for the state machine itself.
//!
//! Ticking happens on a 1Hz background thread regardless of whether the
//! frontend is listening — same "keeps running while the panel is closed"
//! behavior the egui app has today — and pushes a snapshot event on every
//! tick rather than making the frontend poll a command in a timer loop (see
//! the migration plan's §2.1 on why: polling across the IPC boundary adds
//! latency a push doesn't have).

use std::sync::Mutex;
use std::time::Instant;

use serde::Serialize;
use smaragd_core::pomodoro::{PomodoroDurations, PomodoroPhase, PomodoroState};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_notification::NotificationExt;

const TICK_EVENT: &str = "pomodoro://tick";

pub struct PomodoroHandle {
    state: Mutex<PomodoroState>,
    durations: PomodoroDurations,
}

impl PomodoroHandle {
    /// Phase 1 scaffold uses the built-in defaults (25/5/15/4) rather than
    /// reading `Settings` — `Settings` is still egui-coupled in the `app`
    /// crate (see the Phase 0 notes) and hasn't been ported here yet.
    fn default_durations() -> PomodoroDurations {
        PomodoroDurations {
            work: std::time::Duration::from_secs(25 * 60),
            short_break: std::time::Duration::from_secs(5 * 60),
            long_break: std::time::Duration::from_secs(15 * 60),
            cycles_before_long_break: 4,
        }
    }
}

impl Default for PomodoroHandle {
    fn default() -> Self {
        let durations = Self::default_durations();
        Self {
            state: Mutex::new(PomodoroState::new(&durations)),
            durations,
        }
    }
}

#[derive(Serialize, Clone)]
pub struct PomodoroSnapshot {
    phase: &'static str,
    remaining_secs: u64,
    running: bool,
    completed_work_sessions: u32,
}

fn phase_label(phase: PomodoroPhase) -> &'static str {
    phase.label()
}

fn snapshot(handle: &PomodoroHandle) -> PomodoroSnapshot {
    let state = handle.state.lock().expect("pomodoro state mutex poisoned");
    PomodoroSnapshot {
        phase: phase_label(state.phase()),
        remaining_secs: state.remaining().as_secs(),
        running: state.is_running(),
        completed_work_sessions: state.completed_work_sessions(),
    }
}

#[tauri::command]
pub fn pomodoro_snapshot(handle: State<PomodoroHandle>) -> PomodoroSnapshot {
    snapshot(&handle)
}

#[tauri::command]
pub fn pomodoro_start(handle: State<PomodoroHandle>) {
    handle
        .state
        .lock()
        .expect("pomodoro state mutex poisoned")
        .start(Instant::now());
}

#[tauri::command]
pub fn pomodoro_pause(handle: State<PomodoroHandle>) {
    handle
        .state
        .lock()
        .expect("pomodoro state mutex poisoned")
        .pause();
}

#[tauri::command]
pub fn pomodoro_skip(handle: State<PomodoroHandle>) {
    let mut state = handle.state.lock().expect("pomodoro state mutex poisoned");
    state.skip(&handle.durations);
}

/// Spawns the 1Hz background ticker. Called once from `run()` at startup.
///
/// Phase 7: `notify-rust` -> `tauri-plugin-notification`, the one native
/// integration swap from the plan's §4 list that already had a concrete,
/// single call site to mirror exactly (see app/src/notifications.rs's doc
/// comment: Pomodoro phase changes are its *only* user). `tick()` already
/// returns `Option<PhaseTransition>` on exactly the frame a phase completes
/// on its own (never on `skip` — see its doc comment) for precisely this.
pub fn spawn_ticker(app: AppHandle) {
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
            let handle = app.state::<PomodoroHandle>();
            let transition = {
                let mut state = handle.state.lock().expect("pomodoro state mutex poisoned");
                state.tick(Instant::now(), &handle.durations)
            };
            if let Some(transition) = transition {
                let _ = app
                    .notification()
                    .builder()
                    .title(format!("{} complete", transition.completed.label()))
                    .body(format!("Starting {}", transition.next.label()))
                    .show();
            }
            let payload = snapshot(&handle);
            let _ = app.emit(TICK_EVENT, payload);
        }
    });
}
