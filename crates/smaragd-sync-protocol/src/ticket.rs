//! The pasteable pairing code a second device uses to join a vault.
//!
//! Modeled on `src/collab/ticket.rs`'s `CollabTicket` (postcard, then base58 —
//! chosen because it has no visually ambiguous characters, which is what matters
//! for a string a human copies by hand). It carries *routing only*: where the
//! server is, which vault, and a **single-use, short-lived** pairing code that the
//! new device redeems for its own long-lived device token. It never carries a
//! bearer token or any key material — the encryption passphrase is typed
//! separately on each device.

use serde::{Deserialize, Serialize};

use crate::ids::VaultId;

/// Where a sync server lives — mirrors the Settings dialog's fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerAddr {
    pub host: String,
    pub port: u16,
    pub use_tls: bool,
    /// Optional reverse-proxy path prefix (e.g. `smaragd` for
    /// `https://example.com/smaragd/v1/...`). Empty for none.
    pub path: String,
}

impl ServerAddr {
    /// `scheme://host:port[/path]` with no trailing slash, ready to have
    /// [`crate::api::API_PREFIX`] and an endpoint appended.
    pub fn base_url(&self) -> String {
        let scheme = if self.use_tls { "https" } else { "http" };
        let host = if self.host.contains(':') && !self.host.starts_with('[') {
            format!("[{}]", self.host)
        } else {
            self.host.clone()
        };
        let path = self.path.trim_matches('/');
        if path.is_empty() {
            format!("{scheme}://{host}:{}", self.port)
        } else {
            format!("{scheme}://{host}:{}/{path}", self.port)
        }
    }
}

/// Everything a new device needs to reach a vault and redeem its pairing code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncTicket {
    /// Format version, so this can evolve without breaking old pasted codes.
    pub version: u8,
    pub server: ServerAddr,
    pub vault_id: VaultId,
    pub pairing_code: String,
}

const CURRENT_VERSION: u8 = 1;

#[derive(Debug, thiserror::Error)]
pub enum TicketError {
    #[error("pairing code is not valid base58: {0}")]
    Base58(#[from] bs58::decode::Error),
    #[error("pairing code is malformed: {0}")]
    Postcard(#[from] postcard::Error),
    #[error("pairing code is from an unsupported format version {0}")]
    UnsupportedVersion(u8),
}

impl SyncTicket {
    pub fn new(server: ServerAddr, vault_id: VaultId, pairing_code: String) -> Self {
        Self {
            version: CURRENT_VERSION,
            server,
            vault_id,
            pairing_code,
        }
    }

    pub fn encode(&self) -> String {
        let bytes = postcard::to_stdvec(self).expect("SyncTicket always serializes");
        bs58::encode(bytes).into_string()
    }

    pub fn decode(code: &str) -> Result<Self, TicketError> {
        let bytes = bs58::decode(code.trim()).into_vec()?;
        let ticket: Self = postcard::from_bytes(&bytes)?;
        if ticket.version != CURRENT_VERSION {
            return Err(TicketError::UnsupportedVersion(ticket.version));
        }
        Ok(ticket)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn addr() -> ServerAddr {
        ServerAddr {
            host: "sync.example.com".into(),
            port: 8443,
            use_tls: true,
            path: String::new(),
        }
    }

    fn ticket() -> SyncTicket {
        SyncTicket::new(addr(), VaultId(Uuid::from_u128(99)), "ABCD-EFGH".into())
    }

    #[test]
    fn a_ticket_round_trips_through_encode_and_decode() {
        let t = ticket();
        assert_eq!(SyncTicket::decode(&t.encode()).unwrap(), t);
    }

    #[test]
    fn surrounding_whitespace_from_pasting_is_tolerated() {
        let t = ticket();
        assert_eq!(
            SyncTicket::decode(&format!("  {}\n", t.encode())).unwrap(),
            t
        );
    }

    #[test]
    fn the_encoded_code_contains_no_ambiguous_base58_characters() {
        let code = ticket().encode();
        for forbidden in ['0', 'O', 'I', 'l'] {
            assert!(!code.contains(forbidden), "{code:?} contains {forbidden:?}");
        }
    }

    #[test]
    fn decoding_garbage_fails_cleanly() {
        assert!(SyncTicket::decode("not a valid code").is_err());
        assert!(SyncTicket::decode("").is_err());
    }

    #[test]
    fn decoding_a_future_format_version_is_rejected() {
        let mut t = ticket();
        t.version = CURRENT_VERSION + 1;
        let code = bs58::encode(postcard::to_stdvec(&t).unwrap()).into_string();
        assert!(matches!(
            SyncTicket::decode(&code),
            Err(TicketError::UnsupportedVersion(v)) if v == CURRENT_VERSION + 1
        ));
    }

    #[test]
    fn base_url_handles_scheme_path_and_ipv6() {
        assert_eq!(addr().base_url(), "https://sync.example.com:8443");

        let mut a = addr();
        a.use_tls = false;
        a.path = "/smaragd/".into();
        assert_eq!(a.base_url(), "http://sync.example.com:8443/smaragd");

        a.host = "::1".into();
        a.path.clear();
        assert_eq!(a.base_url(), "http://[::1]:8443");
    }
}
