//! Writing-streak evaluation: pure logic, no egui dependency — same
//! "logic separate from rendering" split as `pomodoro.rs`/`search.rs`.
//! `ui::streak_panel` and `app::mod`'s status bar render the result;
//! neither of them (nor this module) knows about egui colors — that
//! mapping lives at each rendering call site.
//!
//! The user configures a weekly word-count schedule
//! ([`WeeklySchedule`]) and how strictly a week counts as "met"
//! ([`StreakEvaluationMode`]) per project (`ProjectMeta`, not `Settings` —
//! different projects can reasonably want different paces). Per-project
//! daily word totals (`ProjectMeta::daily_word_counts`) are compared against
//! that schedule by [`evaluate_streak`], which judges only fully-completed
//! Mon-Sun weeks — never the still-in-progress current week — so the
//! traffic light can't turn red before a week has even had a chance to
//! finish (e.g. Monday morning).
//!
//! This module deliberately has no dependency on `project`/`settings` types
//! (`resolve_streak_config` takes plain values, not a `&ProjectMeta`) —
//! `project::word_count` already depends on this module (for
//! `prune_daily_history`), so keeping the dependency one-directional avoids
//! a needless mutual coupling between the two.

use std::collections::BTreeMap;

use chrono::{Datelike, NaiveDate, Weekday};
use serde::{Deserialize, Serialize};

/// A word-count target for each day of the week. Named fields rather than
/// `HashMap<Weekday, u32>` or `[u32; 7]`: a map keyed by an enum variant is
/// a TOML round-trip risk (see `ShortcutMap`'s doc comment in
/// `shortcuts.rs` for why), and a bare array loses the on-disk
/// self-documentation named fields give (`monday = 500` vs. an opaque
/// `[0, 500, ...]`). `0` is a legitimate "day off" value here, not an
/// unconfigured marker — unlike most `Settings` fields, this struct has no
/// blank-means-unset resolver, since an all-zero schedule is a valid (if
/// degenerate) configuration in its own right.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct WeeklySchedule {
    pub monday: u32,
    pub tuesday: u32,
    pub wednesday: u32,
    pub thursday: u32,
    pub friday: u32,
    pub saturday: u32,
    pub sunday: u32,
}

impl WeeklySchedule {
    pub fn target_for(&self, weekday: Weekday) -> u32 {
        match weekday {
            Weekday::Mon => self.monday,
            Weekday::Tue => self.tuesday,
            Weekday::Wed => self.wednesday,
            Weekday::Thu => self.thursday,
            Weekday::Fri => self.friday,
            Weekday::Sat => self.saturday,
            Weekday::Sun => self.sunday,
        }
    }
}

/// How a week is judged "met". User-configurable per project (the Streak
/// dock tab) — deliberately not hardcoded, since a big Saturday covering a
/// missed Tuesday is a legitimate writing rhythm for some and not others.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum StreakEvaluationMode {
    /// Sum of actual words written Mon-Sun >= sum of that week's targets.
    #[default]
    CumulativeWeekly,
    /// Every day with a nonzero target must individually meet it that day.
    /// A day with a `0` target (day off) never counts as a miss.
    PerDayStrict,
}

/// The resolved, ready-to-evaluate bundle of streak settings — mirrors
/// `pomodoro::PomodoroDurations`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreakConfig {
    pub enabled: bool,
    pub schedule: WeeklySchedule,
    pub mode: StreakEvaluationMode,
    pub red_threshold_weeks: u32,
}

const DEFAULT_RED_THRESHOLD_WEEKS: u32 = 2;

/// `red_threshold_weeks == 0` (i.e. `ProjectMeta::streak_red_threshold_weeks`)
/// means "not yet configured," resolved to [`DEFAULT_RED_THRESHOLD_WEEKS`]
/// here — same blank-means-unset convention as `pomodoro::resolve_durations`.
pub fn resolve_streak_config(
    enabled: bool,
    schedule: WeeklySchedule,
    mode: StreakEvaluationMode,
    red_threshold_weeks: u32,
) -> StreakConfig {
    StreakConfig {
        enabled,
        schedule,
        mode,
        red_threshold_weeks: if red_threshold_weeks > 0 {
            red_threshold_weeks
        } else {
            DEFAULT_RED_THRESHOLD_WEEKS
        },
    }
}

