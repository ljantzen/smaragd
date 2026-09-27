//! IntelliJ-style Search Everywhere — one modal for documents (by name), text
//! inside documents, actions (every `ShortcutAction` plus plugin `:` commands, by
//! label), and settings. Opened by a double tap of Shift (`crate::double_tap`) or
//! `ShortcutAction::SearchEverywhere` (Ctrl+Shift+A by default).
//!
//! A pure rendering layer, like `command_prompt.rs`: `app.rs` supplies the
//! candidates and carries out the returned `SearchEverywhereOutcome`.

use std::path::PathBuf;

use egui::{Id, Key, Modifiers};

use crate::fuzzy::fuzzy_rank;
use crate::search::{find_matches, line_at};
use crate::shortcuts::ShortcutTarget;
use crate::ui::settings_panel::{SETTINGS_INDEX, SettingsCategory};

/// Cap on results in a single-source tab — also what keeps text search from
/// scanning past the first page of hits in a large project.
const MAX_RESULTS: usize = 50;
/// Cap on results per section in the All tab, where every source shares the list.
const ALL_TAB_SECTION_LIMIT: usize = 5;
/// Shortest query text search runs for — one or two characters would match
/// nearly every line of a manuscript.
const MIN_TEXT_QUERY_CHARS: usize = 3;
/// Max height of the scrollable results list, in points.
const RESULTS_MAX_HEIGHT: f32 = 360.0;
/// Longest line preview shown for a text match, in characters.
const LINE_PREVIEW_CHARS: usize = 90;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchTab {
    #[default]
    All,
    Documents,
    Text,
    Actions,
    Settings,
}

impl SearchTab {
    pub const ALL: [SearchTab; 5] = [
        Self::All,
        Self::Documents,
        Self::Text,
        Self::Actions,
        Self::Settings,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Documents => "Documents",
            Self::Text => "Text",
            Self::Actions => "Actions",
            Self::Settings => "Settings",
        }
    }

    fn step(self, forward: bool) -> Self {
        let len = Self::ALL.len();
        let index = Self::ALL.iter().position(|tab| *tab == self).unwrap_or(0);
        let next = if forward {
            (index + 1) % len
        } else {
            (index + len - 1) % len
        };
        Self::ALL[next]
    }
}

/// One document's text, loaded once per opening of the modal so text search
/// doesn't hit the disk on every keystroke.
pub struct TextDocument {
    pub display: String,
    pub path: PathBuf,
    pub content: String,
}

/// A single text-search hit.
#[derive(Debug, Clone, PartialEq)]
pub struct TextHit {
    pub display: String,
    pub path: PathBuf,
    pub line: usize,
    pub line_text: String,
    pub byte_start: usize,
}

/// A runnable action, ready to display.
pub struct ActionCandidate {
    pub label: String,
    /// The current binding, already formatted for display (`format_shortcut`).
    pub shortcut: Option<String>,
    pub target: ShortcutTarget,
}

/// UI state, owned by `app.rs` for the app's lifetime.
#[derive(Default)]
pub struct SearchEverywhereState {
    pub open: bool,
    /// Set alongside `open` so `show` focuses the input once rather than fighting
    /// the user for focus on every frame the modal is visible.
    pub focus_requested: bool,
    pub query: String,
    pub tab: SearchTab,
    /// Index into the current frame's selectable rows, clamped each frame.
    pub selected: usize,
    /// Every document's text, filled in by `app.rs` the first frame the modal is
    /// open (`None` until then) and dropped again on close.
    pub text_cache: Option<Vec<TextDocument>>,
    /// Text hits for `text_hits_query`, recomputed only when the query changes.
    text_hits: Vec<TextHit>,
    text_hits_query: Option<String>,
}

impl SearchEverywhereState {
    pub fn request_open(&mut self) {
        self.open = true;
        self.focus_requested = true;
        self.query.clear();
        self.tab = SearchTab::All;
        self.selected = 0;
        self.text_cache = None;
        self.text_hits.clear();
        self.text_hits_query = None;
    }

    fn close(&mut self) {
        self.open = false;
        self.text_cache = None;
        self.text_hits.clear();
        self.text_hits_query = None;
    }

