//! Request/response bodies for the sync server's HTTP API (see the table in the
//! crate docs for the endpoints).
//!
//! Bodies are JSON so the API stays debuggable with `curl`, with binary fields
//! (sealed envelopes, KDF salts) carried as standard base64 strings via [`b64`].
//! The one exception is *pushing* an update, whose request body is the raw sealed
//! envelope bytes (`application/octet-stream`) — no wrapper, nothing to get wrong.

use serde::{Deserialize, Serialize};

use crate::ids::{DeviceId, DocId, VaultId};

/// Every endpoint lives under this prefix; bump it for a breaking API change.
pub const API_PREFIX: &str = "/v1";
/// Largest sealed update or snapshot the server accepts.
pub const MAX_BLOB_BYTES: usize = 8 * 1024 * 1024;
/// How long a pairing code stays redeemable.
pub const PAIRING_CODE_TTL_SECS: u64 = 600;
/// Length of the per-vault Argon2id salt.
pub const KDF_SALT_LEN: usize = 16;
/// The key-derivation scheme new vaults are created with (see the envelope's
/// `key_version`); the client's `sync::crypto::KEY_VERSION` must match it.
pub const INITIAL_KEY_VERSION: u8 = 1;
/// Header carrying the server's admin token when creating a vault on a server
/// that has open registration turned off.
pub const ADMIN_TOKEN_HEADER: &str = "x-admin-token";
/// Alphabet for pairing codes: no `0/O/1/I/L`, so a code read aloud or retyped
/// can't be misread.
pub const PAIRING_CODE_ALPHABET: &[u8; 31] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";
/// Characters in a pairing code, before display grouping (`XXXX-XXXX-XXXX`).
pub const PAIRING_CODE_LEN: usize = 12;

/// Canonical form of a pairing code for comparison and hashing: upper-cased with
/// dashes and whitespace removed, so `abcd-efgh jk23` matches `ABCDEFGHJK23`.
pub fn normalize_pairing_code(code: &str) -> String {
    code.chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .flat_map(char::to_uppercase)
        .collect()
}

/// `GET /health` body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
}

/// Serde adapter: `Vec<u8>` <-> standard base64 string.
pub mod b64 {
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&STANDARD.encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let text = String::deserialize(d)?;
        STANDARD.decode(text).map_err(serde::de::Error::custom)
    }
}

/// The JSON body of every non-2xx response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiError {
    pub error: String,
}

/// The public part of a vault: everything a device needs, besides the
/// passphrase, to derive its keys.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VaultInfo {
    pub vault_id: VaultId,
    /// Random, non-secret Argon2id salt, chosen by the creating client.
    #[serde(with = "b64")]
    pub kdf_salt: Vec<u8>,
    /// Which derived key new updates are sealed with (see the envelope).
    pub key_version: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateVaultRequest {
    pub device_name: String,
    #[serde(with = "b64")]
    pub kdf_salt: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateVaultResponse {
    pub vault: VaultInfo,
    pub device_id: DeviceId,
    /// This device's bearer token. Shown once; the server keeps only a hash.
    pub device_token: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreatePairingCodeResponse {
    pub code: String,
    pub expires_in_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedeemPairingRequest {
    pub code: String,
    pub device_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedeemPairingResponse {
    pub vault: VaultInfo,
    pub device_id: DeviceId,
    pub device_token: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub device_id: DeviceId,
    pub name: String,
    pub created_at_unix: u64,
    pub last_seen_unix: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListDevicesResponse {
    pub devices: Vec<DeviceInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocSummary {
    pub doc_id: DocId,
    /// Highest `seq` the server holds for this document.
    pub latest_seq: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListDocsResponse {
    pub docs: Vec<DocSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushUpdateResponse {
    /// The sequence number the server assigned (per vault+doc, gap-free, from 1).
    pub seq: u64,
}

/// A compaction snapshot: the client's merged state of a document as of `upto_seq`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub upto_seq: u64,
    /// A sealed envelope, like any update.
    #[serde(with = "b64")]
    pub blob: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredUpdate {
    pub seq: u64,
    pub device_id: DeviceId,
    #[serde(with = "b64")]
    pub blob: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullUpdatesResponse {
    /// Present when the document has been compacted past the requested `since`;
    /// apply it first, then `updates`.
    pub snapshot: Option<Snapshot>,
    /// Updates with `seq` greater than both `since` and the snapshot's `upto_seq`.
    pub updates: Vec<StoredUpdate>,
}

/// `PUT .../snapshot` body. The server stores it and deletes updates with
/// `seq <= upto_seq`; anything newer is kept, and because CRDT updates are
/// idempotent a snapshot that overlaps newer updates is harmless.
pub type PutSnapshotRequest = Snapshot;

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn binary_fields_travel_as_base64_json() {
        let resp = PullUpdatesResponse {
            snapshot: Some(Snapshot {
                upto_seq: 4,
                blob: vec![0, 1, 2, 250, 255],
            }),
            updates: vec![StoredUpdate {
                seq: 5,
                device_id: DeviceId(Uuid::from_u128(7)),
                blob: vec![9; 40],
            }],
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"AAEC+v8=\""), "expected base64 in {json}");
        assert_eq!(
            serde_json::from_str::<PullUpdatesResponse>(&json).unwrap(),
            resp
        );
    }

    #[test]
    fn invalid_base64_is_a_parse_error_not_a_panic() {
        let json = r#"{"upto_seq":1,"blob":"!!not base64!!"}"#;
        assert!(serde_json::from_str::<Snapshot>(json).is_err());
    }

    #[test]
    fn pairing_codes_normalize_regardless_of_case_dashes_and_spaces() {
        assert_eq!(normalize_pairing_code("abcd-efgh jk23"), "ABCDEFGHJK23");
        assert_eq!(normalize_pairing_code(" ABCD-EFGH-JK23\n"), "ABCDEFGHJK23");
    }

    #[test]
    fn vault_info_round_trips() {
        let info = VaultInfo {
            vault_id: VaultId(Uuid::from_u128(1)),
            kdf_salt: vec![3; KDF_SALT_LEN],
            key_version: 1,
        };
        let json = serde_json::to_string(&info).unwrap();
        assert_eq!(serde_json::from_str::<VaultInfo>(&json).unwrap(), info);
    }
}
