use super::*;

impl SmaragdApp {
    /// Open or close a dock tab: present → removed, absent → opened in whichever
    /// leaf currently has focus.
    pub(super) fn toggle_dock_tab(&mut self, tab: DockTab) {
        if let Some(path) = self.dock_state.find_tab(&tab) {
            self.dock_state.remove_tab(path);
        } else {
            self.dock_state.push_to_focused_leaf(tab);
        }
    }

    /// Opens `tab` as a new tab in the same dock node as `anchor` — e.g. next to
    /// the editor — rather than wherever `push_to_focused_leaf` would land it
    /// (whichever leaf last had focus, which could be Binder's or anything else).
    /// Falls back to `push_to_focused_leaf` if `anchor` isn't currently open.
    fn open_tab_next_to(&mut self, tab: DockTab, anchor: DockTab) {
        match self.dock_state.find_tab(&anchor) {
            Some(path) => self.dock_state[path.surface][path.node].append_tab(tab),
            None => self.dock_state.push_to_focused_leaf(tab),
        }
    }

    /// Like `toggle_dock_tab`, but opens `tab` next to `anchor` (see
    /// `open_tab_next_to`) instead of wherever's focused — for tabs that
    /// conceptually pair with the editor (Preview, Corkboard), so toggling them
    /// doesn't land somewhere surprising depending on what the user last clicked.
    pub(super) fn toggle_dock_tab_near(&mut self, tab: DockTab, anchor: DockTab) {
        if let Some(path) = self.dock_state.find_tab(&tab) {
            self.dock_state.remove_tab(path);
        } else {
            self.open_tab_next_to(tab, anchor);
        }
    }

