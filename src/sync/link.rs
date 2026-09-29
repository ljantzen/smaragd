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
//! Because the link travels with the project, anyone who can change the project's files
//! (a git collaborator, a restored backup) can rewrite it. So the link is never trusted
//! with anything that matters: the credentials also record the server that issued the
//! token and the vault's salt ([`TrustedVault`]), and [`DeviceCredentials::trusted_link`]
//! always takes those from the credentials. A `sync.json` naming another server can't
//! send the token — or the project, sealed under the passphrase — anywhere else.
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
    /// The server that issued `token` and the vault's key-derivation facts, as this
    /// device learned them when it paired. `None` only in credentials saved before this
    /// was recorded; [`Self::trusted_link`] fills it in.
    #[serde(default)]
    pub vault: Option<TrustedVault>,
}

/// What the credentials, not the project's link file, say about the vault.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustedVault {
    pub server: ServerAddr,
    #[serde(with = "b64")]
    pub kdf_salt: Vec<u8>,
    pub key_version: u8,
}

impl TrustedVault {
    fn of(link: &ProjectLink) -> Self {
        Self {
            server: link.server.clone(),
            kdf_salt: link.kdf_salt.clone(),
            key_version: link.key_version,
        }
    }
}

/// The link to sync with, as [`DeviceCredentials::trusted_link`] settles it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustedLink {
    /// The project's vault, with the server and salt taken from the credentials.
    pub link: ProjectLink,
    /// The project's `sync.json` names a different server or salt than this device was
    /// paired with — ignored, but worth telling the user about.
    pub file_differs: bool,
    /// The credentials predate [`DeviceCredentials::vault`] and were just bound to the
    /// link as it stands; save them.
    pub newly_bound: bool,
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
    /// Fresh credentials from pairing, bound to the server and vault they came from.
    pub fn new(device_id: DeviceId, token: String, link: &ProjectLink) -> Self {
        Self {
            device_id,
            token,
            vault: Some(TrustedVault::of(link)),
        }
    }

    /// The link to actually use for `from_project` (the project's `sync.json`): its vault,
    /// but the server, salt and key version these credentials were issued with.
    ///
    /// Credentials saved before the server was recorded are bound to `from_project` as it
    /// is now (trust on first use) — the best that can be done for them, and no worse than
    /// before; the caller saves them so it happens only once.
    pub fn trusted_link(&mut self, from_project: &ProjectLink) -> TrustedLink {
        let newly_bound = self.vault.is_none();
        let trusted = self
            .vault
            .get_or_insert_with(|| TrustedVault::of(from_project))
            .clone();
        let file_differs = trusted != TrustedVault::of(from_project);
        TrustedLink {
            link: ProjectLink {
                version: CURRENT_VERSION,
                server: trusted.server,
                vault_id: from_project.vault_id,
                kdf_salt: trusted.kdf_salt,
                key_version: trusted.key_version,
            },
            file_differs,
            newly_bound,
        }
    }

    pub fn load(state: &dyn StateStore) -> Option<Self> {
        let bytes = state.get(CREDENTIALS_KEY).ok()??;
        serde_json::from_slice(&bytes).ok()
    }

    pub fn save(&self, state: &mut dyn StateStore) -> io::Result<()> {
        let bytes = serde_json::to_vec(self)?;
        state.put(CREDENTIALS_KEY, &bytes)
    }

    /// Narrows the stored credentials to owner-only — for a token saved before sync
    /// wrote it that way.
    pub fn make_private(state: &super::state::DirStateStore) -> io::Result<()> {
        state.make_private(CREDENTIALS_KEY)
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
    fn a_rewritten_link_file_cannot_redirect_the_token_or_change_the_salt() {
        let genuine = link();
        let mut creds = DeviceCredentials::new(DeviceId(Uuid::from_u128(3)), "t".into(), &genuine);

        let untouched = creds.trusted_link(&genuine);
        assert_eq!(untouched.link, genuine);
        assert!(!untouched.file_differs && !untouched.newly_bound);

        let mut hostile = genuine.clone();
        hostile.server.host = "attacker.example".into();
        hostile.server.use_tls = false;
        hostile.kdf_salt = vec![0; 16];
        let settled = creds.trusted_link(&hostile);
        assert_eq!(
            settled.link, genuine,
            "the credentials' server and salt win"
        );
        assert!(settled.file_differs);
        assert!(!settled.newly_bound);
    }

    #[test]
    fn credentials_saved_before_binding_are_bound_on_first_use() {
        // What an older version saved: no `vault` field.
        let json = br#"{"device_id":"00000000-0000-0000-0000-000000000003","token":"t"}"#;
        let mut store = MemoryStateStore::default();
        store.put("device", json).unwrap();
        let mut creds = DeviceCredentials::load(&store).expect("still readable");
        assert_eq!(creds.vault, None);

        let first = creds.trusted_link(&link());
        assert!(first.newly_bound && !first.file_differs);
        assert_eq!(first.link, link());

        let mut moved = link();
        moved.server.host = "elsewhere.example".into();
        let later = creds.trusted_link(&moved);
        assert_eq!(later.link, link(), "bound to what it first saw");
        assert!(later.file_differs && !later.newly_bound);
    }

    #[test]
    fn credentials_round_trip_and_never_print_the_token() {
        let mut store = MemoryStateStore::default();
        assert_eq!(DeviceCredentials::load(&store), None);
        let creds = DeviceCredentials::new(
            DeviceId(Uuid::from_u128(3)),
            "sst_supersecret".into(),
            &link(),
        );
        creds.save(&mut store).unwrap();
        assert_eq!(DeviceCredentials::load(&store), Some(creds.clone()));
        assert!(!format!("{creds:?}").contains("supersecret"));
    }
}
