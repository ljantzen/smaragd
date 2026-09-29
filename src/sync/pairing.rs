//! Setting a project up for sync and managing its vault: the one-off, blocking server
//! operations behind the Sync panel. Each is a plain function so the app can run it on a
//! thread and the end-to-end tests can drive it against a real server.
//!
//! Where things go (see `link`): the non-secret [`ProjectLink`] is written into the
//! project, and this device's [`DeviceCredentials`] into the OS data directory
//! (`data_root`), never the project. The data root is a parameter so tests can use a
//! temporary one.

use std::path::Path;
use std::sync::Arc;

use smaragd_sync_protocol::DeviceId;
use smaragd_sync_protocol::api::{DeviceInfo, HealthResponse, KDF_SALT_LEN};
use smaragd_sync_protocol::ticket::{ServerAddr, SyncTicket};

use super::client::HttpClient;
use super::link::{DeviceCredentials, ProjectLink, forget_local_state, state_dir};
use super::state::DirStateStore;
use super::transport::TransportError;
use crate::project::store::ProjectStore;

/// A project that is now paired with a vault.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paired {
    pub link: ProjectLink,
    pub credentials: DeviceCredentials,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PairError {
    /// The server only lets its administrator create vaults; ask for the admin token.
    #[error("this server only lets its administrator create vaults")]
    NeedsAdminToken,
    #[error("The server is unreachable: {0}")]
    Offline(String),
    #[error("{0}")]
    Rejected(String),
    #[error("{0}")]
    Local(String),
}

impl From<TransportError> for PairError {
    fn from(err: TransportError) -> Self {
        match err {
            TransportError::Offline(why) => PairError::Offline(why),
            TransportError::Unauthorized => PairError::Rejected(
                "The server no longer accepts this device (it may have been revoked).".into(),
            ),
            TransportError::Rejected(why) | TransportError::Server(why) => PairError::Rejected(why),
        }
    }
}

fn persist(
    files: &Arc<dyn ProjectStore>,
    project_root: &Path,
    data_root: &Path,
    paired: &Paired,
) -> Result<(), PairError> {
    let mut state = DirStateStore::new(
        Arc::clone(files),
        state_dir(data_root, paired.link.vault_id),
    );
    paired
        .credentials
        .save(&mut state)
        .map_err(|err| PairError::Local(format!("saving this device's credentials: {err}")))?;
    paired
        .link
        .save(&**files, project_root)
        .map_err(|err| PairError::Local(format!("saving the project's sync link: {err}")))
}

/// "Test Connection": is there a Smaragd sync server at `server`?
pub fn test_connection(server: &ServerAddr) -> Result<HealthResponse, TransportError> {
    HttpClient::new(server).health()
}

/// Creates a new vault for this project and pairs this device with it. Pass the admin
/// token only if the server needs one; a server that does returns
/// [`PairError::NeedsAdminToken`] when it's missing.
pub fn create_vault(
    files: &Arc<dyn ProjectStore>,
    project_root: &Path,
    data_root: &Path,
    server: &ServerAddr,
    admin_token: Option<&str>,
    device_name: &str,
) -> Result<Paired, PairError> {
    let salt: [u8; KDF_SALT_LEN] = rand::random();
    let created = HttpClient::new(server)
        .create_vault(admin_token, device_name, &salt)
        .map_err(|err| match (err, admin_token) {
            (TransportError::Rejected(_), None) => PairError::NeedsAdminToken,
            (TransportError::Rejected(_), Some(_)) => {
                PairError::Rejected("The server refused that admin token.".into())
            }
            (other, _) => other.into(),
        })?;
    let paired = Paired {
        link: ProjectLink::new(server.clone(), &created.vault),
        credentials: DeviceCredentials {
            device_id: created.device_id,
            token: created.device_token,
        },
    };
    persist(files, project_root, data_root, &paired)?;
    Ok(paired)
}

