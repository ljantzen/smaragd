//! The manifest: which synced documents exist, where they live, and whether
//! they've been deleted. One CRDT document per vault (`DocId::MANIFEST`).
//!
//! CRDT file documents are addressed by a stable [`DocId`], never a path, so a
//! rename is just an edit to one entry's `path` and never disturbs the file's own
//! edit history. Each entry is a nested `YMap`, so a rename on one device and a
//! delete on another (or two renames of different entries) both survive a merge.
//!
//! **Deletes are tombstones** (`deleted = true`), never removals: with a removal, a
//! device that deleted a file while another concurrently edited it would have no way
//! to tell "deleted" from "not synced yet". This deliberately mirrors
//! `ProjectMeta::trashed_origins`, the codebase's existing soft-delete concept.
//!
//! **Paths are untrusted input.** Only other paired devices can write here (the
//! server can't read or forge entries), but a compromised device shouldn't be able
//! to make another one write outside its project, so every path must pass
//! [`is_safe_relative_path`] before the engine touches the filesystem with it.

use std::collections::BTreeMap;

use smaragd_sync_protocol::DocId;
use yrs::error::Error;
use yrs::updates::decoder::Decode;
use yrs::{Any, Doc, Map, MapPrelim, MapRef, Out, ReadTxn, StateVector, Transact, Update};

/// What a manifest entry stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EntryKind {
    /// A markdown file, with a CRDT document of its own.
    #[default]
    Doc,
    /// A directory. It has no content, but it needs a stable id so path-keyed
    /// project metadata (folder roles, ordering, ...) survives renames.
    Dir,
}

impl EntryKind {
    fn as_str(self) -> &'static str {
        match self {
            EntryKind::Doc => "doc",
            EntryKind::Dir => "dir",
        }
    }
}

/// One synced document or directory as the manifest describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestEntry {
    pub doc_id: DocId,
    pub kind: EntryKind,
    /// `/`-separated path relative to the project root (the same convention
    /// `ProjectMeta`'s path keys use, so it's portable across host OSes).
    pub path: String,
    pub deleted: bool,
}

fn safe_components(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.contains('\0')
        // No empty, `.`/`..` or hidden components (`.git`, `.smaragd`, ...): hidden
        // entries are never synced (the folder scan skips them), so a peer naming
        // one is either confused or hostile.
        && path
            .split('/')
            .all(|part| !part.is_empty() && !part.starts_with('.') && !part.contains(':'))
}

/// Whether `path` is a plain relative `.md` path that stays inside the project:
/// no absolute paths, drive prefixes, backslashes, NULs, or empty/`.`/`..`/hidden
/// components.
pub fn is_safe_relative_path(path: &str) -> bool {
    path.ends_with(".md") && safe_components(path)
}

/// Like [`is_safe_relative_path`], for a directory (no `.md` requirement).
pub fn is_safe_relative_dir_path(path: &str) -> bool {
    safe_components(path)
}

pub struct ManifestDoc {
    doc: Doc,
    entries: MapRef,
}

impl ManifestDoc {
    pub fn new() -> Self {
        let doc = Doc::new();
        let entries = doc.get_or_insert_map("entries");
        Self { doc, entries }
    }

    pub fn from_state(state: &[u8]) -> Result<Self, Error> {
        let mut manifest = Self::new();
        manifest.apply_update(state)?;
        Ok(manifest)
    }

    pub fn encode_state(&self) -> Vec<u8> {
        self.doc
            .transact()
            .encode_state_as_update_v1(&StateVector::default())
    }

    pub fn apply_update(&mut self, update: &[u8]) -> Result<(), Error> {
        let update = Update::decode_v1(update)?;
        self.doc.transact_mut().apply_update(update)?;
        Ok(())
    }

