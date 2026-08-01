use super::*;

impl SmaragdApp {
    pub(super) fn handle_streak_event(&mut self, event: ui::streak_panel::StreakEvent) {
        use ui::streak_panel::StreakEvent;
        let Some(project) = &mut self.project else {
            return;
        };
        let result = match event {
            StreakEvent::SetEnabled(enabled) => project.set_streak_enabled(enabled),
            StreakEvent::SetSchedule(schedule) => project.set_streak_schedule(schedule),
            StreakEvent::SetEvaluationMode(mode) => project.set_streak_evaluation_mode(mode),
            StreakEvent::SetRedThresholdWeeks(weeks) => {
                project.set_streak_red_threshold_weeks(weeks)
            }
        };
        if let Err(err) = result {
            self.push_error_toast(format!("Couldn't save streak settings: {err}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::streak::{StreakEvaluationMode, WeeklySchedule};
    use crate::ui::streak_panel::StreakEvent;

    #[test]
    fn streak_event_set_enabled_persists_on_the_open_project() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::initialize(dir.path()).unwrap();
        let mut app = SmaragdApp::test_fixture();
        app.project = Some(project);

        app.handle_streak_event(StreakEvent::SetEnabled(true));

        assert!(app.project.as_ref().unwrap().meta.streak_enabled);
    }

    #[test]
    fn streak_event_set_schedule_persists_on_the_open_project() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::initialize(dir.path()).unwrap();
        let mut app = SmaragdApp::test_fixture();
        app.project = Some(project);
        let schedule = WeeklySchedule {
            monday: 750,
            ..Default::default()
        };

        app.handle_streak_event(StreakEvent::SetSchedule(schedule));

        assert_eq!(app.project.as_ref().unwrap().meta.streak_schedule, schedule);
    }

    #[test]
    fn streak_event_set_evaluation_mode_persists_on_the_open_project() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::initialize(dir.path()).unwrap();
        let mut app = SmaragdApp::test_fixture();
        app.project = Some(project);

        app.handle_streak_event(StreakEvent::SetEvaluationMode(
            StreakEvaluationMode::PerDayStrict,
        ));

        assert_eq!(
            app.project.as_ref().unwrap().meta.streak_evaluation_mode,
            StreakEvaluationMode::PerDayStrict
        );
    }

    #[test]
    fn streak_event_set_red_threshold_weeks_persists_on_the_open_project() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::initialize(dir.path()).unwrap();
        let mut app = SmaragdApp::test_fixture();
        app.project = Some(project);

        app.handle_streak_event(StreakEvent::SetRedThresholdWeeks(4));

        assert_eq!(
            app.project
                .as_ref()
                .unwrap()
                .meta
                .streak_red_threshold_weeks,
            4
        );
    }

    #[test]
    fn streak_event_is_a_no_op_without_an_open_project() {
        let mut app = SmaragdApp::test_fixture();

        // Must not panic when there's nothing to apply the edit to.
        app.handle_streak_event(StreakEvent::SetEnabled(true));

        assert!(app.project.is_none());
    }
}
