//! Abstracts the point read/write/delete operations `project/`'s production
//! code (not its test fixtures, which exercise a real tempdir directly) does
//! against a project's files, so a future browser build can swap in a
//! browser-storage-backed implementation without touching the logic above it
//! — see the wasm feasibility plan's "storage abstraction layer" phase.
//!
//! Deliberately narrow for now: the point read/write/delete operations
//! `project/`'s production code actually calls (`fs::write`/`read_to_string`/
//! `create_dir_all`/`rename`/`remove_dir_all`/`remove_file`), a flat,
//! single-level `read_dir` (added for `backup::prune_old_backups` and
//! `plugins::load`, both of which just list one directory's immediate
//! entries), and a recursive, gitignore-aware `list_tree` (added for
//! `scan::scan_project`).
//!
//! `list_tree` is the trait's biggest concession to how much harder a real
//! browser implementation would be than `NativeStore`'s: on native it's a
//! near-verbatim port of `scan_project`'s old `ignore::WalkBuilder` loop
//! (same `.gitignore`/hidden-file/symlink-exclusion behavior, see its own doc
//! comment), but a browser backend has no such crate to build on against
//! IndexedDB/OPFS — it would need to reimplement equivalent `.gitignore`
//! pattern matching itself (or ship a wasm-compatible gitignore-matching
//! crate) against whatever files it actually has stored. This trait only
//! commits to the *shape* of that capability, not how a browser store would
//! satisfy it.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// One entry from [`ProjectStore::list_tree`]'s recursive walk — promoted
/// from what used to be `scan::scan_project`'s own private `EntryKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeEntryKind {
    Dir,
    Doc,
}

/// Send + Sync: a `Project`'s store crosses the background thread
/// `app::refresh::spawn_word_count_recompute` spawns for its snapshot.
pub trait ProjectStore: std::fmt::Debug + Send + Sync {
    fn read_to_string(&self, path: &Path) -> io::Result<String>;
    fn write(&self, path: &Path, contents: &[u8]) -> io::Result<()>;
    fn create_dir_all(&self, path: &Path) -> io::Result<()>;
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;
    fn remove_dir_all(&self, path: &Path) -> io::Result<()>;
    fn remove_file(&self, path: &Path) -> io::Result<()>;
    /// `path`'s immediate entries (not recursive) — unreadable entries are
    /// silently skipped rather than failing the whole listing, same as every
    /// caller's own `.filter_map(|entry| entry.ok())` did before this trait
    /// existed.
    fn read_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>>;
    /// Every directory and `.md` document under `root` (`root` itself not
    /// included), respecting whatever ignore/hidden-file rules this store
    /// applies — see this module's doc comment for what that means for a
    /// non-native implementation. Never fails outright (an unreadable
    /// sub-entry is just skipped, mirroring `read_dir`), matching
    /// `scan_project`'s own total-success contract.
    fn list_tree(&self, root: &Path) -> Vec<(PathBuf, TreeEntryKind)>;
    /// Whether anything exists at `path` at all (file or directory) — added
    /// alongside `is_dir` once a non-native store existed: production code
    /// used to call `Path::exists()` directly, which is silently wrong for
    /// any path that isn't a real filesystem path (see `BrowserStore`).
    fn exists(&self, path: &Path) -> bool;
    /// Whether `path` exists and is specifically a directory (not a file).
    fn is_dir(&self, path: &Path) -> bool;
}

/// The only implementation today: a thin wrapper over `std::fs`, behaving
/// exactly as `project/`'s code did before this trait existed.
#[derive(Debug, Clone, Copy, Default)]
pub struct NativeStore;

impl ProjectStore for NativeStore {
    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        std::fs::read_to_string(path)
    }

    fn write(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
        std::fs::write(path, contents)
    }

    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        std::fs::create_dir_all(path)
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        std::fs::rename(from, to)
    }

    fn remove_dir_all(&self, path: &Path) -> io::Result<()> {
        std::fs::remove_dir_all(path)
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        std::fs::remove_file(path)
    }

    fn read_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>> {
        Ok(std::fs::read_dir(path)?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .collect())
    }

    // `require_git(false)` is set deliberately: the `ignore` crate only
    // honors `.gitignore` files by default when the scanned folder is inside
    // an actual `.git` repository. A project folder won't always be one (and
    // isn't required to be), so gitignore rules must apply regardless.
    fn list_tree(&self, root: &Path) -> Vec<(PathBuf, TreeEntryKind)> {
        let walker = ignore::WalkBuilder::new(root).require_git(false).build();
        let mut entries = Vec::new();
        for entry in walker {
            let Ok(entry) = entry else { continue };
            let path = entry.path().to_path_buf();
            if path == root {
                continue;
            }

            // `entry.file_type()` reports the entry's own type (a symlink is
            // neither a dir nor a file here, since the walker isn't
            // following links). Requiring `is_file()`, not just a `.md`
            // extension, keeps a symlink named e.g. `Notes.md` out of the
            // tree entirely — see `scan_project`'s own doc comment for why
            // that matters (a synced project could otherwise plant a
            // symlink pointing outside the project).
            let file_type = entry.file_type();
            if file_type.is_some_and(|ft| ft.is_dir()) {
                entries.push((path, TreeEntryKind::Dir));
            } else if file_type.is_some_and(|ft| ft.is_file())
                && path.extension().and_then(|ext| ext.to_str()) == Some("md")
            {
                entries.push((path, TreeEntryKind::Doc));
            }
        }
        entries
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn is_dir(&self, path: &Path) -> bool {
        path.is_dir()
    }
}

/// Convenience for constructing the default (native) store as the
/// `Arc<dyn ProjectStore>` `Project` actually stores.
pub fn native_store() -> Arc<dyn ProjectStore> {
    Arc::new(NativeStore)
}