    /// Every entry (tombstones included), in stable `DocId` order. Entries whose key
    /// isn't a valid id or that lack a `path` are skipped rather than trusted.
    pub fn entries(&self) -> Vec<ManifestEntry> {
        let txn = self.doc.transact();
        let mut out = BTreeMap::new();
        for (key, value) in self.entries.iter(&txn) {
            let (Ok(doc_id), Out::YMap(fields)) = (key.parse::<DocId>(), value) else {
                continue;
            };
            let Some(Out::Any(Any::String(path))) = fields.get(&txn, "path") else {
                continue;
            };
            let deleted = matches!(fields.get(&txn, "deleted"), Some(Out::Any(Any::Bool(true))));
            let kind = match fields.get(&txn, "kind") {
                Some(Out::Any(Any::String(kind))) if &*kind == "dir" => EntryKind::Dir,
                _ => EntryKind::Doc,
            };
            out.insert(
                doc_id,
                ManifestEntry {
                    doc_id,
                    kind,
                    path: path.to_string(),
                    deleted,
                },
            );
        }
        out.into_values().collect()
    }

    pub fn get(&self, id: DocId) -> Option<ManifestEntry> {
        self.entries().into_iter().find(|entry| entry.doc_id == id)
    }

    /// Registers a new live entry at `path`, returning the update to push.
    pub fn add(&mut self, id: DocId, path: &str, kind: EntryKind) -> Vec<u8> {
        let mut txn = self.doc.transact_mut();
        let fields = self
            .entries
            .insert(&mut txn, id.to_string(), MapPrelim::default());
        fields.insert(&mut txn, "path", path);
        fields.insert(&mut txn, "kind", kind.as_str());
        fields.insert(&mut txn, "deleted", false);
        txn.encode_update_v1()
    }

    /// Moves a document to `path`. `None` if it's unknown or already there.
    pub fn rename(&mut self, id: DocId, path: &str) -> Option<Vec<u8>> {
        if self.get(id)?.path == path {
            return None;
        }
        let mut txn = self.doc.transact_mut();
        let Some(Out::YMap(fields)) = self.entries.get(&txn, &id.to_string()) else {
            return None;
        };
        fields.insert(&mut txn, "path", path);
        Some(txn.encode_update_v1())
    }

    /// Tombstones (or un-tombstones) a document. `None` if unknown or unchanged.
    pub fn set_deleted(&mut self, id: DocId, deleted: bool) -> Option<Vec<u8>> {
        if self.get(id)?.deleted == deleted {
            return None;
        }
        let mut txn = self.doc.transact_mut();
        let Some(Out::YMap(fields)) = self.entries.get(&txn, &id.to_string()) else {
            return None;
        };
        fields.insert(&mut txn, "deleted", deleted);
        Some(txn.encode_update_v1())
    }
}

impl Default for ManifestDoc {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn id(n: u128) -> DocId {
        DocId(Uuid::from_u128(n + 1000))
    }

    fn fork(manifest: &ManifestDoc) -> ManifestDoc {
        ManifestDoc::from_state(&manifest.encode_state()).unwrap()
    }

