//! Peer-to-peer real-time collaborative editing.
//!
//! Design note: the CRDT (`crdt`) and its text diffing (`diff`) both run on
//! the main/UI thread rather than on the background networking thread,
//! because the diff step needs direct access to the live `egui` text buffer
//! anyway. Keeping apply/encrypt there too means neither the CRDT document
//! nor cipher state ever needs to be `Send`, and keeps this module's logic
//! fully synchronous and unit-testable without a real network. The
//! networking layer (`net`) only ever moves already-encoded/encrypted bytes
//! between here and the peer.
//!
//! Phase A: the CRDT engine (`crdt`) and its diffing (`diff`), proven
//! convergent against in-process documents only.
//!
//! Phase B: `ticket` (the pasteable connection code) and `net` (the iroh
//! networking, on its own background thread).
//!
//! Phase C: `crypto`, the app-level end-to-end encryption layered on top of
//! iroh's own transport security, wired into `net` so every frame exchanged
//! between peers is ciphertext.
//!
//! Phase D: [`CollabSession`] — the `SmaragdApp`-facing surface
//! tying `crdt`/`diff` to a running `net` session. Its `doc`/`last_synced`
//! baseline always start empty regardless of role: whichever side already
//! has the document open (normally the host) has its very next
//! [`CollabSession::sync_local_edit`] diff the whole existing buffer against
//! that empty baseline, producing one big "insert everything" edit that
//! bootstraps the shared document — the ordinary per-frame diffing loop
//! doubles as the initial sync, with no separate state-vector round trip
//! needed for two peers.
//!
//! Phase E: dropping one side's `CollabSession` (not just an explicit
//! `EndSession`) still closes its connection promptly — the command channel
//! closing tears the whole session down, not just the reader half.
//!
//! Phase F (current): reconnection (`net`'s module doc has the details) — a
//! dropped connection no longer ends the session outright, it tries to get
//! the peer back first, bounded by `net::RECONNECT_TIMEOUT`.

pub mod crdt;
pub mod crypto;
pub mod diff;
pub mod net;
pub mod ticket;

/// Commands sent from the main thread to a session's background thread.
#[derive(Debug)]
pub enum CollabCommand {
    /// A locally produced, already-encoded update to ship to the peer.
    LocalEdit(Vec<u8>),
    /// Tear the session down and let the background thread exit.
    EndSession,
}

/// Events sent from a session's background thread back to the main thread.
#[derive(Debug)]
pub enum CollabEvent {
    /// Hosting is ready; the pasteable connection code to share with a peer.
    HostReady(String),
    /// The peer's connection (and its one bidirectional stream) is up and
    /// the encrypted handshake completed — the peer has proven it holds the
    /// session key, and vice versa (see `net::establish`). Carries a short
    /// display fingerprint for the peer (`iroh::PublicKey::fmt_short`).
    PeerConnected(String),
    /// An already-encoded update received from the peer.
    RemoteUpdate(Vec<u8>),
    /// The connection dropped and the background thread is now trying to get
    /// the peer back (see `net`'s module doc) — not fatal on its own; either
    /// another `PeerConnected` (reconnected) or, if that doesn't happen
    /// before `net::RECONNECT_TIMEOUT`, a `PeerDisconnected` follows.
    Reconnecting,
    /// The peer's connection ended for good — either reconnection was
    /// exhausted, or the session ended before ever connecting.
    PeerDisconnected,
    /// Something went wrong; the session did not start or has ended.
    Error(String),
}

/// Which side of a session this instance is playing.
#[derive(Debug, Clone)]
pub enum SessionRole {
    /// Generate a connection code and wait for a peer to paste it in.
    Host,
    /// Join a session using a code pasted from a host.
    Join(String),
}

/// Handle to a running collaboration session's background thread.
pub struct CollabHandle {
    pub cmd_tx: tokio::sync::mpsc::UnboundedSender<CollabCommand>,
    pub event_rx: std::sync::mpsc::Receiver<CollabEvent>,
}

