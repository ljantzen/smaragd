//! Resolves the Pomodoro timer's configured durations from `Settings` — the
//! one piece of `smaragd_core::pomodoro` that needs a UI-crate type
//! (`Settings`), so it stays here rather than in `core`. The state machine
//! itself (`PomodoroState`, `PomodoroPhase`, ...) lives in `smaragd_core::pomodoro`
//! and is re-exported so existing call sites don't need to change.

pub use smaragd_core::pomodoro::*;

use std::time::Duration;

use crate::settings::Settings;

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

#[cfg(test)]
mod tests {
    use super::*;

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