    #[test]
    fn added_entries_are_listed_in_id_order() {
        let mut m = ManifestDoc::new();
        m.add(id(2), "b.md", EntryKind::Doc);
        m.add(id(1), "a.md", EntryKind::Doc);
        let entries = m.entries();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].path, "a.md");
        assert_eq!(entries[1].path, "b.md");
        assert!(entries.iter().all(|e| !e.deleted));
    }

    #[test]
    fn rename_and_delete_update_the_entry_and_report_no_ops() {
        let mut m = ManifestDoc::new();
        m.add(id(1), "a.md", EntryKind::Doc);
        assert!(m.rename(id(1), "b.md").is_some());
        assert!(m.rename(id(1), "b.md").is_none(), "unchanged");
        assert!(m.rename(id(9), "x.md").is_none(), "unknown");
        assert_eq!(m.get(id(1)).unwrap().path, "b.md");

        assert!(m.set_deleted(id(1), true).is_some());
        assert!(m.set_deleted(id(1), true).is_none());
        assert!(m.get(id(1)).unwrap().deleted);
        assert!(m.set_deleted(id(1), false).is_some());
        assert!(!m.get(id(1)).unwrap().deleted);
    }

    #[test]
    fn a_rename_and_a_concurrent_delete_of_one_entry_both_survive() {
        let mut base = ManifestDoc::new();
        base.add(id(1), "a.md", EntryKind::Doc);
        let mut a = fork(&base);
        let mut b = fork(&base);

        let ua = a.rename(id(1), "renamed.md").unwrap();
        let ub = b.set_deleted(id(1), true).unwrap();
        a.apply_update(&ub).unwrap();
        b.apply_update(&ua).unwrap();

        assert_eq!(a.entries(), b.entries());
        let merged = a.get(id(1)).unwrap();
        assert_eq!(merged.path, "renamed.md");
        assert!(merged.deleted);
    }

    #[test]
    fn concurrent_adds_of_different_documents_union() {
        let mut a = ManifestDoc::new();
        let mut b = ManifestDoc::new();
        let ua = a.add(id(1), "a.md", EntryKind::Doc);
        let ub = b.add(id(2), "b.md", EntryKind::Doc);
        a.apply_update(&ub).unwrap();
        b.apply_update(&ua).unwrap();
        assert_eq!(a.entries(), b.entries());
        assert_eq!(a.entries().len(), 2);
    }

    #[test]
    fn concurrent_renames_of_different_entries_both_apply() {
        let mut base = ManifestDoc::new();
        base.add(id(1), "a.md", EntryKind::Doc);
        base.add(id(2), "b.md", EntryKind::Doc);
        let mut x = fork(&base);
        let mut y = fork(&base);
        let ux = x.rename(id(1), "a2.md").unwrap();
        let uy = y.rename(id(2), "b2.md").unwrap();
        x.apply_update(&uy).unwrap();
        y.apply_update(&ux).unwrap();
        assert_eq!(x.entries(), y.entries());
        assert_eq!(x.get(id(1)).unwrap().path, "a2.md");
        assert_eq!(x.get(id(2)).unwrap().path, "b2.md");
    }

    #[test]
    fn state_round_trips() {
        let mut m = ManifestDoc::new();
        m.add(id(1), "Chapters/One.md", EntryKind::Doc);
        m.set_deleted(id(1), true);
        assert_eq!(fork(&m).entries(), m.entries());
    }

    #[test]
    fn directories_are_kept_apart_from_documents() {
        let mut m = ManifestDoc::new();
        m.add(id(1), "Notes", EntryKind::Dir);
        m.add(id(2), "Notes/a.md", EntryKind::Doc);
        let by_id: Vec<_> = m.entries().into_iter().map(|e| (e.path, e.kind)).collect();
        assert_eq!(
            by_id,
            vec![
                ("Notes".to_string(), EntryKind::Dir),
                ("Notes/a.md".to_string(), EntryKind::Doc)
            ]
        );
        assert_eq!(fork(&m).entries(), m.entries());
    }

    #[test]
    fn unsafe_directory_paths_are_rejected() {
        for bad in [
            "",
            "/abs",
            "../x",
            "a//b",
            "a/../b",
            ".git",
            "a/.hidden",
            "C:/x",
            "a\\b",
            "dir/",
        ] {
            assert!(
                !is_safe_relative_dir_path(bad),
                "{bad:?} should be rejected"
            );
        }
        for good in ["Notes", "World/Places", "Kapittel 1"] {
            assert!(
                is_safe_relative_dir_path(good),
                "{good:?} should be accepted"
            );
        }
    }

    #[test]
    fn unsafe_paths_are_rejected() {
        for bad in [
            "",
            "/etc/passwd.md",
            "../escape.md",
            "a/../../escape.md",
            "a//b.md",
            "./a.md",
            "a\\b.md",
            "C:/x.md",
            "notmarkdown.txt",
            ".md",
            ".hidden.md",
            ".git/hooks/x.md",
            "dir/.md",
            "nul\0.md",
            "dir/",
        ] {
            assert!(!is_safe_relative_path(bad), "{bad:?} should be rejected");
        }
        for good in ["a.md", "Chapters/One.md", "World/Places/Oslo & Bergen.md"] {
            assert!(is_safe_relative_path(good), "{good:?} should be accepted");
        }
    }
}