/// Starts a collaboration session on its own background thread (which hosts
/// its own single-purpose `tokio` runtime for the session's lifetime — see
/// `net::run`), returning a handle to command it and receive its events.
///
/// Spawned fresh per session rather than once at app startup: most users
/// never start one, and tearing the whole thread/runtime/endpoint down
/// together on [`CollabCommand::EndSession`] gives a clean, total teardown
/// with no lingering background state to reason about.
pub fn spawn_collab_session(role: SessionRole, ctx: egui::Context) -> CollabHandle {
    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
    let (event_tx, event_rx) = std::sync::mpsc::channel();

    std::thread::spawn(move || {
        net::run(role, cmd_rx, event_tx, ctx);
    });

    CollabHandle { cmd_tx, event_rx }
}

/// Which side of a session `SmaragdApp` is playing — display-only (see
/// `ui::collab_panel`); the wire protocol itself treats both sides
/// identically once connected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollabRole {
    Host,
    Joiner,
}

/// What happened this frame, for `SmaragdApp` to react to — returned by
/// [`CollabSession::poll`].
pub enum SessionUpdate {
    /// The document's text changed because of a remote edit; apply
    /// `new_text` to the editor buffer and use `change` to keep the local
    /// cursor in the right place (see `collab::diff::adjust_cursor`).
    TextChanged {
        new_text: String,
        change: diff::TextChange,
    },
    PeerConnected {
        fingerprint: String,
    },
    /// The connection dropped and the background thread is trying to get the
    /// peer back — see `CollabEvent::Reconnecting`. `reconnecting` stays
    /// `true` (and `peer_connected` `false`) until either another
    /// `PeerConnected` arrives or the session gives up for good.
    Reconnecting,
    PeerDisconnected,
    Error(String),
}

/// The `SmaragdApp`-facing handle to one running collaboration session: the
/// background thread's channels, the CRDT document, and the last-synced text
/// baseline `sync_local_edit` diffs the live editor buffer against.
pub struct CollabSession {
    handle: CollabHandle,
    pub role: CollabRole,
    /// The pasteable connection code, once the host's networking has come up
    /// (see `CollabEvent::HostReady`) — always `None` for a joiner.
    pub code: Option<String>,
    pub peer_connected: bool,
    /// The connection dropped and the background thread is trying to get the
    /// peer back — see `CollabEvent::Reconnecting`. Mutually exclusive with
    /// `peer_connected`: never both `true` at once.
    pub reconnecting: bool,
    /// Short display fingerprint for the peer, set once `peer_connected`
    /// becomes true (see `CollabEvent::PeerConnected`) — kept (not cleared)
    /// while `reconnecting` or after a final disconnect, so those panel
    /// states can still name who was lost.
    pub peer_fingerprint: Option<String>,
    /// Set once the background thread has actually stopped for good — a
    /// dropped peer that reconnection couldn't get back, or a real network
    /// disconnect before ever connecting — after which this session can no
    /// longer do anything and the caller should treat it as over (see
    /// `ui::collab_panel::CollabStatus::Disconnected`). Never set while
    /// `reconnecting` is `true`: the background thread is still trying at
    /// that point, and the caller should keep waiting rather than treat the
    /// session as over.
    pub session_ended: bool,
    doc: crdt::CrdtDoc,
    last_synced_text: String,
}

impl CollabSession {
    /// Starts hosting. `doc`/`last_synced_text` deliberately start empty
    /// regardless of the caller's actual buffer — see the module doc.
    pub fn host(ctx: egui::Context) -> Self {
        Self {
            handle: spawn_collab_session(SessionRole::Host, ctx),
            role: CollabRole::Host,
            code: None,
            peer_connected: false,
            reconnecting: false,
            peer_fingerprint: None,
            session_ended: false,
            doc: crdt::CrdtDoc::new(),
            last_synced_text: String::new(),
        }
    }

