//! Phase 6: collab wired to real push events instead of egui's per-frame
//! `CollabSession::poll()`. The migration plan's architecture for this
//! (§2): a small background thread drains the session the moment new state
//! is ready and `emit()`s it immediately — lower latency than polling, not
//! higher, since nothing waits for the next frame tick. The session's own
//! `RepaintNotifier` closure *is* that wake signal here: instead of calling
//! `egui::Context::request_repaint()`, it pings a channel this thread blocks
//! on, with a short fallback timeout so a missed wake (or the session
//! ending without one — see `collab_end`) is never more than briefly stale.
//!
//! No live cursor-sharing feature exists in this codebase today (checked:
//! `CollabEvent`/`SessionUpdate` only carry text-change/connection events),
//! so the plan's "throttle collab cursors" verification note doesn't apply
//! as written — there's nothing to throttle yet. The push-event + wake-drain
//! architecture built here is exactly where that throttling would go if a
//! cursor-sharing feature is added later: throttle inside the drain loop,
//! before `app.emit()`, same as the plan describes.

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use smaragd_core::collab::{CollabRole, CollabSession, RepaintNotifier, SessionUpdate};
use tauri::{AppHandle, Emitter, State};

const EVENT: &str = "collab://event";
/// Upper bound on how stale the drained state can be if a wake is ever
/// missed (e.g. the notifier fires a moment before this thread starts
/// listening) — not the normal path, which is wake-driven and near-instant.
const WAKE_FALLBACK: Duration = Duration::from_millis(250);

#[derive(Default)]
pub struct CollabState(Arc<Mutex<Option<CollabSession>>>);

#[derive(Serialize, Clone)]
#[serde(tag = "kind")]
enum CollabUpdateDto {
    /// Hosting came up; `code` is the pasteable connection code to share.
    /// Not one of `SessionUpdate`'s variants — `CollabSession::poll()`
    /// records the host code internally rather than surfacing it as an
    /// update (see its `CollabEvent::HostReady` arm) — so the drain loop
    /// below watches `session.code` directly instead.
    HostReady {
        code: String,
    },
    /// The frontend replaces its whole buffer with `new_text` rather than
    /// applying `SessionUpdate::TextChanged`'s minimal `diff::TextChange` —
    /// CodeMirror re-renders the diff itself, and wiring collab directly
    /// into the live editor's own change stream (rather than swapping the
    /// whole buffer on every remote update) is a later refinement once an
    /// editor-integrated collab panel exists.
    TextChanged {
        new_text: String,
    },
    PeerConnected {
        fingerprint: String,
    },
    Reconnecting,
    PeerDisconnected,
    Error {
        message: String,
    },
}

impl From<SessionUpdate> for CollabUpdateDto {
    fn from(update: SessionUpdate) -> Self {
        match update {
            SessionUpdate::TextChanged { new_text, .. } => {
                CollabUpdateDto::TextChanged { new_text }
            }
            SessionUpdate::PeerConnected { fingerprint } => {
                CollabUpdateDto::PeerConnected { fingerprint }
            }
            SessionUpdate::Reconnecting => CollabUpdateDto::Reconnecting,
            SessionUpdate::PeerDisconnected => CollabUpdateDto::PeerDisconnected,
            SessionUpdate::Error(message) => CollabUpdateDto::Error { message },
        }
    }
}

#[derive(Serialize)]
pub struct CollabStatus {
    active: bool,
    role: Option<&'static str>,
    code: Option<String>,
    peer_connected: bool,
    reconnecting: bool,
    peer_fingerprint: Option<String>,
    session_ended: bool,
}

fn status_of(session: &CollabSession) -> CollabStatus {
    CollabStatus {
        active: true,
        role: Some(match session.role {
            CollabRole::Host => "host",
            CollabRole::Joiner => "joiner",
        }),
        code: session.code.clone(),
        peer_connected: session.peer_connected,
        reconnecting: session.reconnecting,
        peer_fingerprint: session.peer_fingerprint.clone(),
        session_ended: session.session_ended,
    }
}

#[tauri::command]
pub fn collab_status(state: State<CollabState>) -> CollabStatus {
    let guard = state.0.lock().expect("collab state mutex poisoned");
    match guard.as_ref() {
        Some(session) => status_of(session),
        None => CollabStatus {
            active: false,
            role: None,
            code: None,
            peer_connected: false,
            reconnecting: false,
            peer_fingerprint: None,
            session_ended: false,
        },
    }
}

fn start(
    app: AppHandle,
    state: State<CollabState>,
    build: impl FnOnce(RepaintNotifier) -> CollabSession,
) -> Result<(), String> {
    let mut guard = state.0.lock().expect("collab state mutex poisoned");
    if guard.is_some() {
        return Err("a collab session is already active".into());
    }
    let (wake_tx, wake_rx) = mpsc::channel::<()>();
    let notify_repaint: RepaintNotifier = Arc::new(move || {
        let _ = wake_tx.send(());
    });
    *guard = Some(build(notify_repaint));
    drop(guard);
    spawn_drain_thread(app, state.0.clone(), wake_rx);
    Ok(())
}

#[tauri::command]
pub fn collab_host(app: AppHandle, state: State<CollabState>) -> Result<(), String> {
    start(app, state, CollabSession::host)
}

#[tauri::command]
pub fn collab_join(app: AppHandle, state: State<CollabState>, code: String) -> Result<(), String> {
    start(app, state, |notify_repaint| {
        CollabSession::join(code, notify_repaint)
    })
}

#[tauri::command]
pub fn collab_end(state: State<CollabState>) {
    // Dropping the session (rather than explicitly calling `.end()` first)
    // still tears the background thread down cleanly: `CollabSession` holds
    // no `Drop` impl of its own, but its `CollabHandle`'s `cmd_tx` closing
    // when this `Option` is cleared is exactly what `.end()` itself sends.
    // The drain thread below notices on its next wake (or `WAKE_FALLBACK`
    // at the latest) and exits.
    let mut guard = state.0.lock().expect("collab state mutex poisoned");
    if let Some(session) = guard.take() {
        session.end();
    }
}

#[tauri::command]
pub fn collab_sync_local_edit(state: State<CollabState>, text: String) {
    let mut guard = state.0.lock().expect("collab state mutex poisoned");
    if let Some(session) = guard.as_mut() {
        session.sync_local_edit(&text);
    }
}

fn spawn_drain_thread(
    app: AppHandle,
    session: Arc<Mutex<Option<CollabSession>>>,
    wake_rx: mpsc::Receiver<()>,
) {
    std::thread::spawn(move || {
        let mut host_code_emitted = false;
        loop {
            let _ = wake_rx.recv_timeout(WAKE_FALLBACK);
            let mut guard = session.lock().expect("collab state mutex poisoned");
            let Some(active) = guard.as_mut() else {
                return;
            };
            let updates = active.poll();
            if !host_code_emitted && let Some(code) = active.code.clone() {
                host_code_emitted = true;
                drop(guard);
                let _ = app.emit(EVENT, CollabUpdateDto::HostReady { code });
            } else {
                drop(guard);
            }
            for update in updates {
                let _ = app.emit(EVENT, CollabUpdateDto::from(update));
            }
        }
    });
}
