use super::*;

impl SmaragdApp {
    /// Writes to `settings_path_override` if set (always the case for a
    /// `test_fixture`-built app — see that field's doc comment), otherwise
    /// the real `settings::config_file_path()`. Never resolve the real path
    /// directly here or in any other persistence method a test could reach
    /// — a test exercising `open_project`/`create_project` once wrote
    /// straight through to a developer's actual
    /// `~/.config/smaragd/smaragd.toml`, silently overwriting their real
    /// settings with `Settings::default()` plus whatever tempdir path the
    /// test happened to open, before this override existed.
    pub(super) fn persist_settings(&mut self) {
        let path = match &self.settings_path_override {
            Some(path) => path.clone(),
            None => {
                let Some(path) = crate::settings::config_file_path() else {
                    return;
                };
                path
            }
        };
        if let Err(err) = self.settings.save_to_path(&path) {
            self.push_error_toast(format!("Couldn't save settings: {err}"));
        }
    }

    /// Load the dock layout persisted by a previous run (see `persist_dock_layout`),
    /// falling back to `default_dock_state()` if there's nothing on disk yet or it
    /// fails to parse. Never a hard error: a missing/corrupt layout file shouldn't
    /// prevent the app from starting.
    ///
    /// Also guards against a layout that deserializes fine but has no `Editor` tab
    /// anywhere — e.g. one persisted by a build from before the editor became a
    /// dock tab, or a hand-edited file — which would otherwise leave no way to
    /// edit any document at all.
    pub(super) fn load_dock_state() -> egui_dock::DockState<DockTab> {
        let mut state = crate::settings::dock_layout_file_path()
            .and_then(|path| fs::read_to_string(path).ok())
            .and_then(|contents| serde_json::from_str(&contents).ok())
            .unwrap_or_else(default_dock_state);
        ensure_editor_tab_present(&mut state);
        state
    }

    /// Save which dock tabs are open and how they're split/floated, so the layout
    /// is exactly as the user left it next launch. Called once, when a window
    /// close is first requested (see the `close_requested` check in `ui`) — that's
    /// the one point that both still has a live `ctx` (needed by
    /// `capture_floating_window_positions`) and is guaranteed to see the final
    /// state; layout changes (dragging, splitting, closing a tab) happen far less
    /// often than every-frame writes would justify.
    pub(super) fn persist_dock_layout(&mut self, ctx: &egui::Context) {
        capture_floating_window_positions(&mut self.dock_state, ctx);
        let Some(path) = crate::settings::dock_layout_file_path() else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(contents) = serde_json::to_string_pretty(&self.dock_state) {
            let _ = fs::write(path, contents);
        }
    }

    /// Load the user's named, saved dock layouts (see `saved_layouts`), falling
    /// back to an empty map if there's nothing on disk yet or it fails to parse.
    pub(super) fn load_saved_layouts()
    -> std::collections::BTreeMap<String, egui_dock::DockState<DockTab>> {
        crate::settings::saved_layouts_file_path()
            .and_then(|path| fs::read_to_string(path).ok())
            .and_then(|contents| serde_json::from_str(&contents).ok())
            .unwrap_or_default()
    }

    /// Persist `saved_layouts` immediately — called right after a save, not
    /// deferred to shutdown like `persist_dock_layout`, since this only happens
    /// on an explicit user action rather than every frame.
    fn persist_saved_layouts(&self) {
        let Some(path) = crate::settings::saved_layouts_file_path() else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(contents) = serde_json::to_string_pretty(&self.saved_layouts) {
            let _ = fs::write(path, contents);
        }
    }

    /// Open the "Save Layout" name-prompt modal, so the user can name the current
    /// dock arrangement before it's added to `saved_layouts` (see `finish_prompt`'s
    /// `PromptAction::SaveLayout` arm).
    pub(super) fn prompt_save_layout(&mut self) {
        self.prompt = Some(PendingPrompt {
            action: PromptAction::SaveLayout,
            state: NamePromptState::new("Save Layout", "Save", ""),
        });
    }

    /// Snapshot the current dock layout under `name` (overwriting any existing
    /// layout of that name) and persist it right away.
    pub(super) fn save_named_layout(&mut self, ctx: &egui::Context, name: &str) {
        let mut snapshot = self.dock_state.clone();
        capture_floating_window_positions(&mut snapshot, ctx);
        self.saved_layouts.insert(name.to_string(), snapshot);
        self.persist_saved_layouts();
    }
}

#[cfg(test)]
mod settings_persistence_isolation_tests {
    use super::SmaragdApp;

    /// Regression test for the incident this override exists to prevent: a
    /// `test_fixture`-built app must never be able to write settings to the
    /// developer's real config path, no matter which app method a future
    /// test happens to exercise.
    #[test]
    fn test_fixture_always_overrides_the_settings_path_away_from_the_real_one() {
        let app = SmaragdApp::test_fixture();

        let override_path = app
            .settings_path_override
            .clone()
            .expect("test_fixture must always set an override");

        assert_ne!(Some(override_path), crate::settings::config_file_path());
    }

