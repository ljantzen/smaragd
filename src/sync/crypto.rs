//! Key derivation and the sealed-update envelope for sync.
//!
//! The sync server is untrusted: it holds every ciphertext forever and can drop,
//! reorder, replay or substitute blobs. So, unlike `collab::crypto` (which rides
//! one ordered, reliable stream and uses implicit counter nonces):
//!
//! - **Random nonces.** Many devices write under one vault key with no shared
//!   ordered stream, so counters would collide (catastrophic for a stream cipher).
//!   Every update gets a fresh random 24-byte XChaCha20 nonce, which is what makes
//!   the extended-nonce variant the right choice here.
//! - **AAD binding.** Each envelope is authenticated against
//!   `vault_id ‖ doc_id ‖ format_version ‖ key_version`
//!   ([`smaragd_sync_protocol::envelope::aad`]), so a valid blob moved to another
//!   document or vault fails to open instead of silently corrupting it.
//! - **A real password KDF.** The key comes from a human passphrase and the
//!   ciphertext sits on someone else's machine, so an offline attacker can
//!   brute-force guesses. [`derive_vault_key`] runs Argon2id (memory-hard) with the
//!   vault's random, non-secret salt, then splits off a purpose-scoped subkey with
//!   `blake3::derive_key` — the same versioned-context convention `collab::crypto`
//!   uses. Plain `blake3::derive_key` over the passphrase would have no work
//!   factor at all.
//! - **Versioned.** `key_version` names the whole derivation scheme (KDF, its
//!   parameters, the subkey context), so a future re-key or parameter change can
//!   coexist with data sealed under the old one instead of orphaning it.
//!
//! There is deliberately no passphrase verifier: a wrong passphrase is detected the
//! first time an update fails to open ([`CryptoError::AuthenticationFailed`]), so
//! the server never holds anything to dictionary-attack besides the ciphertext.
//!
//! Pure and synchronous: bytes in, bytes out.

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, Payload};
use chacha20poly1305::{KeyInit, XChaCha20Poly1305, XNonce};
use smaragd_sync_protocol::api::{KDF_SALT_LEN, MAX_BLOB_BYTES};
use smaragd_sync_protocol::envelope::{
    self, Envelope, EnvelopeError, FORMAT_VERSION, HEADER_LEN, NONCE_LEN, TAG_LEN,
};
use smaragd_sync_protocol::{DocId, VaultId};
use unicode_normalization::UnicodeNormalization;
use zeroize::Zeroize;

/// The only derivation scheme this build understands. Scheme 1 is: NFKC-normalized
/// passphrase → Argon2id (64 MiB, 3 passes, 1 lane, 32-byte output, vault salt) →
/// `blake3::derive_key(CONTEXT_UPDATE_KEY, ..)`.
pub const KEY_VERSION: u8 = 1;

const KDF_MEMORY_KIB: u32 = 64 * 1024;
const KDF_PASSES: u32 = 3;
const KDF_LANES: u32 = 1;
const KEY_LEN: usize = 32;

/// Domain-separation context for the update-sealing subkey. Fixed and versioned
/// per `blake3`'s own guidance; a scheme change gets a new key version *and* a new
/// context rather than editing this string.
const CONTEXT_UPDATE_KEY: &str = "smaragd 2026-09-25 sync update key v1";

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CryptoError {
    #[error("the sync passphrase is empty")]
    EmptyPassphrase,
    #[error("the vault's key-derivation salt has an invalid length ({0} bytes)")]
    BadSalt(usize),
    #[error("this vault uses key version {0}, which this version of Smaragd doesn't support")]
    UnsupportedKeyVersion(u8),
    #[error("key derivation failed: {0}")]
    Kdf(String),
    #[error("sealing failed")]
    SealFailed,
    #[error("sealed update would be {0} bytes, over the server's size limit")]
    TooLarge(usize),
    #[error("malformed sealed update: {0}")]
    Envelope(#[from] EnvelopeError),
    #[error("data was sealed with key version {found}, but this device derived version {expected}")]
    KeyVersionMismatch { found: u8, expected: u8 },
    #[error(
        "the sync passphrase doesn't match this vault (or the data was tampered with or misrouted)"
    )]
    AuthenticationFailed,
}

/// The symmetric key that seals and opens a vault's updates. Held in memory only
/// (never written to disk, so changing the passphrase can't leave a stale key
/// behind) and wiped on drop.
pub struct VaultKey {
    key: [u8; KEY_LEN],
    key_version: u8,
}

impl std::fmt::Debug for VaultKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VaultKey")
            .field("key_version", &self.key_version)
            .field("key", &"<redacted>")
            .finish()
    }
}

impl Drop for VaultKey {
    fn drop(&mut self) {
        self.key.zeroize();
    }
}

