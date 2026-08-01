use chrono::Datelike;

use crate::project::Project;
use crate::streak::{self, StreakEvaluationMode, StreakStatus, WeeklySchedule};

/// Outcomes of user interaction with the Streak panel, handled by the caller
/// (`app.rs`) rather than mutated here — same pure-rendering-layer split as
/// `word_count_panel`/`WordCountEvent`. All per-project (`ProjectMeta`), not
/// global `Settings` — different projects can reasonably want different
/// paces, or none at all.
pub enum StreakEvent {
    SetEnabled(bool),
    SetSchedule(WeeklySchedule),
    SetEvaluationMode(StreakEvaluationMode),
    SetRedThresholdWeeks(u32),
}

/// Which of the Streak dock tab's two inner tabs is showing — purely UI
/// navigation state, not persisted (unlike everything in `ProjectMeta`
/// itself), so it's owned by `SmaragdApp` (`streak_sub_tab`) the same way
/// `SettingsCategory` is, rather than living on `ProjectMeta`. `set_project`
/// (`app/project_lifecycle.rs`) resets it to the sensible default —
/// `Streak` if the newly opened project already has tracking on, `Configure`
/// otherwise — every time a project is opened; the user can freely switch
/// away from that default afterward, and nothing here snaps it back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreakSubTab {
    Streak,
    Configure,
}

/// Renders the Streak dock tab as two inner tabs, à la a mini `egui_dock`:
/// **Configure** (enable flag, weekly targets, evaluation mode, red
/// threshold — edited inline the same way the Word Count panel edits its
/// own Draft Target/Session Target/Track scope, rather than through the
/// global Settings dialog) and **Streak** (the "Last completed week"
/// traffic-light badge and the separately labeled "Progress this week" bar
/// — live, but never drives the badge's color, see
/// `streak::evaluate_streak`'s doc comment for why — plus a table of this
/// week's targets vs. actuals). `today_words_so_far` is the same "session
/// words" quantity `word_count_panel::show` computes (`current_total -
/// session_baseline_words`).
pub fn show(
    ui: &mut egui::Ui,
    project: &Project,
    today_words_so_far: u32,
    sub_tab: &mut StreakSubTab,
) -> Option<StreakEvent> {
    ui.heading("Streak");
    ui.horizontal(|ui| {
        if ui
            .selectable_label(*sub_tab == StreakSubTab::Streak, "Streak")
            .clicked()
        {
            *sub_tab = StreakSubTab::Streak;
        }
        if ui
            .selectable_label(*sub_tab == StreakSubTab::Configure, "Configure")
            .clicked()
        {
            *sub_tab = StreakSubTab::Configure;
        }
    });
    ui.separator();

    match sub_tab {
        StreakSubTab::Configure => show_configure_tab(ui, project),
        StreakSubTab::Streak => {
            if project.meta.streak_enabled {
                show_streak_tab(ui, project, today_words_so_far);
            } else {
                ui.label("Streak tracking isn't enabled for this project yet.");
                if ui.button("Go to Configure").clicked() {
                    *sub_tab = StreakSubTab::Configure;
                }
            }
            None
        }
    }
}

fn show_configure_tab(ui: &mut egui::Ui, project: &Project) -> Option<StreakEvent> {
    let mut event = None;

    let mut enabled = project.meta.streak_enabled;
    if ui
        .checkbox(&mut enabled, "Track a writing streak for this project")
        .changed()
    {
        event = Some(StreakEvent::SetEnabled(enabled));
    }
    ui.add_space(8.0);
    ui.weak(
        "Weeks run Monday-Sunday; the light reflects your most recently completed \
         week, not today's progress.",
    );
    ui.add_space(12.0);

    ui.label("Weekly word targets:");
    let mut schedule = project.meta.streak_schedule;
    let mut schedule_changed = false;
    let mut day_row = |ui: &mut egui::Ui, label: &str, value: &mut u32| {
        ui.horizontal(|ui| {
            ui.label(label);
            if ui
                .add(
                    egui::DragValue::new(value)
                        .range(0..=20_000)
                        .suffix(" words"),
                )
                .changed()
            {
                schedule_changed = true;
            }
        });
    };
    day_row(ui, "Monday:", &mut schedule.monday);
    day_row(ui, "Tuesday:", &mut schedule.tuesday);
    day_row(ui, "Wednesday:", &mut schedule.wednesday);
    day_row(ui, "Thursday:", &mut schedule.thursday);
    day_row(ui, "Friday:", &mut schedule.friday);
    day_row(ui, "Saturday:", &mut schedule.saturday);
    day_row(ui, "Sunday:", &mut schedule.sunday);
    if schedule_changed {
        event = Some(StreakEvent::SetSchedule(schedule));
    }

    ui.add_space(12.0);
    ui.label("A week counts as met when:");
    ui.horizontal(|ui| {
        if ui
            .radio(
                project.meta.streak_evaluation_mode == StreakEvaluationMode::CumulativeWeekly,
                "Cumulative weekly total",
            )
            .clicked()
        {
            event = Some(StreakEvent::SetEvaluationMode(
                StreakEvaluationMode::CumulativeWeekly,
            ));
        }
        if ui
            .radio(
                project.meta.streak_evaluation_mode == StreakEvaluationMode::PerDayStrict,
                "Every day individually",
            )
            .clicked()
        {
            event = Some(StreakEvent::SetEvaluationMode(
                StreakEvaluationMode::PerDayStrict,
            ));
        }
    });

    ui.add_space(12.0);
    ui.horizontal(|ui| {
        ui.label("Consecutive missed weeks before the light turns red:");
        let mut threshold = if project.meta.streak_red_threshold_weeks > 0 {
            project.meta.streak_red_threshold_weeks
        } else {
            2
        };
        if ui
            .add(egui::DragValue::new(&mut threshold).range(1..=52))
            .changed()
        {
            event = Some(StreakEvent::SetRedThresholdWeeks(threshold));
        }
    });

    event
}

