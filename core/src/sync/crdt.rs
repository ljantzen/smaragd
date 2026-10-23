//! The CRDT document behind one synced markdown file.
//!
//! A file is *not* one big text CRDT. A character-level merge of two concurrent
//! edits to the same `status:` line would splice them into invalid YAML
//! (`status: draftdone`), so a file is split at its frontmatter block
//! (`frontmatter::split_block`) into:
//!
//! - **`body`** — a `YText` of everything after the block (character-level merge,
//!   exactly like `collab::crdt::CrdtDoc`);
//! - **`fm`** — a `YMap` from each top-level frontmatter key to that value's
//!   canonical YAML text, one last-writer-wins register per key, so concurrent
//!   edits to *different* keys both survive and the same key resolves cleanly;
//! - **`fmraw`** — one register holding the raw block text the keys came from.
//!
//! `fmraw` exists so an unchanged block is never reformatted: `serde_norway` can't
//! keep comments or key order, so when the merged keys still match what the raw
//! block parses to, [`FileDoc::render`] emits the raw block byte for byte. Only
//! when a merge produced keys that no single raw block describes (two devices
//! changed different keys) is the block regenerated from the merged keys.
//!
//! Frontmatter that isn't a valid YAML mapping is not split at all: the whole file
//! is treated as body text.
//!
//! Pure and synchronous, like `collab::crdt`.

use std::collections::BTreeMap;

use yrs::error::Error;
use yrs::updates::decoder::Decode;
use yrs::{
    Any, Doc, GetString, Map, MapRef, Out, ReadTxn, StateVector, Text, TextRef, Transact, Update,
};

use crate::collab::diff;
use crate::frontmatter;

const RAW_KEY: &str = "raw";

/// A file's contents pulled apart the way [`FileDoc`] stores them.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedFile {
    raw: Option<String>,
    keys: BTreeMap<String, String>,
    body: String,
}

fn parse_keys(yaml: &str) -> Option<BTreeMap<String, String>> {
    if yaml.trim().is_empty() {
        return Some(BTreeMap::new());
    }
    let mapping: serde_norway::Mapping = serde_norway::from_str(yaml).ok()?;
    let mut keys = BTreeMap::new();
    for (key, value) in mapping {
        keys.insert(
            key.as_str()?.to_string(),
            serde_norway::to_string(&value).ok()?,
        );
    }
    Some(keys)
}

fn parse_file(contents: &str) -> ParsedFile {
    if let Some((block, yaml, body)) = frontmatter::split_block(contents)
        && let Some(keys) = parse_keys(yaml)
    {
        return ParsedFile {
            raw: Some(block.to_string()),
            keys,
            body: body.to_string(),
        };
    }
    ParsedFile {
        raw: None,
        keys: BTreeMap::new(),
        body: contents.to_string(),
    }
}

fn render_file(keys: &BTreeMap<String, String>, raw: Option<&str>, body: &str) -> String {
    if keys.is_empty() && raw.is_none() {
        return body.to_string();
    }
    if let Some(raw) = raw
        && let Some((_, yaml, _)) = frontmatter::split_block(raw)
        && parse_keys(yaml).as_ref() == Some(keys)
    {
        return format!("{raw}{body}");
    }
    if keys.is_empty() {
        return body.to_string();
    }
    let mut mapping = serde_norway::Mapping::new();
    for (key, value) in keys {
        let value = serde_norway::from_str(value).unwrap_or(serde_norway::Value::Null);
        mapping.insert(serde_norway::Value::String(key.clone()), value);
    }
    let yaml = serde_norway::to_string(&mapping).unwrap_or_default();
    format!("---\n{yaml}---\n{body}")
}

fn any_string(out: Out) -> Option<String> {
    match out {
        Out::Any(Any::String(s)) => Some(s.to_string()),
        _ => None,
    }
}

/// One synced markdown file as a CRDT. See the module docs for its shape.
pub struct FileDoc {
    doc: Doc,
    body: TextRef,
    fm: MapRef,
    fm_raw: MapRef,
}

impl FileDoc {
    pub fn new() -> Self {
        let doc = Doc::new();
        let body = doc.get_or_insert_text("body");
        let fm = doc.get_or_insert_map("fm");
        let fm_raw = doc.get_or_insert_map("fmraw");
        Self {
            doc,
            body,
            fm,
            fm_raw,
        }
    }

    /// Rebuilds a document from a full-state update ([`Self::encode_state`]).
    pub fn from_state(state: &[u8]) -> Result<Self, Error> {
        let mut doc = Self::new();
        doc.apply_update(state)?;
        Ok(doc)
    }

    /// The complete state, for persisting locally and for snapshots.
    pub fn encode_state(&self) -> Vec<u8> {
        self.encode_state_since(&StateVector::default())
    }