/// Derives a vault's key from the user's passphrase and the vault's public salt.
///
/// Deliberately slow (~hundreds of ms, 64 MiB): call it once per vault per session
/// and keep the [`VaultKey`], not once per update.
pub fn derive_vault_key(
    passphrase: &str,
    salt: &[u8],
    key_version: u8,
) -> Result<VaultKey, CryptoError> {
    if key_version != KEY_VERSION {
        return Err(CryptoError::UnsupportedKeyVersion(key_version));
    }
    let params = Params::new(KDF_MEMORY_KIB, KDF_PASSES, KDF_LANES, Some(KEY_LEN))
        .map_err(|err| CryptoError::Kdf(err.to_string()))?;
    derive_with_params(passphrase, salt, key_version, params)
}

fn derive_with_params(
    passphrase: &str,
    salt: &[u8],
    key_version: u8,
    params: Params,
) -> Result<VaultKey, CryptoError> {
    if passphrase.is_empty() {
        return Err(CryptoError::EmptyPassphrase);
    }
    if salt.len() != KDF_SALT_LEN {
        return Err(CryptoError::BadSalt(salt.len()));
    }

    // NFKC so the same passphrase typed on macOS (which tends to produce
    // decomposed forms, e.g. "a" + combining ring) and on Linux/Windows (composed
    // "å") derives the same key. Not trimmed: spaces may be part of a passphrase.
    let mut normalized: String = passphrase.nfkc().collect();

    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut master = [0u8; KEY_LEN];
    let result = argon2.hash_password_into(normalized.as_bytes(), salt, &mut master);
    normalized.zeroize();
    if let Err(err) = result {
        master.zeroize();
        return Err(CryptoError::Kdf(err.to_string()));
    }

    let key = blake3::derive_key(CONTEXT_UPDATE_KEY, &master);
    master.zeroize();
    Ok(VaultKey { key, key_version })
}

/// Shortest passphrase a new vault accepts, in characters (after NFKC).
pub const MIN_PASSPHRASE_CHARS: usize = 12;
/// Fewest distinct characters a new vault's passphrase must use.
const MIN_DISTINCT_CHARS: usize = 6;

/// Why `passphrase` is too weak to protect a new vault, or `None` if it will do.
///
/// Every sealed blob lets whoever holds the server's data test guesses offline, so the
/// passphrase is the whole of the protection; Argon2id only slows each guess down.
/// These are coarse floors, not a strength meter: long enough, not one repeated
/// pattern, not just a number. Only *creating* a vault enforces them — an existing
/// vault's passphrase can't be changed (there is no re-keying), so joining or syncing
/// one must keep working whatever it is.
pub fn passphrase_weakness(passphrase: &str) -> Option<&'static str> {
    let normalized: String = passphrase.nfkc().collect();
    let chars = normalized.chars().count();
    let distinct = normalized
        .chars()
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    if chars < MIN_PASSPHRASE_CHARS {
        Some("it is shorter than 12 characters")
    } else if distinct < MIN_DISTINCT_CHARS {
        Some("it repeats too few different characters")
    } else if normalized
        .chars()
        .all(|c| c.is_ascii_digit() || c.is_whitespace())
    {
        Some("it is only a number")
    } else {
        None
    }
}

/// A key derived with the cheapest legal Argon2 parameters, for other modules'
/// tests (the production cost would make every simulated device take a second).
#[cfg(test)]
pub(crate) fn cheap_test_key(passphrase: &str, salt: &[u8]) -> VaultKey {
    let params = Params::new(8, 1, 1, Some(KEY_LEN)).unwrap();
    derive_with_params(passphrase, salt, KEY_VERSION, params).unwrap()
}

impl VaultKey {
    pub fn key_version(&self) -> u8 {
        self.key_version
    }

    /// Seals one plaintext update for `(vault, doc)` into envelope bytes ready to
    /// push, under a fresh random nonce.
    pub fn seal(
        &self,
        vault: VaultId,
        doc: DocId,
        plaintext: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        let nonce: [u8; NONCE_LEN] = rand::random();
        self.seal_with_nonce(vault, doc, plaintext, nonce)
    }

    fn seal_with_nonce(
        &self,
        vault: VaultId,
        doc: DocId,
        plaintext: &[u8],
        nonce: [u8; NONCE_LEN],
    ) -> Result<Vec<u8>, CryptoError> {
        let sealed_len = HEADER_LEN + plaintext.len() + TAG_LEN;
        if sealed_len > MAX_BLOB_BYTES {
            return Err(CryptoError::TooLarge(sealed_len));
        }
        let aad = envelope::aad(vault, doc, FORMAT_VERSION, self.key_version);
        let ciphertext = XChaCha20Poly1305::new(&self.key.into())
            .encrypt(
                &XNonce::from(nonce),
                Payload {
                    msg: plaintext,
                    aad: &aad,
                },
            )
            .map_err(|_| CryptoError::SealFailed)?;
        Ok(Envelope::new(self.key_version, nonce, ciphertext).to_bytes())
    }

