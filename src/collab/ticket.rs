//! The one-time connection code peers paste into Smaragd to pair up.
//!
//! Wraps iroh's own [`iroh::EndpointAddr`] (which already knows how to
//! serialize itself) together with a session secret that never touches
//! iroh's relay, keeping the app-level encryption keys (derived from that
//! secret together with the host's endpoint id — see
//! `src/collab/crypto.rs`) cryptographically independent of iroh's own
//! transport security.

use serde::{Deserialize, Serialize};

/// Everything one peer needs to reach and join the other's collaboration
/// session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CollabTicket {
    /// Format version, so this can evolve without breaking old pasted codes.
    pub version: u8,
    pub endpoint_addr: iroh::EndpointAddr,
    pub session_secret: [u8; 32],
}

const CURRENT_VERSION: u8 = 1;

#[derive(Debug, thiserror::Error)]
pub enum TicketError {
    #[error("connection code is not valid base58: {0}")]
    Base58(#[from] bs58::decode::Error),
    #[error("connection code is malformed: {0}")]
    Postcard(#[from] postcard::Error),
    #[error("connection code is from an unsupported format version {0}")]
    UnsupportedVersion(u8),
}

impl CollabTicket {
    pub fn new(endpoint_addr: iroh::EndpointAddr, session_secret: [u8; 32]) -> Self {
        Self {
            version: CURRENT_VERSION,
            endpoint_addr,
            session_secret,
        }
    }

    /// Encodes this ticket as a short, pasteable string: postcard for a
    /// compact binary form, then base58 (not base64/z-base-32) specifically
    /// because it excludes visually ambiguous characters (`0`/`O`, `I`/`l`),
    /// which is what actually matters for a string a human copies by hand.
    pub fn encode(&self) -> String {
        let bytes = postcard::to_stdvec(self).expect("CollabTicket always serializes");
        bs58::encode(bytes).into_string()
    }

    pub fn decode(code: &str) -> Result<Self, TicketError> {
        let bytes = bs58::decode(code).into_vec()?;
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
    use iroh::{EndpointAddr, EndpointId, SecretKey};

    fn synthetic_addr() -> EndpointAddr {
        let id: EndpointId = SecretKey::generate().public();
        EndpointAddr::new(id)
    }

    #[test]
    fn a_ticket_round_trips_through_encode_and_decode() {
        let secret = [7u8; 32];
        let ticket = CollabTicket::new(synthetic_addr(), secret);

        let code = ticket.encode();
        let decoded = CollabTicket::decode(&code).unwrap();

        assert_eq!(decoded, ticket);
    }

    #[test]
    fn the_encoded_code_contains_no_ambiguous_base58_characters() {
        let ticket = CollabTicket::new(synthetic_addr(), [42u8; 32]);
        let code = ticket.encode();
        for forbidden in ['0', 'O', 'I', 'l'] {
            assert!(
                !code.contains(forbidden),
                "code {code:?} unexpectedly contains {forbidden:?}"
            );
        }
    }

    #[test]
    fn decoding_garbage_fails_cleanly_instead_of_panicking() {
        assert!(CollabTicket::decode("not a valid code").is_err());
        assert!(CollabTicket::decode("").is_err());
    }

    #[test]
    fn decoding_a_future_format_version_is_rejected() {
        let mut ticket = CollabTicket::new(synthetic_addr(), [1u8; 32]);
        ticket.version = CURRENT_VERSION + 1;
        let bytes = postcard::to_stdvec(&ticket).unwrap();
        let code = bs58::encode(bytes).into_string();

        match CollabTicket::decode(&code) {
            Err(TicketError::UnsupportedVersion(v)) => assert_eq!(v, CURRENT_VERSION + 1),
            other => panic!("expected UnsupportedVersion, got {other:?}"),
        }
    }
}
