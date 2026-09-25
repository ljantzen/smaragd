//! Background project sync through a self-hosted, end-to-end-encrypted sync
//! server (see `crates/smaragd-sync-server` and the manual's "Sync" chapter).
//!
//! Unlike `collab` (live, peer-to-peer, one document, no server), sync keeps a
//! whole project identical across a user's own devices via a *blind* server that
//! only stores and relays opaque encrypted blobs. All merging happens here, on the
//! clients, after decryption — every synced file is a CRDT document.
//!
//! This module and its children are pure and synchronous, and deliberately free of
//! native-only dependencies so the same core compiles for the browser build; only
//! the eventual HTTP transport is native-only.
//!
//! - [`crypto`] — passphrase → key derivation (Argon2id) and sealing/opening the
//!   per-update envelope (XChaCha20-Poly1305, random nonce, AAD-bound to the
//!   vault and document).

pub mod crypto;