/// The traffic-light state a caller renders. `Disabled` is never produced
/// by [`evaluate_streak`] itself — it's for callers to short-circuit to
/// before even calling it (feature off, or no project open), so every
/// consumer matches one enum rather than juggling an `Option` on top.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreakStatus {
    Disabled,
    /// Enabled, but no fully-completed week has any tracked history yet
    /// (brand-new project, or the feature was just turned on) — kept
    /// visually distinct from `Green` so day one never looks misleadingly
    /// "on track."
    InsufficientData,
    Green,
    Yellow,
    Red,
}

/// How many days of `ProjectMeta::daily_word_counts` history to retain.
/// ~57 weeks — comfortably covers any sane `red_threshold_weeks` (even a
/// generous 12-week threshold needs at most ~13 weeks of lookback) with a
/// large safety margin, while keeping `project.json`'s growth bounded
/// (worst case ~400 short `"YYYY-MM-DD": N` entries, a few KB).
pub const DAILY_HISTORY_RETENTION_DAYS: i64 = 400;

/// Drops every entry older than [`DAILY_HISTORY_RETENTION_DAYS`] (and any
/// entry whose key isn't a valid `%Y-%m-%d` date, defensively — never
/// panics on a hand-edited or corrupted `project.json`). Called from
/// `Project::maybe_roll_over_session` on every rollover, so history never
/// grows unbounded without needing a separate cleanup pass.
pub fn prune_daily_history(history: &mut BTreeMap<String, u32>, today: NaiveDate) {
    let cutoff = today - chrono::Duration::days(DAILY_HISTORY_RETENTION_DAYS);
    history.retain(|key, _| {
        NaiveDate::parse_from_str(key, "%Y-%m-%d").is_ok_and(|date| date >= cutoff)
    });
}

/// The Monday of the Mon-Sun week `date` falls in. Exposed (not just an
/// internal helper of [`evaluate_streak`]) because `ui::streak_panel` needs
/// the same calculation to render the current week's live progress.
pub fn week_monday(date: NaiveDate) -> NaiveDate {
    date - chrono::Duration::days(i64::from(date.weekday().num_days_from_monday()))
}

/// `(actual words so far, target sum)` for the Mon-Sun week containing
/// `today` — the still-in-progress current week, unlike [`evaluate_streak`]
/// which only ever judges completed ones. Shared by `ui::streak_panel` (the
/// "Progress this week" bar) and `app::mod`'s status bar glyph (a compact
/// percentage) so the two never disagree. `today_words_so_far` is folded in
/// for `today` itself since `ProjectMeta::daily_word_counts` doesn't get
/// today's entry until the next day's rollover (see
/// `Project::maybe_roll_over_session`) — every earlier day in the week comes
/// from `history` instead.
pub fn current_week_progress(
    schedule: &WeeklySchedule,
    history: &BTreeMap<String, u32>,
    today_words_so_far: u32,
    today: NaiveDate,
) -> (u32, u32) {
    let this_monday = week_monday(today);
    let mut actual_so_far = 0u32;
    let mut target_sum = 0u32;
    for offset in 0..7i64 {
        let date = this_monday + chrono::Duration::days(offset);
        target_sum += schedule.target_for(date.weekday());
        match date.cmp(&today) {
            std::cmp::Ordering::Equal => actual_so_far += today_words_so_far,
            std::cmp::Ordering::Less => {
                actual_so_far += history
                    .get(&date.format("%Y-%m-%d").to_string())
                    .copied()
                    .unwrap_or(0);
            }
            std::cmp::Ordering::Greater => {}
        }
    }
    (actual_so_far, target_sum)
}

