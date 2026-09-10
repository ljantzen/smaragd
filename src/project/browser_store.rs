//! An in-memory [`ProjectStore`] for the web build — lets "New Project" work
//! at all in a browser, where there's no real filesystem to point
//! [`super::store::NativeStore`] at.
//!
//! Bridges the sync-vs-async mismatch the wasm feasibility plan's Phase 3
//! flagged (`ProjectStore`'s methods are synchronous; every browser storage
//! API is Promise-based) with an eager-load/write-behind design rather than
//! making the trait itself async: every mutating method updates this
//! in-memory map synchronously (so the trait's callers, all of Phase 1's
//! work, need no changes at all), then — on wasm32 only, see the `idb`
//! submodule — fires off a background task that clears and rewrites the
//! *entire* IndexedDB object store from a fresh snapshot of the map.
//! Deliberately a full resync rather than mirroring each specific mutation
//! (a targeted put/delete per operation) — much simpler and more robust
//! against subtle incremental-sync bugs, and fine for what this is: a
//! "browser-sized" single project, not a large repository. The tradeoff is
//! genuine eventual consistency, not a durability guarantee — a crash or
//! closed tab before a pending write-behind task finishes can lose the last
//! few edits, same class of risk as any write-behind cache.
//!
//! Directory *walking* is much simpler here than [`super::store::NativeStore`]'s
//! `ignore`-crate-based one: a fresh browser project has no `.gitignore` to
//! respect (nothing was ever imported from a real, possibly-ignored-file-laden
//! folder), so `list_tree` just walks this store's own two maps directly.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use super::store::{ProjectStore, TreeEntryKind};

#[cfg(target_arch = "wasm32")]
mod idb;

#[derive(Debug, Default)]
pub struct BrowserStore {
    /// Guarded by a `Mutex` (rather than a plain `RefCell`) purely to satisfy
    /// `ProjectStore: Send + Sync` — see that trait's own doc comment on why
    /// a `Project`'s store needs to cross a thread boundary on native. wasm32
    /// has no real threads in this build, so contention is a non-issue here;
    /// this is a formality, not a real concurrency guarantee.
    entries: Mutex<HashMap<PathBuf, Entry>>,
}

#[derive(Debug, Clone)]
enum Entry {
    Dir,
    File(Vec<u8>),
}