    #[test]
    fn persist_settings_writes_to_the_override_path_not_the_real_one() {
        let mut app = SmaragdApp::test_fixture();
        let override_path = app.settings_path_override.clone().unwrap();

        app.persist_settings();

        assert!(
            override_path.exists(),
            "persist_settings should have written to the override path"
        );
        let _ = std::fs::remove_file(&override_path);
    }
}

#[cfg(test)]
mod dock_layout_persistence_tests {
    use super::DockTab;

    /// `egui_dock::DockState` only derives `Clone`/`Debug`, not `PartialEq` — so
    /// round-tripping is checked by comparing the set of open tabs (and, since
    /// `iter_all_tabs` walks every surface/node, this also exercises a split
    /// layout's extra surface, not just the default single-surface case).
    fn tab_set(state: &egui_dock::DockState<DockTab>) -> Vec<DockTab> {
        let mut tabs: Vec<DockTab> = state.iter_all_tabs().map(|(_, tab)| *tab).collect();
        tabs.sort_by_key(|tab| format!("{tab:?}"));
        tabs
    }

    struct NoopViewer;

    impl egui_dock::TabViewer for NoopViewer {
        type Tab = DockTab;

        fn title(&mut self, tab: &mut DockTab) -> egui::WidgetText {
            format!("{tab:?}").into()
        }

        fn ui(&mut self, ui: &mut egui::Ui, _tab: &mut DockTab) {
            ui.label("test");
        }
    }