    fn refresh_text_hits(&mut self) {
        if self.text_hits_query.as_deref() == Some(self.query.as_str()) {
            return;
        }
        self.text_hits = match &self.text_cache {
            Some(documents) if self.query.chars().count() >= MIN_TEXT_QUERY_CHARS => {
                search_text(documents, &self.query, MAX_RESULTS)
            }
            _ => Vec::new(),
        };
        // Left unset while the cache is still loading, so the first frame with
        // it available searches rather than returning the stale empty result.
        self.text_hits_query = self.text_cache.is_some().then(|| self.query.clone());
    }
}

/// Case-insensitive plain-text search over the cached documents, in document
/// order, stopping at `limit` hits.
pub fn search_text(documents: &[TextDocument], query: &str, limit: usize) -> Vec<TextHit> {
    let mut hits = Vec::new();
    for document in documents {
        for (byte_start, _) in find_matches(&document.content, query, false) {
            if hits.len() == limit {
                return hits;
            }
            let (line, line_text) = line_at(&document.content, byte_start);
            hits.push(TextHit {
                display: document.display.clone(),
                path: document.path.clone(),
                line,
                line_text,
                byte_start,
            });
        }
    }
    hits
}

/// What the user chose. `app.rs` carries it out.
#[derive(Debug, Clone, PartialEq)]
pub enum SearchEverywhereOutcome {
    OpenDocument(PathBuf),
    /// Open the document and put the cursor at this byte offset.
    OpenAt(PathBuf, usize),
    Run(ShortcutTarget),
    OpenSettings(SettingsCategory),
}

/// A searchable setting: either a single entry from `SETTINGS_INDEX`, or a
/// whole category page.
struct SettingCandidate {
    label: &'static str,
    /// `label` plus keywords — what the query is matched against.
    haystack: String,
    category: SettingsCategory,
}

fn setting_candidates() -> Vec<SettingCandidate> {
    SettingsCategory::ALL
        .iter()
        .map(|&category| SettingCandidate {
            label: category.label(),
            haystack: format!("{} settings", category.label()),
            category,
        })
        .chain(SETTINGS_INDEX.iter().map(|entry| SettingCandidate {
            label: entry.label,
            haystack: format!("{} {}", entry.label, entry.keywords),
            category: entry.category,
        }))
        .collect()
}

/// One selectable result row, borrowed from its source.
enum Row<'a> {
    Document(&'a (String, PathBuf)),
    Text(&'a TextHit),
    Action(&'a ActionCandidate),
    Setting(&'a SettingCandidate),
}

impl Row<'_> {
    fn title(&self) -> String {
        match self {
            Row::Document((name, _)) => document_title(name).to_string(),
            Row::Text(hit) => truncate_chars(hit.line_text.trim(), LINE_PREVIEW_CHARS),
            Row::Action(action) => action.label.clone(),
            Row::Setting(setting) => setting.label.to_string(),
        }
    }

    fn context(&self) -> String {
        match self {
            Row::Document((name, _)) => name.clone(),
            Row::Text(hit) => format!("{}:{}", hit.display, hit.line),
            Row::Action(action) => action.shortcut.clone().unwrap_or_default(),
            Row::Setting(setting) => format!("Settings › {}", setting.category.label()),
        }
    }

    fn outcome(&self) -> SearchEverywhereOutcome {
        match self {
            Row::Document((_, path)) => SearchEverywhereOutcome::OpenDocument(path.clone()),
            Row::Text(hit) => SearchEverywhereOutcome::OpenAt(hit.path.clone(), hit.byte_start),
            Row::Action(action) => SearchEverywhereOutcome::Run(action.target.clone()),
            Row::Setting(setting) => SearchEverywhereOutcome::OpenSettings(setting.category),
        }
    }
}

/// The last `/`-separated component of a document's display path — its name.
fn document_title(display: &str) -> &str {
    display.rsplit('/').next().unwrap_or(display)
}

fn truncate_chars(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((index, _)) => format!("{}…", &text[..index]),
        None => text.to_string(),
    }
}

/// The rows to show for `tab`, grouped under an optional section heading (only
/// the All tab has headings).
fn rows<'a>(
    tab: SearchTab,
    query: &str,
    documents: &'a [(String, PathBuf)],
    text_hits: &'a [TextHit],
    actions: &'a [ActionCandidate],
    settings: &'a [SettingCandidate],
) -> Vec<(Option<&'static str>, Row<'a>)> {
    let section = |tab: SearchTab, limit: usize| -> Vec<Row<'a>> {
        match tab {
            SearchTab::Documents => {
                fuzzy_rank(documents, |(name, _)| name.as_str(), query, limit, true)
                    .into_iter()
                    .map(|(document, _)| Row::Document(document))
                    .collect()
            }
            SearchTab::Text => text_hits.iter().take(limit).map(Row::Text).collect(),
            SearchTab::Actions => {
                fuzzy_rank(actions, |action| action.label.as_str(), query, limit, false)
                    .into_iter()
                    .map(|(action, _)| Row::Action(action))
                    .collect()
            }
            SearchTab::Settings => fuzzy_rank(
                settings,
                |setting| setting.haystack.as_str(),
                query,
                limit,
                false,
            )
            .into_iter()
            .map(|(setting, _)| Row::Setting(setting))
            .collect(),
            SearchTab::All => Vec::new(),
        }
    };
    match tab {
        SearchTab::All => [
            SearchTab::Documents,
            SearchTab::Actions,
            SearchTab::Settings,
            SearchTab::Text,
        ]
        .into_iter()
        .flat_map(|source| {
            section(source, ALL_TAB_SECTION_LIMIT)
                .into_iter()
                .map(move |row| (Some(source.label()), row))
        })
        .collect(),
        single => section(single, MAX_RESULTS)
            .into_iter()
            .map(|row| (None, row))
            .collect(),
    }
}

