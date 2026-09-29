//! The pairing between one project and one vault, and this device's credentials for it.
//!
//! Two deliberately separate pieces, split by what git and backups can see:
//!
//! - **[`ProjectLink`]** lives in the project at `.smaragd/sync.json` and holds only
//!   non-secret facts: which server, which vault, and the vault's public key-derivation
//!   salt. Presence means "this project is paired". It is safe to commit, copy or
//!   back up (`git.rs` runs `git add -A` and `backup.rs` zips `.smaragd/`); a copy on
//!   another device just prompts to pair, since it carries no credentials.
//! - **[`DeviceCredentials`]** — this device's access token — live *outside* the project,
//!   in the same OS data-dir store as the engine's CRDT state ([`state_dir`]).
//!
//! The encryption key is in neither: it is derived in memory from the passphrase in
//! Settings and never written anywhere.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use smaragd_sync_protocol::api::{VaultInfo, b64};
use smaragd_sync_protocol::ticket::ServerAddr;
use smaragd_sync_protocol::{DeviceId, VaultId};

use super::state::StateStore;
use crate::project::store::ProjectStore;

/// Where the link lives, relative to the project root.
pub const LINK_PATH: &str = ".smaragd/sync.json";
const CREDENTIALS_KEY: &str = "device";
const CURRENT_VERSION: u8 = 1;

/// Non-secret facts about a project's vault. See the module docs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectLink {
    pub version: u8,
    /// The server this project was paired with (it may differ from Settings, which is
    /// only the default for *new* pairings).
    pub server: ServerAddr,
    pub vault_id: VaultId,
    /// The vault's Argon2id salt (public), so starting sync needs no network round trip.
    #[serde(with = "b64")]
    pub kdf_salt: Vec<u8>,
    pub key_version: u8,
}

impl ProjectLink {
    pub fn new(server: ServerAddr, vault: &VaultInfo) -> Self {
        Self {
            version: CURRENT_VERSION,
            server,
            vault_id: vault.vault_id,
            kdf_salt: vault.kdf_salt.clone(),
            key_version: vault.key_version,
        }
    }

    /// The link stored in `root`, if the project is paired. An unreadable or
    /// unrecognised file counts as "not paired" rather than an error.
    pub fn load(store: &dyn ProjectStore, root: &Path) -> Option<Self> {
        let text = store.read_to_string(&root.join(LINK_PATH)).ok()?;
        let link: Self = serde_json::from_str(&text).ok()?;
        (link.version == CURRENT_VERSION).then_some(link)
    }

    pub fn save(&self, store: &dyn ProjectStore, root: &Path) -> io::Result<()> {
        let path = root.join(LINK_PATH);
        if let Some(dir) = path.parent() {
            store.create_dir_all(dir)?;
        }
        let text = serde_json::to_string_pretty(self)?;
        store.write(&path, text.as_bytes())
    }

    /// Un-pairs the project (the files stay exactly as they are).
    pub fn remove(store: &dyn ProjectStore, root: &Path) -> io::Result<()> {
        match store.remove_file(&root.join(LINK_PATH)) {
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
            other => other,
        }
    }
}

/// This device's identity in a vault and the bearer token that proves it.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceCredentials {
    pub device_id: DeviceId,
    pub token: String,
}

impl std::fmt::Debug for DeviceCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceCredentials")
            .field("device_id", &self.device_id)
            .field("token", &"<redacted>")
            .finish()
    }
}

impl DeviceCredentials {
    pub fn load(state: &dyn StateStore) -> Option<Self> {
        let bytes = state.get(CREDENTIALS_KEY).ok()??;
        serde_json::from_slice(&bytes).ok()
    }

    pub fn save(&self, state: &mut dyn StateStore) -> io::Result<()> {
        let bytes = serde_json::to_vec(self)?;
        state.put(CREDENTIALS_KEY, &bytes)
    }
}

/// The directory under which every vault's local state and this device's credentials
/// live: the OS data directory, never a project folder. `None` if the platform has no
/// data dir (and always on the browser build, which has no sync yet).
#[cfg(not(target_arch = "wasm32"))]
pub fn data_root() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "smaragd").map(|dirs| dirs.data_dir().join("sync"))
}

