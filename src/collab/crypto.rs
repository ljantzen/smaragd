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
//! Because this layer's stated job is surviving compromised infrastructure,
//! it can't lean on the transport for replay or ordering guarantees either:
//!
//! - **Directional keys.** Each direction of a session gets its own key
//!   ([`derive_directional_key`]), so a frame can never be reflected back at
//!   its own sender and accepted.
//! - **Implicit counter nonces.** Frames ride an ordered, reliable QUIC
//!   stream, so both ends can count them: the nonce is a per-direction
//!   counter that never goes on the wire ([`SealCipher`]/[`OpenCipher`]).
//!   A replayed, reordered, or dropped frame decrypts against the wrong
//!   counter and fails authentication — fail closed, no bookkeeping to send.
//! - **Host identity in the key.** The host's iroh endpoint id is folded
//!   into derivation, binding a connection code's keys to the one endpoint
//!   it was minted for.
//!
//! Pure and synchronous: no networking, no CRDT — just bytes in, bytes out.

use chacha20poly1305::aead::Aead;
use chacha20poly1305::{KeyInit, XChaCha20Poly1305, XNonce};

/// A derived symmetric key for one direction of one collaboration session.
pub type SessionKey = [u8; 32];

/// Domain-separation contexts for [`blake3::derive_key`]. Fixed and versioned
/// per `blake3`'s own guidance, so a future change to this scheme can't
/// accidentally collide keys with this one. One per direction — that's what
/// makes the two directions' keys independent.
const CONTEXT_HOST_TO_JOINER: &str = "smaragd 2026-07-31 collab host->joiner frame key v1";
const CONTEXT_JOINER_TO_HOST: &str = "smaragd 2026-07-31 collab joiner->host frame key v1";

const NONCE_LEN: usize = 24;
/// XChaCha20Poly1305's authentication tag — the minimum size of any sealed
/// frame (an empty plaintext seals to just the tag).
const TAG_LEN: usize = 16;

/// Which way a frame travels. Host and joiner sides are not symmetric here:
/// each side seals with its own direction's key and opens with the other's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    HostToJoiner,
    JoinerToHost,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DecryptError {
    #[error("message is too short to contain an authentication tag")]
    TooShort,
    #[error(
        "decryption failed (wrong key, a replayed or out-of-order frame, or the message was tampered with)"
    )]
    AuthenticationFailed,
}

/// Derives one direction's symmetric key from the random secret embedded in
/// the connection code and the host's iroh endpoint id (the id the ticket
/// itself names — both sides know it before connecting). Deliberately
/// independent of iroh's transport security: only the two peers who
/// exchanged the connection code (and nothing that ever touched iroh's
/// relay) can derive these keys.
pub fn derive_directional_key(
    session_secret: &[u8; 32],
    host_endpoint_id: &[u8; 32],
    direction: Direction,
) -> SessionKey {
    let context = match direction {
        Direction::HostToJoiner => CONTEXT_HOST_TO_JOINER,
        Direction::JoinerToHost => CONTEXT_JOINER_TO_HOST,
    };
    let mut material = [0u8; 64];
    material[..32].copy_from_slice(session_secret);
    material[32..].copy_from_slice(host_endpoint_id);
    blake3::derive_key(context, &material)
}

/// The counter-derived nonce for the `counter`-th frame in one direction.
/// Counters start at 0 and only ever count up, and each direction has its
/// own key, so no (key, nonce) pair is ever reused.
fn nonce_for(counter: u64) -> XNonce {
    let mut bytes = [0u8; NONCE_LEN];
    bytes[..8].copy_from_slice(&counter.to_le_bytes());
    XNonce::from(bytes)
}

/// Seals outgoing frames for one direction of one session. Stateful: each
/// call consumes the next counter nonce, so a sealer must never be shared
/// across directions or reused across connections.
pub struct SealCipher {
    cipher: XChaCha20Poly1305,
    counter: u64,
}

impl SealCipher {
    pub fn new(key: &SessionKey) -> Self {
        Self {
            cipher: XChaCha20Poly1305::new(&(*key).into()),
            counter: 0,
        }
    }

    /// Encrypts `plaintext` as the next frame in this direction. The nonce
    /// is implicit (the frame counter), so the output is just
    /// ciphertext-plus-tag — nothing about the sequencing goes on the wire.
    pub fn seal(&mut self, plaintext: &[u8]) -> Vec<u8> {
        let nonce = nonce_for(self.counter);
        self.counter = self
            .counter
            .checked_add(1)
            .expect("2^64 frames in one session is unreachable");
        self.cipher
            .encrypt(&nonce, plaintext)
            .expect("in-memory AEAD encryption cannot fail")
    }
}

/// Opens incoming frames for one direction of one session. Stateful mirror
/// of [`SealCipher`]: each successful open advances the expected counter, so
/// a replayed, reordered, or dropped frame fails authentication instead of
/// being accepted. A failed open does not advance the counter — but every
/// caller treats a failure as fatal to the session anyway.
pub struct OpenCipher {
    cipher: XChaCha20Poly1305,
    counter: u64,
}

impl OpenCipher {
    pub fn new(key: &SessionKey) -> Self {
        Self {
            cipher: XChaCha20Poly1305::new(&(*key).into()),
            counter: 0,
        }
    }

