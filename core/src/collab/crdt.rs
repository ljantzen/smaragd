//! Thin wrapper over `yrs` (the Rust port of Yjs) giving the rest of the
//! `collab` module a plain "one text document" API, hiding `yrs`'s general
//! shared-types machinery behind the handful of operations this feature
//! actually needs.
//!
//! Deliberately synchronous and free of egui/networking concerns: this lives
//! entirely on the main thread (see the module doc in `src/collab/mod.rs` for
//! why), which is what makes it straightforward to unit-test in isolation.

use yrs::error::Error;
use yrs::updates::decoder::Decode;
use yrs::updates::encoder::Encode;
use yrs::{Doc, GetString, ReadTxn, StateVector, Text, TextRef, Transact, Update};

use crate::collab::diff::TextChange;

/// A single collaboratively-edited document. Wraps one `yrs::Doc` with one
/// `TextRef` named `"body"` — this codebase edits one document at a time
/// (see `EditorState`), so there is no need for `yrs`'s richer multi-shared-
/// type document model.
pub struct CrdtDoc {
    doc: Doc,
    text: TextRef,
}

impl CrdtDoc {
    /// Creates a new, empty collaborative document.
    pub fn new() -> Self {
        let doc = Doc::new();
        let text = doc.get_or_insert_text("body");
        Self { doc, text }
    }

    /// Seeds a new collaborative document with `initial_text` — used when
    /// hosting a session against an already-open, non-empty buffer.
    pub fn with_initial_text(initial_text: &str) -> Self {
        let crdt = Self::new();
        if !initial_text.is_empty() {
            let mut txn = crdt.doc.transact_mut();
            crdt.text.insert(&mut txn, 0, initial_text);
        }
        crdt
    }

    /// The document's current text content.
    pub fn current_text(&self) -> String {
        self.text.get_string(&self.doc.transact())
    }

    /// Applies a local edit (as computed by [`crate::collab::diff::diff`]
    /// against the live egui buffer) to this document, returning the
    /// encoded update to ship to the peer.
    ///
    /// Byte offsets, not char offsets: a fresh `yrs::Doc` defaults to
    /// `OffsetKind::Bytes`, matching this codebase's own byte-offset
    /// convention (see `autocomplete::char_offset_to_byte`), so `TextChange`
    /// offsets can be passed straight through with no conversion.
    pub fn apply_local_change(&mut self, change: &TextChange) -> Vec<u8> {
        let mut txn = self.doc.transact_mut();
        if change.deleted_len > 0 {
            self.text
                .remove_range(&mut txn, change.pos as u32, change.deleted_len as u32);
        }
        if !change.inserted.is_empty() {
            self.text
                .insert(&mut txn, change.pos as u32, &change.inserted);
        }
        txn.encode_update_v1()
    }

    /// Applies an update received from a peer (already decrypted by the
    /// caller) and returns the document's resulting text, for the caller to
    /// diff against the live egui buffer and reposition the cursor.
    pub fn apply_remote_update(&mut self, update: &[u8]) -> Result<String, Error> {
        let update = Update::decode_v1(update)?;
        let mut txn = self.doc.transact_mut();
        txn.apply_update(update)?;
        drop(txn);
        Ok(self.current_text())
    }

    /// This document's state vector — sent to a peer so it can compute (via
    /// [`Self::diff_since`]) exactly what it's missing.
    pub fn state_vector(&self) -> Vec<u8> {
        self.doc.transact().state_vector().encode_v1()
    }

    /// Encodes everything this document has that `remote_state_vector`
    /// (produced by a peer's [`Self::state_vector`]) doesn't yet — used to
    /// bring a newly joined peer's document up to date in one update,
    /// rather than replaying the whole edit history.
    pub fn diff_since(&self, remote_state_vector: &[u8]) -> Result<Vec<u8>, Error> {
        let sv = StateVector::decode_v1(remote_state_vector)?;
        Ok(self.doc.transact().encode_diff_v1(&sv))
    }
}

impl Default for CrdtDoc {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collab::diff;

