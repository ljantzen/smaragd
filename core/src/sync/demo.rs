//! An ephemeral, no-server sync demo.
//!
//! Wires a real [`SyncEngine`] to the in-memory fake transport
//! ([`super::fake::MemoryServer`]/`MemoryTransport`) so the Tauri command
//! layer can run a genuine [`SyncRunner`] — the same push-event background
//! thread real sync uses — end to end, without a running
//! `smaragd-sync-server` or a completed device-pairing flow. Neither exists
//! anywhere in the egui->Tauri migration plan yet (pairing UI was never one
//! of its phases), so this stands in for "already paired" until one does.
//!
//! Not a stand-in for real sync *semantics*: there's no second device here,
//! so every pass just uploads local state into a vault nobody else reads.
//! It only proves the event-push plumbing (Phase 6's actual job) — the one
//! piece of this that production sync and the demo share byte-for-byte.
//!
//! Gated behind `test-support` since it reuses `fake`, same as every other
//! non-production-but-not-literally-`#[cfg(test)]` fixture in this crate.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use smaragd_sync_protocol::{DeviceId, VaultId};
use uuid::Uuid;

use super::crypto::cheap_test_key;
use super::engine::{EngineConfig, SyncEngine};
use super::fake::{MemoryServer, MemoryTransport};
use super::runner::{Builder, SyncRunner};
use super::state::DirStateStore;
use super::transport::SyncTransport;
use crate::project::store::ProjectStore;

const DEMO_PASSPHRASE: &str = "smaragd-sync-demo";
const DEMO_SALT: [u8; 16] = *b"smaragd-demo-16!";

/// Starts the demo loop for `root`, running a pass immediately and then every
/// `interval`. State lives under `.smaragd/sync-demo-state/` — created fresh
/// on every start and never read back by anything else, since this vault
/// only ever has the one device in it.
pub fn start_ephemeral_demo(
    root: &Path,
    files: Arc<dyn ProjectStore>,
    interval: Duration,
    wake: Box<dyn Fn() + Send>,
) -> SyncRunner {
    let state_dir = root.join(".smaragd").join("sync-demo-state");
    let root = root.to_path_buf();
    let build: Builder = Box::new(move || {
        let key = cheap_test_key(DEMO_PASSPHRASE, &DEMO_SALT);
        let server = MemoryServer::default();
        let device = DeviceId(Uuid::new_v4());
        let transport = MemoryTransport::new(&server, device);
        let state = DirStateStore::new(files.clone(), state_dir);
        let engine = SyncEngine::open(
            EngineConfig {
                vault: VaultId(Uuid::new_v4()),
                device,
                key,
                root,
            },
            files,
            Box::new(state),
        )
        .map_err(|err| err.to_string())?;
        Ok((engine, Box::new(transport) as Box<dyn SyncTransport>))
    });
    SyncRunner::spawn_with(build, interval, wake)
}
