//! Secrets: device tokens, pairing codes and how they're stored.
//!
//! Nothing secret is stored in the clear: the database holds only SHA-256 hashes
//! of device tokens and pairing codes. That is enough because both are high-entropy
//! random values (256 and ~59 bits respectively), so — unlike a human password —
//! they don't need a slow hash to resist guessing offline.

use sha2::{Digest, Sha256};
use smaragd_sync_protocol::api::{PAIRING_CODE_ALPHABET, PAIRING_CODE_LEN};

fn to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// Lower-case hex SHA-256 of a secret, the form stored in the database.
pub fn hash_secret(secret: &str) -> String {
    to_hex(&Sha256::digest(secret.as_bytes()))
}

/// A fresh 256-bit device token. The `sst_` prefix makes it recognizable in logs
/// and secret scanners; it carries no meaning.
pub fn new_device_token() -> String {
    let bytes: [u8; 32] = rand::random();
    format!("sst_{}", to_hex(&bytes))
}

/// A fresh pairing code, displayed as `XXXX-XXXX-XXXX`. Characters are drawn by
/// rejection sampling so every symbol is exactly equally likely.
pub fn new_pairing_code() -> String {
    let limit = 256 - (256 % PAIRING_CODE_ALPHABET.len());
    let mut code = String::with_capacity(PAIRING_CODE_LEN + 2);
    let mut produced = 0;
    while produced < PAIRING_CODE_LEN {
        let byte: u8 = rand::random();
        if usize::from(byte) >= limit {
            continue;
        }
        if produced > 0 && produced % 4 == 0 {
            code.push('-');
        }
        code.push(PAIRING_CODE_ALPHABET[usize::from(byte) % PAIRING_CODE_ALPHABET.len()] as char);
        produced += 1;
    }
    code
}

/// Length-independent-of-content comparison, for the admin token.
pub fn constant_time_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use smaragd_sync_protocol::api::normalize_pairing_code;

    #[test]
    fn hashes_are_stable_hex_and_differ_per_secret() {
        let a = hash_secret("a");
        assert_eq!(a, hash_secret("a"));
        assert_ne!(a, hash_secret("b"));
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn tokens_are_long_unique_and_prefixed() {
        let (a, b) = (new_device_token(), new_device_token());
        assert_ne!(a, b);
        assert!(a.starts_with("sst_"));
        assert_eq!(a.len(), 4 + 64);
    }

    #[test]
    fn pairing_codes_use_only_unambiguous_characters_in_groups_of_four() {
        for _ in 0..200 {
            let code = new_pairing_code();
            let groups: Vec<&str> = code.split('-').collect();
            assert_eq!(groups.len(), 3, "{code}");
            assert!(groups.iter().all(|g| g.len() == 4), "{code}");
            assert_eq!(normalize_pairing_code(&code).len(), PAIRING_CODE_LEN);
            assert!(
                normalize_pairing_code(&code)
                    .bytes()
                    .all(|b| PAIRING_CODE_ALPHABET.contains(&b)),
                "{code}"
            );
        }
    }

    #[test]
    fn constant_time_eq_compares_content_and_length() {
        assert!(constant_time_eq("secret", "secret"));
        assert!(!constant_time_eq("secret", "secreT"));
        assert!(!constant_time_eq("secret", "secret2"));
    }
}
