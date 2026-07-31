//! Peer-to-peer real-time collaborative editing.
//!
//! Design note: the CRDT (`crdt`) and its text diffing (`diff`) both run on
//! the main/UI thread rather than on the background networking thread,
//! because the diff step needs direct access to the live `egui` text buffer
//! anyway. Keeping apply/encrypt there too means neither the CRDT document
//! nor cipher state ever needs to be `Send`, and keeps this module's logic
//! fully synchronous and unit-testable without a real network. The
//! networking layer (added in a later phase) only ever moves already-
//! encoded/encrypted bytes between here and the peer.
//!
//! Phase A (current): the CRDT engine (`crdt`) and its diffing (`diff`),
//! proven convergent against in-process documents only — no networking, no
//! encryption, no UI integration yet.

pub mod crdt;
pub mod diff;
