//! The app-level end-to-end encryption layered on top of iroh's own
//! transport security (see the module doc on `src/collab/mod.rs`).
//!
//! Even though iroh's QUIC connections are already encrypted point-to-point
//! (and its relay can't decrypt what it relays), this layer keeps the
//! manuscript content unreadable to iroh's infrastructure specifically —
//! matching CryptPad's own zero-knowledge model — by keying it from a
//! secret that lives only in the connection code, never in iroh's own node
//! identity or its relay.
//!
//! Pure and synchronous: no networking, no CRDT — just bytes in, bytes out.

use chacha20poly1305::aead::Aead;
use chacha20poly1305::{KeyInit, XChaCha20Poly1305, XNonce};

/// A derived symmetric key, ready to encrypt or decrypt messages for one
/// collaboration session.
pub type SessionKey = [u8; 32];

/// Domain-separation context for [`blake3::derive_key`]. Fixed and versioned
/// per `blake3`'s own guidance, so a future change to this scheme can't
/// accidentally collide keys with this one.
const KEY_DERIVATION_CONTEXT: &str = "smaragd 2026-07-31 collab session key v1";

const NONCE_LEN: usize = 24;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DecryptError {
    #[error("message is too short to contain a nonce")]
    TooShort,
    #[error("decryption failed (wrong key, or the message was tampered with)")]
    AuthenticationFailed,
}

/// Derives this session's symmetric key from the random secret embedded in
/// the connection code. Deliberately independent of iroh's own node
/// keypair — only the two peers who exchanged the connection code (and
/// nothing that ever touched iroh's relay) can derive this key.
pub fn derive_key(session_secret: &[u8; 32]) -> SessionKey {
    blake3::derive_key(KEY_DERIVATION_CONTEXT, session_secret)
}

/// Encrypts `plaintext`, returning a message with a fresh random nonce
/// prepended. `XChaCha20Poly1305`'s 192-bit nonce is large enough that a
/// fresh random one per message is safe for a session's entire lifetime —
/// no sequence-number bookkeeping needed to avoid nonce reuse.
pub fn encrypt(key: &SessionKey, plaintext: &[u8]) -> Vec<u8> {
    let cipher = XChaCha20Poly1305::new(&(*key).into());
    let nonce_bytes: [u8; NONCE_LEN] = rand::random();
    let nonce = XNonce::from(nonce_bytes);

    let mut out = Vec::with_capacity(NONCE_LEN + plaintext.len() + 16);
    out.extend_from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(&nonce, plaintext)
        .expect("in-memory AEAD encryption cannot fail");
    out.extend_from_slice(&ciphertext);
    out
}

/// Decrypts a message produced by [`encrypt`] with the same key. Fails if
/// the key is wrong or the message was altered in transit — the two are
/// indistinguishable by design, an AEAD tag mismatch is all either produces.
pub fn decrypt(key: &SessionKey, message: &[u8]) -> Result<Vec<u8>, DecryptError> {
    if message.len() < NONCE_LEN {
        return Err(DecryptError::TooShort);
    }
    let (nonce_bytes, ciphertext) = message.split_at(NONCE_LEN);
    let nonce = XNonce::try_from(nonce_bytes).expect("checked length above");
    let cipher = XChaCha20Poly1305::new(&(*key).into());
    cipher
        .decrypt(&nonce, ciphertext)
        .map_err(|_| DecryptError::AuthenticationFailed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_round_trips_through_encrypt_and_decrypt() {
        let key = derive_key(&[1u8; 32]);
        let plaintext = b"the manuscript's opening line";

        let encrypted = encrypt(&key, plaintext);
        let decrypted = decrypt(&key, &encrypted).unwrap();

        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn two_encryptions_of_the_same_plaintext_use_different_nonces() {
        let key = derive_key(&[2u8; 32]);
        let a = encrypt(&key, b"hello");
        let b = encrypt(&key, b"hello");
        assert_ne!(a, b, "each encryption must use a fresh random nonce");
    }

    #[test]
    fn decrypting_with_the_wrong_key_fails() {
        let key_a = derive_key(&[3u8; 32]);
        let key_b = derive_key(&[4u8; 32]);

        let encrypted = encrypt(&key_a, b"secret manuscript text");
        let result = decrypt(&key_b, &encrypted);

        assert_eq!(result, Err(DecryptError::AuthenticationFailed));
    }

    #[test]
    fn tampering_with_the_ciphertext_is_detected() {
        let key = derive_key(&[5u8; 32]);
        let mut encrypted = encrypt(&key, b"secret manuscript text");
        let last = encrypted.len() - 1;
        encrypted[last] ^= 0x01;

        assert_eq!(
            decrypt(&key, &encrypted),
            Err(DecryptError::AuthenticationFailed)
        );
    }

    #[test]
    fn tampering_with_the_nonce_is_also_detected() {
        let key = derive_key(&[6u8; 32]);
        let mut encrypted = encrypt(&key, b"secret manuscript text");
        encrypted[0] ^= 0x01;

        assert_eq!(
            decrypt(&key, &encrypted),
            Err(DecryptError::AuthenticationFailed)
        );
    }

    #[test]
    fn a_too_short_message_is_rejected_without_panicking() {
        let key = derive_key(&[7u8; 32]);
        assert_eq!(decrypt(&key, b"short"), Err(DecryptError::TooShort));
        assert_eq!(decrypt(&key, b""), Err(DecryptError::TooShort));
    }

    #[test]
    fn deriving_from_different_secrets_gives_different_keys() {
        let key_a = derive_key(&[8u8; 32]);
        let key_b = derive_key(&[9u8; 32]);
        assert_ne!(key_a, key_b);
    }

    #[test]
    fn deriving_from_the_same_secret_is_deterministic() {
        let secret = [10u8; 32];
        assert_eq!(derive_key(&secret), derive_key(&secret));
    }
}
