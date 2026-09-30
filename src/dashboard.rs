//! Pure aggregation logic for the Dashboard dock tab (`ui::dashboard_panel`):
//! session bookkeeping (`SessionRecord`) and the day/weekday/hour bucketing
//! functions its charts are built from. No egui dependency — same "logic
//! separate from rendering" split as `streak.rs`/`pomodoro.rs`.

use std::collections::BTreeMap;

use chrono::{Datelike, Local, NaiveDate, NaiveDateTime, Timelike};
use serde::{Deserialize, Serialize};

/// One completed app session against a project: opened when a project
/// becomes the active one (`Project::start_session`) and closed when it
/// stops being active — either the app shuts down or another project is
/// opened in its place (`Project::close_session`) — see
/// `ProjectMeta::session_log`. Timestamps are local-time, formatted
/// `%Y-%m-%dT%H:%M:%S` — a plain string rather than a `chrono` type, same
/// convention `ProjectMeta::session_baseline_date`/`daily_word_counts`
/// already use, for the same reason: `chrono`'s `Serialize`/`Deserialize`
/// impls need its `serde` cargo feature, which isn't enabled (see
/// Cargo.toml).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionRecord {
    pub started: String,
    pub ended: String,
    /// Words written during the session: the project's tracked word count
    /// (`ProjectMeta::word_count_scope`) at close time minus at start time,
    /// floored at 0 — a session that only deleted text logs no negative
    /// number.
    pub words_written: u32,
}

const TIMESTAMP_FORMAT: &str = "%Y-%m-%dT%H:%M:%S";

/// How many days of `ProjectMeta::session_log` history to retain — same
/// retention window and pruning rationale as
/// `streak::DAILY_HISTORY_RETENTION_DAYS`.
pub const SESSION_LOG_RETENTION_DAYS: i64 = 400;

/// The current local time, formatted for `SessionRecord::started`/`ended`
/// and `ProjectMeta::document_created`.
pub fn now_timestamp() -> String {
    Local::now().format(TIMESTAMP_FORMAT).to_string()
}

fn parse_timestamp(s: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(s, TIMESTAMP_FORMAT).ok()
}

/// A `std::time::SystemTime` (e.g. a document's on-disk mtime) converted to
/// local time — the bridge between `Project::document_modified_times`
/// (filesystem-sourced) and the same `NaiveDateTime` domain the timestamp
/// strings above parse into, so both can feed `count_by_day`.
pub fn system_time_to_local(time: std::time::SystemTime) -> NaiveDateTime {
    chrono::DateTime::<Local>::from(time).naive_local()
}

/// Drop every session whose `started` timestamp is older than
/// `SESSION_LOG_RETENTION_DAYS`, or fails to parse at all — defensive,
/// never panics on a hand-edited or corrupted `project.json`, same
/// tolerant-load philosophy `streak::prune_daily_history` follows.
pub fn prune_session_log(log: &mut Vec<SessionRecord>, today: NaiveDate) {
    let cutoff = today - chrono::Duration::days(SESSION_LOG_RETENTION_DAYS);
    log.retain(|session| parse_timestamp(&session.started).is_some_and(|dt| dt.date() >= cutoff));
}

/// Which quantity the Dashboard's day-of-week/hour-of-day activity charts
/// sum per bucket — a toggle the user switches live (`ui::dashboard_panel`),
/// not persisted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ActivityMetric {
    #[default]
    Words,
    Time,
}

/// A session's duration in whole minutes, or `None` if either timestamp
/// fails to parse. Never negative — `close_session` always writes `ended`
/// after `started`, but this clamps defensively rather than trusting that.
fn session_minutes(session: &SessionRecord) -> Option<u64> {
    let started = parse_timestamp(&session.started)?;
    let ended = parse_timestamp(&session.ended)?;
    Some((ended - started).num_minutes().max(0) as u64)
}

fn metric_value(session: &SessionRecord, metric: ActivityMetric) -> u64 {
    match metric {
        ActivityMetric::Words => u64::from(session.words_written),
        ActivityMetric::Time => session_minutes(session).unwrap_or(0),
    }
}