impl BrowserStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Loads whatever this store persisted last (see `idb`'s module doc) —
    /// an empty store if nothing was ever saved, IndexedDB isn't available,
    /// or anything about the read fails; best-effort, matching this whole
    /// module's "never fails outright" philosophy elsewhere. On any target
    /// other than wasm32 there's nothing to load from, so this is just
    /// `Self::new()` — kept as an `async fn` on every target so callers
    /// don't need their own `#[cfg]` to call it.
    #[cfg(target_arch = "wasm32")]
    pub async fn load_persisted() -> Self {
        let entries = idb::load_all().await.into_iter().collect();
        Self {
            entries: Mutex::new(entries),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn load_persisted() -> Self {
        Self::new()
    }

    /// Snapshot the current map and, on wasm32, fire off a background task
    /// to persist it — see this module's own doc comment for why this is a
    /// full resync rather than a targeted per-mutation write, and what that
    /// trades away. A no-op on every other target.
    #[cfg(target_arch = "wasm32")]
    fn persist(&self) {
        let snapshot: Vec<(PathBuf, Entry)> = self
            .entries
            .lock()
            .expect("not poisoned")
            .iter()
            .map(|(path, entry)| (path.clone(), entry.clone()))
            .collect();
        idb::spawn_save(snapshot);
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn persist(&self) {}

    /// Mark `path` and every ancestor as directories — mirrors
    /// `std::fs::create_dir_all`'s "create every missing intermediate
    /// component" behavior. Stops once an ancestor is already known (rather
    /// than always walking to the filesystem root) purely as a cheap
    /// short-circuit; re-marking an already-`Dir` ancestor would be harmless.
    fn mark_dir_and_ancestors(entries: &mut HashMap<PathBuf, Entry>, path: &Path) {
        let mut current = Some(path.to_path_buf());
        while let Some(dir) = current {
            if matches!(entries.get(&dir), Some(Entry::Dir)) {
                break;
            }
            entries.insert(dir.clone(), Entry::Dir);
            current = dir.parent().map(Path::to_path_buf);
        }
    }

    fn not_found(path: &Path) -> io::Error {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("{} not found in browser storage", path.display()),
        )
    }
}

impl ProjectStore for BrowserStore {
    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        let entries = self.entries.lock().expect("not poisoned");
        match entries.get(path) {
            Some(Entry::File(bytes)) => String::from_utf8(bytes.clone())
                .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err)),
            Some(Entry::Dir) => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{} is a directory", path.display()),
            )),
            None => Err(Self::not_found(path)),
        }
    }

    fn write(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
        {
            let mut entries = self.entries.lock().expect("not poisoned");
            // A write implicitly ensures its parent exists, same as every real
            // filesystem's "the containing folder must already be there" — but
            // callers here always `create_dir_all` a new folder before writing
            // into it (see `Project::create_folder`), so this is defense in
            // depth, not the primary way directories come to exist.
            if let Some(parent) = path.parent() {
                entries.entry(parent.to_path_buf()).or_insert(Entry::Dir);
            }
            entries.insert(path.to_path_buf(), Entry::File(contents.to_vec()));
        }
        self.persist();
        Ok(())
    }

    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        {
            let mut entries = self.entries.lock().expect("not poisoned");
            Self::mark_dir_and_ancestors(&mut entries, path);
        }
        self.persist();
        Ok(())
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        {
            let mut entries = self.entries.lock().expect("not poisoned");
            let Some(entry) = entries.remove(from) else {
                return Err(Self::not_found(from));
            };
            let is_dir = matches!(entry, Entry::Dir);
            entries.insert(to.to_path_buf(), entry);
            if is_dir {
                // Move every descendant along with it — `from` may have children
                // keyed by their own full paths, which don't update just because
                // their ancestor's own map entry moved.
                let descendants: Vec<PathBuf> = entries
                    .keys()
                    .filter(|path| path.starts_with(from) && path.as_path() != from)
                    .cloned()
                    .collect();
                for old_path in descendants {
                    if let Some(entry) = entries.remove(&old_path)
                        && let Ok(suffix) = old_path.strip_prefix(from)
                    {
                        entries.insert(to.join(suffix), entry);
                    }
                }
            }
        }
        self.persist();
        Ok(())
    }

    fn remove_dir_all(&self, path: &Path) -> io::Result<()> {
        {
            let mut entries = self.entries.lock().expect("not poisoned");
            entries.retain(|entry_path, _| entry_path != path && !entry_path.starts_with(path));
        }
        self.persist();
        Ok(())
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        let result = {
            let mut entries = self.entries.lock().expect("not poisoned");
            match entries.remove(path) {
                Some(Entry::File(_)) => Ok(()),
                Some(dir @ Entry::Dir) => {
                    // Put it back — this is the wrong removal method for a
                    // directory, same as `std::fs::remove_file` refusing one.
                    entries.insert(path.to_path_buf(), dir);
                    Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!("{} is a directory", path.display()),
                    ))
                }
                None => Err(Self::not_found(path)),
            }
        };
        if result.is_ok() {
            self.persist();
        }
        result
    }

    fn read_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>> {
        let entries = self.entries.lock().expect("not poisoned");
        Ok(entries
            .keys()
            .filter(|entry_path| entry_path.parent() == Some(path))
            .cloned()
            .collect())
    }

    fn exists(&self, path: &Path) -> bool {
        self.entries
            .lock()
            .expect("not poisoned")
            .contains_key(path)
    }

    fn is_dir(&self, path: &Path) -> bool {
        matches!(
            self.entries.lock().expect("not poisoned").get(path),
            Some(Entry::Dir)
        )
    }

    fn list_tree(&self, root: &Path) -> Vec<(PathBuf, TreeEntryKind)> {
        let entries = self.entries.lock().expect("not poisoned");
        entries
            .iter()
            .filter(|(path, _)| path.as_path() != root && path.starts_with(root))
            .filter_map(|(path, entry)| match entry {
                Entry::Dir => Some((path.clone(), TreeEntryKind::Dir)),
                Entry::File(_) if path.extension().and_then(|ext| ext.to_str()) == Some("md") => {
                    Some((path.clone(), TreeEntryKind::Doc))
                }
                Entry::File(_) => None,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_then_read_round_trips() {
        let store = BrowserStore::new();
        store.write(Path::new("/p/notes.md"), b"hello").unwrap();
        assert_eq!(
            store.read_to_string(Path::new("/p/notes.md")).unwrap(),
            "hello"
        );
    }

    #[test]
    fn read_missing_file_is_not_found() {
        let store = BrowserStore::new();
        assert!(store.read_to_string(Path::new("/p/missing.md")).is_err());
    }

    #[test]
    fn create_dir_all_marks_every_ancestor() {
        let store = BrowserStore::new();
        store
            .create_dir_all(Path::new("/p/Chapter 1/Scenes"))
            .unwrap();
        let tree = store.list_tree(Path::new("/p"));
        assert!(tree.contains(&(PathBuf::from("/p/Chapter 1"), TreeEntryKind::Dir)));
        assert!(tree.contains(&(PathBuf::from("/p/Chapter 1/Scenes"), TreeEntryKind::Dir)));
    }

    #[test]
    fn rename_moves_a_file() {
        let store = BrowserStore::new();
        store.write(Path::new("/p/old.md"), b"x").unwrap();
        store
            .rename(Path::new("/p/old.md"), Path::new("/p/new.md"))
            .unwrap();
        assert!(store.read_to_string(Path::new("/p/old.md")).is_err());
        assert_eq!(store.read_to_string(Path::new("/p/new.md")).unwrap(), "x");
    }

    #[test]
    fn rename_a_directory_moves_its_descendants() {
        let store = BrowserStore::new();
        store.create_dir_all(Path::new("/p/Old")).unwrap();
        store.write(Path::new("/p/Old/scene.md"), b"x").unwrap();
        store
            .rename(Path::new("/p/Old"), Path::new("/p/New"))
            .unwrap();
        assert_eq!(
            store.read_to_string(Path::new("/p/New/scene.md")).unwrap(),
            "x"
        );
        assert!(store.read_to_string(Path::new("/p/Old/scene.md")).is_err());
    }

    #[test]
    fn remove_dir_all_drops_every_descendant() {
        let store = BrowserStore::new();
        store.create_dir_all(Path::new("/p/Chapter 1")).unwrap();
        store.write(Path::new("/p/Chapter 1/a.md"), b"x").unwrap();
        store.write(Path::new("/p/keep.md"), b"y").unwrap();
        store.remove_dir_all(Path::new("/p/Chapter 1")).unwrap();
        assert!(
            store
                .read_to_string(Path::new("/p/Chapter 1/a.md"))
                .is_err()
        );
        assert_eq!(store.read_to_string(Path::new("/p/keep.md")).unwrap(), "y");
    }

    #[test]
    fn list_tree_excludes_non_markdown_files() {
        let store = BrowserStore::new();
        store.write(Path::new("/p/chapter.md"), b"x").unwrap();
        store.write(Path::new("/p/cover.png"), b"x").unwrap();
        let tree = store.list_tree(Path::new("/p"));
        assert_eq!(
            tree,
            vec![(PathBuf::from("/p/chapter.md"), TreeEntryKind::Doc)]
        );
    }

    #[test]
    fn read_dir_lists_only_immediate_children() {
        let store = BrowserStore::new();
        store.create_dir_all(Path::new("/p/Chapter 1")).unwrap();
        store
            .write(Path::new("/p/Chapter 1/scene.md"), b"x")
            .unwrap();
        store.write(Path::new("/p/root.md"), b"x").unwrap();
        let mut children = store.read_dir(Path::new("/p")).unwrap();
        children.sort();
        assert_eq!(
            children,
            vec![PathBuf::from("/p/Chapter 1"), PathBuf::from("/p/root.md")]
        );
    }
}