fn week_pass(
    schedule: &WeeklySchedule,
    mode: StreakEvaluationMode,
    history: &BTreeMap<String, u32>,
    week_monday: NaiveDate,
) -> bool {
    let actual_on = |date: NaiveDate| -> u32 {
        history
            .get(&date.format("%Y-%m-%d").to_string())
            .copied()
            .unwrap_or(0)
    };
    match mode {
        StreakEvaluationMode::CumulativeWeekly => {
            let mut actual_sum = 0u32;
            let mut target_sum = 0u32;
            for offset in 0..7 {
                let date = week_monday + chrono::Duration::days(offset);
                target_sum += schedule.target_for(date.weekday());
                actual_sum += actual_on(date);
            }
            actual_sum >= target_sum
        }
        StreakEvaluationMode::PerDayStrict => (0..7).all(|offset| {
            let date = week_monday + chrono::Duration::days(offset);
            let target = schedule.target_for(date.weekday());
            target == 0 || actual_on(date) >= target
        }),
    }
}

/// The core status computation. Pure and deterministic — `today` is a
/// parameter rather than read from the wall clock internally, so it's
/// fully unit-testable; real call sites pass
/// `chrono::Local::now().date_naive()`.
///
/// Judges only fully-completed Mon-Sun weeks, walking backwards from the
/// most recently completed one. A week only counts as "judgeable" if
/// tracking had already begun by its Monday (i.e. its Monday isn't earlier
/// than the week-Monday containing `history`'s earliest entry) — this
/// correctly excludes the partial pre-tracking days of the very first week
/// a project ever logs anything, without needing a separate "tracking
/// started on" marker field. Today's/this week's in-progress numbers never
/// enter this computation at all.
pub fn evaluate_streak(
    schedule: &WeeklySchedule,
    mode: StreakEvaluationMode,
    red_threshold_weeks: u32,
    history: &BTreeMap<String, u32>,
    today: NaiveDate,
) -> StreakStatus {
    let red_threshold_weeks = red_threshold_weeks.max(1);
    let Some(earliest) = history
        .keys()
        .filter_map(|key| NaiveDate::parse_from_str(key, "%Y-%m-%d").ok())
        .min()
    else {
        return StreakStatus::InsufficientData;
    };
    let earliest_week_monday = week_monday(earliest);
    let this_monday = week_monday(today);

    let mut passes = Vec::new();
    let mut cursor = this_monday - chrono::Duration::days(7);
    while cursor >= earliest_week_monday && (passes.len() as u32) < red_threshold_weeks {
        passes.push(week_pass(schedule, mode, history, cursor));
        cursor -= chrono::Duration::days(7);
    }

    let Some(&most_recent_pass) = passes.first() else {
        return StreakStatus::InsufficientData;
    };
    if most_recent_pass {
        return StreakStatus::Green;
    }
    if passes.len() as u32 >= red_threshold_weeks && passes.iter().all(|pass| !pass) {
        StreakStatus::Red
    } else {
        StreakStatus::Yellow
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    /// 500 words every day, Monday through Sunday.
    fn uniform_schedule() -> WeeklySchedule {
        WeeklySchedule {
            monday: 500,
            tuesday: 500,
            wednesday: 500,
            thursday: 500,
            friday: 500,
            saturday: 500,
            sunday: 500,
        }
    }

    /// 500 words Monday-Saturday, Sunday off.
    fn weekday_only_schedule() -> WeeklySchedule {
        WeeklySchedule {
            sunday: 0,
            ..uniform_schedule()
        }
    }

    fn history_for(days: &[(NaiveDate, u32)]) -> BTreeMap<String, u32> {
        days.iter()
            .map(|(d, words)| (d.format("%Y-%m-%d").to_string(), *words))
            .collect()
    }

    // 2024-01-01 was a Monday, so:
    //   week 1: Jan 1-7      week 2: Jan 8-14      week 3: Jan 15-21

    #[test]
    fn empty_history_is_insufficient_data() {
        let status = evaluate_streak(
            &uniform_schedule(),
            StreakEvaluationMode::CumulativeWeekly,
            2,
            &BTreeMap::new(),
            date(2024, 1, 15),
        );
        assert_eq!(status, StreakStatus::InsufficientData);
    }

    #[test]
    fn history_only_inside_the_current_incomplete_week_is_insufficient_data() {
        let history = history_for(&[(date(2024, 1, 15), 500)]); // today, current week
        let status = evaluate_streak(
            &uniform_schedule(),
            StreakEvaluationMode::CumulativeWeekly,
            2,
            &history,
            date(2024, 1, 15),
        );
        assert_eq!(status, StreakStatus::InsufficientData);
    }

    #[test]
    fn a_completed_week_meeting_the_cumulative_target_is_green() {
        let history = history_for(&[
            (date(2024, 1, 8), 500),
            (date(2024, 1, 9), 500),
            (date(2024, 1, 10), 500),
            (date(2024, 1, 11), 500),
            (date(2024, 1, 12), 500),
            (date(2024, 1, 13), 500),
            (date(2024, 1, 14), 500),
        ]);
        let status = evaluate_streak(
            &uniform_schedule(),
            StreakEvaluationMode::CumulativeWeekly,
            2,
            &history,
            date(2024, 1, 15),
        );
        assert_eq!(status, StreakStatus::Green);
    }

    #[test]
    fn a_completed_week_falling_short_of_the_cumulative_target_is_yellow() {
        let history = history_for(&[
            (date(2024, 1, 8), 400),
            (date(2024, 1, 9), 400),
            (date(2024, 1, 10), 400),
            (date(2024, 1, 11), 400),
            (date(2024, 1, 12), 400),
            (date(2024, 1, 13), 400),
            (date(2024, 1, 14), 400),
        ]);
        let status = evaluate_streak(
            &uniform_schedule(),
            StreakEvaluationMode::CumulativeWeekly,
            2,
            &history,
            date(2024, 1, 15),
        );
        assert_eq!(status, StreakStatus::Yellow);
    }

    #[test]
    fn two_consecutive_missed_weeks_hits_the_default_red_threshold() {
        let history = history_for(&[(date(2024, 1, 1), 100), (date(2024, 1, 8), 100)]);
        let status = evaluate_streak(
            &uniform_schedule(),
            StreakEvaluationMode::CumulativeWeekly,
            2,
            &history,
            date(2024, 1, 15),
        );
        assert_eq!(status, StreakStatus::Red);
    }

    #[test]
    fn a_pass_earlier_in_the_lookback_window_breaks_a_red_streak_into_yellow() {
        // Most recent completed week (Jan 8-14) missed, but the one before
        // it (Jan 1-7) met target — the streak of misses is broken.
        let history = history_for(&[
            (date(2024, 1, 1), 3500), // full week 1 target
            (date(2024, 1, 8), 100),  // week 2 falls short
        ]);
        let status = evaluate_streak(
            &uniform_schedule(),
            StreakEvaluationMode::CumulativeWeekly,
            2,
            &history,
            date(2024, 1, 15),
        );
        assert_eq!(status, StreakStatus::Yellow);
    }

    #[test]
    fn a_higher_red_threshold_needs_more_consecutive_misses() {
        let history = history_for(&[(date(2024, 1, 1), 100), (date(2024, 1, 8), 100)]);
        let status = evaluate_streak(
            &uniform_schedule(),
            StreakEvaluationMode::CumulativeWeekly,
            3,
            &history,
            date(2024, 1, 15),
        );
        assert_eq!(status, StreakStatus::Yellow);
    }

    #[test]
    fn a_red_threshold_of_one_turns_red_on_a_single_missed_week() {
        let history = history_for(&[(date(2024, 1, 8), 100)]);
        let status = evaluate_streak(
            &uniform_schedule(),
            StreakEvaluationMode::CumulativeWeekly,
            1,
            &history,
            date(2024, 1, 15),
        );
        assert_eq!(status, StreakStatus::Red);
    }

    #[test]
    fn per_day_strict_is_green_when_every_nonzero_target_day_is_individually_met() {
        let history = history_for(&[
            (date(2024, 1, 8), 500),
            (date(2024, 1, 9), 500),
            (date(2024, 1, 10), 500),
            (date(2024, 1, 11), 500),
            (date(2024, 1, 12), 500),
            (date(2024, 1, 13), 500),
            // Sunday Jan 14 has a 0 target (day off) and no logged words.
        ]);
        let status = evaluate_streak(
            &weekday_only_schedule(),
            StreakEvaluationMode::PerDayStrict,
            2,
            &history,
            date(2024, 1, 15),
        );
        assert_eq!(status, StreakStatus::Green);
    }

    #[test]
    fn per_day_strict_is_yellow_when_one_nonzero_target_day_falls_short() {
        let history = history_for(&[
            (date(2024, 1, 8), 500),
            (date(2024, 1, 9), 500),
            (date(2024, 1, 10), 400), // short
            (date(2024, 1, 11), 500),
            (date(2024, 1, 12), 500),
            (date(2024, 1, 13), 500),
        ]);
        let status = evaluate_streak(
            &weekday_only_schedule(),
            StreakEvaluationMode::PerDayStrict,
            2,
            &history,
            date(2024, 1, 15),
        );
        assert_eq!(status, StreakStatus::Yellow);
    }

    #[test]
    fn a_day_absent_from_history_with_a_nonzero_target_counts_as_a_miss() {
        // Same as the "falls short" case above, but the day is missing
        // entirely rather than explicitly logged low.
        let history = history_for(&[
            (date(2024, 1, 8), 500),
            (date(2024, 1, 9), 500),
            // Jan 10 never logged at all.
            (date(2024, 1, 11), 500),
            (date(2024, 1, 12), 500),
            (date(2024, 1, 13), 500),
        ]);
        let status = evaluate_streak(
            &weekday_only_schedule(),
            StreakEvaluationMode::PerDayStrict,
            2,
            &history,
            date(2024, 1, 15),
        );
        assert_eq!(status, StreakStatus::Yellow);

        let cumulative_status = evaluate_streak(
            &weekday_only_schedule(),
            StreakEvaluationMode::CumulativeWeekly,
            2,
            &history,
            date(2024, 1, 15),
        );
        assert_eq!(cumulative_status, StreakStatus::Yellow);
    }

    #[test]
    fn today_being_the_monday_or_sunday_of_the_current_week_yields_the_same_completed_week() {
        // Week 2 (Jan 8-14) is fully green either way, since the current
        // week (week 3, Jan 15-21) hasn't completed yet on either date.
        let history = history_for(&[(date(2024, 1, 8), 3500)]);
        let on_monday = evaluate_streak(
            &uniform_schedule(),
            StreakEvaluationMode::CumulativeWeekly,
            2,
            &history,
            date(2024, 1, 15),
        );
        let on_sunday = evaluate_streak(
            &uniform_schedule(),
            StreakEvaluationMode::CumulativeWeekly,
            2,
            &history,
            date(2024, 1, 21),
        );
        assert_eq!(on_monday, StreakStatus::Green);
        assert_eq!(on_sunday, StreakStatus::Green);

        // The next Monday, week 3 (Jan 15-21, no logged words) becomes the
        // most recently completed week, so the status changes.
        let on_next_monday = evaluate_streak(
            &uniform_schedule(),
            StreakEvaluationMode::CumulativeWeekly,
            2,
            &history,
            date(2024, 1, 22),
        );
        assert_eq!(on_next_monday, StreakStatus::Yellow);
    }

    #[test]
    fn prune_daily_history_drops_entries_older_than_the_retention_window() {
        let today = date(2024, 6, 1);
        let cutoff = today - chrono::Duration::days(DAILY_HISTORY_RETENTION_DAYS);
        let mut history = history_for(&[
            (cutoff, 100),                             // boundary: kept
            (cutoff - chrono::Duration::days(1), 100), // just past: dropped
            (today, 200),                              // recent: kept
        ]);
        prune_daily_history(&mut history, today);
        assert_eq!(history.len(), 2);
        assert!(history.contains_key(&cutoff.format("%Y-%m-%d").to_string()));
        assert!(history.contains_key(&today.format("%Y-%m-%d").to_string()));
    }
}