    /// Opens envelope bytes pulled from the server, expecting them to belong to
    /// `(vault, doc)`. Fails closed on a wrong key, tampering, or a blob the server
    /// moved from another document/vault.
    pub fn open(&self, vault: VaultId, doc: DocId, sealed: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let envelope = Envelope::parse(sealed)?;
        if envelope.key_version != self.key_version {
            return Err(CryptoError::KeyVersionMismatch {
                found: envelope.key_version,
                expected: self.key_version,
            });
        }
        let aad = envelope::aad(vault, doc, envelope.format_version, envelope.key_version);
        XChaCha20Poly1305::new(&self.key.into())
            .decrypt(
                &XNonce::from(envelope.nonce),
                Payload {
                    msg: &envelope.ciphertext,
                    aad: &aad,
                },
            )
            .map_err(|_| CryptoError::AuthenticationFailed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    const SALT: [u8; KDF_SALT_LEN] = [5u8; KDF_SALT_LEN];

    /// Production Argon2 parameters take ~hundreds of ms and 64 MiB, which
    /// would make every test here painfully slow in a debug build; this keeps
    /// the same algorithm with the minimum legal cost.
    fn cheap_params() -> Params {
        Params::new(8, 1, 1, Some(KEY_LEN)).unwrap()
    }

    fn key(passphrase: &str, salt: &[u8]) -> VaultKey {
        derive_with_params(passphrase, salt, KEY_VERSION, cheap_params()).unwrap()
    }

    fn vault(n: u128) -> VaultId {
        VaultId(Uuid::from_u128(n))
    }

    fn doc(n: u128) -> DocId {
        DocId(Uuid::from_u128(n))
    }

    #[test]
    fn weak_passphrases_are_named_and_reasonable_ones_pass() {
        for weak in [
            "pw",
            "hunter2",
            "elevenchars",
            "aaaaaaaaaaaaaaaa",
            "abababababababab",
            "123123123123123",
            "4815 1623 4200 1111",
        ] {
            assert!(passphrase_weakness(weak).is_some(), "{weak:?} passed");
        }
        for fine in [
            "correct horse battery staple",
            "Tr0ub4dor&3xyz",
            "blåbærsyltetøy er godt",
        ] {
            assert_eq!(passphrase_weakness(fine), None, "{fine:?}");
        }
        // Counted in characters, not bytes: twelve non-ASCII letters are enough.
        assert_eq!(passphrase_weakness("æøåäöüßéèêñç"), None);
    }

    #[test]
    fn seal_then_open_round_trips() {
        let k = key("correct horse", &SALT);
        let sealed = k.seal(vault(1), doc(10), b"hello manuscript").unwrap();
        assert_eq!(
            k.open(vault(1), doc(10), &sealed).unwrap(),
            b"hello manuscript"
        );
    }

    #[test]
    fn an_empty_update_seals_and_opens() {
        let k = key("pw", &SALT);
        let sealed = k.seal(vault(1), doc(10), b"").unwrap();
        assert_eq!(sealed.len(), HEADER_LEN + TAG_LEN);
        assert_eq!(k.open(vault(1), doc(10), &sealed).unwrap(), b"");
    }

    #[test]
    fn the_same_plaintext_never_seals_to_the_same_bytes() {
        let k = key("pw", &SALT);
        let a = k.seal(vault(1), doc(10), b"same").unwrap();
        let b = k.seal(vault(1), doc(10), b"same").unwrap();
        assert_ne!(a, b, "each update must use a fresh random nonce");
    }

    #[test]
    fn the_wrong_passphrase_fails_authentication() {
        let sealed = key("right", &SALT)
            .seal(vault(1), doc(10), b"secret")
            .unwrap();
        assert_eq!(
            key("wrong", &SALT).open(vault(1), doc(10), &sealed),
            Err(CryptoError::AuthenticationFailed)
        );
    }

    #[test]
    fn a_blob_moved_to_another_document_is_rejected() {
        let k = key("pw", &SALT);
        let sealed = k.seal(vault(1), doc(10), b"chapter one").unwrap();
        assert_eq!(
            k.open(vault(1), doc(11), &sealed),
            Err(CryptoError::AuthenticationFailed)
        );
    }

    #[test]
    fn a_blob_moved_to_another_vault_is_rejected() {
        let k = key("pw", &SALT);
        let sealed = k.seal(vault(1), doc(10), b"chapter one").unwrap();
        assert_eq!(
            k.open(vault(2), doc(10), &sealed),
            Err(CryptoError::AuthenticationFailed)
        );
    }

    #[test]
    fn flipping_any_ciphertext_or_nonce_byte_is_detected() {
        let k = key("pw", &SALT);
        let sealed = k.seal(vault(1), doc(10), b"tamper target").unwrap();
        for i in 2..sealed.len() {
            let mut bad = sealed.clone();
            bad[i] ^= 0x01;
            assert_eq!(
                k.open(vault(1), doc(10), &bad),
                Err(CryptoError::AuthenticationFailed),
                "byte {i} was not authenticated"
            );
        }
    }

    #[test]
    fn a_different_key_version_is_reported_distinctly() {
        let k = key("pw", &SALT);
        let mut sealed = k.seal(vault(1), doc(10), b"x").unwrap();
        sealed[1] = KEY_VERSION + 1;
        assert_eq!(
            k.open(vault(1), doc(10), &sealed),
            Err(CryptoError::KeyVersionMismatch {
                found: KEY_VERSION + 1,
                expected: KEY_VERSION
            })
        );
    }

    #[test]
    fn malformed_input_is_an_error_not_a_panic() {
        let k = key("pw", &SALT);
        assert!(matches!(
            k.open(vault(1), doc(10), &[1, 2, 3]),
            Err(CryptoError::Envelope(_))
        ));
    }

    #[test]
    fn oversized_updates_are_refused_before_sealing() {
        let k = key("pw", &SALT);
        let big = vec![0u8; MAX_BLOB_BYTES];
        assert!(matches!(
            k.seal(vault(1), doc(10), &big),
            Err(CryptoError::TooLarge(_))
        ));
    }

    #[test]
    fn derivation_is_deterministic_and_depends_on_passphrase_and_salt() {
        let sealed = key("pw", &SALT).seal(vault(1), doc(10), b"x").unwrap();
        assert!(key("pw", &SALT).open(vault(1), doc(10), &sealed).is_ok());
        assert!(key("pw2", &SALT).open(vault(1), doc(10), &sealed).is_err());
        assert!(
            key("pw", &[6u8; KDF_SALT_LEN])
                .open(vault(1), doc(10), &sealed)
                .is_err()
        );
    }

    #[test]
    fn composed_and_decomposed_passphrases_derive_the_same_key() {
        // "Blåbær" with precomposed å vs. "a" + U+030A combining ring (what a
        // macOS keyboard can produce): visually identical, different bytes.
        let composed = "Bl\u{e5}b\u{e6}r";
        let decomposed = "Bla\u{30a}b\u{e6}r";
        assert_ne!(composed, decomposed);
        let sealed = key(composed, &SALT).seal(vault(1), doc(10), b"x").unwrap();
        assert!(
            key(decomposed, &SALT)
                .open(vault(1), doc(10), &sealed)
                .is_ok()
        );
    }

    #[test]
    fn bad_inputs_to_the_kdf_are_rejected() {
        assert!(matches!(
            derive_with_params("", &SALT, KEY_VERSION, cheap_params()),
            Err(CryptoError::EmptyPassphrase)
        ));
        assert!(matches!(
            derive_with_params("pw", &[1, 2, 3], KEY_VERSION, cheap_params()),
            Err(CryptoError::BadSalt(3))
        ));
        assert!(matches!(
            derive_vault_key("pw", &SALT, KEY_VERSION + 1),
            Err(CryptoError::UnsupportedKeyVersion(_))
        ));
    }

    #[test]
    fn the_production_kdf_parameters_are_accepted_by_argon2() {
        // Params::new validates ranges without running the (slow) hash.
        Params::new(KDF_MEMORY_KIB, KDF_PASSES, KDF_LANES, Some(KEY_LEN)).unwrap();
    }

    /// Exercises the real 64 MiB / 3-pass derivation end to end. Ignored by
    /// default because it's slow in a debug build; run with
    /// `cargo test --release -- --ignored production_kdf` to see the real cost.
    #[test]
    #[ignore = "slow: runs the production Argon2id parameters"]
    fn production_kdf_derives_a_working_key() {
        let start = std::time::Instant::now();
        let k = derive_vault_key("a real passphrase", &SALT, KEY_VERSION).unwrap();
        eprintln!("production Argon2id derivation took {:?}", start.elapsed());
        let sealed = k.seal(vault(1), doc(10), b"x").unwrap();
        assert_eq!(k.open(vault(1), doc(10), &sealed).unwrap(), b"x");
    }

    #[test]
    fn debug_output_never_contains_key_material() {
        let k = key("pw", &SALT);
        let shown = format!("{k:?}");
        assert!(shown.contains("redacted"));
        assert!(!shown.contains(&format!("{:?}", k.key)));
    }
}
