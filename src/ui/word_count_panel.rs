use crate::project::{Project, WordCountScope};

/// Outcomes of user interaction with the Word Count panel, handled by the caller
/// (`app.rs`) rather than mutated here — same pure-rendering-layer split as
/// `corkboard_panel`/`CorkboardEvent`.
pub enum WordCountEvent {
    SetDraftTarget(Option<u32>),
    SetSessionTarget(Option<u32>),
    SetScope(WordCountScope),
    Refresh,
    ResetSession,
}

/// Renders the Word Count dock tab: a scope toggle, the Draft Target (overall
/// manuscript goal) with progress bar, the Session Target (today's writing
/// goal, relative to `project.meta.session_baseline_words`) with its own
/// progress bar, and a target-less "characters typed this session" activity
/// counter. `current_total` is `SmaragdApp::word_count_cache` — recomputed on a
/// background thread on a handful of triggers (see `app.rs`'s
/// `spawn_word_count_recompute`), never every frame, so it can lag slightly
/// behind the true on-disk count until the next trigger or an explicit Refresh.
/// `chars_typed` is `SmaragdApp::char_activity` — updated live, every frame.
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

    let session_words = current_total.saturating_sub(project.meta.session_baseline_words as usize);
    ui.label(format!("Session: {session_words} words"));
    ui.horizontal(|ui| {
        ui.label("Session Target:");
        let mut target_text = project
            .meta
            .session_target_words
            .map(|target| target.to_string())
            .unwrap_or_default();
        if ui.text_edit_singleline(&mut target_text).changed() {
            event = Some(WordCountEvent::SetSessionTarget(target_text.parse().ok()));
        }
    });
    if let Some(target) = project
        .meta
        .session_target_words
        .filter(|target| *target > 0)
    {
        let fraction = (session_words as f32 / target as f32).min(1.0);
        ui.add(egui::ProgressBar::new(fraction).text(format!("{session_words} / {target}")));
    }
    ui.label(format!(
        "Characters typed this session: {chars_typed} (no target — informational only)"
    ));
    ui.separator();

    ui.horizontal(|ui| {
        if ui.button("Refresh").clicked() {
            event = Some(WordCountEvent::Refresh);
        }
        if ui.button("Reset Session").clicked() {
            event = Some(WordCountEvent::ResetSession);
        }
    });

    event
}
