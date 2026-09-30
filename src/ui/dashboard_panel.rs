use egui_plot::{Bar, BarChart, Plot};

use crate::dashboard::{self, ActivityMetric};
use crate::project::Project;

/// How many trailing calendar days the daily bar charts (word count,
/// sessions, documents created/modified) cover — long enough to show a real
/// trend, short enough that each bar stays legible without needing pan/zoom
/// (which are deliberately disabled — see `day_bar_chart`).
const DAY_WINDOW: i64 = 60;

/// Renders the Dashboard dock tab: writing statistics as they evolve over
/// time, text and graphs (GH #15) — session count/length, word count,
/// documents created/modified, and two activity-pattern charts (by day of
/// week, by hour of day). Read-only: unlike `word_count_panel`/
/// `streak_panel`, nothing here is user-editable, so there's no event type
/// or `DockAction` — `metric` is the one piece of UI-local state (the
/// Words/Time toggle for the two activity charts), mutated in place the same
/// way `streak_sub_tab`/`belief_timeline_character` are.
pub fn show(ui: &mut egui::Ui, project: &Project, metric: &mut ActivityMetric) {
    ui.heading("Dashboard");
    ui.separator();

    let summary = dashboard::summary(&project.meta.session_log);
    egui::Grid::new("dashboard_summary_grid")
        .num_columns(2)
        .show(ui, |ui| {
            ui.label("Sessions:");
            ui.label(summary.session_count.to_string());
            ui.end_row();
            ui.label("Total writing time:");
            ui.label(format_minutes(summary.total_minutes));
            ui.end_row();
            ui.label("Words written across sessions:");
            ui.label(summary.total_words.to_string());
            ui.end_row();
            ui.label("Documents created (all time):");
            ui.label(project.meta.document_created.len().to_string());
            ui.end_row();
        });

    let today = chrono::Local::now().date_naive();
    let days = dashboard::last_n_days(today, DAY_WINDOW);

    ui.add_space(12.0);
    ui.separator();
    ui.label(format!("Word count, last {DAY_WINDOW} days:"));
    let word_counts: Vec<u32> = days
        .iter()
        .map(|date| {
            project
                .meta
                .daily_word_counts
                .get(&date.format("%Y-%m-%d").to_string())
                .copied()
                .unwrap_or(0)
        })
        .collect();
    day_bar_chart(ui, "dashboard_word_count_chart", &days, &word_counts);

    ui.add_space(12.0);
    ui.label(format!("Sessions, last {DAY_WINDOW} days:"));
    let sessions_by_day = dashboard::sessions_per_day(&project.meta.session_log);
    let session_counts: Vec<u32> = days
        .iter()
        .map(|date| sessions_by_day.get(date).copied().unwrap_or(0))
        .collect();
    day_bar_chart(ui, "dashboard_sessions_chart", &days, &session_counts);

    ui.add_space(12.0);
    ui.label(format!("Documents created, last {DAY_WINDOW} days:"));
    let created_by_day = dashboard::count_by_day_strings(project.meta.document_created.values());
    let created_counts: Vec<u32> = days
        .iter()
        .map(|date| created_by_day.get(date).copied().unwrap_or(0))
        .collect();
    day_bar_chart(ui, "dashboard_created_chart", &days, &created_counts);

    ui.add_space(12.0);
    ui.label(format!("Documents modified, last {DAY_WINDOW} days:"));
    show_modified_chart(ui, project, &days);

    ui.add_space(16.0);
    ui.separator();
    ui.horizontal(|ui| {
        ui.label("Activity by:");
        ui.selectable_value(metric, ActivityMetric::Words, "Words");
        ui.selectable_value(metric, ActivityMetric::Time, "Time (minutes)");
    });

    ui.add_space(4.0);
    ui.label("Activity by day of week:");
    let weekday_values = dashboard::activity_by_weekday(&project.meta.session_log, *metric);
    labeled_bar_chart(
        ui,
        "dashboard_weekday_chart",
        &weekday_values,
        &WEEKDAY_LABELS,
    );

    ui.add_space(12.0);
    ui.label("Activity by hour of day:");
    let hour_values = dashboard::activity_by_hour(&project.meta.session_log, *metric);
    let hour_labels: Vec<String> = (0..24).map(|h| h.to_string()).collect();
    labeled_bar_chart(ui, "dashboard_hour_chart", &hour_values, &hour_labels);
}

