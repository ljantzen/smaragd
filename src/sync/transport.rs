//! The engine's view of the sync server: a small, blocking, data-plane-only trait.
//!
//! The engine never talks HTTP itself. Keeping it behind this trait means (a) the
//! whole engine is testable against an in-memory fake server with several
//! simulated devices, (b) the native `ureq` client and a future browser (`fetch`)
//! client are interchangeable, and (c) vault creation, pairing and device
//! management — the *control* plane, which the engine doesn't need — stay out of
//! it. A transport instance is already bound to one vault and one device token.
//!
//! Everything crossing this boundary is opaque: `sealed` blobs are envelopes from
//! `sync::crypto`, and the server never sees inside them.

use smaragd_sync_protocol::DocId;
use smaragd_sync_protocol::api::{ListDocsResponse, PullUpdatesResponse, Snapshot};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TransportError {
    /// The server couldn't be reached (network down, DNS, timeout...). Retryable;
    /// local edits keep queuing meanwhile.
    #[error("sync server unreachable: {0}")]
    Offline(String),
    /// The device token was rejected (revoked, or the vault was deleted).
    #[error("sync server rejected this device's credentials")]
    Unauthorized,
    /// The server refused the request (e.g. over the size limit).
    #[error("sync server rejected the request: {0}")]
    Rejected(String),
    /// The server failed unexpectedly.
    #[error("sync server error: {0}")]
    Server(String),
}

/// `Send` because the real client runs on a background thread.
pub trait SyncTransport: Send {
    /// Every document the server holds updates for, with its latest sequence number,
    /// and the server's advertised file-size limit.
    fn list_docs(&self) -> Result<ListDocsResponse, TransportError>;

    /// The latest snapshot (if the document was compacted past `since`) plus the
    /// updates with `seq > since` — one page of them; `more` says to ask again.
    fn pull(&self, doc: DocId, since: u64) -> Result<PullUpdatesResponse, TransportError>;

    /// Stores one sealed update; returns the sequence number the server assigned.
    fn push(&self, doc: DocId, sealed: &[u8]) -> Result<u64, TransportError>;

    /// Stores a compaction snapshot and lets the server drop updates it covers.
    fn put_snapshot(&self, doc: DocId, snapshot: &Snapshot) -> Result<(), TransportError>;
}
