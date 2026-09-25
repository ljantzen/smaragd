//! Strongly typed ids, so a vault id can't be passed where a doc id is expected.
//!
//! This crate only parses and formats ids; generating fresh ones (`Uuid::new_v4`)
//! is left to the server (vaults/devices) and the client (docs), which already
//! have a randomness source on every target.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! id_type {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub Uuid);

        impl $name {
            pub const fn from_uuid(uuid: Uuid) -> Self {
                Self(uuid)
            }

            pub const fn as_uuid(&self) -> Uuid {
                self.0
            }

            pub const fn as_bytes(&self) -> &[u8; 16] {
                self.0.as_bytes()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.hyphenated().fmt(f)
            }
        }

        impl FromStr for $name {
            type Err = uuid::Error;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Uuid::parse_str(s).map(Self)
            }
        }
    };
}

id_type!(
    /// One synced project on the server. Public: appears in URLs and the AAD.
    VaultId
);
id_type!(
    /// One paired device of a vault (its access token is scoped to the vault).
    DeviceId
);
id_type!(
    /// One synced CRDT document: a markdown file, a directory, or one of the
    /// reserved singleton documents below. Stable across renames — paths live
    /// in the (encrypted) manifest document, never on the server.
    DocId
);

impl DocId {
    /// The manifest document: `doc_id -> { path, kind, deleted }`.
    pub const MANIFEST: DocId = DocId(Uuid::from_u128(1));
    /// The `ProjectMeta` (`project.json`) document.
    pub const PROJECT_META: DocId = DocId(Uuid::from_u128(2));
    /// The project root directory's id, used as the key for root-level
    /// entries (the `""` key in path-keyed `ProjectMeta` maps).
    pub const PROJECT_ROOT: DocId = DocId(Uuid::from_u128(3));

    /// Whether this is one of the reserved singleton ids rather than a
    /// per-file/per-directory document.
    pub fn is_reserved(&self) -> bool {
        *self == Self::MANIFEST || *self == Self::PROJECT_META || *self == Self::PROJECT_ROOT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip_through_their_string_form() {
        let id = VaultId(Uuid::from_u128(0xdead_beef));
        let parsed: VaultId = id.to_string().parse().unwrap();
        assert_eq!(parsed, id);
    }

    #[test]
    fn ids_serialize_as_plain_uuid_strings_in_json() {
        let id = DocId(Uuid::from_u128(42));
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, format!("\"{id}\""));
        assert_eq!(serde_json::from_str::<DocId>(&json).unwrap(), id);
    }

    #[test]
    fn reserved_ids_are_distinct_and_recognised() {
        let reserved = [DocId::MANIFEST, DocId::PROJECT_META, DocId::PROJECT_ROOT];
        for (i, a) in reserved.iter().enumerate() {
            assert!(a.is_reserved());
            for b in &reserved[i + 1..] {
                assert_ne!(a, b);
            }
        }
        assert!(!DocId(Uuid::from_u128(0xabcdef)).is_reserved());
    }

    #[test]
    fn garbage_fails_to_parse() {
        assert!("not-a-uuid".parse::<DeviceId>().is_err());
    }
}