    /// Joins a session using a code pasted from a host.
    pub fn join(code: String, ctx: egui::Context) -> Self {
        Self {
            handle: spawn_collab_session(SessionRole::Join(code), ctx),
            role: CollabRole::Joiner,
            code: None,
            peer_connected: false,
            reconnecting: false,
            peer_fingerprint: None,
            session_ended: false,
            doc: crdt::CrdtDoc::new(),
            last_synced_text: String::new(),
        }
    }

    /// A fake session for tests that need a `CollabSession` in a given role
    /// without any real networking — same raw-struct-literal approach as
    /// `poll_reports_disconnect_once_not_every_frame_after` below, just
    /// exposed for use from outside this module (e.g.
    /// `app::project_lifecycle`'s tests).
    #[cfg(test)]
    pub(crate) fn test_fixture(role: CollabRole) -> Self {
        let (cmd_tx, _cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (_event_tx, event_rx) = std::sync::mpsc::channel();
        Self {
            handle: CollabHandle { cmd_tx, event_rx },
            role,
            code: None,
            peer_connected: true,
            reconnecting: false,
            peer_fingerprint: None,
            session_ended: false,
            doc: crdt::CrdtDoc::new(),
            last_synced_text: String::new(),
        }
    }

    /// Ends the session: tells the background thread to stop, then drops
    /// its channels — the thread tears down its connection/endpoint/runtime
    /// and exits on its own.
    pub fn end(self) {
        let _ = self.handle.cmd_tx.send(CollabCommand::EndSession);
    }

    /// Drains queued [`CollabEvent`]s, applying remote updates to the
    /// internal CRDT document as they arrive, and returns what the caller
    /// needs to react to. Call once per frame, before rendering the editor.
    ///
    /// A no-op once [`Self::session_ended`] is already `true`: that flag is
    /// only ever set below, the moment the background channel is first
    /// observed disconnected (or a fatal error/explicit peer-disconnect
    /// event arrives) — and a disconnected `mpsc::Receiver` reports
    /// `Disconnected` on every subsequent `try_recv` forever. Without this
    /// guard, a caller polling once per frame after that point would
    /// re-discover "the peer disconnected" anew on every single frame (e.g.
    /// re-pushing a toast every frame — see the `SmaragdApp` caller).
    pub fn poll(&mut self) -> Vec<SessionUpdate> {
        if self.session_ended {
            return Vec::new();
        }
        let mut updates = Vec::new();
        loop {
            let event = match self.handle.event_rx.try_recv() {
                Ok(event) => event,
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.peer_connected = false;
                    self.reconnecting = false;
                    self.session_ended = true;
                    updates.push(SessionUpdate::PeerDisconnected);
                    break;
                }
            };
            match event {
                CollabEvent::HostReady(code) => self.code = Some(code),
                CollabEvent::PeerConnected(fingerprint) => {
                    self.peer_connected = true;
                    self.reconnecting = false;
                    self.peer_fingerprint = Some(fingerprint.clone());
                    updates.push(SessionUpdate::PeerConnected { fingerprint });
                }
                CollabEvent::Reconnecting => {
                    self.peer_connected = false;
                    self.reconnecting = true;
                    updates.push(SessionUpdate::Reconnecting);
                }
                CollabEvent::RemoteUpdate(bytes) => match self.doc.apply_remote_update(&bytes) {
                    Ok(new_text) => {
                        if let Some(change) = diff::diff(&self.last_synced_text, &new_text) {
                            self.last_synced_text = new_text.clone();
                            updates.push(SessionUpdate::TextChanged { new_text, change });
                        }
                    }
                    Err(err) => {
                        updates.push(SessionUpdate::Error(format!(
                            "received an unreadable update from the peer: {err}"
                        )));
                    }
                },
                CollabEvent::PeerDisconnected => {
                    self.peer_connected = false;
                    self.reconnecting = false;
                    self.session_ended = true;
                    updates.push(SessionUpdate::PeerDisconnected);
                }
                CollabEvent::Error(message) => {
                    // Every `CollabEvent::Error` net.rs sends is fatal — the
                    // background thread always exits shortly after, so this
                    // session is just as over as an explicit disconnect.
                    self.reconnecting = false;
                    self.session_ended = true;
                    updates.push(SessionUpdate::Error(message));
                }
            }
        }
        updates
    }