    /// Everything a peer that has seen `since` is missing (deleted content included
    /// only as compact tombstones). See `engine::repack_pending`.
    pub fn encode_state_since(&self, since: &StateVector) -> Vec<u8> {
        self.doc.transact().encode_state_as_update_v1(since)
    }

    pub fn state_vector(&self) -> StateVector {
        self.doc.transact().state_vector()
    }

    /// Applies an update (from a peer, already decrypted). Idempotent.
    pub fn apply_update(&mut self, update: &[u8]) -> Result<(), Error> {
        let update = Update::decode_v1(update)?;
        self.doc.transact_mut().apply_update(update)?;
        Ok(())
    }

    fn read(&self) -> ParsedFile {
        let txn = self.doc.transact();
        let keys = self
            .fm
            .iter(&txn)
            .filter_map(|(key, value)| Some((key.to_string(), any_string(value)?)))
            .collect();
        ParsedFile {
            raw: self.fm_raw.get(&txn, RAW_KEY).and_then(any_string),
            keys,
            body: self.body.get_string(&txn),
        }
    }

    /// The whole file as it should exist on disk.
    pub fn render(&self) -> String {
        let parsed = self.read();
        render_file(&parsed.keys, parsed.raw.as_deref(), &parsed.body)
    }

    /// Whether this document holds nothing at all (a brand-new, never-seeded doc).
    pub fn is_empty(&self) -> bool {
        let parsed = self.read();
        parsed.body.is_empty() && parsed.keys.is_empty() && parsed.raw.is_none()
    }

    /// Makes this document's content equal `full_text` (a file as read from disk),
    /// returning the encoded update describing the change, or `None` if it already
    /// matched. Body changes become one contiguous text edit (like an editor
    /// keystroke); frontmatter changes become per-key register writes.
    pub fn set_text(&mut self, full_text: &str) -> Option<Vec<u8>> {
        let target = parse_file(full_text);
        let current = self.read();
        let body_change = diff::diff(&current.body, &target.body);
        if body_change.is_none() && current.keys == target.keys && current.raw == target.raw {
            return None;
        }

        let mut txn = self.doc.transact_mut();
        if let Some(change) = body_change {
            if change.deleted_len > 0 {
                self.body
                    .remove_range(&mut txn, change.pos as u32, change.deleted_len as u32);
            }
            if !change.inserted.is_empty() {
                self.body
                    .insert(&mut txn, change.pos as u32, &change.inserted);
            }
        }
        for (key, value) in &target.keys {
            if current.keys.get(key) != Some(value) {
                self.fm.insert(&mut txn, key.as_str(), value.as_str());
            }
        }
        for key in current.keys.keys() {
            if !target.keys.contains_key(key) {
                self.fm.remove(&mut txn, key);
            }
        }
        if current.raw != target.raw {
            match &target.raw {
                Some(raw) => {
                    self.fm_raw.insert(&mut txn, RAW_KEY, raw.as_str());
                }
                None => {
                    self.fm_raw.remove(&mut txn, RAW_KEY);
                }
            }
        }
        Some(txn.encode_update_v1())
    }
}

impl Default for FileDoc {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc_with(text: &str) -> FileDoc {
        let mut doc = FileDoc::new();
        doc.set_text(text);
        doc
    }

    /// A second device that starts from `doc`'s current state.
    fn fork(doc: &FileDoc) -> FileDoc {
        FileDoc::from_state(&doc.encode_state()).unwrap()
    }

    #[test]
    fn a_file_without_frontmatter_round_trips() {
        let doc = doc_with("Just a body.\n\nSecond paragraph.\n");
        assert_eq!(doc.render(), "Just a body.\n\nSecond paragraph.\n");
    }

    #[test]
    fn an_unchanged_frontmatter_block_is_reproduced_byte_for_byte() {
        // Comment, unusual quoting and key order that serde_norway would destroy.
        let text = "---\n# my note\nstatus:   'draft'\npov: Anna\n---\nBody text\n";
        let doc = doc_with(text);
        assert_eq!(doc.render(), text);
    }

    #[test]
    fn setting_identical_text_is_a_no_op() {
        let text = "---\nstatus: draft\n---\nBody\n";
        let mut doc = doc_with(text);
        assert!(doc.set_text(text).is_none());
    }

    #[test]
    fn state_round_trips() {
        let text = "---\nstatus: draft\n---\nBæ ø å\n";
        let doc = doc_with(text);
        assert_eq!(fork(&doc).render(), text);
    }

