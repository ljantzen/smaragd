use smaragd_core::project::{Project, WordCountScope};

/// Outcomes of user interaction with the Word Count panel, handled by the caller
/// (`app.rs`) rather than mutated here — same pure-rendering-layer split as
/// `corkboard_panel`/`CorkboardEvent`.
pub enum WordCountEvent {
    SetDraftTarget(Option<u32>),
    SetTodayTarget(Option<u32>),
    SetScope(WordCountScope),
    Refresh,
    ResetToday,
}

/// Renders the Word Count dock tab: a scope toggle, the Draft Target (overall
/// manuscript goal) with progress bar, Today's Target (today's writing goal,
/// a *signed* delta relative to
/// `project.today_baseline_words(project.meta.word_count_scope)` — like
/// Scrivener's Session Target, a day with more deletions than additions
/// reads as a negative count rather than being floored at 0 — unrelated to
/// `Project::start_session`/`close_session`'s per-app-launch sessions) with
/// its own progress bar, and a target-less "characters typed" activity
/// counter. `current_total` is
/// `SmaragdApp::word_count_cache` — recomputed on a
/// background thread on a handful of triggers (see `app.rs`'s
/// `spawn_word_count_recompute`), never every frame, so it can lag slightly
/// behind the true on-disk count until the next trigger or an explicit Refresh.
/// `chars_typed` is `WordCountState::char_activity` for the active scope —
/// updated live, every frame.
pub fn show(
    ui: &mut egui::Ui,
    project: &Project,
    current_total: usize,
    chars_typed: u64,
) -> Option<WordCountEvent> {
    let mut event = None;

    ui.heading("Word Count");
    ui.separator();

    ui.horizontal(|ui| {
        ui.label("Track:");
        let scope = project.meta.word_count_scope;
        if ui
            .radio(
                scope == WordCountScope::ManuscriptOnly,
                "Manuscript folders only",
            )
            .clicked()
        {
            event = Some(WordCountEvent::SetScope(WordCountScope::ManuscriptOnly));
        }
        if ui
            .radio(
                scope == WordCountScope::EverythingExceptTrash,
                "Everything except Trash",
            )
            .clicked()
        {
            event = Some(WordCountEvent::SetScope(
                WordCountScope::EverythingExceptTrash,
            ));
        }
    });
    ui.separator();

    ui.label(format!("Current: {current_total} words"));
    ui.horizontal(|ui| {
        ui.label("Draft Target:");
        let mut target_text = project
            .meta
            .draft_target_words
            .map(|target| target.to_string())
            .unwrap_or_default();
        if ui.text_edit_singleline(&mut target_text).changed() {
            event = Some(WordCountEvent::SetDraftTarget(target_text.parse().ok()));
        }
    });
    if let Some(target) = project.meta.draft_target_words.filter(|target| *target > 0) {
        let fraction = (current_total as f32 / target as f32).min(1.0);
        ui.add(egui::ProgressBar::new(fraction).text(format!("{current_total} / {target}")));
    }
    ui.separator();

    // Signed, like Scrivener's Session Target: a day that nets more deletions
    // than additions reads as a negative count rather than being floored at
    // 0, which would otherwise make "nothing written" and "a lot deleted"
    // look identical.
    let today_words =
        current_total as i64 - project.today_baseline_words(project.meta.word_count_scope) as i64;
    ui.label(format!("Today: {today_words} words"));
    ui.horizontal(|ui| {
        ui.label("Today's Target:");
        let mut target_text = project
            .meta
            .today_target_words
            .map(|target| target.to_string())
            .unwrap_or_default();
        if ui.text_edit_singleline(&mut target_text).changed() {
            event = Some(WordCountEvent::SetTodayTarget(target_text.parse().ok()));
        }
    });
    if let Some(target) = project.meta.today_target_words.filter(|target| *target > 0) {
        // A negative `today_words` would otherwise read as a negative
        // fraction — `egui::ProgressBar` doesn't clamp that itself.
        let fraction = (today_words as f32 / target as f32).clamp(0.0, 1.0);
        ui.add(egui::ProgressBar::new(fraction).text(format!("{today_words} / {target}")));
    }
    ui.label(format!("Characters typed: {chars_typed}"));
    ui.separator();

    ui.horizontal(|ui| {
        if ui.button("Refresh").clicked() {
            event = Some(WordCountEvent::Refresh);
        }
        if ui.button("Reset Today's Count").clicked() {
            event = Some(WordCountEvent::ResetToday);
        }
    });

    event
}