    /// Enable/disable Focus Mode, keeping the OS window maximized in lock-step
    /// with it — Scrivener's Composition Mode works the same way (entering
    /// always goes fullscreen, leaving always leaves it), so there's no
    /// separate "was already maximized" state to track. Refuses to *enter*
    /// with no document open — there'd be nothing to show and no Binder to
    /// pick one from, since Focus Mode hides everything else.
    ///
    /// Uses `Maximized`, not `Fullscreen`: on Wayland compositors with patchy
    /// `xdg-shell` fullscreen support (e.g. niri, a scrollable-tiling
    /// compositor where "fullscreen" is a less-trodden path than the
    /// tiling-native "maximize") `Fullscreen` can report a viewport size that
    /// doesn't match what's actually visible — a real winit/niri interaction
    /// bug, not something fixable from egui's side of the layout. `Maximized`
    /// is what a tiling compositor already handles constantly and reliably.
    pub(super) fn set_focus_mode(&mut self, ctx: &egui::Context, enabled: bool) {
        if enabled && self.editor.open_path.is_none() {
            self.push_error_toast("Open a document before entering Focus Mode.");
            return;
        }
        self.focus_mode = enabled;
        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(enabled));
    }

    /// Resolve a `[[wikilink]]` activated (clicked in the preview, or Ctrl+Enter in
    /// the editor) to a document in the current project (matched by filename,
    /// case-insensitively) and open it. If it doesn't exist and `force_create` was
    /// requested (Ctrl/Cmd was held), create it in the same folder as the document
    /// the link was activated from.
    pub(super) fn activate_wikilink(&mut self, activation: WikilinkActivation) {
        let WikilinkActivation {
            target,
            force_create,
        } = activation;
        let Some(project) = &self.project else {
            self.push_error_toast(format!("No project open — can't resolve [[{target}]]"));
            return;
        };
        if let Some(node) = project.tree.find_document_by_stem(&target) {
            let path = node.path.clone();
            self.open_document(&path);
            return;
        }
        if !force_create {
            self.push_error_toast(format!("No note found for [[{target}]]"));
            return;
        }
        self.create_wikilink_target(&target);
    }

    /// Create a new document named `target` in the same folder as the document
    /// currently open (i.e. the one containing the wikilink that was activated), then
    /// open it.
    fn create_wikilink_target(&mut self, target: &str) {
        let Some(parent) = self
            .editor
            .open_path
            .as_ref()
            .and_then(|p| p.parent())
            .map(Path::to_path_buf)
        else {
            self.push_error_toast(format!(
                "Couldn't create a note for [[{target}]]: no document is open"
            ));
            return;
        };
        self.create_document(&parent, target);
    }

    pub(super) fn handle_binder_event(&mut self, ctx: &egui::Context, event: BinderEvent) {
        match event {
            BinderEvent::Selected(path) => self.open_document(&path),
            BinderEvent::NewFile { parent } => self.prompt_new_file(parent),
            BinderEvent::NewFolder { parent } => self.prompt_new_folder(parent),
            BinderEvent::NewFileFromTemplate {
                parent,
                template_path,
            } => self.prompt_new_file_from_template(parent, template_path),
            BinderEvent::Rename { path } => self.prompt_rename(path),
            BinderEvent::Delete { path } => self.delete_node(&path),
            BinderEvent::Restore { path } => self.restore_node(&path),
            BinderEvent::SetFolderRole { path, role } => self.set_folder_role(ctx, &path, role),
            BinderEvent::SetPicklistFolder { field, path } => self.set_picklist_folder(field, path),
            BinderEvent::EmptyTrash { path } => self.empty_trash_folder(&path),
            BinderEvent::MoveItem { path, new_parent } => self.move_item(&path, &new_parent),
            BinderEvent::MoveItemBefore { path, before } => self.move_item_before(&path, &before),
            BinderEvent::Export { path } => self.open_export(path),
        }
    }

    pub(super) fn handle_corkboard_event(&mut self, event: CorkboardEvent) {
        match event {
            CorkboardEvent::CreateCard => self.card_draft = Some(CardDraft::new()),
            CorkboardEvent::EditCard(id) => {
                if let Some(project) = &self.project
                    && let Some(card) = project.story_card(id)
                {
                    self.card_draft = Some(CardDraft::from_card(card));
                }
            }
            CorkboardEvent::DeleteCard(id) => {
                if let Some(project) = &mut self.project
                    && let Err(err) = project.delete_story_card(id)
                {
                    self.push_error_toast(format!("Couldn't delete card: {err}"));
                }
            }
            CorkboardEvent::MoveCard { id, new_index } => {
                if let Some(project) = &mut self.project
                    && let Err(err) = project.move_story_card(id, new_index)
                {
                    self.push_error_toast(format!("Couldn't reorder card: {err}"));
                }
            }
            CorkboardEvent::OpenLinkedDocument(path) => {
                self.open_linked_document_and_focus_editor(path, DockTab::Corkboard);
            }
            CorkboardEvent::SetProtagonistDesire(desire) => {
                if let Some(project) = &mut self.project
                    && let Err(err) = project.set_protagonist_desire(desire)
                {
                    self.push_error_toast(format!("Couldn't save desire: {err}"));
                }
            }
            CorkboardEvent::SetProtagonistMisbelief(misbelief) => {
                if let Some(project) = &mut self.project
                    && let Err(err) = project.set_protagonist_misbelief(misbelief)
                {
                    self.push_error_toast(format!("Couldn't save misbelief: {err}"));
                }
            }
        }
    }

    /// Open `path` in the Editor tab and focus it, falling back to opening a new
    /// Editor tab next to `anchor` if none is open yet — shared by both Corkboard's
    /// and the Story Grid's "open linked document" link, which differ only in
    /// which tab they fall back to opening the Editor next to.
    fn open_linked_document_and_focus_editor(&mut self, path: PathBuf, anchor: DockTab) {
        self.open_document(&path);
        match self.dock_state.find_tab(&DockTab::Editor) {
            Some(tab_path) => {
                let _ = self.dock_state.set_active_tab(tab_path);
            }
            None => self.open_tab_next_to(DockTab::Editor, anchor),
        }
    }

    pub(super) fn handle_story_grid_event(&mut self, event: StoryGridEvent) {
        match event {
            StoryGridEvent::OpenLinkedDocument(path) => {
                self.open_linked_document_and_focus_editor(path, DockTab::StoryGrid);
            }
            StoryGridEvent::EditCard(id) => {
                self.handle_corkboard_event(CorkboardEvent::EditCard(id))
            }
            StoryGridEvent::SetUnplacedPosition(position) => {
                self.settings.unplaced_story_cards_position = position;
                self.persist_settings();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corkboard_event_updates_protagonist_desire_on_the_open_project() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::initialize(dir.path()).unwrap();
        let mut app = SmaragdApp::test_fixture();
        app.project = Some(project);

        app.handle_corkboard_event(CorkboardEvent::SetProtagonistDesire(
            "Reclaim the throne".to_string(),
        ));

        assert_eq!(
            app.project.as_ref().unwrap().meta.protagonist_desire,
            "Reclaim the throne"
        );
    }

    #[test]
    fn corkboard_event_is_a_no_op_without_an_open_project() {
        let mut app = SmaragdApp::test_fixture();

        // Must not panic when there's nothing to apply the edit to.
        app.handle_corkboard_event(CorkboardEvent::SetProtagonistMisbelief(
            "Unworthy of the crown".to_string(),
        ));

        assert!(app.project.is_none());
    }
}