    /// Decrypts the next frame in this direction. Fails if the key is wrong,
    /// the frame is not the exact next one in sequence, or the message was
    /// altered in transit — all indistinguishable by design, an AEAD tag
    /// mismatch is all any of them produces.
    pub fn open(&mut self, message: &[u8]) -> Result<Vec<u8>, DecryptError> {
        if message.len() < TAG_LEN {
            return Err(DecryptError::TooShort);
        }
        let nonce = nonce_for(self.counter);
        let plaintext = self
            .cipher
            .decrypt(&nonce, message)
            .map_err(|_| DecryptError::AuthenticationFailed)?;
        self.counter = self
            .counter
            .checked_add(1)
            .expect("2^64 frames in one session is unreachable");
        Ok(plaintext)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: [u8; 32] = [1u8; 32];
    const HOST_ID: [u8; 32] = [2u8; 32];

    fn pair(direction: Direction) -> (SealCipher, OpenCipher) {
        let key = derive_directional_key(&SECRET, &HOST_ID, direction);
        (SealCipher::new(&key), OpenCipher::new(&key))
    }

    #[test]
    fn a_message_round_trips_through_seal_and_open() {
        let (mut sealer, mut opener) = pair(Direction::HostToJoiner);
        let plaintext = b"the manuscript's opening line";

        let sealed = sealer.seal(plaintext);
        let opened = opener.open(&sealed).unwrap();

        assert_eq!(opened, plaintext);
    }

    #[test]
    fn a_stream_of_messages_round_trips_in_order() {
        let (mut sealer, mut opener) = pair(Direction::JoinerToHost);
        for message in [&b"one"[..], b"", b"three", b"four"] {
            let sealed = sealer.seal(message);
            assert_eq!(opener.open(&sealed).unwrap(), message);
        }
    }

    #[test]
    fn sealing_the_same_plaintext_twice_gives_different_ciphertexts() {
        let (mut sealer, _) = pair(Direction::HostToJoiner);
        let a = sealer.seal(b"hello");
        let b = sealer.seal(b"hello");
        assert_ne!(a, b, "each frame must consume a fresh counter nonce");
    }

    #[test]
    fn a_replayed_frame_is_rejected() {
        let (mut sealer, mut opener) = pair(Direction::HostToJoiner);
        let sealed = sealer.seal(b"delete everything");

        assert!(opener.open(&sealed).is_ok());
        assert_eq!(
            opener.open(&sealed),
            Err(DecryptError::AuthenticationFailed),
            "the same frame must never be accepted twice"
        );
    }

    #[test]
    fn a_dropped_or_reordered_frame_is_rejected() {
        let (mut sealer, mut opener) = pair(Direction::HostToJoiner);
        let first = sealer.seal(b"first");
        let second = sealer.seal(b"second");

        // Deliver the second frame without the first: counter mismatch.
        assert_eq!(
            opener.open(&second),
            Err(DecryptError::AuthenticationFailed)
        );
        // A failed open must not advance the counter — the genuinely-next
        // frame still opens.
        assert_eq!(opener.open(&first).unwrap(), b"first");
    }

    #[test]
    fn the_two_directions_use_independent_keys() {
        let (mut host_sealer, _) = pair(Direction::HostToJoiner);
        let (_, mut host_side_opener) = pair(Direction::JoinerToHost);

        // A frame sealed host->joiner reflected back at the host (which
        // opens with the joiner->host key) must not be accepted.
        let sealed = host_sealer.seal(b"reflected frame");
        assert_eq!(
            host_side_opener.open(&sealed),
            Err(DecryptError::AuthenticationFailed)
        );
    }

    #[test]
    fn opening_with_a_key_from_the_wrong_secret_fails() {
        let (mut sealer, _) = pair(Direction::HostToJoiner);
        let wrong_key = derive_directional_key(&[9u8; 32], &HOST_ID, Direction::HostToJoiner);
        let mut opener = OpenCipher::new(&wrong_key);

        let sealed = sealer.seal(b"secret manuscript text");
        assert_eq!(
            opener.open(&sealed),
            Err(DecryptError::AuthenticationFailed)
        );
    }

    #[test]
    fn the_host_endpoint_id_is_bound_into_the_keys() {
        let a = derive_directional_key(&SECRET, &HOST_ID, Direction::HostToJoiner);
        let b = derive_directional_key(&SECRET, &[3u8; 32], Direction::HostToJoiner);
        assert_ne!(
            a, b,
            "the same secret must derive different keys for different hosts"
        );
    }

    #[test]
    fn tampering_with_the_ciphertext_is_detected() {
        let (mut sealer, mut opener) = pair(Direction::HostToJoiner);
        let mut sealed = sealer.seal(b"secret manuscript text");
        let last = sealed.len() - 1;
        sealed[last] ^= 0x01;

        assert_eq!(
            opener.open(&sealed),
            Err(DecryptError::AuthenticationFailed)
        );
    }

    #[test]
    fn a_too_short_message_is_rejected_without_panicking() {
        let (_, mut opener) = pair(Direction::HostToJoiner);
        assert_eq!(opener.open(b"short"), Err(DecryptError::TooShort));
        assert_eq!(opener.open(b""), Err(DecryptError::TooShort));
    }

    #[test]
    fn key_derivation_is_deterministic() {
        assert_eq!(
            derive_directional_key(&SECRET, &HOST_ID, Direction::HostToJoiner),
            derive_directional_key(&SECRET, &HOST_ID, Direction::HostToJoiner)
        );
    }
}