const WEEKDAY_LABELS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

fn show_modified_chart(ui: &mut egui::Ui, project: &Project, days: &[chrono::NaiveDate]) {
    // Read live from disk rather than a persisted field — see
    // `Project::document_modified_times`'s doc comment for why. A plain
    // `fs::metadata` stat per tracked document (no file content read,
    // unlike `word_count`) is cheap enough to call directly here rather
    // than needing its own background-thread cache.
    let modified_by_day = dashboard::count_by_day(
        project
            .document_modified_times()
            .into_values()
            .map(dashboard::system_time_to_local),
    );
    let counts: Vec<u32> = days
        .iter()
        .map(|date| modified_by_day.get(date).copied().unwrap_or(0))
        .collect();
    day_bar_chart(ui, "dashboard_modified_chart", days, &counts);
}

/// A bar chart over `days` (index i -> `values[i]`), with the x-axis grid
/// labeled by date (`"Jan 5"`) rather than a bare index. Panning/zooming are
/// disabled: this is a fixed, at-a-glance report, not an interactive
/// explorer — the last `DAY_WINDOW` days is always the whole story.
fn day_bar_chart(
    ui: &mut egui::Ui,
    id_source: &'static str,
    days: &[chrono::NaiveDate],
    values: &[u32],
) {
    let bars: Vec<Bar> = values
        .iter()
        .enumerate()
        .map(|(i, v)| Bar::new(i as f64, f64::from(*v)))
        .collect();
    let chart = BarChart::new(id_source, bars).color(ui.visuals().selection.bg_fill);
    let labels: Vec<String> = days
        .iter()
        .map(|d| d.format("%b %-d").to_string())
        .collect();
    Plot::new(id_source)
        .height(120.0)
        .allow_zoom(false)
        .allow_scroll(false)
        .allow_boxed_zoom(false)
        .allow_drag(false)
        .include_y(0.0)
        .x_axis_formatter(move |mark, _range| {
            labels
                .get(mark.value.round() as usize)
                .cloned()
                .unwrap_or_default()
        })
        .show(ui, |plot_ui| plot_ui.bar_chart(chart));
}

/// Same as `day_bar_chart`, but for a fixed-size, already-labeled bucket
/// array (weekday names or hour numbers) rather than a date window.
fn labeled_bar_chart(
    ui: &mut egui::Ui,
    id_source: &'static str,
    values: &[u64],
    labels: &[impl AsRef<str>],
) {
    let bars: Vec<Bar> = values
        .iter()
        .enumerate()
        .map(|(i, v)| Bar::new(i as f64, *v as f64))
        .collect();
    let chart = BarChart::new(id_source, bars).color(ui.visuals().selection.bg_fill);
    let labels: Vec<String> = labels.iter().map(|l| l.as_ref().to_string()).collect();
    Plot::new(id_source)
        .height(120.0)
        .allow_zoom(false)
        .allow_scroll(false)
        .allow_boxed_zoom(false)
        .allow_drag(false)
        .include_y(0.0)
        .x_axis_formatter(move |mark, _range| {
            labels
                .get(mark.value.round() as usize)
                .cloned()
                .unwrap_or_default()
        })
        .show(ui, |plot_ui| plot_ui.bar_chart(chart));
}

fn format_minutes(total_minutes: u64) -> String {
    let hours = total_minutes / 60;
    let minutes = total_minutes % 60;
    if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{minutes}m")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_minutes_shows_hours_only_when_there_are_any() {
        assert_eq!(format_minutes(45), "45m");
        assert_eq!(format_minutes(90), "1h 30m");
        assert_eq!(format_minutes(0), "0m");
    }
}
