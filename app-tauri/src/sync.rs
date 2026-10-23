//! Phase 6: sync wired to real push events, same wake-driven drain-thread
//! architecture as `collab.rs`. Unlike collab, no pairing UI exists anywhere
//! in the migration plan — a user can't yet connect a project to a real
//! `smaragd-sync-server` through this frontend — so there is no real vault
//! to sync against. `smaragd_core::sync::demo::start_ephemeral_demo` stands
//! in for that: a genuine `SyncRunner` running a genuine `SyncEngine`
//! against an in-memory fake transport, which is the same production
//! `SyncRunner`/`SyncEvent` plumbing real sync will use once pairing exists.
//! It proves the event-push architecture this phase is about; it does not
//! prove multi-device sync semantics (there's only ever one device in it).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use smaragd_core::sync::runner::{HaltReason, SyncEvent, SyncRunner};
use tauri::{AppHandle, Emitter, State};

const EVENT: &str = "sync://event";
const DEMO_INTERVAL: Duration = Duration::from_secs(10);

#[derive(Default)]
pub struct SyncState(Arc<Mutex<Option<SyncRunner>>>);

#[derive(Serialize, Clone)]
#[serde(tag = "kind")]
enum SyncEventDto {
    Syncing,
    /// A reduced summary of `SyncReport` — the risk register's "keep event
    /// payloads minimal" guidance, not the whole struct (which also carries
    /// per-path `Vec<String>`s not worth shipping over IPC every pass).
    Synced {
        pushed_updates: usize,
        pulled_updates: usize,
        files_written: usize,
        files_created: usize,
        files_removed: usize,
    },
    Offline {
        message: String,
    },
    Failed {
        message: String,
    },
    Halted {
        reason: String,
    },
}

impl From<SyncEvent> for SyncEventDto {
    fn from(event: SyncEvent) -> Self {
        match event {
            SyncEvent::Syncing => SyncEventDto::Syncing,
            SyncEvent::Synced(report) => SyncEventDto::Synced {
                pushed_updates: report.pushed_updates,
                pulled_updates: report.pulled_updates,
                files_written: report.files_written,
                files_created: report.files_created,
                files_removed: report.files_removed,
            },
            SyncEvent::Offline(message) => SyncEventDto::Offline { message },
            SyncEvent::Failed(message) => SyncEventDto::Failed { message },
            SyncEvent::Halted(reason) => SyncEventDto::Halted {
                reason: match reason {
                    HaltReason::WrongPassphrase => "wrong passphrase".into(),
                    HaltReason::Revoked => "device revoked".into(),
                    HaltReason::ProjectMissing => "project folder missing".into(),
                    HaltReason::Fatal(message) => message,
                },
            },
        }
    }
}

#[tauri::command]
pub fn sync_demo_start(
    app: AppHandle,
    state: State<SyncState>,
    root: String,
) -> Result<(), String> {
    let mut guard = state.0.lock().expect("sync state mutex poisoned");
    if guard.is_some() {
        return Err("a sync session is already active".into());
    }
    let root = std::path::PathBuf::from(root);
    let files = smaragd_core::project::store::native_store();
    let (wake_tx, wake_rx) = std::sync::mpsc::channel::<()>();
    let wake = Box::new(move || {
        let _ = wake_tx.send(());
    });
    let runner = smaragd_core::sync::demo::start_ephemeral_demo(&root, files, DEMO_INTERVAL, wake);
    *guard = Some(runner);
    drop(guard);
    spawn_drain_thread(app, state.0.clone(), wake_rx);
    Ok(())
}

#[tauri::command]
pub fn sync_demo_stop(state: State<SyncState>) {
    // Dropping the `SyncRunner` sends it `Command::Stop` itself (see its
    // `Drop` impl) — no explicit teardown call needed here.
    state.0.lock().expect("sync state mutex poisoned").take();
}

#[tauri::command]
pub fn sync_demo_now(state: State<SyncState>) {
    if let Some(runner) = state.0.lock().expect("sync state mutex poisoned").as_ref() {
        runner.sync_now();
    }
}

fn spawn_drain_thread(
    app: AppHandle,
    runner: Arc<Mutex<Option<SyncRunner>>>,
    wake_rx: std::sync::mpsc::Receiver<()>,
) {
    std::thread::spawn(move || {
        loop {
            let _ = wake_rx.recv_timeout(Duration::from_millis(250));
            let guard = runner.lock().expect("sync state mutex poisoned");
            let Some(active) = guard.as_ref() else {
                return;
            };
            let events = active.poll();
            drop(guard);
            for event in events {
                let _ = app.emit(EVENT, SyncEventDto::from(event));
            }
        }
    });
}