#[cfg(target_arch = "wasm32")]
pub fn data_root() -> Option<PathBuf> {
    None
}

/// One vault's state directory under `data_root`.
pub fn state_dir(data_root: &Path, vault: VaultId) -> PathBuf {
    data_root.join(vault.to_string())
}

/// Deletes a vault's local state and credentials (when leaving it).
pub fn forget_local_state(
    store: &dyn ProjectStore,
    data_root: &Path,
    vault: VaultId,
) -> io::Result<()> {
    let dir = state_dir(data_root, vault);
    if store.exists(&dir) {
        store.remove_dir_all(&dir)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::store::native_store;
    use crate::sync::state::MemoryStateStore;
    use uuid::Uuid;

    fn link() -> ProjectLink {
        ProjectLink::new(
            ServerAddr {
                host: "sync.example.com".into(),
                port: 443,
                use_tls: true,
                path: String::new(),
            },
            &VaultInfo {
                vault_id: VaultId(Uuid::from_u128(7)),
                kdf_salt: vec![9; 16],
                key_version: 1,
            },
        )
    }

    #[test]
    fn a_link_round_trips_through_the_project_folder() {
        let dir = tempfile::tempdir().unwrap();
        let store = native_store();
        assert_eq!(ProjectLink::load(&*store, dir.path()), None);

        link().save(&*store, dir.path()).unwrap();
        assert_eq!(ProjectLink::load(&*store, dir.path()), Some(link()));

        ProjectLink::remove(&*store, dir.path()).unwrap();
        assert_eq!(ProjectLink::load(&*store, dir.path()), None);
        ProjectLink::remove(&*store, dir.path()).unwrap(); // already gone is fine
    }

    #[test]
    fn the_link_file_holds_nothing_secret() {
        let dir = tempfile::tempdir().unwrap();
        link().save(&*native_store(), dir.path()).unwrap();
        let text = std::fs::read_to_string(dir.path().join(LINK_PATH)).unwrap();
        assert!(text.contains("sync.example.com") && text.contains("kdf_salt"));
        assert!(
            !text.contains("token") && !text.contains("passphrase"),
            "{text}"
        );
    }

    #[test]
    fn garbage_or_a_future_version_reads_as_not_paired() {
        let dir = tempfile::tempdir().unwrap();
        let store = native_store();
        std::fs::create_dir_all(dir.path().join(".smaragd")).unwrap();
        std::fs::write(dir.path().join(LINK_PATH), "{ nope").unwrap();
        assert_eq!(ProjectLink::load(&*store, dir.path()), None);

        let mut future = link();
        future.version = 99;
        future.save(&*store, dir.path()).unwrap();
        assert_eq!(ProjectLink::load(&*store, dir.path()), None);
    }

    #[test]
    fn forgetting_local_state_removes_only_that_vaults_directory() {
        let root = tempfile::tempdir().unwrap();
        let store = native_store();
        let (a, b) = (VaultId(Uuid::from_u128(1)), VaultId(Uuid::from_u128(2)));
        for vault in [a, b] {
            std::fs::create_dir_all(state_dir(root.path(), vault)).unwrap();
        }
        forget_local_state(&*store, root.path(), a).unwrap();
        assert!(!state_dir(root.path(), a).exists());
        assert!(state_dir(root.path(), b).exists());
        forget_local_state(&*store, root.path(), a).unwrap(); // already gone is fine
    }

    #[test]
    fn credentials_round_trip_and_never_print_the_token() {
        let mut store = MemoryStateStore::default();
        assert_eq!(DeviceCredentials::load(&store), None);
        let creds = DeviceCredentials {
            device_id: DeviceId(Uuid::from_u128(3)),
            token: "sst_supersecret".into(),
        };
        creds.save(&mut store).unwrap();
        assert_eq!(DeviceCredentials::load(&store), Some(creds.clone()));
        assert!(!format!("{creds:?}").contains("supersecret"));
    }
}
