use super::*;

impl SmaragdApp {
    /// Whether `self.collab` is a *live*, still-usable session — `true` for
    /// a session that's still trying to reconnect (`session_ended` stays
    /// `false` throughout, see `CollabSession::reconnecting`), not just one
    /// that's currently connected. An ended one (reconnection was exhausted,
    /// or a fatal error occurred — see `CollabSession::session_ended`) is
    /// cleared silently here rather than left blocking a fresh Host/Join
    /// with a stale "already active" error: once a session is truly over,
    /// the only way forward is starting a new one, and that shouldn't
    /// require an extra manual "End Session" click first.
    pub(super) fn collab_is_live(&mut self) -> bool {
        match &self.collab {
            Some(session) if !session.session_ended => true,
            Some(_) => {
                self.collab.take().expect("checked above").end();
                false
            }
            None => false,
        }
    }

    /// Starts hosting a collaboration session against whatever document is
    /// currently open — see `CollabSession::host`'s doc comment for why it
    /// starts empty and lets the first `sync_local_collab_edit` bootstrap
    /// the shared document from the live buffer.
    pub(super) fn start_collab_host(&mut self, ctx: &egui::Context) {
        if self.collab_is_live() {
            self.push_error_toast("A collaboration session is already active");
            return;
        }
        if self.editor.open_path.is_none() {
            self.push_error_toast("Open a document before hosting a collaboration session");
            return;
        }
        self.collab = Some(CollabSession::host(ctx.clone()));
        self.set_status_message("Hosting collaboration session…");
    }

    /// Joins a collaboration session using a pasted connection code. Refuses
    /// while a document is already open, sidestepping any question of what
    /// should happen to it: the shared document a join receives isn't tied
    /// to any of the joiner's own files (see `CollabSession`'s module doc).
    pub(super) fn start_collab_join(&mut self, ctx: &egui::Context, code: &str) {
        if self.collab_is_live() {
            self.push_error_toast("A collaboration session is already active");
            return;
        }
        if self.editor.open_path.is_some() {
            self.push_error_toast(
                "Close the current document before joining a collaboration session",
            );
            return;
        }
        self.collab = Some(CollabSession::join(code.to_string(), ctx.clone()));
        self.set_status_message("Joining collaboration session…");
    }

    /// Ends the active collaboration session, if any — used both by the
    /// explicit "End Session" action and by the document-switch teardown
    /// hook in `open_document`/`close_document`.
    pub(super) fn end_collab_session(&mut self, reason: impl Into<String>) {
        if let Some(session) = self.collab.take() {
            session.end();
            self.set_status_message(reason);
        }
    }

    pub(super) fn handle_collab_panel_event(
        &mut self,
        ctx: &egui::Context,
        event: CollabPanelEvent,
    ) {
        match event {
            CollabPanelEvent::HostRequested => self.start_collab_host(ctx),
            CollabPanelEvent::JoinRequested => {
                self.prompt = Some(PendingPrompt {
                    action: PromptAction::JoinCollabSession,
                    state: NamePromptState::new("Join Collaboration Session", "Join", ""),
                });
            }
            CollabPanelEvent::EndRequested => {
                self.end_collab_session("Collaboration session ended")
            }
        }
    }

    /// Applies queued collaboration events to the editor buffer — remote
    /// edits merged in, with the local cursor adjusted so it doesn't jump
    /// (see `collab::diff::adjust_cursor`) — and surfaces connection status
    /// changes. Called once per frame, before the editor renders, so this
    /// frame's `TextEdit` already reflects any merge.
    pub(super) fn poll_collab_events(&mut self, ctx: &egui::Context) {
        let Some(session) = &mut self.collab else {
            return;
        };
        for update in session.poll() {
            match update {
                SessionUpdate::TextChanged { new_text, change } => {
                    let editor_id = ui::editor_panel::editor_text_edit_id();
                    let cursor_byte = egui::TextEdit::load_state(ctx, editor_id)
                        .and_then(|state| state.cursor.char_range())
                        .map(|range| {
                            crate::autocomplete::char_offset_to_byte(
                                &self.editor.buffer,
                                range.primary.index.0,
                            )
                        });
                    let old_len = self.editor.buffer.len();
                    self.editor.buffer = new_text;
                    self.editor.mark_dirty();
                    // Heuristic, not a protocol-level signal (no wire
                    // changes needed for this): a `TextChange` that deletes
                    // the whole previous buffer from position 0 looks the
                    // same whether it's the host switching documents (see
                    // `open_document`) or an ordinary "select all, paste
                    // something else"/"replace all" edit that happens to
                    // touch everything — the latter is an accepted, minor
                    // false-positive here. `old_len > 0` excludes the
                    // initial empty-to-full bootstrap on first connect,
                    // which isn't a "switch".
                    if change.pos == 0 && change.deleted_len == old_len && old_len > 0 {
                        self.set_status_message("Your collaborator switched documents");
                    }
                    if let Some(cursor_byte) = cursor_byte {
                        let adjusted = crate::collab::diff::adjust_cursor(cursor_byte, &change);
                        ui::editor_panel::move_cursor_to(
                            ctx,
                            editor_id,
                            &self.editor.buffer,
                            adjusted,
                        );
                    }
                }
                SessionUpdate::PeerConnected { .. } => {
                    self.set_status_message("Collaborator connected");
                }
                SessionUpdate::Reconnecting => {
                    self.push_error_toast(
                        "Lost connection to your collaborator — trying to reconnect…",
                    );
                }
                SessionUpdate::PeerDisconnected => {
                    self.push_error_toast("Collaboration peer disconnected");
                }
                SessionUpdate::Error(message) => {
                    self.push_error_toast(format!("Collaboration error: {message}"));
                }
            }
        }
    }

    /// Diffs the live editor buffer against the collaboration session's
    /// last-synced baseline and ships any local edit to the peer. Called
    /// once per frame, after the editor has had a chance to render/mutate
    /// its buffer.
    pub(super) fn sync_local_collab_edit(&mut self) {
        if let Some(session) = &mut self.collab {
            session.sync_local_edit(&self.editor.buffer);
        }
    }
}
