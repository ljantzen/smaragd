use crate::search::{SearchMatch, SearchScope};

/// UI state for the Find and Replace window, owned by `app.rs` for the app's
/// lifetime (rather than only while open) so results and field contents survive
/// closing and reopening the panel.
#[derive(Default)]
pub struct FindReplaceState {
    pub open: bool,
    /// Set alongside `open` whenever the panel is (re)opened via menu or shortcut, so
    /// `show` can focus the query field once rather than fighting the user for focus
    /// on every frame the window is visible.
    pub focus_requested: bool,
    pub query: String,
    pub replacement: String,
    pub case_sensitive: bool,
    pub scope: SearchScope,
    pub results: Vec<SearchMatch>,
}

impl FindReplaceState {
    /// Open the panel (or bring it back to front if already open) and queue focusing
    /// the query field.
    pub fn request_open(&mut self) {
        self.open = true;
        self.focus_requested = true;
    }
}

/// What the panel wants `app.rs` to do this frame; `app.rs` owns the actual file I/O
/// and buffer mutation since it alone knows which file (if any) is the live editor
/// buffer versus one that must be read/written on disk.
pub enum FindReplaceEvent {
    Search,
    ReplaceAll,
    /// Index into `FindReplaceState::results` of the match the user clicked.
    OpenResult(usize),
}

/// Renders the Find and Replace window if `state.open`. Returns `Some` if the user
/// triggered a search, a replace-all, or clicked a result this frame.
pub fn show(ctx: &egui::Context, state: &mut FindReplaceState) -> Option<FindReplaceEvent> {
    if !state.open {
        return None;
    }

    let mut event = None;
    let mut open = state.open;
    egui::Window::new("Find and Replace")
        .open(&mut open)
        .resizable(true)
        .default_width(360.0)
        .show(ctx, |ui| {
            let query_response = ui.horizontal(|ui| {
                ui.label("Find:");
                ui.text_edit_singleline(&mut state.query)
            });
            let query_response = query_response.inner;
            if state.focus_requested {
                query_response.request_focus();
                state.focus_requested = false;
            }
            let searched_by_enter =
                query_response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));

            ui.horizontal(|ui| {
                ui.label("Replace:");
                ui.text_edit_singleline(&mut state.replacement);
            });

            ui.checkbox(&mut state.case_sensitive, "Case sensitive");

            ui.horizontal(|ui| {
                ui.label("Scope:");
                egui::ComboBox::new("find_replace_scope", "")
                    .selected_text(state.scope.label())
                    .show_ui(ui, |ui| {
                        for scope in SearchScope::ALL {
                            ui.selectable_value(&mut state.scope, scope, scope.label());
                        }
                    });
            });

            ui.horizontal(|ui| {
                if ui.button("Find All").clicked() || searched_by_enter {
                    event = Some(FindReplaceEvent::Search);
                }
                let replace_enabled = !state.query.is_empty();
                if ui
                    .add_enabled(replace_enabled, egui::Button::new("Replace All"))
                    .clicked()
                {
                    event = Some(FindReplaceEvent::ReplaceAll);
                }
            });

            ui.separator();
            ui.label(match state.results.len() {
                0 => "No matches".to_string(),
                1 => "1 match".to_string(),
                n => format!("{n} matches"),
            });
            egui::ScrollArea::vertical()
                .max_height(240.0)
                .show(ui, |ui| {
                    for (index, result) in state.results.iter().enumerate() {
                        let label = format!(
                            "{}:{}: {}",
                            result.path.display(),
                            result.line,
                            result.line_text.trim()
                        );
                        if ui.selectable_label(false, label).clicked() {
                            event = Some(FindReplaceEvent::OpenResult(index));
                        }
                    }
                });
        });
    state.open = open;

    event
}