enum NavAction {
    Next,
    Prev,
    NextTab,
    PrevTab,
}

/// Consume (and act on) the keys meant for the result list and tab row, so the
/// `TextEdit` underneath never sees them — Tab would otherwise move focus off
/// the field entirely.
fn steal_nav_key(ctx: &egui::Context) -> Option<NavAction> {
    ctx.input_mut(|i| {
        if i.consume_key(Modifiers::NONE, Key::ArrowDown) {
            Some(NavAction::Next)
        } else if i.consume_key(Modifiers::NONE, Key::ArrowUp) {
            Some(NavAction::Prev)
        } else if i.consume_key(Modifiers::SHIFT, Key::Tab) {
            Some(NavAction::PrevTab)
        } else if i.consume_key(Modifiers::NONE, Key::Tab) {
            Some(NavAction::NextTab)
        } else {
            None
        }
    })
}

/// Renders the modal if `state.open`. `documents` is `(display_path,
/// absolute_path)` pairs, as for the Open Document switcher; `actions` is every
/// action currently available. Text search runs over `state.text_cache`, which
/// the caller fills. Returns `Some` the frame the user picks a result.
pub fn show(
    ctx: &egui::Context,
    state: &mut SearchEverywhereState,
    documents: &[(String, PathBuf)],
    actions: &[ActionCandidate],
) -> Option<SearchEverywhereOutcome> {
    if !state.open {
        return None;
    }

    let nav_action = steal_nav_key(ctx);
    match nav_action {
        Some(NavAction::NextTab) => {
            state.tab = state.tab.step(true);
            state.selected = 0;
        }
        Some(NavAction::PrevTab) => {
            state.tab = state.tab.step(false);
            state.selected = 0;
        }
        _ => {}
    }

    let mut outcome = None;
    let mut close = false;
    let settings = setting_candidates();
    egui::Modal::new(Id::new("search_everywhere_modal")).show(ctx, |ui| {
        ui.set_min_width(560.0);
        ui.horizontal(|ui| {
            for tab in SearchTab::ALL {
                if ui.selectable_label(state.tab == tab, tab.label()).clicked() {
                    state.tab = tab;
                    state.selected = 0;
                }
            }
        });
        ui.add_space(6.0);

        let previous_query = state.query.clone();
        let response = ui.add(
            egui::TextEdit::singleline(&mut state.query)
                .desired_width(f32::INFINITY)
                // Claims Tab for this field so egui's own focus traversal
                // (`Memory::begin_pass`, which runs before `steal_nav_key` can
                // consume the key) doesn't move focus off it — Tab switches
                // tabs here instead, and is consumed before the field would
                // insert a tab character.
                .lock_focus(true)
                .hint_text("Search documents, text, actions, and settings"),
        );
        if state.focus_requested {
            response.request_focus();
            state.focus_requested = false;
        }
        if state.query != previous_query {
            state.selected = 0;
        }
        if matches!(state.tab, SearchTab::All | SearchTab::Text) {
            state.refresh_text_hits();
        }

        let rows = rows(
            state.tab,
            &state.query,
            documents,
            &state.text_hits,
            actions,
            &settings,
        );
        if !rows.is_empty() {
            state.selected = state.selected.min(rows.len() - 1);
            match nav_action {
                Some(NavAction::Next) => state.selected = (state.selected + 1) % rows.len(),
                Some(NavAction::Prev) => {
                    state.selected = (state.selected + rows.len() - 1) % rows.len();
                }
                _ => {}
            }
        }
        let just_navigated = matches!(nav_action, Some(NavAction::Next | NavAction::Prev));

        if response.lost_focus()
            && ui.input(|i| i.key_pressed(Key::Enter))
            && let Some((_, row)) = rows.get(state.selected)
        {
            outcome = Some(row.outcome());
        }

        ui.add_space(8.0);
        if rows.is_empty() {
            ui.weak(empty_message(state));
        }
        egui::ScrollArea::vertical()
            .max_height(RESULTS_MAX_HEIGHT)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                let mut current_section = None;
                for (index, (section, row)) in rows.iter().enumerate() {
                    if section.is_some() && *section != current_section {
                        if current_section.is_some() {
                            ui.add_space(4.0);
                        }
                        ui.label(
                            egui::RichText::new(section.unwrap_or_default())
                                .small()
                                .strong(),
                        );
                        current_section = *section;
                    }
                    let selected = index == state.selected;
                    let response = ui
                        .horizontal(|ui| {
                            let response = ui.selectable_label(selected, row.title());
                            let context = row.context();
                            if !context.is_empty() {
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| ui.weak(context),
                                );
                            }
                            response
                        })
                        .inner;
                    if selected && just_navigated {
                        response.scroll_to_me(Some(egui::Align::Center));
                    }
                    if response.clicked() {
                        outcome = Some(row.outcome());
                    }
                }
            });

        if ui.input(|i| i.key_pressed(Key::Escape)) {
            close = true;
        }
    });

    if outcome.is_some() || close {
        state.close();
    }
    outcome
}