fn show_streak_tab(ui: &mut egui::Ui, project: &Project, today_words_so_far: u32) {
    let config = streak::resolve_streak_config(
        project.meta.streak_enabled,
        project.meta.streak_schedule,
        project.meta.streak_evaluation_mode,
        project.meta.streak_red_threshold_weeks,
    );
    let today = chrono::Local::now().date_naive();
    let status = streak::evaluate_streak(
        &config.schedule,
        config.mode,
        config.red_threshold_weeks,
        &project.meta.daily_word_counts,
        today,
    );

    ui.label("Last completed week:");
    ui.horizontal(|ui| {
        let (rect, _response) =
            ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
        ui.painter()
            .circle_filled(rect.center(), 7.0, status_color(ui, status));
        ui.label(status_label(status, config.red_threshold_weeks));
    });
    ui.add_space(8.0);

    let this_monday = streak::week_monday(today);
    let (actual_so_far, target_sum) = streak::current_week_progress(
        &config.schedule,
        &project.meta.daily_word_counts,
        today_words_so_far,
        today,
    );

    ui.label("Progress this week:");
    if target_sum > 0 {
        let fraction = (actual_so_far as f32 / target_sum as f32).min(1.0);
        ui.add(
            egui::ProgressBar::new(fraction).text(format!("{actual_so_far} / {target_sum} words")),
        );
    } else {
        // A 0/0 progress bar is meaningless (and egui still paints a small
        // filled nub at fraction 0.0), so skip it entirely rather than show
        // something that looks broken when no targets are configured yet.
        ui.weak("No word targets set for this week — set your weekly targets in Configure.");
    }
    ui.add_space(12.0);

    ui.separator();
    ui.label("This week:");
    egui::Grid::new("streak_week_grid")
        .striped(true)
        .show(ui, |ui| {
            ui.label("Day");
            ui.label("Target");
            ui.label("Actual");
            ui.end_row();
            for offset in 0..7i64 {
                let date = this_monday + chrono::Duration::days(offset);
                let target = config.schedule.target_for(date.weekday());
                ui.label(weekday_label(date.weekday()));
                ui.label(target.to_string());
                match date.cmp(&today) {
                    std::cmp::Ordering::Equal => {
                        ui.label(format!("{today_words_so_far} (so far)"));
                    }
                    std::cmp::Ordering::Less => match logged_words_opt(project, date) {
                        Some(words) => {
                            ui.label(words.to_string());
                        }
                        None => {
                            ui.label("—");
                        }
                    },
                    std::cmp::Ordering::Greater => {
                        ui.label("—");
                    }
                }
                ui.end_row();
            }
        });
}

fn logged_words_opt(project: &Project, date: chrono::NaiveDate) -> Option<u32> {
    project
        .meta
        .daily_word_counts
        .get(&date.format("%Y-%m-%d").to_string())
        .copied()
}

/// Shared with `app::mod`'s status bar glyph so the two indicators always
/// agree on what each color means.
pub fn status_color(ui: &egui::Ui, status: StreakStatus) -> egui::Color32 {
    match status {
        StreakStatus::Disabled => egui::Color32::TRANSPARENT,
        StreakStatus::InsufficientData => ui.visuals().weak_text_color(),
        StreakStatus::Green => egui::Color32::from_rgb(60, 180, 75),
        StreakStatus::Yellow => egui::Color32::from_rgb(230, 180, 30),
        // Same red already used for the Settings window's validation-error
        // label, for visual consistency.
        StreakStatus::Red => egui::Color32::from_rgb(200, 60, 60),
    }
}

fn status_label(status: StreakStatus, red_threshold_weeks: u32) -> String {
    match status {
        StreakStatus::Disabled => "Disabled".to_string(),
        StreakStatus::InsufficientData => "Not enough data yet".to_string(),
        StreakStatus::Green => "On track".to_string(),
        StreakStatus::Yellow => "Off track this week".to_string(),
        StreakStatus::Red => format!("Off track — {red_threshold_weeks}+ weeks running"),
    }
}

fn weekday_label(weekday: chrono::Weekday) -> &'static str {
    match weekday {
        chrono::Weekday::Mon => "Monday",
        chrono::Weekday::Tue => "Tuesday",
        chrono::Weekday::Wed => "Wednesday",
        chrono::Weekday::Thu => "Thursday",
        chrono::Weekday::Fri => "Friday",
        chrono::Weekday::Sat => "Saturday",
        chrono::Weekday::Sun => "Sunday",
    }
}
