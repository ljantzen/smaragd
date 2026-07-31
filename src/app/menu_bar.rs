use super::*;

impl SmaragdApp {
    /// Renders the top menu bar (File/Edit/View/Tools/Versions/Window/Help) —
    /// hidden during Focus Mode. Extracted from `ui()` verbatim (2026-07-31
    /// code-quality review: that function was 766 lines).
    pub(super) fn show_menu_bar(&mut self, ui: &mut egui::Ui) {
        if !self.focus_mode {
            egui::Panel::top("menu_bar").show(ui, |ui| {
                egui::MenuBar::new().ui(ui, |ui| {
                    top_menu_button(ui, "File", egui::Key::F, |ui, nav| {
                        let new_project_shortcut =
                            self.settings.shortcuts.get(ShortcutAction::NewProject);
                        let open_project_shortcut =
                            self.settings.shortcuts.get(ShortcutAction::OpenProject);
                        let open_settings_shortcut =
                            self.settings.shortcuts.get(ShortcutAction::OpenSettings);
                        let exit_shortcut = self.settings.shortcuts.get(ShortcutAction::Exit);
                        let open_document_shortcut =
                            self.settings.shortcuts.get(ShortcutAction::OpenDocument);
                        let close_document_shortcut =
                            self.settings.shortcuts.get(ShortcutAction::CloseDocument);

                        if nav
                            .shortcut_button(ui, "New Project", new_project_shortcut)
                            .clicked()
                        {
                            self.start_new_project();
                        }
                        if nav
                            .shortcut_button(ui, "Open Project", open_project_shortcut)
                            .clicked()
                        {
                            self.browse_for_project(ui.ctx());
                        }
                        // `SubMenuButton`, not `MenuButton` — see the matching comment on
                        // View's "Theme" submenu for why. Trigger row tracked the same way.
                        let (recent_trigger, _) = egui::containers::menu::SubMenuButton::new(
                            "Recent Projects",
                        )
                        .ui(ui, |ui| {
                            if self.settings.recent_project_paths.is_empty() {
                                ui.add_enabled(false, egui::Button::new("No recent projects"));
                            } else {
                                // Collected up front rather than iterating
                                // `self.settings.recent_project_paths` directly:
                                // clicking an entry needs `&mut self`, which an
                                // active immutable borrow of `self.settings` (the
                                // loop) would conflict with.
                                let paths = self.settings.recent_project_paths.clone();
                                for path in paths {
                                    let label = path
                                        .file_name()
                                        .map(|name| name.to_string_lossy().into_owned())
                                        .unwrap_or_else(|| path.display().to_string());
                                    if ui
                                        .button(label)
                                        .on_hover_text(path.display().to_string())
                                        .clicked()
                                    {
                                        self.open_project_or_offer_to_adopt(ui.ctx(), &path);
                                        ui.close();
                                    }
                                }
                            }
                        });
                        nav.track(ui, &recent_trigger);
                        ui.add_enabled(false, egui::Button::new("Close Project"));
                        ui.separator();
                        if nav
                            .shortcut_button(ui, "Open Document…", open_document_shortcut)
                            .clicked()
                        {
                            if self.project.is_some() {
                                self.open_document_prompt.request_open();
                            } else {
                                self.push_error_toast("No project open");
                            }
                        }
                        if nav
                            .shortcut_button(ui, "Close Document", close_document_shortcut)
                            .clicked()
                        {
                            let ctx = ui.ctx().clone();
                            self.close_document(&ctx);
                        }
                        if nav
                            .shortcut_button(ui, "Save Project as Template…", None)
                            .clicked()
                        {
                            if self.project.is_some() {
                                self.prompt_save_project_as_template();
                            } else {
                                self.push_error_toast("No project open");
                            }
                        }
                        // Manuscript isn't an exclusive role — a project can have
                        // several Manuscript folders at once (see
                        // `FolderRole::is_exclusive`) — so this offers a submenu to
                        // choose among them once there's more than one, rather than
                        // silently picking just the first.
                        let manuscript_folders = self
                            .project
                            .as_ref()
                            .map(|project| {
                                project.folder_role_paths(crate::project::FolderRole::Manuscript)
                            })
                            .unwrap_or_default();
                        match manuscript_folders.as_slice() {
                            [] => {
                                if nav
                                    .shortcut_button(ui, "Export Manuscript…", None)
                                    .clicked()
                                {
                                    if let Some(project) = &self.project {
                                        self.open_export(project.root.clone());
                                    } else {
                                        self.push_error_toast("No project open");
                                    }
                                }
                            }
                            [only] => {
                                if nav
                                    .shortcut_button(ui, "Export Manuscript…", None)
                                    .clicked()
                                {
                                    self.open_export(only.clone());
                                }
                            }
                            many => {
                                // Not keyboard-navigable past its own trigger row — see
                                // the plan's scope note on nested submenus (Theme/
                                // Layouts/this) staying mouse/hover-only for now.
                                let outer = ui.menu_button("Export Manuscript", |ui| {
                                    for path in many {
                                        let label = self
                                            .project
                                            .as_ref()
                                            .and_then(|project| project.tree.find_by_path(path))
                                            .map(|node| node.name.clone())
                                            .unwrap_or_else(|| path.display().to_string());
                                        if ui.button(format!("{label}…")).clicked() {
                                            self.open_export(path.clone());
                                            ui.close();
                                        }
                                    }
                                });
                                nav.track(ui, &outer.response);
                            }
                        }
                        ui.separator();
                        if nav
                            .shortcut_button(ui, "Settings", open_settings_shortcut)
                            .clicked()
                        {
                            self.show_settings = true;
                        }
                        ui.separator();
                        if nav.shortcut_button(ui, "Exit", exit_shortcut).clicked() {
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });
                    top_menu_button(ui, "Edit", egui::Key::E, |ui, nav| {
                        if nav
                            .shortcut_button(
                                ui,
                                "Cut",
                                Some(egui::KeyboardShortcut::new(
                                    egui::Modifiers::COMMAND,
                                    egui::Key::X,
                                )),
                            )
                            .clicked()
                        {
                            ui.ctx()
                                .send_viewport_cmd(egui::ViewportCommand::RequestCut);
                        }
                        if nav
                            .shortcut_button(
                                ui,
                                "Copy",
                                Some(egui::KeyboardShortcut::new(
                                    egui::Modifiers::COMMAND,
                                    egui::Key::C,
                                )),
                            )
                            .clicked()
                        {
                            ui.ctx()
                                .send_viewport_cmd(egui::ViewportCommand::RequestCopy);
                        }
                        if nav
                            .shortcut_button(
                                ui,
                                "Paste",
                                Some(egui::KeyboardShortcut::new(
                                    egui::Modifiers::COMMAND,
                                    egui::Key::V,
                                )),
                            )
                            .clicked()
                        {
                            ui.ctx()
                                .send_viewport_cmd(egui::ViewportCommand::RequestPaste);
                        }
                        ui.separator();
                        let find_replace_shortcut =
                            self.settings.shortcuts.get(ShortcutAction::FindReplace);
                        if nav
                            .shortcut_button(ui, "Find and Replace", find_replace_shortcut)
                            .clicked()
                        {
                            self.find_replace.request_open();
                        }
                        let metadata_shortcut =
                            self.settings.shortcuts.get(ShortcutAction::EditMetadata);
                        if nav
                            .shortcut_button(ui, "Document Metadata", metadata_shortcut)
                            .clicked()
                        {
                            self.toggle_dock_tab(DockTab::Metadata);
                        }
                    });
                    top_menu_button(ui, "View", egui::Key::V, |ui, nav| {
                        if nav.button(ui, "Focus Mode").clicked() {
                            let ctx = ui.ctx().clone();
                            self.set_focus_mode(&ctx, !self.focus_mode);
                        }
                        ui.separator();
                        if nav.button(ui, "Editor").clicked() {
                            self.toggle_dock_tab(DockTab::Editor);
                        }
                        if nav.button(ui, "Preview").clicked() {
                            self.toggle_dock_tab_near(DockTab::Preview, DockTab::Editor);
                        }
                        if nav.button(ui, "Corkboard").clicked() {
                            self.toggle_dock_tab_near(DockTab::Corkboard, DockTab::Editor);
                        }
                        ui.separator();
                        if nav.button(ui, "Binder").clicked() {
                            self.toggle_dock_tab(DockTab::Binder);
                        }
                        if nav.button(ui, "Backlinks").clicked() {
                            self.toggle_dock_tab(DockTab::Backlinks);
                        }
                        if nav.button(ui, "Tags").clicked() {
                            self.toggle_dock_tab(DockTab::Tags);
                        }
                        ui.separator();
                        // `SubMenuButton`, not `MenuButton`: this is nested *inside* the
                        // View menu, and `MenuButton` is for top-level, click-to-open menu
                        // bar buttons. Using it here meant clicking "Theme" behaved like
                        // opening a second, independent top-level menu rather than a
                        // proper submenu — items inside never got a chance to run, since
                        // the parent popup's own close-on-click handling collapsed it out
                        // from under `SubMenuButton`'s (hover-to-open, keeps parents open)
                        // dedicated handling for exactly this case. Its trigger row is
                        // still tracked (so Up/Down/Left/Right can reach it like any other
                        // row), but not arrow-navigable past it — the flyout stays
                        // mouse/hover-only for now (see the arrow-nav plan's scope note).
                        let (theme_trigger, _) =
                            egui::containers::menu::SubMenuButton::new("Theme").ui(ui, |ui| {
                                if ui.button("Reload Custom Themes").clicked() {
                                    let ctx = ui.ctx().clone();
                                    self.reload_color_themes(&ctx);
                                }
                                ui.separator();
                                // Cloned rather than borrowed: `set_color_theme` below needs
                                // `&mut self`, which a live borrow of `self.settings`/
                                // `self.color_themes` here would conflict with across loop
                                // iterations.
                                let current = self.settings.color_theme.clone();
                                let themes = self.color_themes.clone();
                                if ui.radio(current.is_none(), "Default").clicked() {
                                    self.set_color_theme(ui.ctx(), None);
                                }
                                for theme in &themes {
                                    if ui
                                        .radio(
                                            current.as_deref() == Some(theme.id.as_str()),
                                            &theme.label,
                                        )
                                        .clicked()
                                    {
                                        self.set_color_theme(ui.ctx(), Some(&theme.id));
                                    }
                                }
                            });
                        nav.track(ui, &theme_trigger);
                    });
                    top_menu_button(ui, "Tools", egui::Key::T, |ui, nav| {
                        let command_prompt_shortcut =
                            self.settings.shortcuts.get(ShortcutAction::CommandPrompt);
                        if nav
                            .shortcut_button(ui, "Command Prompt", command_prompt_shortcut)
                            .clicked()
                        {
                            self.command_prompt.request_open();
                        }
                        let pomodoro_shortcut =
                            self.settings.shortcuts.get(ShortcutAction::TogglePomodoro);
                        if nav
                            .shortcut_button(ui, "Pomodoro Timer", pomodoro_shortcut)
                            .clicked()
                        {
                            self.toggle_dock_tab(DockTab::Pomodoro);
                        }
                        let word_count_shortcut =
                            self.settings.shortcuts.get(ShortcutAction::ToggleWordCount);
                        if nav
                            .shortcut_button(ui, "Word Count", word_count_shortcut)
                            .clicked()
                        {
                            self.toggle_dock_tab(DockTab::WordCount);
                        }
                        let refresh_word_count_shortcut = self
                            .settings
                            .shortcuts
                            .get(ShortcutAction::RefreshWordCount);
                        if nav
                            .shortcut_button(ui, "Refresh Word Count", refresh_word_count_shortcut)
                            .clicked()
                        {
                            self.spawn_word_count_recompute(ui.ctx());
                        }
                        ui.separator();
                        if nav.button(ui, "Reload Plugins").clicked() {
                            self.reload_plugins();
                        }
                        let project_plugins_enabled = self
                            .project
                            .as_ref()
                            .is_some_and(|project| project.meta.plugins_enabled);
                        if self.project.is_some()
                            && !project_plugins_enabled
                            && nav.button(ui, "Enable Project Plugins").clicked()
                        {
                            if let Some(project) = &mut self.project
                                && let Err(err) = project.set_plugins_enabled(true)
                            {
                                self.push_error_toast(format!(
                                    "Couldn't enable project plugins: {err}"
                                ));
                            }
                            self.reload_plugins();
                        }
                    });
                    // "S" rather than "V" (Versions' first letter) since View already
                    // claims Alt+V — matches the classic Windows-mnemonic convention
                    // of falling back to a distinguishing later letter on collision.
                    top_menu_button(ui, "Versions", egui::Key::S, |ui, nav| {
                        let git_enabled = self
                            .project
                            .as_ref()
                            .is_some_and(|project| project.meta.git_enabled);
                        if !git_enabled {
                            if nav.button(ui, "Enable Git Support").clicked() {
                                self.enable_git_support_manually();
                            }
                        } else {
                            let commit_shortcut =
                                self.settings.shortcuts.get(ShortcutAction::GitCommit);
                            if nav.shortcut_button(ui, "Commit", commit_shortcut).clicked() {
                                self.prompt_git_commit(false);
                            }
                            // Push/pull run on a background thread (see `spawn_git_operation`);
                            // disabled while one is already in flight rather than letting a
                            // second click queue up or race it. `MenuNav::track`'s
                            // `ui.is_enabled()` check means this trio is automatically
                            // skipped by arrow-key navigation while busy, with no separate
                            // bookkeeping needed.
                            let git_busy = self.pending_git.is_some();
                            ui.add_enabled_ui(!git_busy, |ui| {
                                if nav.button(ui, "Commit and Push").clicked() {
                                    self.prompt_git_commit(true);
                                }
                                let push_shortcut =
                                    self.settings.shortcuts.get(ShortcutAction::GitPush);
                                if nav.shortcut_button(ui, "Push", push_shortcut).clicked() {
                                    self.run_git_push(ui.ctx());
                                }
                                if nav.button(ui, "Pull").clicked() {
                                    self.run_git_pull(ui.ctx());
                                }
                            });
                        }
                    });
                    top_menu_button(ui, "Collaborate", egui::Key::C, |ui, nav| {
                        // A session that's already ended (peer disconnected, or a
                        // fatal error — see `CollabSession::session_ended`) doesn't
                        // count as "active" here: `collab_is_live` clears it
                        // automatically once Host/Join is actually clicked, so
                        // graying those out after a disconnect would just add an
                        // extra, unnecessary "End Session" click before starting over.
                        let collab_live = self.collab.as_ref().is_some_and(|s| !s.session_ended);
                        let can_host = !collab_live && self.editor.open_path.is_some();
                        ui.add_enabled_ui(can_host, |ui| {
                            if nav.button(ui, "Host Session").clicked() {
                                self.start_collab_host(ui.ctx());
                            }
                        });
                        ui.add_enabled_ui(!collab_live, |ui| {
                            if nav.button(ui, "Join Session…").clicked() {
                                self.prompt = Some(PendingPrompt {
                                    action: PromptAction::JoinCollabSession,
                                    state: NamePromptState::new(
                                        "Join Collaboration Session",
                                        "Join",
                                        "",
                                    ),
                                });
                            }
                        });
                        ui.add_enabled_ui(self.collab.is_some(), |ui| {
                            if nav.button(ui, "End Session").clicked() {
                                self.end_collab_session("Collaboration session ended");
                            }
                        });
                        ui.separator();
                        let toggle_collab_shortcut = self
                            .settings
                            .shortcuts
                            .get(ShortcutAction::ToggleCollabPanel);
                        if nav
                            .shortcut_button(ui, "Collaboration Panel", toggle_collab_shortcut)
                            .clicked()
                        {
                            self.toggle_dock_tab(DockTab::Collab);
                        }
                    });
                    top_menu_button(ui, "Window", egui::Key::W, |ui, nav| {
                        if nav.button(ui, "Save Current Layout…").clicked() {
                            self.prompt_save_layout();
                        }
                        // `SubMenuButton`, not `MenuButton` — see the matching comment on
                        // View's "Theme" submenu for why. Trigger row tracked the same way.
                        let (layouts_trigger, _) =
                            egui::containers::menu::SubMenuButton::new("Layouts").ui(ui, |ui| {
                                if self.saved_layouts.is_empty() {
                                    ui.add_enabled(false, egui::Button::new("No saved layouts"));
                                } else {
                                    // Collected up front rather than iterating
                                    // `self.saved_layouts` directly: clicking an entry needs
                                    // `&mut self.dock_state`, which an active immutable borrow
                                    // of `self.saved_layouts` (the loop) would conflict with.
                                    let names: Vec<String> =
                                        self.saved_layouts.keys().cloned().collect();
                                    for name in names {
                                        if ui.button(&name).clicked()
                                            && let Some(layout) = self.saved_layouts.get(&name)
                                        {
                                            self.dock_state = layout.clone();
                                        }
                                    }
                                }
                            });
                        nav.track(ui, &layouts_trigger);
                        ui.separator();
                        if nav.button(ui, "Restore Default Layout").clicked() {
                            self.dock_state = default_dock_state();
                        }
                    });
                    top_menu_button(ui, "Help", egui::Key::H, |ui, nav| {
                        if nav.button(ui, "About").clicked() {
                            self.show_about = true;
                        }
                    });
                });
            });
        }
    }
}
