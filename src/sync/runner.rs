//! Runs a [`SyncEngine`] on a background thread so the UI never waits on the network.
//!
//! The thread owns the engine and a transport. It syncs on an interval and on request
//! ([`SyncRunner::sync_now`], e.g. right after a save), and reports what happened as
//! [`SyncEvent`]s the UI drains each frame with [`SyncRunner::poll`] — the same
//! command-in / event-out shape `collab::CollabSession` uses.
//!
//! Failures are sorted by what the user can do about them. An unreachable server is
//! [`SyncEvent::Offline`]: local edits keep queuing and the next pass retries. A
//! revoked device or a wrong passphrase is [`SyncEvent::Halted`]: retrying can't help,
//! so the thread stops until the user fixes the setting and restarts sync.

use std::collections::BTreeSet;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::Duration;

use super::engine::{SyncEngine, SyncError, SyncReport};
use super::transport::{SyncTransport, TransportError};

/// Why the runner stopped for good.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HaltReason {
    /// The passphrase doesn't match the one the vault was created with.
    WrongPassphrase,
    /// The server no longer accepts this device (it was revoked, or the vault deleted).
    Revoked,
    /// The project folder is gone.
    ProjectMissing,
    /// Something unrecoverable, with a message for the user.
    Fatal(String),
}

#[derive(Debug, Clone)]
pub enum SyncEvent {
    /// A pass has started.
    Syncing,
    /// A pass finished cleanly (boxed: a report is far bigger than the other events).
    Synced(Box<SyncReport>),
    /// The server can't be reached; the next pass retries.
    Offline(String),
    /// A pass failed for a reason that may pass; the next pass retries.
    Failed(String),
    /// Sync stopped and won't retry until restarted.
    Halted(HaltReason),
}

/// Builds the engine and transport on the runner's thread.
pub type Builder = Box<dyn FnOnce() -> Result<(SyncEngine, Box<dyn SyncTransport>), String> + Send>;

enum Command {
    SyncNow,
    SetHeld(BTreeSet<String>),
    Stop,
}

fn classify(error: SyncError) -> SyncEvent {
    match error {
        SyncError::WrongPassphrase => SyncEvent::Halted(HaltReason::WrongPassphrase),
        SyncError::Transport(TransportError::Unauthorized) => {
            SyncEvent::Halted(HaltReason::Revoked)
        }
        SyncError::ProjectMissing(_) => SyncEvent::Halted(HaltReason::ProjectMissing),
        SyncError::Transport(TransportError::Offline(why)) => SyncEvent::Offline(why),
        SyncError::State(why) => SyncEvent::Halted(HaltReason::Fatal(why)),
        other => SyncEvent::Failed(other.to_string()),
    }
}

/// A running background sync. Dropping it asks the thread to stop (without waiting: a
/// request in flight finishes on its own, and the UI must never block on it).
pub struct SyncRunner {
    commands: Sender<Command>,
    events: Receiver<SyncEvent>,
}

impl SyncRunner {
    /// Starts syncing with an engine that's already built. `wake` is called after every
    /// event so the UI can repaint (typically `ctx.request_repaint()`).
    pub fn spawn(
        engine: SyncEngine,
        transport: Box<dyn SyncTransport>,
        interval: Duration,
        wake: Box<dyn Fn() + Send>,
    ) -> Self {
        Self::spawn_with(Box::new(move || Ok((engine, transport))), interval, wake)
    }

