use super::*;

impl Project {
    /// Turn Writing Streak tracking on/off for this project — see
    /// [`ProjectMeta::streak_enabled`].
    pub fn set_streak_enabled(&mut self, enabled: bool) -> io::Result<()> {
        self.meta.streak_enabled = enabled;
        self.save_metadata()
    }

    /// Set the weekly word-count schedule — see
    /// [`ProjectMeta::streak_schedule`].
    pub fn set_streak_schedule(
        &mut self,
        schedule: crate::streak::WeeklySchedule,
    ) -> io::Result<()> {
        self.meta.streak_schedule = schedule;
        self.save_metadata()
    }

    /// Set how strictly a week counts as "met" — see
    /// [`ProjectMeta::streak_evaluation_mode`].
    pub fn set_streak_evaluation_mode(
        &mut self,
        mode: crate::streak::StreakEvaluationMode,
    ) -> io::Result<()> {
        self.meta.streak_evaluation_mode = mode;
        self.save_metadata()
    }

    /// Set how many consecutive missed weeks turn the streak light red —
    /// see [`ProjectMeta::streak_red_threshold_weeks`].
    pub fn set_streak_red_threshold_weeks(&mut self, weeks: u32) -> io::Result<()> {
        self.meta.streak_red_threshold_weeks = weeks;
        self.save_metadata()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_streak_enabled_persists_across_a_reload() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        assert!(!project.meta.streak_enabled);

        project.set_streak_enabled(true).unwrap();

        let reloaded = Project::load_from_folder(dir.path()).unwrap();
        assert!(reloaded.meta.streak_enabled);
    }

    #[test]
    fn set_streak_schedule_persists_across_a_reload() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let schedule = crate::streak::WeeklySchedule {
            monday: 500,
            ..Default::default()
        };

        project.set_streak_schedule(schedule).unwrap();

        let reloaded = Project::load_from_folder(dir.path()).unwrap();
        assert_eq!(reloaded.meta.streak_schedule, schedule);
    }

    #[test]
    fn set_streak_evaluation_mode_persists_across_a_reload() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();

        project
            .set_streak_evaluation_mode(crate::streak::StreakEvaluationMode::PerDayStrict)
            .unwrap();

        let reloaded = Project::load_from_folder(dir.path()).unwrap();
        assert_eq!(
            reloaded.meta.streak_evaluation_mode,
            crate::streak::StreakEvaluationMode::PerDayStrict
        );
    }

    #[test]
    fn set_streak_red_threshold_weeks_persists_across_a_reload() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();

        project.set_streak_red_threshold_weeks(3).unwrap();

        let reloaded = Project::load_from_folder(dir.path()).unwrap();
        assert_eq!(reloaded.meta.streak_red_threshold_weeks, 3);
    }
}