/// Sum of `metric` across every session in `log`, bucketed by the weekday
/// (Monday = index 0) its `started` timestamp falls on — never `ended`, so a
/// session spanning midnight counts entirely toward the day (and hour, see
/// `activity_by_hour`) it began in rather than being split across two
/// buckets. A session whose `started` fails to parse is skipped.
pub fn activity_by_weekday(log: &[SessionRecord], metric: ActivityMetric) -> [u64; 7] {
    let mut buckets = [0u64; 7];
    for session in log {
        if let Some(started) = parse_timestamp(&session.started) {
            buckets[started.weekday().num_days_from_monday() as usize] +=
                metric_value(session, metric);
        }
    }
    buckets
}

/// Same as `activity_by_weekday`, bucketed by the hour (0-23) `started`
/// falls in instead of the weekday.
pub fn activity_by_hour(log: &[SessionRecord], metric: ActivityMetric) -> [u64; 24] {
    let mut buckets = [0u64; 24];
    for session in log {
        if let Some(started) = parse_timestamp(&session.started) {
            buckets[started.hour() as usize] += metric_value(session, metric);
        }
    }
    buckets
}

/// Number of sessions started on each calendar date — the Dashboard's
/// "Sessions" bar chart.
pub fn sessions_per_day(log: &[SessionRecord]) -> BTreeMap<NaiveDate, u32> {
    let mut counts = BTreeMap::new();
    for session in log {
        if let Some(started) = parse_timestamp(&session.started) {
            *counts.entry(started.date()).or_insert(0) += 1;
        }
    }
    counts
}

/// Total session count / combined duration (whole minutes) / combined words
/// written across `log` — the Dashboard's summary row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DashboardSummary {
    pub session_count: usize,
    pub total_minutes: u64,
    pub total_words: u64,
}

pub fn summary(log: &[SessionRecord]) -> DashboardSummary {
    DashboardSummary {
        session_count: log.len(),
        total_minutes: log.iter().filter_map(session_minutes).sum(),
        total_words: log.iter().map(|s| u64::from(s.words_written)).sum(),
    }
}

/// Number of `dates` falling on each calendar date — shared aggregation for
/// the Dashboard's "Documents Created"/"Documents Modified" bar charts (the
/// latter fed by `Project::document_modified_times`, via
/// `system_time_to_local`).
pub fn count_by_day(dates: impl IntoIterator<Item = NaiveDateTime>) -> BTreeMap<NaiveDate, u32> {
    let mut counts = BTreeMap::new();
    for date in dates {
        *counts.entry(date.date()).or_insert(0) += 1;
    }
    counts
}

/// [`count_by_day`] over `ProjectMeta::document_created`-style timestamp
/// strings, skipping anything unparseable.
pub fn count_by_day_strings<'a>(
    timestamps: impl IntoIterator<Item = &'a String>,
) -> BTreeMap<NaiveDate, u32> {
    count_by_day(timestamps.into_iter().filter_map(|s| parse_timestamp(s)))
}