    /// Like [`Self::spawn`], but builds the engine on the background thread — key
    /// derivation (Argon2id) takes about a second and must not stall the UI. A build
    /// failure becomes a [`SyncEvent::Halted`].
    pub fn spawn_with(build: Builder, interval: Duration, wake: Box<dyn Fn() + Send>) -> Self {
        let (command_tx, command_rx) = mpsc::channel::<Command>();
        let (event_tx, event_rx) = mpsc::channel::<SyncEvent>();
        thread::Builder::new()
            .name("smaragd-sync".into())
            .spawn(move || {
                let emit = |event: SyncEvent| {
                    // A closed channel means the UI dropped us; the next recv notices.
                    let _ = event_tx.send(event);
                    wake();
                };
                emit(SyncEvent::Syncing);
                let (mut engine, transport) = match build() {
                    Ok(built) => built,
                    Err(message) => {
                        emit(SyncEvent::Halted(HaltReason::Fatal(message)));
                        return;
                    }
                };
                // The first pass runs immediately, then on the interval or on request.
                let mut due = true;
                loop {
                    if due {
                        emit(SyncEvent::Syncing);
                        match engine.sync_once(&*transport) {
                            Ok(report) => emit(SyncEvent::Synced(Box::new(report))),
                            Err(error) => {
                                let event = classify(error);
                                let halted = matches!(event, SyncEvent::Halted(_));
                                emit(event);
                                if halted {
                                    return;
                                }
                            }
                        }
                    }
                    due = false;
                    match command_rx.recv_timeout(interval) {
                        Ok(Command::SyncNow) | Err(RecvTimeoutError::Timeout) => due = true,
                        Ok(Command::SetHeld(paths)) => engine.set_held_paths(paths),
                        Ok(Command::Stop) | Err(RecvTimeoutError::Disconnected) => return,
                    }
                }
            })
            .expect("spawning the sync thread");
        Self {
            commands: command_tx,
            events: event_rx,
        }
    }

    /// Runs a pass as soon as the current one (if any) finishes.
    pub fn sync_now(&self) {
        let _ = self.commands.send(Command::SyncNow);
    }

    /// Tells the engine which files have unsaved edits open (see
    /// `SyncEngine::set_held_paths`).
    pub fn set_held_paths(&self, paths: BTreeSet<String>) {
        let _ = self.commands.send(Command::SetHeld(paths));
    }

    /// Everything that happened since the last call.
    pub fn poll(&self) -> Vec<SyncEvent> {
        self.events.try_iter().collect()
    }

    /// Waits up to `timeout` for the next event (used by tests).
    pub fn recv_timeout(&self, timeout: Duration) -> Option<SyncEvent> {
        self.events.recv_timeout(timeout).ok()
    }
}

impl Drop for SyncRunner {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Stop);
    }
}

/// Everything needed to start syncing one paired project.
pub struct StartParams {
    pub link: super::link::ProjectLink,
    pub credentials: super::link::DeviceCredentials,
    pub passphrase: String,
    pub project_root: std::path::PathBuf,
    pub files: std::sync::Arc<dyn crate::project::store::ProjectStore>,
    pub state_dir: std::path::PathBuf,
}