    /// Simulates typing `text` into an initially-empty peer one character
    /// (well, one contiguous edit) at a time, returning the update bytes
    /// produced for each step — standing in for what `sync_local_collab_edit`
    /// will do against the live egui buffer each frame.
    fn type_out(doc: &mut CrdtDoc, text: &str) -> Vec<Vec<u8>> {
        let mut updates = Vec::new();
        let mut buffer = String::new();
        for ch in text.chars() {
            let next = format!("{buffer}{ch}");
            if let Some(change) = diff::diff(&buffer, &next) {
                updates.push(doc.apply_local_change(&change));
            }
            buffer = next;
        }
        updates
    }

    #[test]
    fn a_fresh_doc_is_empty() {
        let doc = CrdtDoc::new();
        assert_eq!(doc.current_text(), "");
    }

    #[test]
    fn local_edits_are_reflected_immediately() {
        let mut doc = CrdtDoc::new();
        let change = diff::diff("", "hello").unwrap();
        doc.apply_local_change(&change);
        assert_eq!(doc.current_text(), "hello");
    }

    #[test]
    fn a_remote_update_converges_two_docs() {
        let mut host = CrdtDoc::new();
        let mut joiner = CrdtDoc::new();

        let updates = type_out(&mut host, "hello world");
        for update in updates {
            joiner.apply_remote_update(&update).unwrap();
        }

        assert_eq!(host.current_text(), "hello world");
        assert_eq!(joiner.current_text(), "hello world");
    }

    #[test]
    fn concurrent_edits_on_both_sides_converge() {
        // Both peers start from the same synced baseline...
        let mut a = CrdtDoc::new();
        let baseline = diff::diff("", "hello world").unwrap();
        let seed_update = a.apply_local_change(&baseline);
        let mut b = CrdtDoc::new();
        b.apply_remote_update(&seed_update).unwrap();
        assert_eq!(a.current_text(), b.current_text());

        // ...then each edits independently, before either has seen the
        // other's change (the concurrent-edit case CRDTs exist for).
        let a_change = diff::diff("hello world", "hello there world").unwrap();
        let a_update = a.apply_local_change(&a_change);

        let b_change = diff::diff("hello world", "hello world!").unwrap();
        let b_update = b.apply_local_change(&b_change);

        // Apply in one order on `a`...
        a.apply_remote_update(&b_update).unwrap();
        // ...and the other order on `b`.
        b.apply_remote_update(&a_update).unwrap();

        assert_eq!(a.current_text(), b.current_text());
        assert_eq!(a.current_text(), "hello there world!");
    }

    #[test]
    fn out_of_order_update_application_still_converges() {
        let mut a = CrdtDoc::new();
        let updates = type_out(&mut a, "abc");

        // Apply the same three updates to two fresh peers in different orders.
        let mut forward = CrdtDoc::new();
        for update in &updates {
            forward.apply_remote_update(update).unwrap();
        }

        let mut reversed = CrdtDoc::new();
        for update in updates.iter().rev() {
            reversed.apply_remote_update(update).unwrap();
        }

        assert_eq!(forward.current_text(), "abc");
        assert_eq!(reversed.current_text(), "abc");
        assert_eq!(forward.current_text(), reversed.current_text());
    }

    #[test]
    fn state_vector_diff_brings_a_new_joiner_up_to_date_in_one_update() {
        let mut host = CrdtDoc::with_initial_text("existing manuscript text");
        // Make a further local edit so the host's history has more than one block.
        let change = diff::diff("existing manuscript text", "existing manuscript text!").unwrap();
        host.apply_local_change(&change);

        let joiner = CrdtDoc::new();
        let joiner_sv = joiner.state_vector();
        let catch_up = host.diff_since(&joiner_sv).unwrap();

        let mut joiner = joiner;
        let text = joiner.apply_remote_update(&catch_up).unwrap();
        assert_eq!(text, "existing manuscript text!");
        assert_eq!(joiner.current_text(), host.current_text());
    }

    #[test]
    fn deleting_and_retyping_converges() {
        let mut a = CrdtDoc::with_initial_text("hello world");
        let mut b = CrdtDoc::new();
        b.apply_remote_update(&a.diff_since(&b.state_vector()).unwrap())
            .unwrap();

        let del = diff::diff("hello world", "hello").unwrap();
        let a_update = a.apply_local_change(&del);
        b.apply_remote_update(&a_update).unwrap();

        assert_eq!(a.current_text(), "hello");
        assert_eq!(b.current_text(), "hello");
    }
}
