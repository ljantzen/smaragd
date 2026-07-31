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
//! Phase C (current): `crypto`, the app-level end-to-end encryption layered
//! on top of iroh's own transport security, wired into `net` so every frame
//! exchanged between peers is ciphertext. No CRDT/UI integration yet.

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
    /// The peer's connection (and its one bidirectional stream) is up.
    PeerConnected,
    /// An already-encoded update received from the peer.
    RemoteUpdate(Vec<u8>),
    /// The peer's connection ended.
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
