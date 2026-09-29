//! The byte layout of one sealed (encrypted) update, and the AAD both ends bind
//! it to.
//!
//! ```text
//! [ format_version: u8 ][ key_version: u8 ][ nonce: 24 bytes ][ ciphertext || tag ]
//! ```
//!
//! The cipher is XChaCha20-Poly1305 with a **random** nonce per update (never a
//! counter: many devices write under one key with no shared ordered stream, so
//! counters would collide). The authenticated-but-not-encrypted data (AAD) is
//! `vault_id ‖ doc_id ‖ format_version ‖ key_version` (see [`aad`]), so an
//! untrusted server cannot move a valid blob from one document or vault to
//! another. `seq` is deliberately *not* in the AAD — the server assigns it after
//! the client has already sealed the blob.
//!
//! Sealing/opening is implemented in the client (`src/sync/crypto.rs`); this
//! module only owns the framing so client and server can't drift apart.

use crate::ids::{DocId, VaultId};

/// Current envelope layout version.
pub const FORMAT_VERSION: u8 = 1;
/// XChaCha20-Poly1305 nonce length.
pub const NONCE_LEN: usize = 24;
/// Poly1305 authentication tag length — the minimum ciphertext size.
pub const TAG_LEN: usize = 16;
/// Bytes before the ciphertext: two version bytes plus the nonce.
pub const HEADER_LEN: usize = 2 + NONCE_LEN;
/// Length of the AAD produced by [`aad`].
pub const AAD_LEN: usize = 16 + 16 + 2;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EnvelopeError {
    #[error("sealed update is too short to be valid ({0} bytes)")]
    Truncated(usize),
    #[error("sealed update uses an unsupported format version {0}")]
    UnsupportedFormat(u8),
}

/// A parsed (but not yet decrypted) sealed update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Envelope {
    pub format_version: u8,
    /// Which derived key sealed this update; lets a future re-key operation
    /// coexist with older data.
    pub key_version: u8,
    pub nonce: [u8; NONCE_LEN],
    /// Ciphertext with the authentication tag appended.
    pub ciphertext: Vec<u8>,
}

impl Envelope {
    pub fn new(key_version: u8, nonce: [u8; NONCE_LEN], ciphertext: Vec<u8>) -> Self {
        Self {
            format_version: FORMAT_VERSION,
            key_version,
            nonce,
            ciphertext,
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(HEADER_LEN + self.ciphertext.len());
        out.push(self.format_version);
        out.push(self.key_version);
        out.extend_from_slice(&self.nonce);
        out.extend_from_slice(&self.ciphertext);
        out
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, EnvelopeError> {
        if bytes.len() < HEADER_LEN + TAG_LEN {
            return Err(EnvelopeError::Truncated(bytes.len()));
        }
        let format_version = bytes[0];
        if format_version != FORMAT_VERSION {
            return Err(EnvelopeError::UnsupportedFormat(format_version));
        }
        let mut nonce = [0u8; NONCE_LEN];
        nonce.copy_from_slice(&bytes[2..HEADER_LEN]);
        Ok(Self {
            format_version,
            key_version: bytes[1],
            nonce,
            ciphertext: bytes[HEADER_LEN..].to_vec(),
        })
    }
}

/// The AAD a sealed update for `(vault, doc)` must be bound to.
pub fn aad(vault: VaultId, doc: DocId, format_version: u8, key_version: u8) -> [u8; AAD_LEN] {
    let mut out = [0u8; AAD_LEN];
    out[..16].copy_from_slice(vault.as_bytes());
    out[16..32].copy_from_slice(doc.as_bytes());
    out[32] = format_version;
    out[33] = key_version;
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn sample() -> Envelope {
        Envelope::new(1, [7u8; NONCE_LEN], vec![9u8; TAG_LEN + 5])
    }

    #[test]
    fn an_envelope_round_trips_through_bytes() {
        let env = sample();
        assert_eq!(Envelope::parse(&env.to_bytes()).unwrap(), env);
    }

    #[test]
    fn too_short_input_is_rejected_not_panicked_on() {
        assert_eq!(Envelope::parse(&[]), Err(EnvelopeError::Truncated(0)));
        let mut bytes = sample().to_bytes();
        bytes.truncate(HEADER_LEN + TAG_LEN - 1);
        assert!(matches!(
            Envelope::parse(&bytes),
            Err(EnvelopeError::Truncated(_))
        ));
    }

    #[test]
    fn an_unknown_format_version_is_rejected() {
        let mut bytes = sample().to_bytes();
        bytes[0] = FORMAT_VERSION + 1;
        assert_eq!(
            Envelope::parse(&bytes),
            Err(EnvelopeError::UnsupportedFormat(FORMAT_VERSION + 1))
        );
    }

    #[test]
    fn aad_binds_vault_doc_and_versions() {
        let v1 = VaultId(Uuid::from_u128(1));
        let v2 = VaultId(Uuid::from_u128(2));
        let d1 = DocId(Uuid::from_u128(10));
        let d2 = DocId(Uuid::from_u128(20));
        let base = aad(v1, d1, FORMAT_VERSION, 1);
        assert_ne!(base, aad(v2, d1, FORMAT_VERSION, 1), "vault must matter");
        assert_ne!(base, aad(v1, d2, FORMAT_VERSION, 1), "doc must matter");
        assert_ne!(
            base,
            aad(v1, d1, FORMAT_VERSION, 2),
            "key version must matter"
        );
        assert_ne!(
            base,
            aad(v1, d1, FORMAT_VERSION + 1, 1),
            "format must matter"
        );
    }
}