/// The last `n` calendar dates up to and including `today`, oldest first —
/// the fixed x-axis window the Dashboard's daily bar charts are drawn
/// against, so a day with no activity still shows as an explicit
/// zero-height bar rather than a gap.
pub fn last_n_days(today: NaiveDate, n: i64) -> Vec<NaiveDate> {
    (0..n)
        .rev()
        .map(|offset| today - chrono::Duration::days(offset))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn session(started: &str, ended: &str, words: u32) -> SessionRecord {
        SessionRecord {
            started: started.to_string(),
            ended: ended.to_string(),
            words_written: words,
        }
    }

    #[test]
    fn prune_session_log_drops_entries_older_than_the_retention_window() {
        let today = date(2024, 6, 1);
        let cutoff = today - chrono::Duration::days(SESSION_LOG_RETENTION_DAYS);
        let mut log = vec![
            session(
                &format!("{cutoff}T09:00:00"),
                &format!("{cutoff}T10:00:00"),
                100,
            ),
            session(
                &format!("{}T09:00:00", cutoff - chrono::Duration::days(1)),
                &format!("{}T10:00:00", cutoff - chrono::Duration::days(1)),
                100,
            ),
            session(
                &format!("{today}T09:00:00"),
                &format!("{today}T10:00:00"),
                100,
            ),
        ];

        prune_session_log(&mut log, today);

        assert_eq!(log.len(), 2);
    }

    #[test]
    fn prune_session_log_drops_unparseable_entries() {
        let mut log = vec![session("not a timestamp", "also not", 100)];

        prune_session_log(&mut log, date(2024, 1, 1));

        assert!(log.is_empty());
    }

    #[test]
    fn activity_by_weekday_buckets_by_the_start_timestamps_weekday() {
        // 2024-01-08 is a Monday.
        let log = vec![
            session("2024-01-08T09:00:00", "2024-01-08T10:00:00", 500),
            session("2024-01-10T09:00:00", "2024-01-10T10:30:00", 300),
        ];

        let words = activity_by_weekday(&log, ActivityMetric::Words);
        assert_eq!(words[0], 500); // Monday
        assert_eq!(words[2], 300); // Wednesday

        let minutes = activity_by_weekday(&log, ActivityMetric::Time);
        assert_eq!(minutes[0], 60);
        assert_eq!(minutes[2], 90);
    }

    #[test]
    fn a_session_spanning_midnight_counts_entirely_toward_its_start_day_and_hour() {
        let log = vec![session("2024-01-08T23:30:00", "2024-01-09T00:45:00", 200)];

        let weekday = activity_by_weekday(&log, ActivityMetric::Words);
        assert_eq!(weekday[0], 200); // Monday (the 8th), not Tuesday

        let hour = activity_by_hour(&log, ActivityMetric::Words);
        assert_eq!(hour[23], 200);
        assert_eq!(hour[0], 0);
    }

    #[test]
    fn activity_by_hour_buckets_by_the_start_timestamps_hour() {
        let log = vec![
            session("2024-01-08T09:15:00", "2024-01-08T09:45:00", 200),
            session("2024-01-09T09:00:00", "2024-01-09T09:30:00", 100),
        ];

        let words = activity_by_hour(&log, ActivityMetric::Words);
        assert_eq!(words[9], 300);
        assert_eq!(words[10], 0);
    }

    #[test]
    fn empty_log_yields_all_zero_buckets_and_a_default_summary() {
        assert_eq!(activity_by_weekday(&[], ActivityMetric::Words), [0; 7]);
        assert_eq!(activity_by_hour(&[], ActivityMetric::Time), [0; 24]);
        assert_eq!(summary(&[]), DashboardSummary::default());
    }

    #[test]
    fn summary_totals_sessions_minutes_and_words() {
        let log = vec![
            session("2024-01-08T09:00:00", "2024-01-08T09:30:00", 500),
            session("2024-01-09T09:00:00", "2024-01-09T10:00:00", 300),
        ];

        let result = summary(&log);

        assert_eq!(result.session_count, 2);
        assert_eq!(result.total_minutes, 90);
        assert_eq!(result.total_words, 800);
    }

    #[test]
    fn sessions_per_day_counts_multiple_sessions_on_the_same_day() {
        let log = vec![
            session("2024-01-08T09:00:00", "2024-01-08T09:30:00", 100),
            session("2024-01-08T14:00:00", "2024-01-08T14:30:00", 100),
            session("2024-01-09T09:00:00", "2024-01-09T09:30:00", 100),
        ];

        let counts = sessions_per_day(&log);

        assert_eq!(counts.get(&date(2024, 1, 8)), Some(&2));
        assert_eq!(counts.get(&date(2024, 1, 9)), Some(&1));
    }

    #[test]
    fn count_by_day_strings_skips_unparseable_timestamps() {
        let timestamps = vec![
            "2024-01-08T09:00:00".to_string(),
            "garbage".to_string(),
            "2024-01-08T14:00:00".to_string(),
        ];

        let counts = count_by_day_strings(&timestamps);

        assert_eq!(counts.get(&date(2024, 1, 8)), Some(&2));
        assert_eq!(counts.len(), 1);
    }

    #[test]
    fn last_n_days_returns_n_consecutive_dates_ending_today() {
        let days = last_n_days(date(2024, 1, 10), 3);

        assert_eq!(
            days,
            vec![date(2024, 1, 8), date(2024, 1, 9), date(2024, 1, 10)]
        );
    }
}