fn empty_message(state: &SearchEverywhereState) -> &'static str {
    let text_too_short = state.query.chars().count() < MIN_TEXT_QUERY_CHARS;
    match state.tab {
        SearchTab::Text if state.text_cache.is_none() => "Searching…",
        SearchTab::Text if text_too_short => "Type at least 3 characters to search text.",
        _ => "No results.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shortcuts::ShortcutAction;

    fn documents() -> Vec<(String, PathBuf)> {
        vec![
            ("Manus/Chapter 1".to_string(), PathBuf::from("/p/c1.md")),
            ("Research/Notes".to_string(), PathBuf::from("/p/notes.md")),
        ]
    }

    fn actions() -> Vec<ActionCandidate> {
        vec![ActionCandidate {
            label: "Toggle Spell Check".to_string(),
            shortcut: Some("F7".to_string()),
            target: ShortcutTarget::BuiltIn(ShortcutAction::ToggleSpellCheck),
        }]
    }

    fn text_cache() -> Vec<TextDocument> {
        vec![TextDocument {
            display: "Manus/Chapter 1".to_string(),
            path: PathBuf::from("/p/c1.md"),
            content: "It was a dark night.\nThe emerald glowed.".to_string(),
        }]
    }

    #[test]
    fn search_text_reports_line_and_offset() {
        let hits = search_text(&text_cache(), "EMERALD", 10);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].line, 2);
        assert_eq!(hits[0].line_text, "The emerald glowed.");
        assert_eq!(
            &text_cache()[0].content[hits[0].byte_start..][..7],
            "emerald"
        );
    }

    #[test]
    fn search_text_stops_at_the_limit() {
        let documents = vec![TextDocument {
            display: "a".to_string(),
            path: PathBuf::from("a"),
            content: "word ".repeat(100),
        }];
        assert_eq!(search_text(&documents, "word", 7).len(), 7);
    }

    #[test]
    fn tab_steps_wrap_in_both_directions() {
        assert_eq!(SearchTab::Settings.step(true), SearchTab::All);
        assert_eq!(SearchTab::All.step(false), SearchTab::Settings);
    }

    #[test]
    fn every_settings_category_and_entry_is_searchable() {
        let settings = setting_candidates();
        assert_eq!(
            settings.len(),
            SettingsCategory::ALL.len() + SETTINGS_INDEX.len()
        );
        let hits = fuzzy_rank(&settings, |s| s.haystack.as_str(), "curly quotes", 5, false);
        assert_eq!(hits[0].0.category, SettingsCategory::Editor);
    }

    #[test]
    fn all_tab_groups_every_source_under_its_heading() {
        let documents = documents();
        let actions = actions();
        let settings = setting_candidates();
        let hits = search_text(&text_cache(), "spell", 10);
        let rows = rows(
            SearchTab::All,
            "spell",
            &documents,
            &hits,
            &actions,
            &settings,
        );
        let sections: Vec<&str> = rows.iter().filter_map(|(section, _)| *section).collect();
        assert!(sections.contains(&"Actions"));
        assert!(sections.contains(&"Settings"));
        assert!(!sections.contains(&"Documents"));
    }

    struct Harness {
        ctx: egui::Context,
    }

    impl Harness {
        fn frame(
            &self,
            state: &mut SearchEverywhereState,
            events: Vec<egui::Event>,
        ) -> Option<SearchEverywhereOutcome> {
            let input = egui::RawInput {
                events,
                ..Default::default()
            };
            let documents = documents();
            let actions = actions();
            let mut outcome = None;
            crate::egui_test_support::run_ui_and_discard(&self.ctx, input, |ui| {
                outcome = show(ui.ctx(), state, &documents, &actions);
            });
            outcome
        }

        fn key(key: Key, modifiers: Modifiers) -> egui::Event {
            egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers,
            }
        }
    }

    fn opened() -> (Harness, SearchEverywhereState) {
        let harness = Harness {
            ctx: egui::Context::default(),
        };
        let mut state = SearchEverywhereState::default();
        state.request_open();
        state.text_cache = Some(text_cache());
        // First frame grants focus; a second lets it settle before a keypress,
        // same as `open_document_prompt.rs`'s test.
        harness.frame(&mut state, vec![]);
        harness.frame(&mut state, vec![]);
        (harness, state)
    }

    #[test]
    fn typing_and_enter_runs_the_matching_action() {
        let (harness, mut state) = opened();
        harness.frame(&mut state, vec![egui::Event::Text("spell che".to_string())]);
        let outcome = harness.frame(&mut state, vec![Harness::key(Key::Enter, Modifiers::NONE)]);
        assert_eq!(
            outcome,
            Some(SearchEverywhereOutcome::Run(ShortcutTarget::BuiltIn(
                ShortcutAction::ToggleSpellCheck
            )))
        );
        assert!(!state.open);
    }

    #[test]
    fn tab_switches_tabs_and_text_tab_opens_at_the_match() {
        let (harness, mut state) = opened();
        harness.frame(&mut state, vec![egui::Event::Text("emerald".to_string())]);
        harness.frame(&mut state, vec![Harness::key(Key::Tab, Modifiers::NONE)]);
        assert_eq!(state.tab, SearchTab::Documents);
        harness.frame(&mut state, vec![Harness::key(Key::Tab, Modifiers::NONE)]);
        assert_eq!(state.tab, SearchTab::Text);
        let outcome = harness.frame(&mut state, vec![Harness::key(Key::Enter, Modifiers::NONE)]);
        assert_eq!(
            outcome,
            Some(SearchEverywhereOutcome::OpenAt(
                PathBuf::from("/p/c1.md"),
                25
            ))
        );
    }

    #[test]
    fn escape_closes_and_drops_the_text_cache() {
        let (harness, mut state) = opened();
        harness.frame(&mut state, vec![Harness::key(Key::Escape, Modifiers::NONE)]);
        assert!(!state.open);
        assert!(state.text_cache.is_none());
    }
}