    /// Diffs `current_text` (the live editor buffer) against the last-synced
    /// baseline and, if it changed, applies the edit to the local CRDT
    /// document and ships the resulting update to the peer. Call once per
    /// frame, after the editor has had a chance to render/mutate its buffer.
    pub fn sync_local_edit(&mut self, current_text: &str) {
        let Some(change) = diff::diff(&self.last_synced_text, current_text) else {
            return;
        };
        let update = self.doc.apply_local_change(&change);
        let _ = self.handle.cmd_tx.send(CollabCommand::LocalEdit(update));
        self.last_synced_text = current_text.to_string();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression test for a toast-spam bug: once the background channel is
    /// gone, `mpsc::Receiver::try_recv` reports `Disconnected` on every call
    /// forever, so without the `session_ended` guard at the top of `poll`,
    /// each frame's poll would re-discover "peer disconnected" and the
    /// caller (`SmaragdApp::poll_collab_events`) would push a fresh toast
    /// every single frame. No real networking needed: a dropped `event_tx`
    /// reproduces the same disconnected-channel state a torn-down
    /// background thread leaves behind.
    #[test]
    fn poll_reports_disconnect_once_not_every_frame_after() {
        let (cmd_tx, _cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (event_tx, event_rx) = std::sync::mpsc::channel();
        drop(event_tx);
        let mut session = CollabSession {
            handle: CollabHandle { cmd_tx, event_rx },
            role: CollabRole::Host,
            code: None,
            peer_connected: true,
            reconnecting: false,
            peer_fingerprint: Some("abcd1234".to_string()),
            session_ended: false,
            doc: crdt::CrdtDoc::new(),
            last_synced_text: String::new(),
        };

        let first = session.poll();
        assert_eq!(first.len(), 1);
        assert!(matches!(first[0], SessionUpdate::PeerDisconnected));
        assert!(session.session_ended);
        assert!(!session.peer_connected);

        // Simulating several more frames' worth of polling: none of them
        // should produce another `PeerDisconnected` (or any other update).
        for _ in 0..5 {
            assert!(session.poll().is_empty());
        }
    }

    fn fake_session(role: CollabRole) -> (CollabSession, std::sync::mpsc::Sender<CollabEvent>) {
        let (cmd_tx, _cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (event_tx, event_rx) = std::sync::mpsc::channel();
        let session = CollabSession {
            handle: CollabHandle { cmd_tx, event_rx },
            role,
            code: None,
            peer_connected: true,
            reconnecting: false,
            peer_fingerprint: Some("abcd1234".to_string()),
            session_ended: false,
            doc: crdt::CrdtDoc::new(),
            last_synced_text: String::new(),
        };
        (session, event_tx)
    }

    /// A `Reconnecting` event flips `peer_connected` off and `reconnecting`
    /// on, without touching `session_ended` — the session is still live, see
    /// `CollabSession::session_ended`'s doc comment.
    #[test]
    fn poll_applies_a_reconnecting_event() {
        let (mut session, event_tx) = fake_session(CollabRole::Host);

        event_tx.send(CollabEvent::Reconnecting).unwrap();
        let updates = session.poll();

        assert!(matches!(updates.as_slice(), [SessionUpdate::Reconnecting]));
        assert!(!session.peer_connected);
        assert!(session.reconnecting);
        assert!(!session.session_ended);
    }

    /// A `PeerConnected` arriving while `reconnecting` is `true` (a
    /// successful reconnect, not the first connection) clears it, same as
    /// any other `PeerConnected`.
    #[test]
    fn poll_clears_reconnecting_once_the_peer_is_back() {
        let (mut session, event_tx) = fake_session(CollabRole::Host);
        event_tx.send(CollabEvent::Reconnecting).unwrap();
        session.poll();
        assert!(session.reconnecting);

        event_tx
            .send(CollabEvent::PeerConnected("abcd1234".to_string()))
            .unwrap();
        let updates = session.poll();

        assert!(matches!(
            updates.as_slice(),
            [SessionUpdate::PeerConnected { .. }]
        ));
        assert!(session.peer_connected);
        assert!(!session.reconnecting);
    }

    use std::time::{Duration, Instant};

    /// Exercises the full `SmaragdApp`-facing surface end to end over a real
    /// iroh connection: hosting against an already-"open" buffer, joining,
    /// the bootstrap-via-first-diff mechanism (see the module doc), and
    /// concurrent edits on both sides converging — everything Phase D wires
    /// up except the actual egui widgets, which can't be driven headlessly
    /// here. Real internet access to iroh's public relay infrastructure is
    /// required, so — like the `net` module's own live tests — this is
    /// excluded from the default `cargo test` run; run it manually with
    /// `cargo test --lib collab::tests -- --ignored`.
    #[test]
    #[ignore = "requires real internet access to iroh's public relay infrastructure"]
    fn two_sessions_converge_on_a_shared_document_with_concurrent_edits() {
        let ctx = egui::Context::default();
        let timeout = Duration::from_secs(30);

        let mut host_text = "Chapter One\n\nIt was a dark and stormy night.".to_string();
        let mut host = CollabSession::host(ctx.clone());

        let code = drain_until(&mut host, timeout, |session| session.code.clone());
        let mut joiner = CollabSession::join(code, ctx.clone());

        drain_until(&mut joiner, timeout, |session| {
            session.peer_connected.then_some(())
        });
        drain_until(&mut host, timeout, |session| {
            session.peer_connected.then_some(())
        });

        // The host's very first sync bootstraps its already-open document to
        // the joiner (see the module doc) — no separate "initial sync" step.
        host.sync_local_edit(&host_text);
        drain_until(&mut joiner, timeout, |session| {
            (session.last_synced_text == host_text).then_some(())
        });
        let mut joiner_text = joiner.last_synced_text.clone();
        assert_eq!(joiner_text, host_text);

        // Concurrent edits: each side types something different before
        // either has seen the other's change.
        host_text.push_str(" The wind howled.");
        host.sync_local_edit(&host_text);

        joiner_text.insert_str(0, "PROLOGUE\n\n");
        joiner.sync_local_edit(&joiner_text);

        // Both sides should converge on an identical merged document.
        drain_until(&mut host, timeout, |session| {
            let text = &session.last_synced_text;
            (text.contains("PROLOGUE") && text.contains("wind howled")).then_some(())
        });
        drain_until(&mut joiner, timeout, |session| {
            let text = &session.last_synced_text;
            (text.contains("PROLOGUE") && text.contains("wind howled")).then_some(())
        });

        assert_eq!(host.last_synced_text, joiner.last_synced_text);

        host.end();
        joiner.end();
    }

    /// Repeatedly polls `session` (applying its `SessionUpdate`s, including
    /// exercising `diff::adjust_cursor` on every `TextChanged` the same way
    /// `SmaragdApp::poll_collab_events` does) until `check` returns `Some`,
    /// or panics after `timeout`.
    fn drain_until<T>(
        session: &mut CollabSession,
        timeout: Duration,
        mut check: impl FnMut(&CollabSession) -> Option<T>,
    ) -> T {
        let deadline = Instant::now() + timeout;
        loop {
            for update in session.poll() {
                if let SessionUpdate::TextChanged { change, .. } = &update {
                    let _ = diff::adjust_cursor(0, change);
                }
            }
            if let Some(value) = check(session) {
                return value;
            }
            assert!(Instant::now() < deadline, "timed out waiting for condition");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Phase E regression test: dropping one side's session (rather than
    /// gracefully calling `end()`) still closes its connection promptly
    /// (`CollabHandle::cmd_tx` dropping closes the background thread's
    /// command channel, which now — via `net::run_session`'s
    /// `tokio::select!` — tears the whole session down, not just the reader
    /// half), and a brand new session can be started immediately afterward
    /// with no restart of anything.
    #[test]
    #[ignore = "requires real internet access to iroh's public relay infrastructure"]
    fn dropping_a_session_tears_it_down_and_a_fresh_one_can_start_immediately() {
        let ctx = egui::Context::default();
        let timeout = Duration::from_secs(30);

        let mut host = CollabSession::host(ctx.clone());
        let code = drain_until(&mut host, timeout, |session| session.code.clone());
        let mut joiner = CollabSession::join(code, ctx.clone());

        drain_until(&mut joiner, timeout, |session| {
            session.peer_connected.then_some(())
        });
        drain_until(&mut host, timeout, |session| {
            session.peer_connected.then_some(())
        });
        assert!(!host.session_ended);

        // Drop (not `.end()`) the joiner's own session — nothing left on
        // this side to reconnect from, so its background thread just tears
        // down (see the doc comment above).
        drop(joiner);

        // A fresh session should work immediately, independent of the
        // dropped one's teardown/reconnect handling.
        let mut fresh_host = CollabSession::host(ctx.clone());
        let fresh_code = drain_until(&mut fresh_host, timeout, |session| session.code.clone());
        assert!(!fresh_code.is_empty());
        fresh_host.end();
        host.end();
    }

    /// #81 regression test: dropping a peer's connection (network loss,
    /// sleep/wake, a crash) no longer ends the *surviving* side's session
    /// outright — it tries to reconnect first (see `net`'s module doc), and
    /// `session_ended` stays `false` for as long as that attempt is still
    /// under way. Full end-to-end coverage of an actual successful/canceled
    /// reconnect lives in `net`'s own tests
    /// (`a_dropped_connection_reconnects_with_the_same_ticket_and_keeps_working`
    /// / `ending_the_session_while_reconnecting_stops_it_promptly`); this one
    /// stays at the `CollabSession` level to check the flags a UI caller
    /// (`ui::collab_panel::CollabStatus`) actually branches on.
    #[test]
    #[ignore = "requires real internet access to iroh's public relay infrastructure"]
    fn a_dropped_peer_triggers_reconnecting_not_an_immediate_session_end() {
        let ctx = egui::Context::default();
        let timeout = Duration::from_secs(30);

        let mut host = CollabSession::host(ctx.clone());
        let code = drain_until(&mut host, timeout, |session| session.code.clone());
        let mut joiner = CollabSession::join(code, ctx.clone());

        drain_until(&mut joiner, timeout, |session| {
            session.peer_connected.then_some(())
        });
        drain_until(&mut host, timeout, |session| {
            session.peer_connected.then_some(())
        });
        assert!(!host.session_ended);

        // Drop (not `.end()`) the joiner — simulates its connection going
        // away without a chance to clean up, e.g. a crash or a laptop going
        // to sleep.
        drop(joiner);

        drain_until(&mut host, timeout, |session| {
            session.reconnecting.then_some(())
        });
        assert!(!host.peer_connected);
        assert!(
            !host.session_ended,
            "a reconnect attempt is still under way, not over yet"
        );

        // Give up manually rather than waiting out the full reconnect
        // window — cancellation itself is exercised end-to-end (down to the
        // actual background thread exiting) by `net`'s own
        // `ending_the_session_while_reconnecting_stops_it_promptly`.
        host.end();
    }
}