/// Starts background sync for a paired project. Deriving the key and opening the
/// engine happen on the new thread; any failure arrives as [`SyncEvent::Halted`].
pub fn start(params: StartParams, interval: Duration, wake: Box<dyn Fn() + Send>) -> SyncRunner {
    SyncRunner::spawn_with(
        Box::new(move || {
            use super::client::HttpClient;
            use super::crypto::derive_vault_key;
            use super::engine::EngineConfig;
            use super::state::DirStateStore;

            let key = derive_vault_key(
                &params.passphrase,
                &params.link.kdf_salt,
                params.link.key_version,
            )
            .map_err(|err| err.to_string())?;
            let engine = SyncEngine::open(
                EngineConfig {
                    vault: params.link.vault_id,
                    device: params.credentials.device_id,
                    key,
                    root: params.project_root,
                },
                params.files.clone(),
                Box::new(DirStateStore::new(params.files, params.state_dir)),
            )
            .map_err(|err| err.to_string())?;
            let transport = HttpClient::new(&params.link.server)
                .with_token(params.credentials.token)
                .transport(params.link.vault_id);
            Ok((engine, Box::new(transport) as Box<dyn SyncTransport>))
        }),
        interval,
        wake,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::store::native_store;
    use crate::sync::crypto::cheap_test_key;
    use crate::sync::engine::EngineConfig;
    use crate::sync::fake::{MemoryServer, MemoryTransport};
    use crate::sync::state::MemoryStateStore;
    use smaragd_sync_protocol::api::KDF_SALT_LEN;
    use smaragd_sync_protocol::{DeviceId, VaultId};
    use uuid::Uuid;

    const WAIT: Duration = Duration::from_secs(5);

    struct Fixture {
        dir: tempfile::TempDir,
        runner: SyncRunner,
        transport_online: std::sync::Arc<std::sync::atomic::AtomicBool>,
    }

    fn start(server: &MemoryServer, passphrase: &str, interval: Duration) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let device = DeviceId(Uuid::new_v4());
        let engine = SyncEngine::open(
            EngineConfig {
                vault: VaultId(Uuid::from_u128(0xbeef)),
                device,
                key: cheap_test_key(passphrase, &[7u8; KDF_SALT_LEN]),
                root: dir.path().to_path_buf(),
            },
            native_store(),
            Box::new(MemoryStateStore::default()),
        )
        .unwrap();
        let transport = MemoryTransport::new(server, device);
        let transport_online = transport.online_flag();
        Fixture {
            dir,
            runner: SyncRunner::spawn(engine, Box::new(transport), interval, Box::new(|| {})),
            transport_online,
        }
    }

    /// Waits for an event matching `wanted`, skipping others.
    fn wait_for(runner: &SyncRunner, wanted: impl Fn(&SyncEvent) -> bool) -> SyncEvent {
        let deadline = std::time::Instant::now() + WAIT;
        while let Some(left) = deadline.checked_duration_since(std::time::Instant::now()) {
            if let Some(event) = runner.recv_timeout(left)
                && wanted(&event)
            {
                return event;
            }
        }
        panic!("no matching event within {WAIT:?}");
    }

    #[test]
    fn the_first_pass_runs_immediately_and_reports_what_it_did() {
        let server = MemoryServer::default();
        let f = start(&server, "pw", Duration::from_secs(3600));
        std::fs::write(f.dir.path().join("a.md"), "hello\n").unwrap();
        // The immediate pass may have run before the file existed; ask for another.
        f.runner.sync_now();
        let event = wait_for(
            &f.runner,
            |e| matches!(e, SyncEvent::Synced(r) if r.pushed_updates > 0),
        );
        assert!(matches!(event, SyncEvent::Synced(_)));
    }

    #[test]
    fn sync_now_triggers_a_pass_without_waiting_for_the_interval() {
        let server = MemoryServer::default();
        let f = start(&server, "pw", Duration::from_secs(3600));
        wait_for(&f.runner, |e| matches!(e, SyncEvent::Synced(_)));
        f.runner.sync_now();
        wait_for(&f.runner, |e| matches!(e, SyncEvent::Syncing));
        wait_for(&f.runner, |e| matches!(e, SyncEvent::Synced(_)));
    }

    #[test]
    fn passes_repeat_on_the_interval() {
        let server = MemoryServer::default();
        let f = start(&server, "pw", Duration::from_millis(30));
        for _ in 0..3 {
            wait_for(&f.runner, |e| matches!(e, SyncEvent::Synced(_)));
        }
    }

    #[test]
    fn an_unreachable_server_is_offline_and_recovers() {
        let server = MemoryServer::default();
        let f = start(&server, "pw", Duration::from_millis(30));
        wait_for(&f.runner, |e| matches!(e, SyncEvent::Synced(_)));
        f.transport_online
            .store(false, std::sync::atomic::Ordering::SeqCst);
        wait_for(&f.runner, |e| matches!(e, SyncEvent::Offline(_)));
        f.transport_online
            .store(true, std::sync::atomic::Ordering::SeqCst);
        wait_for(&f.runner, |e| matches!(e, SyncEvent::Synced(_)));
    }

    #[test]
    fn a_wrong_passphrase_halts_instead_of_retrying_forever() {
        let server = MemoryServer::default();
        let good = start(&server, "right", Duration::from_secs(3600));
        std::fs::write(good.dir.path().join("a.md"), "secret\n").unwrap();
        good.runner.sync_now();
        wait_for(
            &good.runner,
            |e| matches!(e, SyncEvent::Synced(r) if r.pushed_updates > 0),
        );

        let bad = start(&server, "wrong", Duration::from_millis(20));
        let event = wait_for(&bad.runner, |e| matches!(e, SyncEvent::Halted(_)));
        assert!(matches!(
            event,
            SyncEvent::Halted(HaltReason::WrongPassphrase)
        ));
        // Halted means halted: no further passes.
        std::thread::sleep(Duration::from_millis(100));
        assert!(bad.runner.poll().is_empty());
    }
}
