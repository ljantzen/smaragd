use super::*;

use crate::settings::FocusModeSpellCheck;
use crate::spellcheck::SpellCheckLanguage;

impl SmaragdApp {
    /// The language the Editor actually checks against right now:
    /// `focus_mode_spell_check_override` while Focus Mode has one, otherwise
    /// the app-wide `Settings::spell_check_language`.
    pub(super) fn effective_spell_check_language(&self) -> SpellCheckLanguage {
        self.focus_mode_spell_check_override
            .unwrap_or(self.settings.spell_check_language)
    }

    /// The language `toggle_spell_check` turns back on — the open project's
    /// `ProjectMeta::last_spell_check_language`. `None` without a project, or
    /// in one where spell check has never been used.
    fn remembered_spell_check_language(&self) -> Option<SpellCheckLanguage> {
        self.project
            .as_ref()
            .and_then(|project| project.meta.last_spell_check_language)
    }

    /// Record `language` on the open project (a no-op without one, or for
    /// `Off`) — see `Project::remember_spell_check_language`.
    pub(super) fn remember_spell_check_language(&mut self, language: SpellCheckLanguage) {
        let Some(project) = &mut self.project else {
            return;
        };
        if let Err(err) = project.remember_spell_check_language(language) {
            self.push_error_toast(format!("Couldn't save spell-check language: {err}"));
        }
    }

    /// `ShortcutAction::ToggleSpellCheck`/`:spell`: turn spell check off
    /// (remembering the language on the open project first), or back on in
    /// the project's remembered language. Inside Focus Mode with an active
    /// override, only the override flips — `Settings::spell_check_language`
    /// is left for Focus Mode's exit to fall back to; otherwise the setting
    /// itself changes and is persisted.
    pub(super) fn toggle_spell_check(&mut self) {
        let current = self.effective_spell_check_language();
        let next = if current != SpellCheckLanguage::Off {
            self.remember_spell_check_language(current);
            SpellCheckLanguage::Off
        } else if let Some(language) = self.remembered_spell_check_language() {
            language
        } else {
            self.push_error_toast(
                "No spell-check language chosen for this project yet — pick one in \
                 Settings > Spell Check.",
            );
            return;
        };
        if self.focus_mode_spell_check_override.is_some() {
            self.focus_mode_spell_check_override = Some(next);
        } else {
            self.settings.spell_check_language = next;
            self.persist_settings();
        }
        self.set_status_message(match next {
            SpellCheckLanguage::Off => "Spell check off".to_string(),
            language => format!("Spell check on ({})", language.label()),
        });
    }