    #[test]
    fn concurrent_body_edits_both_survive() {
        let base = doc_with("Line one\n\nLine two\n");
        let mut a = fork(&base);
        let mut b = fork(&base);

        let ua = a.set_text("Line one, edited\n\nLine two\n").unwrap();
        let ub = b.set_text("Line one\n\nLine two, edited\n").unwrap();
        a.apply_update(&ub).unwrap();
        b.apply_update(&ua).unwrap();

        assert_eq!(a.render(), b.render());
        assert_eq!(a.render(), "Line one, edited\n\nLine two, edited\n");
    }

    #[test]
    fn concurrent_edits_to_different_frontmatter_keys_merge_into_valid_yaml() {
        let base = doc_with("---\nstatus: draft\npov: Anna\n---\nBody\n");
        let mut a = fork(&base);
        let mut b = fork(&base);

        let ua = a
            .set_text("---\nstatus: final\npov: Anna\n---\nBody\n")
            .unwrap();
        let ub = b
            .set_text("---\nstatus: draft\npov: Bo\n---\nBody\n")
            .unwrap();
        a.apply_update(&ub).unwrap();
        b.apply_update(&ua).unwrap();

        assert_eq!(a.render(), b.render());
        let merged = a.render();
        let (_, yaml, body) = frontmatter::split_block(&merged).unwrap();
        let keys = parse_keys(yaml).expect("merged frontmatter must be valid YAML");
        assert_eq!(keys["status"].trim(), "final");
        assert_eq!(keys["pov"].trim(), "Bo");
        assert_eq!(body, "Body\n");
    }

    #[test]
    fn concurrent_edits_to_the_same_frontmatter_key_converge_on_one_value() {
        let base = doc_with("---\nstatus: draft\n---\nBody\n");
        let mut a = fork(&base);
        let mut b = fork(&base);

        let ua = a.set_text("---\nstatus: final\n---\nBody\n").unwrap();
        let ub = b.set_text("---\nstatus: revised\n---\nBody\n").unwrap();
        a.apply_update(&ub).unwrap();
        b.apply_update(&ua).unwrap();

        assert_eq!(a.render(), b.render());
        let merged = a.render();
        assert!(
            merged.contains("status: final") || merged.contains("status: revised"),
            "{merged}"
        );
        assert!(!merged.contains("finalrevised") && !merged.contains("revisedfinal"));
    }

    #[test]
    fn a_frontmatter_edit_and_a_body_edit_merge() {
        let base = doc_with("---\nstatus: draft\n---\nOnce upon a time\n");
        let mut a = fork(&base);
        let mut b = fork(&base);

        let ua = a
            .set_text("---\nstatus: final\n---\nOnce upon a time\n")
            .unwrap();
        let ub = b
            .set_text("---\nstatus: draft\n---\nOnce upon a time, far away\n")
            .unwrap();
        a.apply_update(&ub).unwrap();
        b.apply_update(&ua).unwrap();

        assert_eq!(a.render(), b.render());
        assert_eq!(
            a.render(),
            "---\nstatus: final\n---\nOnce upon a time, far away\n"
        );
    }

    #[test]
    fn removing_the_frontmatter_block_removes_it_everywhere() {
        let base = doc_with("---\nstatus: draft\n---\nBody\n");
        let mut a = fork(&base);
        let mut b = fork(&base);
        let u = a.set_text("Body\n").unwrap();
        b.apply_update(&u).unwrap();
        assert_eq!(a.render(), "Body\n");
        assert_eq!(b.render(), "Body\n");
    }

    #[test]
    fn invalid_frontmatter_falls_back_to_plain_text() {
        let text = "---\nstatus: [unclosed\n---\nBody\n";
        let doc = doc_with(text);
        assert_eq!(doc.render(), text);
    }

    #[test]
    fn unicode_bodies_edit_on_char_boundaries() {
        let mut doc = doc_with("Blåbærsyltetøy\n");
        let update = doc.set_text("Blåbærsyltetøy og fløte\n").unwrap();
        let mut other = FileDoc::new();
        other.apply_update(&doc.encode_state()).unwrap();
        other.apply_update(&update).unwrap();
        assert_eq!(other.render(), "Blåbærsyltetøy og fløte\n");
    }

    #[test]
    fn a_new_document_is_empty_until_seeded() {
        let mut doc = FileDoc::new();
        assert!(doc.is_empty());
        doc.set_text("x");
        assert!(!doc.is_empty());
    }

    #[test]
    fn applying_the_same_update_twice_is_harmless() {
        let base = doc_with("abc\n");
        let mut a = fork(&base);
        let mut b = fork(&base);
        let u = a.set_text("abcd\n").unwrap();
        b.apply_update(&u).unwrap();
        b.apply_update(&u).unwrap();
        assert_eq!(b.render(), "abcd\n");
    }
}