    /// Actually renders `state` through a real `DockArea` for one frame before
    /// handing it back — every `Node`'s `rect` starts out as `Rect::NOTHING`
    /// (`{+inf, +inf} .. {-inf, -inf}`, see `egui_dock`'s `LeafNode::new`), which
    /// JSON can't represent (`serde_json` silently emits `null` for an infinite
    /// f32, then fails to deserialize that `null` back into a plain, non-`Option`
    /// f32 field) — a real freshly-*un-rendered* `DockState` would hit this same
    /// trap, but `persist_dock_layout` only ever runs once the dock has already
    /// been shown every frame the app was open, so its rects are always concrete
    /// real numbers. Rendering once here first is what makes these tests
    /// representative of that, rather than of a state no code path actually ever
    /// tries to persist.
    fn rendered(mut state: egui_dock::DockState<DockTab>) -> egui_dock::DockState<DockTab> {
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            egui_dock::DockArea::new(&mut state).show_inside(ui, &mut NoopViewer);
        });
        state
    }

    #[test]
    fn default_single_tab_layout_round_trips_through_json() {
        let state = rendered(egui_dock::DockState::new(vec![DockTab::Binder]));

        let json = serde_json::to_string(&state).unwrap();
        let restored: egui_dock::DockState<DockTab> = serde_json::from_str(&json).unwrap();

        assert_eq!(tab_set(&restored), vec![DockTab::Binder]);
    }

    #[test]
    fn a_split_layout_with_multiple_tabs_round_trips_through_json() {
        let mut state = egui_dock::DockState::new(vec![DockTab::Binder]);
        state.push_to_focused_leaf(DockTab::Backlinks);
        state.push_to_focused_leaf(DockTab::Metadata);
        let state = rendered(state);

        let json = serde_json::to_string(&state).unwrap();
        let restored: egui_dock::DockState<DockTab> = serde_json::from_str(&json).unwrap();

        assert_eq!(
            tab_set(&restored),
            vec![DockTab::Backlinks, DockTab::Binder, DockTab::Metadata]
        );
    }

    /// The exact scenario from the bug report: a tab dragged out into its own
    /// floating window (here simulated with `detach_tab` rather than an actual
    /// drag) reopens with the right tabs, but at the wrong on-screen position.
    #[test]
    fn a_floating_windows_position_survives_a_save_and_reload_round_trip() {
        let mut state = egui_dock::DockState::new(vec![DockTab::Binder]);
        let detach_rect =
            egui::Rect::from_min_size(egui::pos2(400.0, 50.0), egui::vec2(200.0, 300.0));
        let window_index = state.detach_tab(
            egui_dock::TabPath::new(
                egui_dock::SurfaceIndex::main(),
                egui_dock::NodeIndex::root(),
                egui_dock::TabIndex(0),
            ),
            detach_rect,
        );

        // Render with one persistent `Context` (unlike `rendered` above, which
        // uses a fresh throwaway one per call) so the floating window's actual
        // placement lands in egui's own Area memory, the same way it would
        // across real frames in one running session.
        let live_ctx = egui::Context::default();
        let _ = live_ctx.run_ui(egui::RawInput::default(), |ui| {
            egui_dock::DockArea::new(&mut state).show_inside(ui, &mut NoopViewer);
        });

        super::super::dock::capture_floating_window_positions(&mut state, &live_ctx);

        // Round-trip through JSON exactly like `persist_dock_layout`/`load_dock_state`.
        let json = serde_json::to_string(&state).unwrap();
        let mut restored: egui_dock::DockState<DockTab> = serde_json::from_str(&json).unwrap();

        // A brand-new `Context` — no memory of the previous session's Area
        // positions at all — mirrors an actual app restart.
        let fresh_ctx = egui::Context::default();
        let _ = fresh_ctx.run_ui(egui::RawInput::default(), |ui| {
            egui_dock::DockArea::new(&mut restored).show_inside(ui, &mut NoopViewer);
        });
        let restored_rect = fresh_ctx
            .memory(|mem| mem.area_rect(super::super::dock::floating_window_id(window_index)))
            .expect("the floating window should have rendered at some rect");

        assert!(
            (restored_rect.min - detach_rect.min).length() < 1.0,
            "expected the floating window to reopen near {:?}, but it reopened at {:?}",
            detach_rect.min,
            restored_rect.min
        );
    }

    #[test]
    fn default_dock_state_has_exactly_binder_editor_metadata_and_backlinks() {
        let state = super::super::dock::default_dock_state();

        assert_eq!(
            tab_set(&state),
            vec![
                DockTab::Backlinks,
                DockTab::Binder,
                DockTab::Editor,
                DockTab::Metadata,
            ]
        );
    }

    #[test]
    fn default_dock_state_gives_the_editor_the_majority_of_the_width() {
        // Regression test: `split_left`'s `fraction` turned out to be the *new*
        // (left/Binder) node's share, not the old node's as its own doc comment
        // claims — a `fraction` of 0.78 was previously giving Binder 78% of the
        // width and Editor only 22%, backwards from the intent of a narrow
        // Binder column with Editor filling the rest.
        let state = rendered(super::super::dock::default_dock_state());

        let mut binder_width = None;
        let mut editor_width = None;
        for node in state.main_surface().iter() {
            let Some(rect) = node.rect() else { continue };
            match node.tabs() {
                Some(tabs) if tabs.contains(&DockTab::Binder) => binder_width = Some(rect.width()),
                Some(tabs) if tabs.contains(&DockTab::Editor) => editor_width = Some(rect.width()),
                _ => {}
            }
        }
        let binder_width = binder_width.expect("Binder should be a leaf with a rect");
        let editor_width = editor_width.expect("Editor should be a leaf with a rect");

        assert!(
            editor_width > binder_width,
            "expected Editor ({editor_width}) to occupy the majority of the width, \
             not Binder ({binder_width})"
        );
    }

    #[test]
    fn ensure_editor_tab_present_adds_editor_when_missing() {
        // Simulates a `dock_layout.json` persisted before the editor became a dock
        // tab (or a hand-edited file) — deserializes fine, but has no Editor tab.
        let mut state = egui_dock::DockState::new(vec![DockTab::Binder]);

        super::super::dock::ensure_editor_tab_present(&mut state);

        assert!(
            tab_set(&state).contains(&DockTab::Editor),
            "expected an Editor tab to be added when one wasn't already present"
        );
    }

    #[test]
    fn ensure_editor_tab_present_is_a_no_op_when_editor_already_exists() {
        let mut state = super::super::dock::default_dock_state();

        super::super::dock::ensure_editor_tab_present(&mut state);

        assert_eq!(
            tab_set(&state),
            vec![
                DockTab::Backlinks,
                DockTab::Binder,
                DockTab::Editor,
                DockTab::Metadata,
            ]
        );
    }

    #[test]
    fn named_saved_layouts_round_trip_through_json() {
        let mut saved: std::collections::BTreeMap<String, egui_dock::DockState<DockTab>> =
            std::collections::BTreeMap::new();
        saved.insert(
            "Writing".to_string(),
            rendered(egui_dock::DockState::new(vec![DockTab::Editor])),
        );
        let mut research = egui_dock::DockState::new(vec![DockTab::Binder]);
        research.push_to_focused_leaf(DockTab::Corkboard);
        saved.insert("Research".to_string(), rendered(research));

        let json = serde_json::to_string(&saved).unwrap();
        let restored: std::collections::BTreeMap<String, egui_dock::DockState<DockTab>> =
            serde_json::from_str(&json).unwrap();

        assert_eq!(
            restored.keys().cloned().collect::<Vec<_>>(),
            vec!["Research".to_string(), "Writing".to_string()],
            "BTreeMap should keep saved layouts sorted by name"
        );
        assert_eq!(tab_set(&restored["Writing"]), vec![DockTab::Editor]);
        assert_eq!(
            tab_set(&restored["Research"]),
            vec![DockTab::Binder, DockTab::Corkboard]
        );
    }
}

