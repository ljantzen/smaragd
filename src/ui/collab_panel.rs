/// What the user asked for this frame, handled by the caller (`app.rs`) —
/// same pure-rendering-layer split as `corkboard_panel`/`CorkboardEvent`.
/// `JoinRequested` doesn't carry the code itself: the caller opens the
/// existing paste-code modal (`ui::name_prompt`) rather than this panel
/// growing its own text field, so joining works the same whether started
/// from the Collaborate menu or from here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollabPanelEvent {
    HostRequested,
    JoinRequested,
    EndRequested,
}

/// The panel's current phase, derived by the caller from `SmaragdApp::collab`
/// — this module has no knowledge of `CollabSession`'s internals, only
/// whatever borrowed pieces it needs to render.
#[derive(Clone, Copy)]
pub enum CollabStatus<'a> {
    Idle,
    /// Networking is still coming up (a fresh host waiting on its own
    /// connection code, or a joiner that hasn't connected yet) — nothing
    /// concrete to show but that something is in progress.
    Connecting,
    Hosting {
        code: &'a str,
    },
    Connected {
        peer_fingerprint: &'a str,
    },
}

/// Renders the Collaborate dock tab: connection code to share while hosting,
/// a short peer fingerprint once connected, and Host/Join/End entry points
/// duplicated here so the panel is self-sufficient without needing the menu.
pub fn show(ui: &mut egui::Ui, status: CollabStatus) -> Option<CollabPanelEvent> {
    let mut event = None;
    ui.vertical_centered(|ui| {
        ui.add_space(16.0);
        match status {
            CollabStatus::Idle => {
                ui.label("No collaboration session active.");
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Host Session").clicked() {
                        event = Some(CollabPanelEvent::HostRequested);
                    }
                    if ui.button("Join Session…").clicked() {
                        event = Some(CollabPanelEvent::JoinRequested);
                    }
                });
            }
            CollabStatus::Connecting => {
                ui.label("Connecting…");
                ui.add_space(8.0);
                if ui.button("Cancel").clicked() {
                    event = Some(CollabPanelEvent::EndRequested);
                }
            }
            CollabStatus::Hosting { code } => {
                ui.label("Share this code with your collaborator:");
                ui.add_space(8.0);
                let mut code_buf = code.to_string();
                ui.add(
                    egui::TextEdit::singleline(&mut code_buf)
                        .desired_width(f32::INFINITY)
                        .interactive(false),
                );
                ui.add_space(4.0);
                if ui.button("Copy").clicked() {
                    ui.ctx().copy_text(code.to_string());
                }
                ui.add_space(8.0);
                ui.weak("Waiting for a peer to join…");
                ui.add_space(8.0);
                if ui.button("Cancel").clicked() {
                    event = Some(CollabPanelEvent::EndRequested);
                }
            }
            CollabStatus::Connected { peer_fingerprint } => {
                ui.label(format!("Connected to peer {peer_fingerprint}"));
                ui.add_space(8.0);
                if ui.button("End Session").clicked() {
                    event = Some(CollabPanelEvent::EndRequested);
                }
            }
        }
    });
    event
}
