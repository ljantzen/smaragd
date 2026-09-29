//! Wire types shared by Smaragd's sync client (`src/sync/`) and the sync server
//! (`crates/smaragd-sync-server`).
//!
//! The server is *blind*: it stores and relays opaque encrypted blobs and never
//! sees document content. Everything here is therefore about routing and framing
//! — ids, the encrypted-envelope layout, the pairing ticket, and the HTTP API's
//! request/response shapes. The cipher itself (Argon2id + XChaCha20-Poly1305)
//! lives in the client only; this crate just defines the byte layout and the AAD
//! both sides must agree on.
//!
//! # API overview (all paths under [`api::API_PREFIX`], i.e. `/v1`)
//!
//! | Method & path | Auth | Purpose |
//! |---|---|---|
//! | `GET /health` | none | liveness / "Test connection" |
//! | `POST /vaults` | admin token or open registration | create a vault; returns first device token |
//! | `GET /vaults/:id` | device | public vault record (KDF salt, key version) |
//! | `DELETE /vaults/:id` | admin token | delete the vault and all its data |
//! | `POST /vaults/:id/pairing-codes` | device | mint a single-use, short-lived pairing code |
//! | `POST /pairing/redeem` | pairing code | exchange a code for this device's own token |
//! | `GET /vaults/:id/devices` | device | list devices |
//! | `DELETE /vaults/:id/devices/:device_id` | device | revoke a device |
//! | `GET /vaults/:id/docs` | device | doc ids + latest sequence numbers |
//! | `POST /vaults/:id/docs/:doc_id/updates` | device | push one sealed envelope (raw bytes) |
//! | `GET /vaults/:id/docs/:doc_id/updates?since=N` | device | latest snapshot (if any) + updates after `N`, paged (`more`) |
//! | `PUT /vaults/:id/docs/:doc_id/snapshot` | device | client-driven compaction |
//!
//! Auth is `Authorization: Bearer <device token>` (tokens are hashed at rest and
//! scoped to one vault), except the two operator actions — creating a vault on a
//! closed server and deleting one — which take the server's admin token in the
//! [`api::ADMIN_TOKEN_HEADER`] header. Deleting is operator-only so a single leaked
//! device token can't wipe a vault; a device leaves by revoking itself.
//! Errors are [`api::ApiError`] JSON with an HTTP status.

pub mod api;
pub mod envelope;
pub mod ids;
pub mod ticket;

pub use ids::{DeviceId, DocId, VaultId};