/// Pairs this device with an existing vault using a ticket made on another device.
pub fn join_vault(
    files: &Arc<dyn ProjectStore>,
    project_root: &Path,
    data_root: &Path,
    ticket: &SyncTicket,
    device_name: &str,
) -> Result<Paired, PairError> {
    let joined =
        HttpClient::new(&ticket.server).redeem_pairing_code(&ticket.pairing_code, device_name)?;
    if joined.vault.vault_id != ticket.vault_id {
        return Err(PairError::Rejected(
            "The server returned a different vault than the ticket names.".into(),
        ));
    }
    let paired = Paired {
        link: ProjectLink::new(ticket.server.clone(), &joined.vault),
        credentials: DeviceCredentials {
            device_id: joined.device_id,
            token: joined.device_token,
        },
    };
    persist(files, project_root, data_root, &paired)?;
    Ok(paired)
}

fn client(link: &ProjectLink, credentials: &DeviceCredentials) -> HttpClient {
    HttpClient::new(&link.server).with_token(credentials.token.clone())
}

/// A single-use ticket another device can redeem to join this vault.
pub fn make_ticket(
    link: &ProjectLink,
    credentials: &DeviceCredentials,
) -> Result<SyncTicket, PairError> {
    let code = client(link, credentials).create_pairing_code(link.vault_id)?;
    Ok(SyncTicket::new(
        link.server.clone(),
        link.vault_id,
        code.code,
    ))
}

pub fn list_devices(
    link: &ProjectLink,
    credentials: &DeviceCredentials,
) -> Result<Vec<DeviceInfo>, PairError> {
    Ok(client(link, credentials).list_devices(link.vault_id)?)
}

pub fn revoke_device(
    link: &ProjectLink,
    credentials: &DeviceCredentials,
    device: DeviceId,
) -> Result<(), PairError> {
    Ok(client(link, credentials).revoke_device(link.vault_id, device)?)
}

/// Stops syncing a project: removes this device from the vault (best effort — if the
/// server is unreachable the local side is still cleaned up, and the returned flag says
/// so), removes the project's link and forgets the local sync state. The project's files
/// are untouched.
pub fn leave_vault(
    files: &Arc<dyn ProjectStore>,
    project_root: &Path,
    data_root: &Path,
    link: &ProjectLink,
    credentials: &DeviceCredentials,
) -> Result<bool, PairError> {
    let removed_from_server = revoke_device(link, credentials, credentials.device_id).is_ok();
    ProjectLink::remove(&**files, project_root)
        .map_err(|err| PairError::Local(format!("removing the project's sync link: {err}")))?;
    forget_local_state(&**files, data_root, link.vault_id)
        .map_err(|err| PairError::Local(format!("removing local sync state: {err}")))?;
    Ok(removed_from_server)
}

/// The vault a link points at, for display.
pub fn describe(link: &ProjectLink) -> String {
    format!("{}:{}", link.server.host, link.server.port)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transport_failures_map_to_user_meaningful_pairing_errors() {
        assert!(matches!(
            PairError::from(TransportError::Offline("refused".into())),
            PairError::Offline(_)
        ));
        assert!(matches!(
            PairError::from(TransportError::Rejected("invalid or expired pairing code".into())),
            PairError::Rejected(message) if message.contains("pairing code")
        ));
        assert!(matches!(
            PairError::from(TransportError::Unauthorized),
            PairError::Rejected(_)
        ));
    }

    #[test]
    fn describe_shows_host_and_port() {
        use smaragd_sync_protocol::VaultId;
        let link = ProjectLink::new(
            ServerAddr {
                host: "sync.example.com".into(),
                port: 8443,
                use_tls: true,
                path: String::new(),
            },
            &smaragd_sync_protocol::api::VaultInfo {
                vault_id: VaultId(uuid::Uuid::from_u128(1)),
                kdf_salt: vec![0; KDF_SALT_LEN],
                key_version: 1,
            },
        );
        assert_eq!(describe(&link), "sync.example.com:8443");
    }
}