    /// Set or clear `focus_mode_spell_check_override` on entering/leaving
    /// Focus Mode, per `Settings::focus_mode_spell_check`. "On" prefers the
    /// currently active language, then the project's remembered one; with
    /// neither there's nothing to turn on, so it stays off.
    pub(super) fn apply_focus_mode_spell_check(&mut self, entering: bool) {
        // So a Focus Mode that starts with spell check forced off can still
        // toggle it back on in the language that was active going in.
        if entering {
            self.remember_spell_check_language(self.settings.spell_check_language);
        }
        self.focus_mode_spell_check_override = if !entering {
            None
        } else {
            match self.settings.focus_mode_spell_check {
                FocusModeSpellCheck::KeepCurrent => None,
                FocusModeSpellCheck::Off => Some(SpellCheckLanguage::Off),
                FocusModeSpellCheck::On => Some(
                    Some(self.settings.spell_check_language)
                        .filter(|language| *language != SpellCheckLanguage::Off)
                        .or_else(|| self.remembered_spell_check_language())
                        .unwrap_or(SpellCheckLanguage::Off),
                ),
            }
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app_with_project() -> (tempfile::TempDir, SmaragdApp) {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::initialize(dir.path()).unwrap();
        let mut app = SmaragdApp::test_fixture();
        app.project = Some(project);
        (dir, app)
    }

    fn remembered(app: &SmaragdApp) -> Option<SpellCheckLanguage> {
        app.project.as_ref().unwrap().meta.last_spell_check_language
    }

    #[test]
    fn toggling_off_remembers_the_language_on_the_project_and_toggling_on_restores_it() {
        let (dir, mut app) = app_with_project();
        app.settings.spell_check_language = SpellCheckLanguage::Norwegian;

        app.toggle_spell_check();
        assert_eq!(app.settings.spell_check_language, SpellCheckLanguage::Off);
        assert_eq!(remembered(&app), Some(SpellCheckLanguage::Norwegian));

        app.toggle_spell_check();
        assert_eq!(
            app.settings.spell_check_language,
            SpellCheckLanguage::Norwegian
        );

        let reloaded = Project::load_from_folder(dir.path()).unwrap();
        assert_eq!(
            reloaded.meta.last_spell_check_language,
            Some(SpellCheckLanguage::Norwegian)
        );
    }

    #[test]
    fn toggling_on_restores_the_open_projects_own_language_not_the_last_global_one() {
        let (_dir, mut app) = app_with_project();
        app.project.as_mut().unwrap().meta.last_spell_check_language =
            Some(SpellCheckLanguage::German);

        app.toggle_spell_check();

        assert_eq!(
            app.settings.spell_check_language,
            SpellCheckLanguage::German
        );
    }

    #[test]
    fn toggling_on_with_nothing_remembered_leaves_spell_check_off() {
        let (_dir, mut app) = app_with_project();

        app.toggle_spell_check();

        assert_eq!(app.settings.spell_check_language, SpellCheckLanguage::Off);
    }

    #[test]
    fn toggling_without_a_project_still_turns_spell_check_off() {
        let mut app = SmaragdApp::test_fixture();
        app.settings.spell_check_language = SpellCheckLanguage::English;

        app.toggle_spell_check();

        assert_eq!(app.settings.spell_check_language, SpellCheckLanguage::Off);
    }

    #[test]
    fn focus_mode_keep_current_sets_no_override() {
        let (_dir, mut app) = app_with_project();
        app.settings.spell_check_language = SpellCheckLanguage::English;
        app.settings.focus_mode_spell_check = FocusModeSpellCheck::KeepCurrent;

        app.apply_focus_mode_spell_check(true);

        assert_eq!(app.focus_mode_spell_check_override, None);
        assert_eq!(
            app.effective_spell_check_language(),
            SpellCheckLanguage::English
        );
    }

    #[test]
    fn focus_mode_off_hides_spell_check_without_touching_the_setting_and_restores_on_exit() {
        let (_dir, mut app) = app_with_project();
        app.settings.spell_check_language = SpellCheckLanguage::English;
        app.settings.focus_mode_spell_check = FocusModeSpellCheck::Off;

        app.apply_focus_mode_spell_check(true);
        assert_eq!(
            app.effective_spell_check_language(),
            SpellCheckLanguage::Off
        );
        assert_eq!(
            app.settings.spell_check_language,
            SpellCheckLanguage::English
        );

        app.apply_focus_mode_spell_check(false);
        assert_eq!(
            app.effective_spell_check_language(),
            SpellCheckLanguage::English
        );
    }

    #[test]
    fn focus_mode_on_falls_back_to_the_projects_remembered_language() {
        let (_dir, mut app) = app_with_project();
        app.project.as_mut().unwrap().meta.last_spell_check_language =
            Some(SpellCheckLanguage::French);
        app.settings.focus_mode_spell_check = FocusModeSpellCheck::On;

        app.apply_focus_mode_spell_check(true);

        assert_eq!(
            app.effective_spell_check_language(),
            SpellCheckLanguage::French
        );
        assert_eq!(app.settings.spell_check_language, SpellCheckLanguage::Off);
    }

    #[test]
    fn focus_mode_on_prefers_the_currently_active_language() {
        let (_dir, mut app) = app_with_project();
        app.project.as_mut().unwrap().meta.last_spell_check_language =
            Some(SpellCheckLanguage::French);
        app.settings.spell_check_language = SpellCheckLanguage::English;
        app.settings.focus_mode_spell_check = FocusModeSpellCheck::On;

        app.apply_focus_mode_spell_check(true);

        assert_eq!(
            app.effective_spell_check_language(),
            SpellCheckLanguage::English
        );
    }

    #[test]
    fn toggling_inside_focus_mode_flips_only_the_override() {
        let (_dir, mut app) = app_with_project();
        app.settings.spell_check_language = SpellCheckLanguage::English;
        app.settings.focus_mode_spell_check = FocusModeSpellCheck::Off;
        app.apply_focus_mode_spell_check(true);

        app.toggle_spell_check();
        assert_eq!(
            app.effective_spell_check_language(),
            SpellCheckLanguage::English
        );

        app.toggle_spell_check();
        assert_eq!(
            app.effective_spell_check_language(),
            SpellCheckLanguage::Off
        );
        assert_eq!(
            app.settings.spell_check_language,
            SpellCheckLanguage::English
        );

        app.apply_focus_mode_spell_check(false);
        assert_eq!(
            app.effective_spell_check_language(),
            SpellCheckLanguage::English
        );
    }
}
