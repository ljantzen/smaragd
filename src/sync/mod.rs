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
//! - `client` (native only) — the blocking `ureq` implementation of the transport, plus
//!   the control-plane calls (create a vault, pair and manage devices).
//! - [`crdt`] — the CRDT document behind one synced markdown file (frontmatter keys
//!   as per-key registers, body as text).
//! - [`crypto`] — passphrase → key derivation (Argon2id) and sealing/opening the
//!   per-update envelope (XChaCha20-Poly1305, random nonce, AAD-bound to the
//!   vault and document).
//! - [`engine`] — one reconcile pass between a project folder and a vault: capture
//!   local edits, pull and merge remote ones, apply renames/deletes, push.
//! - [`transport`] — the engine's blocking view of the server, so it's testable
//!   against an in-memory fake with several simulated devices.
//! - [`state`] — where the engine keeps its local CRDT state between runs.
//! - [`manifest`] — which documents exist, where they live, and whether they were
//!   deleted (tombstones), itself a CRDT so renames and deletes merge.

#[cfg(not(target_arch = "wasm32"))]
pub mod client;
pub mod crdt;
pub mod crypto;
pub mod engine;
pub mod manifest;
pub mod state;
pub mod transport;

#[cfg(test)]
mod fake;
