//! The sync engine: reconciles a project folder with a vault on the sync server.
//!
//! [`SyncEngine::sync_once`] is one pass. It is blocking and transport-agnostic
//! (see `sync::transport`), so it's driven from a background thread in the app
//! and from plain tests here. A pass, in order:
//!
//! 1. **Pull the manifest** so we know which documents exist and where.
//! 2. **Resurrect** tombstoned documents that received edits we hadn't seen
//!    (a concurrent edit beats a delete).
//! 3. **Reconcile the folder with the manifest** — the local-only step that turns
//!    filesystem changes into manifest changes: remote renames are applied to disk;
//!    a missing file is a rename (if a new file has identical content) or a delete
//!    (tombstone); a new file gets a fresh document; a tombstoned file is removed
//!    unless it has unsynced local edits, in which case the edit wins.
//! 4. **Per document**: capture local edits as a CRDT update, pull and apply remote
//!    updates, and write the merged text back.
//! 5. **Push** queued updates, manifest first so peers learn paths before content.
//!
//! Local work (steps 3–4's capture, and queuing) happens even when the server is
//! unreachable; only the network steps are skipped, and the pass then reports the
//! transport error. Nothing is lost: pending updates are persisted and pushed later.
//!
//! # Joining a vault that already has content
//!
//! A device that already has files at paths the vault also has *adopts* them
//! instead of duplicating: it pulls the remote document first. If the local text
//! is identical, nothing more happens. If it differs there is no common ancestor
//! to merge against, so the local version is kept as a sibling
//! `"<name> (conflict copy).md"` (a new document) and the file at the original
//! path takes the vault's content — nothing is ever silently overwritten.
//!
//! # Safety rails
//!
//! - Manifest paths are untrusted and validated ([`is_safe_relative_path`]) before
//!   any filesystem access.
//! - A missing project folder is an error, and an *empty* one is never read as
//!   "the user deleted everything" (an unmounted drive would otherwise tombstone
//!   the whole vault everywhere): the files are restored instead.
//! - Files that aren't valid UTF-8 are skipped, never treated as deleted.
//! - A file is only overwritten if it still holds what was read at capture time,
//!   so an edit made mid-pass is merged on the next pass rather than clobbered.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use smaragd_sync_protocol::{DeviceId, DocId, VaultId};
use uuid::Uuid;

use super::crdt::FileDoc;
use super::crypto::{CryptoError, VaultKey};
use super::manifest::{
    EntryKind, ManifestDoc, ManifestEntry, is_safe_relative_dir_path, is_safe_relative_path,
};
use super::meta_crdt::{MetaDoc, PathIds, SyncedFields};
use super::state::StateStore;
use super::transport::{SyncTransport, TransportError};
use crate::project::ProjectMeta;
use crate::project::store::{ProjectStore, TreeEntryKind};

const MANIFEST_KEY: &str = "manifest";
const DIRS_KEY: &str = "dirs";
const META_KEY: &str = "meta";
const META_PATHS_KEY: &str = "meta_paths";
/// Where the project's metadata lives, relative to the project root.
const META_PATH: &str = ".smaragd/project.json";
/// A local `project.json` is saved here, once, before a first sync replaces it with
/// the vault's version, so nothing the user had there is ever silently lost.
const META_BACKUP_PATH: &str = ".smaragd/project.json.before-sync";

fn doc_key(id: DocId) -> String {
    format!("doc/{id}")
}

#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error("the sync passphrase doesn't match this vault (or the data was tampered with)")]
    WrongPassphrase,
    #[error(transparent)]
    Transport(#[from] TransportError),
    #[error("encryption error: {0}")]
    Crypto(CryptoError),
    #[error("file error: {0}")]
    Io(#[from] io::Error),
    #[error("the local sync state is corrupt: {0}")]
    State(String),
    #[error("CRDT error: {0}")]
    Crdt(String),
    #[error("the project folder is missing: {0}")]
    ProjectMissing(PathBuf),
}

impl From<CryptoError> for SyncError {
    fn from(err: CryptoError) -> Self {
        match err {
            CryptoError::AuthenticationFailed => SyncError::WrongPassphrase,
            other => SyncError::Crypto(other),
        }
    }
}

impl From<yrs::error::Error> for SyncError {
    fn from(err: yrs::error::Error) -> Self {
        SyncError::Crdt(err.to_string())
    }
}

/// What one pass did, for the status panel and for tests.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SyncReport {
    pub pushed_updates: usize,
    pub pulled_updates: usize,
    /// Local edits captured as CRDT updates.
    pub local_edits: usize,
    /// New documents created from new local files.
    pub files_created: usize,
    /// Files written or overwritten with merged/remote content.
    pub files_written: usize,
    pub files_renamed: usize,
    /// Files removed locally because another device deleted them.
    pub files_removed: usize,
    /// Local deletions announced to the vault.
    pub files_tombstoned: usize,
    /// Deleted documents revived because they were edited concurrently.
    pub resurrected: usize,
    /// Local versions kept as sibling files rather than overwritten.
    pub conflict_copies: Vec<String>,
    /// Paths ignored as unsafe or unreadable.
    pub skipped_paths: Vec<String>,
    /// Deletions not announced because the project folder looked empty.
    pub deletions_held_back: usize,
    /// Folders created locally because another device has them.
    pub dirs_created: usize,
    /// Empty folders removed locally (deleted or renamed away elsewhere).
    pub dirs_removed: usize,
    /// Local folder deletions announced to the vault.
    pub dirs_tombstoned: usize,
    /// `project.json` was rewritten with merged remote changes. The app must reload
    /// its in-memory project metadata when this is set, or it will overwrite them.
    pub meta_written: bool,
}

impl SyncReport {
    /// Whether nothing at all happened.
    pub fn is_quiet(&self) -> bool {
        *self == SyncReport::default()
    }
}

pub struct EngineConfig {
    pub vault: VaultId,
    pub device: DeviceId,
    pub key: VaultKey,
    pub root: PathBuf,
}

trait Crdt {
    fn apply(&mut self, update: &[u8]) -> Result<(), yrs::error::Error>;
    fn state(&self) -> Vec<u8>;
}

impl Crdt for FileDoc {
    fn apply(&mut self, update: &[u8]) -> Result<(), yrs::error::Error> {
        self.apply_update(update)
    }

    fn state(&self) -> Vec<u8> {
        self.encode_state()
    }
}

impl Crdt for MetaDoc {
    fn apply(&mut self, update: &[u8]) -> Result<(), yrs::error::Error> {
        self.apply_update(update)
    }

    fn state(&self) -> Vec<u8> {
        self.encode_state()
    }
}

impl Crdt for ManifestDoc {
    fn apply(&mut self, update: &[u8]) -> Result<(), yrs::error::Error> {
        self.apply_update(update)
    }

    fn state(&self) -> Vec<u8> {
        self.encode_state()
    }
}

/// A CRDT document plus this device's sync bookkeeping for it.
struct Tracked<D> {
    doc: D,
    /// Highest server sequence number already applied.
    last_seq: u64,
    /// Local updates not yet pushed, oldest first (plaintext; sealed on push).
    pending: Vec<Vec<u8>>,
    /// Where this device has the file on disk, `None` if not materialized here.
    local_path: Option<String>,
    dirty: bool,
}

impl<D> Tracked<D> {
    fn new(doc: D) -> Self {
        Self {
            doc,
            last_seq: 0,
            pending: Vec::new(),
            local_path: None,
            dirty: true,
        }
    }
}

impl Tracked<FileDoc> {
    /// Never synced, never seeded, never on disk: has to fetch before capturing.
    fn is_fresh(&self) -> bool {
        self.last_seq == 0
            && self.pending.is_empty()
            && self.local_path.is_none()
            && self.doc.is_empty()
    }
}

#[derive(Serialize, Deserialize)]
struct Persisted {
    yrs_state: Vec<u8>,
    last_seq: u64,
    pending: Vec<Vec<u8>>,
    local_path: Option<String>,
}

fn persist_bytes<D: Crdt>(tracked: &Tracked<D>) -> Vec<u8> {
    postcard::to_stdvec(&Persisted {
        yrs_state: tracked.doc.state(),
        last_seq: tracked.last_seq,
        pending: tracked.pending.clone(),
        local_path: tracked.local_path.clone(),
    })
    .expect("Persisted always serializes")
}

fn load_persisted(bytes: &[u8]) -> Result<Persisted, SyncError> {
    postcard::from_bytes(bytes).map_err(|err| SyncError::State(err.to_string()))
}

enum Disk {
    Missing,
    Text(String),
    Unreadable,
}

fn abs(root: &Path, rel: &str) -> PathBuf {
    root.join(rel)
}

fn rel_key(root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    let mut parts = Vec::new();
    for component in rel.components() {
        parts.push(component.as_os_str().to_str()?.to_string());
    }
    Some(parts.join("/"))
}

fn scan_docs(files: &dyn ProjectStore, root: &Path) -> BTreeSet<String> {
    files
        .list_tree(root)
        .into_iter()
        .filter(|(_, kind)| *kind == TreeEntryKind::Doc)
        .filter_map(|(path, _)| rel_key(root, &path))
        .collect()
}

fn scan_dirs(files: &dyn ProjectStore, root: &Path) -> BTreeSet<String> {
    files
        .list_tree(root)
        .into_iter()
        .filter(|(_, kind)| *kind == TreeEntryKind::Dir)
        .filter_map(|(path, _)| rel_key(root, &path))
        .collect()
}

fn dir_is_empty(files: &dyn ProjectStore, root: &Path, rel: &str) -> bool {
    files
        .read_dir(&abs(root, rel))
        .is_ok_and(|entries| entries.is_empty())
}

/// Folder renames implied by file moves: `A/S/f.md -> B/S/f.md` says `A` became `B`
/// (the trailing components both paths share are the untouched part). A file that
/// was itself renamed shares nothing and implies nothing.
fn dir_rename_pairs(renamed: &[(String, String)]) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    for (old, new) in renamed {
        let (old_parts, new_parts): (Vec<&str>, Vec<&str>) =
            (old.split('/').collect(), new.split('/').collect());
        let mut shared = 0;
        while shared < old_parts.len()
            && shared < new_parts.len()
            && old_parts[old_parts.len() - 1 - shared] == new_parts[new_parts.len() - 1 - shared]
        {
            shared += 1;
        }
        let old_dir = old_parts[..old_parts.len() - shared].join("/");
        let new_dir = new_parts[..new_parts.len() - shared].join("/");
        if shared > 0 && !old_dir.is_empty() && !new_dir.is_empty() && old_dir != new_dir {
            pairs.push((old_dir, new_dir));
        }
    }
    pairs
}

/// Where `path` went, if it lies at or under a renamed folder.
fn map_through_pairs(path: &str, pairs: &[(String, String)]) -> Option<String> {
    pairs.iter().find_map(|(from, to)| {
        if path == from {
            Some(to.clone())
        } else {
            path.strip_prefix(&format!("{from}/"))
                .map(|rest| format!("{to}/{rest}"))
        }
    })
}

fn read_disk(files: &dyn ProjectStore, root: &Path, rel: &str) -> Disk {
    match files.read_to_string(&abs(root, rel)) {
        Ok(text) => Disk::Text(text),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Disk::Missing,
        Err(_) => Disk::Unreadable,
    }
}

fn ensure_parent(files: &dyn ProjectStore, path: &Path) -> io::Result<()> {
    match path.parent() {
        Some(parent) => files.create_dir_all(parent),
        None => Ok(()),
    }
}

/// Writes via a temp name and a rename, so a crash never leaves a torn file.
fn write_file(files: &dyn ProjectStore, root: &Path, rel: &str, text: &str) -> io::Result<()> {
    let path = abs(root, rel);
    ensure_parent(files, &path)?;
    let tmp = path.with_extension("md.sync-tmp");
    files.write(&tmp, text.as_bytes())?;
    files.rename(&tmp, &path)
}

fn move_file(files: &dyn ProjectStore, root: &Path, from: &str, to: &str) -> io::Result<()> {
    let target = abs(root, to);
    ensure_parent(files, &target)?;
    files.rename(&abs(root, from), &target)
}

/// `dir/name.md` -> `dir/name (conflict copy).md`
fn conflict_copy_path(path: &str) -> String {
    let stem = path.strip_suffix(".md").unwrap_or(path);
    format!("{stem} (conflict copy).md")
}

/// The deterministic name the loser of a same-path collision is moved to. Both
/// devices compute the identical value, so concurrent renames converge.
fn collision_path(path: &str, id: DocId) -> String {
    let stem = path.strip_suffix(".md").unwrap_or(path);
    let short: String = id.as_uuid().simple().to_string().chars().take(8).collect();
    format!("{stem} (conflict {short}).md")
}

fn pull_into<D: Crdt>(
    key: &VaultKey,
    vault: VaultId,
    device: DeviceId,
    transport: &dyn SyncTransport,
    id: DocId,
    tracked: &mut Tracked<D>,
) -> Result<usize, SyncError> {
    let response = transport.pull(id, tracked.last_seq)?;
    let mut applied = 0;
    if let Some(snapshot) = response.snapshot {
        let plain = key.open(vault, id, &snapshot.blob)?;
        tracked.doc.apply(&plain)?;
        tracked.last_seq = tracked.last_seq.max(snapshot.upto_seq);
        applied += 1;
    }
    for update in response.updates {
        if update.device_id != device {
            let plain = key.open(vault, id, &update.blob)?;
            tracked.doc.apply(&plain)?;
            applied += 1;
        }
        tracked.last_seq = tracked.last_seq.max(update.seq);
    }
    tracked.dirty = true;
    Ok(applied)
}

fn push_from<D: Crdt>(
    key: &VaultKey,
    vault: VaultId,
    transport: &dyn SyncTransport,
    id: DocId,
    tracked: &mut Tracked<D>,
) -> Result<usize, SyncError> {
    let mut pushed = 0;
    while let Some(update) = tracked.pending.first().cloned() {
        let sealed = key.seal(vault, id, &update)?;
        let seq = transport.push(id, &sealed)?;
        tracked.pending.remove(0);
        tracked.dirty = true;
        pushed += 1;
        if seq == tracked.last_seq + 1 {
            tracked.last_seq = seq;
        }
    }
    Ok(pushed)
}

pub struct SyncEngine {
    cfg: EngineConfig,
    files: Arc<dyn ProjectStore>,
    state: Box<dyn StateStore>,
    manifest: Tracked<ManifestDoc>,
    /// The project metadata document (`DocId::PROJECT_META`).
    meta: Tracked<MetaDoc>,
    docs: BTreeMap<DocId, Tracked<FileDoc>>,
    /// Where this device has each synced folder on disk (folders carry no content,
    /// so unlike files they need no CRDT state of their own).
    dir_paths: BTreeMap<DocId, String>,
    dirs_dirty: bool,
    /// The path -> id layout `project.json` was last brought in line with (see
    /// `PathIds::with_previous`).
    meta_paths: BTreeMap<String, DocId>,
    meta_paths_dirty: bool,
}

impl SyncEngine {
    /// Opens the engine, restoring whatever local state a previous run persisted.
    pub fn open(
        cfg: EngineConfig,
        files: Arc<dyn ProjectStore>,
        state: Box<dyn StateStore>,
    ) -> Result<Self, SyncError> {
        let manifest = match state.get(MANIFEST_KEY)? {
            Some(bytes) => {
                let saved = load_persisted(&bytes)?;
                Tracked {
                    doc: ManifestDoc::from_state(&saved.yrs_state)?,
                    last_seq: saved.last_seq,
                    pending: saved.pending,
                    local_path: None,
                    dirty: false,
                }
            }
            None => Tracked::new(ManifestDoc::new()),
        };
        let mut docs = BTreeMap::new();
        for entry in manifest.doc.entries() {
            if let Some(bytes) = state.get(&doc_key(entry.doc_id))? {
                let saved = load_persisted(&bytes)?;
                docs.insert(
                    entry.doc_id,
                    Tracked {
                        doc: FileDoc::from_state(&saved.yrs_state)?,
                        last_seq: saved.last_seq,
                        pending: saved.pending,
                        local_path: saved.local_path,
                        dirty: false,
                    },
                );
            }
        }
        let meta = match state.get(META_KEY)? {
            Some(bytes) => {
                let saved = load_persisted(&bytes)?;
                Tracked {
                    doc: MetaDoc::from_state(&saved.yrs_state)?,
                    last_seq: saved.last_seq,
                    pending: saved.pending,
                    local_path: None,
                    dirty: false,
                }
            }
            None => Tracked::new(MetaDoc::new()),
        };
        let dir_paths = match state.get(DIRS_KEY)? {
            Some(bytes) => {
                postcard::from_bytes(&bytes).map_err(|err| SyncError::State(err.to_string()))?
            }
            None => BTreeMap::new(),
        };
        let meta_paths = match state.get(META_PATHS_KEY)? {
            Some(bytes) => {
                postcard::from_bytes(&bytes).map_err(|err| SyncError::State(err.to_string()))?
            }
            None => BTreeMap::new(),
        };
        Ok(Self {
            cfg,
            files,
            state,
            manifest,
            meta,
            meta_paths,
            meta_paths_dirty: false,
            docs,
            dir_paths,
            dirs_dirty: false,
        })
    }

    /// Every manifest entry (tombstones included), for status displays and tests.
    pub fn manifest_entries(&self) -> Vec<ManifestEntry> {
        self.manifest.doc.entries()
    }

    /// One reconcile pass. Local changes are always captured and persisted, even if
    /// the server is unreachable (the error is returned afterwards).
    pub fn sync_once(&mut self, transport: &dyn SyncTransport) -> Result<SyncReport, SyncError> {
        let mut report = SyncReport::default();
        let outcome = self.run(transport, &mut report);
        self.persist()?;
        outcome.map(|()| report)
    }

    fn run(
        &mut self,
        transport: &dyn SyncTransport,
        report: &mut SyncReport,
    ) -> Result<(), SyncError> {
        if !self.files.is_dir(&self.cfg.root) {
            return Err(SyncError::ProjectMissing(self.cfg.root.clone()));
        }

        let listing = transport.list_docs();
        let remote: Option<HashMap<DocId, u64>> = listing
            .as_ref()
            .ok()
            .map(|docs| docs.iter().map(|d| (d.doc_id, d.latest_seq)).collect());

        if let Some(remote) = &remote {
            self.pull_manifest(transport, remote, report)?;
            self.resurrect_edited_tombstones(transport, remote, report)?;
        }
        self.reconcile(report)?;
        for entry in self.live_entries(report) {
            self.sync_doc(&entry, transport, remote.as_ref(), report)?;
        }
        self.sync_meta(transport, remote.as_ref(), report)?;
        if remote.is_some() {
            self.push_all(transport, report)?;
        }
        listing.map(|_| ()).map_err(SyncError::from)
    }

    fn persist(&mut self) -> Result<(), SyncError> {
        if self.manifest.dirty {
            self.state
                .put(MANIFEST_KEY, &persist_bytes(&self.manifest))?;
            self.manifest.dirty = false;
        }
        if self.meta.dirty {
            self.state.put(META_KEY, &persist_bytes(&self.meta))?;
            self.meta.dirty = false;
        }
        for (id, tracked) in &mut self.docs {
            if tracked.dirty {
                self.state.put(&doc_key(*id), &persist_bytes(tracked))?;
                tracked.dirty = false;
            }
        }
        if self.meta_paths_dirty {
            let bytes = postcard::to_stdvec(&self.meta_paths).expect("meta paths always serialize");
            self.state.put(META_PATHS_KEY, &bytes)?;
            self.meta_paths_dirty = false;
        }
        if self.dirs_dirty {
            let bytes = postcard::to_stdvec(&self.dir_paths).expect("dir paths always serialize");
            self.state.put(DIRS_KEY, &bytes)?;
            self.dirs_dirty = false;
        }
        Ok(())
    }

    fn queue_manifest_update(&mut self, update: Option<Vec<u8>>) {
        if let Some(update) = update {
            self.manifest.pending.push(update);
            self.manifest.dirty = true;
        }
    }

    /// Live entries of `kind` with usable paths; unsafe paths are reported and ignored.
    fn live_of_kind(&self, kind: EntryKind, report: &mut SyncReport) -> Vec<ManifestEntry> {
        let mut out = Vec::new();
        for entry in self.manifest.doc.entries() {
            if entry.deleted || entry.kind != kind || entry.doc_id.is_reserved() {
                continue;
            }
            let safe = match kind {
                EntryKind::Doc => is_safe_relative_path(&entry.path),
                EntryKind::Dir => is_safe_relative_dir_path(&entry.path),
            };
            if safe {
                out.push(entry);
            } else if !report.skipped_paths.contains(&entry.path) {
                report.skipped_paths.push(entry.path);
            }
        }
        out
    }

    fn live_entries(&self, report: &mut SyncReport) -> Vec<ManifestEntry> {
        self.live_of_kind(EntryKind::Doc, report)
    }

    fn live_dir_entries(&self, report: &mut SyncReport) -> Vec<ManifestEntry> {
        self.live_of_kind(EntryKind::Dir, report)
    }

    fn pull_manifest(
        &mut self,
        transport: &dyn SyncTransport,
        remote: &HashMap<DocId, u64>,
        report: &mut SyncReport,
    ) -> Result<(), SyncError> {
        let latest = remote.get(&DocId::MANIFEST).copied().unwrap_or(0);
        if latest > self.manifest.last_seq {
            report.pulled_updates += pull_into(
                &self.cfg.key,
                self.cfg.vault,
                self.cfg.device,
                transport,
                DocId::MANIFEST,
                &mut self.manifest,
            )?;
        }
        Ok(())
    }

    /// A tombstoned document that received updates we hadn't seen was edited
    /// concurrently with (or after) its deletion: the edit wins.
    fn resurrect_edited_tombstones(
        &mut self,
        transport: &dyn SyncTransport,
        remote: &HashMap<DocId, u64>,
        report: &mut SyncReport,
    ) -> Result<(), SyncError> {
        for entry in self.manifest.doc.entries() {
            if !entry.deleted {
                continue;
            }
            let Some(tracked) = self.docs.get_mut(&entry.doc_id) else {
                continue;
            };
            if remote.get(&entry.doc_id).copied().unwrap_or(0) <= tracked.last_seq {
                continue;
            }
            let applied = pull_into(
                &self.cfg.key,
                self.cfg.vault,
                self.cfg.device,
                transport,
                entry.doc_id,
                tracked,
            )?;
            report.pulled_updates += applied;
            if applied > 0 {
                let update = self.manifest.doc.set_deleted(entry.doc_id, false);
                self.queue_manifest_update(update);
                report.resurrected += 1;
            }
        }
        Ok(())
    }

    /// Two live documents can end up at the same path (each device created its own
    /// before seeing the other's). The larger id moves to a deterministic name.
    fn fix_path_collisions(&mut self, report: &mut SyncReport) {
        let mut by_path: BTreeMap<String, Vec<DocId>> = BTreeMap::new();
        for entry in self.live_entries(report) {
            by_path
                .entry(entry.path.to_lowercase())
                .or_default()
                .push(entry.doc_id);
        }
        for ids in by_path.values() {
            if ids.len() < 2 {
                continue;
            }
            let mut ids = ids.clone();
            ids.sort();
            for loser in &ids[1..] {
                let Some(entry) = self.manifest.doc.get(*loser) else {
                    continue;
                };
                let update = self
                    .manifest
                    .doc
                    .rename(*loser, &collision_path(&entry.path, *loser));
                self.queue_manifest_update(update);
            }
        }
    }

    fn reconcile(&mut self, report: &mut SyncReport) -> Result<(), SyncError> {
        self.fix_path_collisions(report);
        let files = &*self.files;
        let root = &self.cfg.root;
        let mut scan = scan_docs(files, root);
        let entries = self.live_entries(report);

        let mut renamed_files: Vec<(String, String)> = Vec::new();

        // Remote renames: move our copy to where the manifest now says it lives.
        for entry in &entries {
            let Some(tracked) = self.docs.get_mut(&entry.doc_id) else {
                continue;
            };
            let Some(local) = tracked.local_path.clone() else {
                continue;
            };
            if local != entry.path && scan.contains(&local) && !scan.contains(&entry.path) {
                move_file(files, root, &local, &entry.path)?;
                scan.remove(&local);
                scan.insert(entry.path.clone());
                renamed_files.push((local, entry.path.clone()));
                tracked.local_path = Some(entry.path.clone());
                tracked.dirty = true;
                report.files_renamed += 1;
            }
        }

        // Which files belong to a document this device already materialized.
        let materialized: BTreeSet<String> = self
            .docs
            .values()
            .filter_map(|tracked| tracked.local_path.clone())
            .collect();
        let never_materialized = |docs: &BTreeMap<DocId, Tracked<FileDoc>>, id: DocId| {
            docs.get(&id).is_none_or(|t| t.local_path.is_none())
        };

        // Files with no materialized owner are either adoptable (a live entry at
        // that path this device never materialized), a case-variant of one, or new.
        let mut new_files = Vec::new();
        for path in scan
            .iter()
            .filter(|path| !materialized.contains(*path))
            .cloned()
            .collect::<Vec<_>>()
        {
            if entries
                .iter()
                .any(|e| e.path == path && never_materialized(&self.docs, e.doc_id))
            {
                continue;
            }
            let lower = path.to_lowercase();
            let variants: Vec<&ManifestEntry> = entries
                .iter()
                .filter(|e| {
                    e.path.to_lowercase() == lower
                        && !scan.contains(&e.path)
                        && never_materialized(&self.docs, e.doc_id)
                })
                .collect();
            if let [only] = variants.as_slice() {
                move_file(files, root, &path, &only.path)?;
                scan.remove(&path);
                scan.insert(only.path.clone());
                continue;
            }
            new_files.push(path);
        }

        // Files we had that are gone.
        let mut missing: Vec<(DocId, String)> = entries
            .iter()
            .filter_map(|entry| {
                let local = self.docs.get(&entry.doc_id)?.local_path.clone()?;
                (!scan.contains(&local)).then_some((entry.doc_id, local))
            })
            .collect();

        // A missing file whose content reappears under a new name is a rename.
        let mut new_texts: HashMap<String, String> = HashMap::new();
        for path in std::mem::take(&mut new_files) {
            match read_disk(files, root, &path) {
                Disk::Text(text) => {
                    new_texts.insert(path.clone(), text);
                    new_files.push(path);
                }
                Disk::Unreadable => report.skipped_paths.push(path),
                Disk::Missing => {}
            }
        }
        let mut renames = Vec::new();
        missing.retain(|(id, old_path)| {
            let Some(tracked) = self.docs.get(id) else {
                return true;
            };
            let rendered = tracked.doc.render();
            match new_files.iter().position(|p| new_texts[p] == rendered) {
                Some(position) => {
                    renames.push((*id, old_path.clone(), new_files.remove(position)));
                    false
                }
                None => true,
            }
        });
        for (id, old_path, new_path) in renames {
            renamed_files.push((old_path, new_path.clone()));
            let update = self.manifest.doc.rename(id, &new_path);
            self.queue_manifest_update(update);
            if let Some(tracked) = self.docs.get_mut(&id) {
                tracked.local_path = Some(new_path);
                tracked.dirty = true;
            }
            report.files_renamed += 1;
        }

        // The rest were deleted here — unless the whole folder looks empty, which
        // is far more likely an unmounted drive than a deliberate mass delete.
        if scan.is_empty() && !missing.is_empty() {
            report.deletions_held_back += missing.len();
        } else {
            for (id, _) in missing {
                let update = self.manifest.doc.set_deleted(id, true);
                self.queue_manifest_update(update);
                if let Some(tracked) = self.docs.get_mut(&id) {
                    tracked.local_path = None;
                    tracked.dirty = true;
                }
                report.files_tombstoned += 1;
            }
        }

        for path in new_files {
            let text = new_texts.remove(&path).unwrap_or_default();
            self.create_local_doc(&path, &text);
            report.files_created += 1;
        }

        self.apply_tombstones(&scan, report)?;
        self.reconcile_dirs(&renamed_files, scan.is_empty(), report)
    }

    /// Two live folders at the same path (each device registered its own): the
    /// larger id is tombstoned, deterministically, so every device agrees.
    fn fix_dir_collisions(&mut self, report: &mut SyncReport) {
        let mut by_path: BTreeMap<String, Vec<DocId>> = BTreeMap::new();
        for entry in self.live_dir_entries(report) {
            by_path
                .entry(entry.path.to_lowercase())
                .or_default()
                .push(entry.doc_id);
        }
        for mut ids in by_path.into_values().filter(|ids| ids.len() > 1) {
            ids.sort();
            for loser in ids.split_off(1) {
                let update = self.manifest.doc.set_deleted(loser, true);
                self.queue_manifest_update(update);
                if self.dir_paths.remove(&loser).is_some() {
                    self.dirs_dirty = true;
                }
            }
        }
    }

    /// Keeps folders in step with the manifest. Folders have no content, so this is
    /// pure bookkeeping: register new local folders, create ones other devices have,
    /// follow renames, and announce or apply deletions. File moves are handled first
    /// (children carry themselves), so a renamed folder is recognised from its
    /// files' moves and an emptied old folder is simply removed.
    fn reconcile_dirs(
        &mut self,
        renamed_files: &[(String, String)],
        no_files: bool,
        report: &mut SyncReport,
    ) -> Result<(), SyncError> {
        let files = Arc::clone(&self.files);
        let root = self.cfg.root.clone();
        self.fix_dir_collisions(report);
        let mut local = scan_dirs(&*files, &root);
        let pairs = dir_rename_pairs(renamed_files);

        // Remote renames: make the new folder, drop the old one if it's empty now.
        // Deepest old folder first, so a child is gone before its parent is checked.
        let mut moved: Vec<(ManifestEntry, String)> = self
            .live_dir_entries(report)
            .into_iter()
            .filter_map(|entry| {
                let old = self.dir_paths.get(&entry.doc_id)?.clone();
                (old != entry.path).then_some((entry, old))
            })
            .collect();
        moved.sort_by(|a, b| b.1.cmp(&a.1));
        for (entry, old) in moved {
            files.create_dir_all(&abs(&root, &entry.path))?;
            local.insert(entry.path.clone());
            if local.contains(&old) && dir_is_empty(&*files, &root, &old) {
                files.remove_dir_all(&abs(&root, &old))?;
                local.remove(&old);
                report.dirs_removed += 1;
            }
            self.dir_paths.insert(entry.doc_id, entry.path);
            self.dirs_dirty = true;
        }

        // Folders we had that are gone: renamed (as their files show) or deleted —
        // unless nothing is left at all, which reads as an unmounted drive.
        let tracked: BTreeSet<String> = self.dir_paths.values().cloned().collect();
        for entry in self.live_dir_entries(report) {
            let Some(old) = self.dir_paths.get(&entry.doc_id).cloned() else {
                continue;
            };
            if local.contains(&old) {
                continue;
            }
            let moved_to = map_through_pairs(&old, &pairs)
                .filter(|new| local.contains(new) && !tracked.contains(new));
            if let Some(new) = moved_to {
                let update = self.manifest.doc.rename(entry.doc_id, &new);
                self.queue_manifest_update(update);
                self.dir_paths.insert(entry.doc_id, new);
            } else if local.is_empty() && no_files {
                report.deletions_held_back += 1;
                continue;
            } else {
                let update = self.manifest.doc.set_deleted(entry.doc_id, true);
                self.queue_manifest_update(update);
                self.dir_paths.remove(&entry.doc_id);
                report.dirs_tombstoned += 1;
            }
            self.dirs_dirty = true;
        }

        // Folders other devices have that we've never made.
        for entry in self.live_dir_entries(report) {
            if self.dir_paths.contains_key(&entry.doc_id) {
                continue;
            }
            if !local.contains(&entry.path) {
                files.create_dir_all(&abs(&root, &entry.path))?;
                local.insert(entry.path.clone());
                report.dirs_created += 1;
            }
            self.dir_paths.insert(entry.doc_id, entry.path);
            self.dirs_dirty = true;
        }

        // Local folders nobody has registered yet, parents before children.
        let tracked: BTreeSet<String> = self.dir_paths.values().cloned().collect();
        for path in &local {
            if tracked.contains(path) || !is_safe_relative_dir_path(path) {
                continue;
            }
            let id = DocId(Uuid::new_v4());
            let update = self.manifest.doc.add(id, path, EntryKind::Dir);
            self.queue_manifest_update(Some(update));
            self.dir_paths.insert(id, path.clone());
            self.dirs_dirty = true;
        }

        // Folders deleted elsewhere: remove ours if it's empty, deepest first.
        let live_paths: BTreeSet<String> = self
            .live_dir_entries(report)
            .into_iter()
            .map(|entry| entry.path.to_lowercase())
            .collect();
        let mut dead: Vec<(DocId, String)> = self
            .manifest
            .doc
            .entries()
            .into_iter()
            .filter(|entry| entry.deleted && entry.kind == EntryKind::Dir)
            .filter_map(|entry| Some((entry.doc_id, self.dir_paths.get(&entry.doc_id)?.clone())))
            .collect();
        dead.sort_by(|a, b| b.1.cmp(&a.1));
        for (id, path) in dead {
            let gone = !local.contains(&path);
            let removable =
                !live_paths.contains(&path.to_lowercase()) && dir_is_empty(&*files, &root, &path);
            if !gone && removable {
                files.remove_dir_all(&abs(&root, &path))?;
                local.remove(&path);
                report.dirs_removed += 1;
            }
            if gone || removable {
                self.dir_paths.remove(&id);
                self.dirs_dirty = true;
            }
        }
        Ok(())
    }

    /// Removes files whose documents another device deleted, unless this device has
    /// unsynced edits to them — then the edit wins and the document is revived.
    fn apply_tombstones(
        &mut self,
        scan: &BTreeSet<String>,
        report: &mut SyncReport,
    ) -> Result<(), SyncError> {
        for entry in self.manifest.doc.entries() {
            if !entry.deleted {
                continue;
            }
            let Some(tracked) = self.docs.get_mut(&entry.doc_id) else {
                continue;
            };
            let Some(local) = tracked.local_path.clone() else {
                continue;
            };
            if !scan.contains(&local) {
                tracked.local_path = None;
                tracked.dirty = true;
                continue;
            }
            match read_disk(&*self.files, &self.cfg.root, &local) {
                Disk::Text(text) if text == tracked.doc.render() => {
                    self.files.remove_file(&abs(&self.cfg.root, &local))?;
                    tracked.local_path = None;
                    tracked.dirty = true;
                    report.files_removed += 1;
                }
                Disk::Text(_) => {
                    let update = self.manifest.doc.set_deleted(entry.doc_id, false);
                    self.queue_manifest_update(update);
                    report.resurrected += 1;
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Registers a brand-new document for a file that already exists at `path`.
    fn create_local_doc(&mut self, path: &str, text: &str) -> DocId {
        let id = DocId(Uuid::new_v4());
        let update = self.manifest.doc.add(id, path, EntryKind::Doc);
        self.queue_manifest_update(Some(update));
        let mut tracked = Tracked::new(FileDoc::new());
        if let Some(update) = tracked.doc.set_text(text) {
            tracked.pending.push(update);
        }
        tracked.local_path = Some(path.to_string());
        self.docs.insert(id, tracked);
        id
    }

    fn unique_path(&self, wanted: &str) -> String {
        let taken = |candidate: &str| {
            self.files.exists(&abs(&self.cfg.root, candidate))
                || self
                    .manifest
                    .doc
                    .entries()
                    .iter()
                    .any(|e| !e.deleted && e.path == candidate)
        };
        if !taken(wanted) {
            return wanted.to_string();
        }
        let stem = wanted.strip_suffix(".md").unwrap_or(wanted);
        (2..)
            .map(|n| format!("{stem} {n}.md"))
            .find(|candidate| !taken(candidate))
            .expect("an unused name exists")
    }

    fn sync_doc(
        &mut self,
        entry: &ManifestEntry,
        transport: &dyn SyncTransport,
        remote: Option<&HashMap<DocId, u64>>,
        report: &mut SyncReport,
    ) -> Result<(), SyncError> {
        let id = entry.doc_id;
        let online = remote.is_some();
        let remote_latest = remote.and_then(|r| r.get(&id)).copied().unwrap_or(0);
        self.docs
            .entry(id)
            .or_insert_with(|| Tracked::new(FileDoc::new()));

        // A document we've never held must be fetched before local text is
        // compared against it, or we'd re-insert content the vault already has.
        let fresh = self.docs[&id].is_fresh();
        if fresh {
            if !online {
                return Ok(());
            }
            if remote_latest > 0 {
                report.pulled_updates += pull_into(
                    &self.cfg.key,
                    self.cfg.vault,
                    self.cfg.device,
                    transport,
                    id,
                    self.docs.get_mut(&id).expect("just inserted"),
                )?;
            }
        }

        let disk = match read_disk(&*self.files, &self.cfg.root, &entry.path) {
            Disk::Unreadable => {
                report.skipped_paths.push(entry.path.clone());
                return Ok(());
            }
            Disk::Missing => None,
            Disk::Text(text) => Some(text),
        };

        if let Some(text) = &disk {
            let held = &self.docs[&id];
            let differs_from_vault =
                held.local_path.is_none() && !held.doc.is_empty() && *text != held.doc.render();
            if differs_from_vault {
                let copy = self.unique_path(&conflict_copy_path(&entry.path));
                write_file(&*self.files, &self.cfg.root, &copy, text)?;
                self.create_local_doc(&copy, text);
                report.conflict_copies.push(copy);
            } else if let Some(update) = self.docs.get_mut(&id).expect("present").doc.set_text(text)
            {
                let tracked = self.docs.get_mut(&id).expect("present");
                tracked.pending.push(update);
                tracked.dirty = true;
                report.local_edits += 1;
            }
        }

        if !fresh && online && remote_latest > self.docs[&id].last_seq {
            report.pulled_updates += pull_into(
                &self.cfg.key,
                self.cfg.vault,
                self.cfg.device,
                transport,
                id,
                self.docs.get_mut(&id).expect("present"),
            )?;
        }

        let merged = self.docs[&id].doc.render();
        let current = read_disk(&*self.files, &self.cfg.root, &entry.path);
        let already_current = matches!(&current, Disk::Text(text) if *text == merged);
        // Only overwrite what we actually read; anything else changed underneath us
        // and is merged on the next pass.
        let safe_to_write = match (&current, &disk) {
            (Disk::Text(now), Some(before)) => now == before,
            (Disk::Missing, None) => true,
            _ => false,
        };
        if !already_current && safe_to_write {
            write_file(&*self.files, &self.cfg.root, &entry.path, &merged)?;
            report.files_written += 1;
        }
        if already_current || safe_to_write {
            let tracked = self.docs.get_mut(&id).expect("present");
            if tracked.local_path.as_deref() != Some(entry.path.as_str()) {
                tracked.local_path = Some(entry.path.clone());
            }
            tracked.dirty = true;
        }
        Ok(())
    }

    /// The live files and folders as ids, for translating project metadata.
    fn path_ids(&self) -> PathIds {
        let usable: Vec<ManifestEntry> = self
            .manifest
            .doc
            .entries()
            .into_iter()
            .filter(|entry| match entry.kind {
                EntryKind::Doc => is_safe_relative_path(&entry.path),
                EntryKind::Dir => is_safe_relative_dir_path(&entry.path),
            })
            .collect();
        PathIds::from_entries(&usable)
    }

    /// Syncs `.smaragd/project.json`. Local edits are captured first (as a diff against
    /// the CRDT, field by field), then remote changes are merged in and written back
    /// with every per-device field left as it was.
    ///
    /// Three ways this could destroy data are closed off explicitly: a `project.json`
    /// that doesn't parse is skipped (capturing "defaults" would delete the vault's
    /// metadata everywhere); a missing one is never captured as "everything was
    /// deleted" (it is simply restored from the vault); and joining a vault that has
    /// metadata adopts the vault's rather than pushing local defaults over it — after
    /// saving the local file to `project.json.before-sync`.
    fn sync_meta(
        &mut self,
        transport: &dyn SyncTransport,
        remote: Option<&HashMap<DocId, u64>>,
        report: &mut SyncReport,
    ) -> Result<(), SyncError> {
        let online = remote.is_some();
        let remote_latest = remote
            .and_then(|r| r.get(&DocId::PROJECT_META))
            .copied()
            .unwrap_or(0);
        let fresh =
            self.meta.last_seq == 0 && self.meta.pending.is_empty() && self.meta.doc.is_empty();
        if fresh {
            if !online {
                return Ok(());
            }
            if remote_latest > 0 {
                report.pulled_updates += pull_into(
                    &self.cfg.key,
                    self.cfg.vault,
                    self.cfg.device,
                    transport,
                    DocId::PROJECT_META,
                    &mut self.meta,
                )?;
            }
        }

        let (local_text, local_meta) = match read_disk(&*self.files, &self.cfg.root, META_PATH) {
            Disk::Missing => (None, None),
            Disk::Unreadable => {
                report.skipped_paths.push(META_PATH.to_string());
                return Ok(());
            }
            Disk::Text(text) => match serde_json::from_str::<ProjectMeta>(&text) {
                Ok(meta) => (Some(text), Some(meta)),
                Err(_) => {
                    report.skipped_paths.push(META_PATH.to_string());
                    return Ok(());
                }
            },
        };

        let ids = self.path_ids().with_previous(self.meta_paths.clone());
        let adopting = fresh && !self.meta.doc.is_empty();
        if let Some(meta) = &local_meta
            && !adopting
            && let Some(update) = self.meta.doc.write(&SyncedFields::from_meta(meta, &ids))
        {
            self.meta.pending.push(update);
            self.meta.dirty = true;
            report.local_edits += 1;
        }
        if !fresh && online && remote_latest > self.meta.last_seq {
            report.pulled_updates += pull_into(
                &self.cfg.key,
                self.cfg.vault,
                self.cfg.device,
                transport,
                DocId::PROJECT_META,
                &mut self.meta,
            )?;
        }

        let merged = self.meta.doc.read();
        if local_meta.is_none() && merged == SyncedFields::default() {
            self.remember_layout(&ids);
            return Ok(());
        }
        let mut updated = local_meta.clone().unwrap_or_default();
        if merged.into_meta(&mut updated, &ids).is_err() {
            report
                .skipped_paths
                .push("(unreadable remote project metadata)".into());
            return Ok(());
        }
        if local_meta.as_ref() == Some(&updated) {
            self.remember_layout(&ids);
            return Ok(());
        }

        // Only replace what we actually read; if the app saved in the meantime, the
        // next pass merges that instead.
        let unchanged = match (
            &local_text,
            read_disk(&*self.files, &self.cfg.root, META_PATH),
        ) {
            (Some(before), Disk::Text(now)) => *before == now,
            (None, Disk::Missing) => true,
            _ => false,
        };
        if !unchanged {
            return Ok(());
        }
        if adopting
            && let (Some(text), Some(meta)) = (&local_text, &local_meta)
            && SyncedFields::from_meta(meta, &ids) != merged
            && !self.files.exists(&abs(&self.cfg.root, META_BACKUP_PATH))
        {
            let backup = abs(&self.cfg.root, META_BACKUP_PATH);
            ensure_parent(&*self.files, &backup)?;
            self.files.write(&backup, text.as_bytes())?;
            report.conflict_copies.push(META_BACKUP_PATH.to_string());
        }
        let json = serde_json::to_string_pretty(&updated)
            .map_err(|err| SyncError::State(err.to_string()))?;
        let target = abs(&self.cfg.root, META_PATH);
        ensure_parent(&*self.files, &target)?;
        let tmp = target.with_extension("json.sync-tmp");
        self.files.write(&tmp, json.as_bytes())?;
        self.files.rename(&tmp, &target)?;
        self.remember_layout(&ids);
        report.meta_written = true;
        Ok(())
    }

    /// Records that `project.json` now uses `ids`' paths.
    fn remember_layout(&mut self, ids: &PathIds) {
        let layout = ids.snapshot();
        if layout != self.meta_paths {
            self.meta_paths = layout;
            self.meta_paths_dirty = true;
        }
    }

    fn push_all(
        &mut self,
        transport: &dyn SyncTransport,
        report: &mut SyncReport,
    ) -> Result<(), SyncError> {
        report.pushed_updates += push_from(
            &self.cfg.key,
            self.cfg.vault,
            transport,
            DocId::MANIFEST,
            &mut self.manifest,
        )?;
        report.pushed_updates += push_from(
            &self.cfg.key,
            self.cfg.vault,
            transport,
            DocId::PROJECT_META,
            &mut self.meta,
        )?;
        for (id, tracked) in &mut self.docs {
            report.pushed_updates +=
                push_from(&self.cfg.key, self.cfg.vault, transport, *id, tracked)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::store::native_store;
    use crate::sync::crypto::cheap_test_key;
    use crate::sync::fake::{MemoryServer, MemoryTransport};
    use crate::sync::state::MemoryStateStore;
    use smaragd_sync_protocol::api::KDF_SALT_LEN;

    const SALT: [u8; KDF_SALT_LEN] = [9u8; KDF_SALT_LEN];

    fn vault() -> VaultId {
        VaultId(Uuid::from_u128(0xfeed))
    }

    struct Device {
        dir: tempfile::TempDir,
        state: MemoryStateStore,
        transport: MemoryTransport,
        engine: SyncEngine,
        device: DeviceId,
        passphrase: String,
    }

    impl Device {
        fn new(server: &MemoryServer) -> Self {
            Self::with_passphrase(server, "correct horse")
        }

        fn with_passphrase(server: &MemoryServer, passphrase: &str) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let device = DeviceId(Uuid::new_v4());
            let state = MemoryStateStore::default();
            let engine = Self::build_engine(&dir, &state, device, passphrase);
            Self {
                transport: MemoryTransport::new(server, device),
                dir,
                state,
                engine,
                device,
                passphrase: passphrase.to_string(),
            }
        }

        fn build_engine(
            dir: &tempfile::TempDir,
            state: &MemoryStateStore,
            device: DeviceId,
            passphrase: &str,
        ) -> SyncEngine {
            SyncEngine::open(
                EngineConfig {
                    vault: vault(),
                    device,
                    key: cheap_test_key(passphrase, &SALT),
                    root: dir.path().to_path_buf(),
                },
                native_store(),
                Box::new(state.clone()),
            )
            .unwrap()
        }

        /// Simulates quitting and relaunching the app on this device.
        fn restart(&mut self) {
            self.engine = Self::build_engine(&self.dir, &self.state, self.device, &self.passphrase);
        }

        fn path(&self, rel: &str) -> PathBuf {
            self.dir.path().join(rel)
        }

        fn write(&self, rel: &str, text: &str) {
            let path = self.path(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }

        fn read(&self, rel: &str) -> String {
            std::fs::read_to_string(self.path(rel)).unwrap()
        }

        fn exists(&self, rel: &str) -> bool {
            self.path(rel).exists()
        }

        fn remove(&self, rel: &str) {
            std::fs::remove_file(self.path(rel)).unwrap();
        }

        fn rename(&self, from: &str, to: &str) {
            let target = self.path(to);
            std::fs::create_dir_all(target.parent().unwrap()).unwrap();
            std::fs::rename(self.path(from), target).unwrap();
        }

        fn sync(&mut self) -> SyncReport {
            self.engine.sync_once(&self.transport).unwrap()
        }

        fn try_sync(&mut self) -> Result<SyncReport, SyncError> {
            self.engine.sync_once(&self.transport)
        }

        fn write_meta(&self, value: serde_json::Value) {
            self.write(".smaragd/project.json", &value.to_string());
        }

        fn meta(&self) -> ProjectMeta {
            serde_json::from_str(&self.read(".smaragd/project.json")).unwrap()
        }

        /// Edits this device's project.json the way the app would.
        fn edit_meta(&self, change: impl FnOnce(&mut ProjectMeta)) {
            let mut meta = self.meta();
            change(&mut meta);
            self.write(
                ".smaragd/project.json",
                &serde_json::to_string_pretty(&meta).unwrap(),
            );
        }

        fn mkdir(&self, rel: &str) {
            std::fs::create_dir_all(self.path(rel)).unwrap();
        }

        fn dir_exists(&self, rel: &str) -> bool {
            self.path(rel).is_dir()
        }

        /// Every (non-hidden) folder in the project.
        fn dirs(&self) -> BTreeSet<String> {
            scan_dirs(&*native_store(), self.dir.path())
        }

        /// Every `.md` file in the project, path -> text.
        fn files(&self) -> BTreeMap<String, String> {
            let root = self.dir.path();
            scan_docs(&*native_store(), root)
                .into_iter()
                .map(|rel| {
                    let text = self.read(&rel);
                    (rel, text)
                })
                .collect()
        }
    }

    /// Syncs every device repeatedly until a full round changes nothing.
    fn converge(devices: &mut [&mut Device]) {
        let mut last_round = Vec::new();
        for _ in 0..8 {
            last_round.clear();
            for (n, device) in devices.iter_mut().enumerate() {
                last_round.push((n, device.sync()));
            }
            if last_round.iter().all(|(_, report)| report.is_quiet()) {
                return;
            }
        }
        panic!("devices never settled; last round: {last_round:#?}");
    }

    #[test]
    fn a_new_file_reaches_another_device() {
        let server = MemoryServer::default();
        let mut a = Device::new(&server);
        let mut b = Device::new(&server);

        a.write("Chapter One.md", "It was a dark night.\n");
        a.write("Notes/Idea.md", "---\nstatus: draft\n---\nWhat if?\n");
        converge(&mut [&mut a, &mut b]);

        assert_eq!(b.read("Chapter One.md"), "It was a dark night.\n");
        assert_eq!(
            b.read("Notes/Idea.md"),
            "---\nstatus: draft\n---\nWhat if?\n"
        );
        assert_eq!(a.files(), b.files());
    }

    #[test]
    fn edits_propagate_in_both_directions() {
        let server = MemoryServer::default();
        let mut a = Device::new(&server);
        let mut b = Device::new(&server);
        a.write("a.md", "one\n");
        converge(&mut [&mut a, &mut b]);

        b.write("a.md", "one\ntwo\n");
        converge(&mut [&mut a, &mut b]);
        assert_eq!(a.read("a.md"), "one\ntwo\n");

        a.write("a.md", "one\ntwo\nthree\n");
        converge(&mut [&mut a, &mut b]);
        assert_eq!(b.read("a.md"), "one\ntwo\nthree\n");
    }

    #[test]
    fn a_pass_with_nothing_to_do_is_quiet_and_own_updates_are_not_echoed() {
        let server = MemoryServer::default();
        let mut a = Device::new(&server);
        a.write("a.md", "hello\n");
        a.sync();
        let report = a.sync();
        assert!(report.is_quiet(), "{report:?}");
    }

    #[test]
    fn concurrent_edits_to_one_file_merge() {
        let server = MemoryServer::default();
        let mut a = Device::new(&server);
        let mut b = Device::new(&server);
        a.write("a.md", "Line one\n\nLine two\n");
        converge(&mut [&mut a, &mut b]);

        a.write("a.md", "Line one, edited on A\n\nLine two\n");
        b.write("a.md", "Line one\n\nLine two, edited on B\n");
        converge(&mut [&mut a, &mut b]);

        let expected = "Line one, edited on A\n\nLine two, edited on B\n";
        assert_eq!(a.read("a.md"), expected);
        assert_eq!(b.read("a.md"), expected);
    }

    #[test]
    fn frontmatter_edits_on_two_devices_merge_into_valid_yaml() {
        let server = MemoryServer::default();
        let mut a = Device::new(&server);
        let mut b = Device::new(&server);
        a.write("scene.md", "---\nstatus: draft\npov: Anna\n---\nBody\n");
        converge(&mut [&mut a, &mut b]);

        a.write("scene.md", "---\nstatus: final\npov: Anna\n---\nBody\n");
        b.write("scene.md", "---\nstatus: draft\npov: Bo\n---\nBody\n");
        converge(&mut [&mut a, &mut b]);

        assert_eq!(a.files(), b.files());
        let merged = a.read("scene.md");
        assert!(merged.contains("status: final"), "{merged}");
        assert!(merged.contains("pov: Bo"), "{merged}");
        assert!(merged.ends_with("---\nBody\n"), "{merged}");
    }

    #[test]
    fn offline_edits_queue_and_sync_once_back_online() {
        let server = MemoryServer::default();
        let mut a = Device::new(&server);
        let mut b = Device::new(&server);
        a.write("a.md", "start\n");
        converge(&mut [&mut a, &mut b]);

        a.transport.set_online(false);
        a.write("a.md", "start\nedited offline\n");
        a.write("new.md", "made offline\n");
        assert!(matches!(
            a.try_sync(),
            Err(SyncError::Transport(TransportError::Offline(_)))
        ));

        // Even a restart while offline keeps the queued work.
        a.restart();
        a.transport.set_online(true);
        converge(&mut [&mut a, &mut b]);

        assert_eq!(b.read("a.md"), "start\nedited offline\n");
        assert_eq!(b.read("new.md"), "made offline\n");
    }

    #[test]
    fn a_rename_propagates_without_losing_history() {
        let server = MemoryServer::default();
        let mut a = Device::new(&server);
        let mut b = Device::new(&server);
        a.write("Draft.md", "text\n");
        converge(&mut [&mut a, &mut b]);

        a.rename("Draft.md", "Chapters/Final.md");
        converge(&mut [&mut a, &mut b]);

        assert!(!b.exists("Draft.md"));
        assert_eq!(b.read("Chapters/Final.md"), "text\n");
    }

    #[test]
    fn a_rename_and_a_concurrent_edit_converge_on_the_renamed_edited_file() {
        let server = MemoryServer::default();
        let mut a = Device::new(&server);
        let mut b = Device::new(&server);
        a.write("old.md", "original\n");
        converge(&mut [&mut a, &mut b]);

        a.rename("old.md", "new.md");
        b.write("old.md", "original\nedited on B\n");
        converge(&mut [&mut a, &mut b]);

        assert!(!a.exists("old.md") && !b.exists("old.md"));
        assert_eq!(a.read("new.md"), "original\nedited on B\n");
        assert_eq!(b.read("new.md"), "original\nedited on B\n");
    }

    #[test]
    fn a_delete_propagates() {
        let server = MemoryServer::default();
        let mut a = Device::new(&server);
        let mut b = Device::new(&server);
        a.write("gone.md", "bye\n");
        a.write("stay.md", "hi\n");
        converge(&mut [&mut a, &mut b]);

        a.remove("gone.md");
        converge(&mut [&mut a, &mut b]);

        assert!(!b.exists("gone.md"));
        assert!(b.exists("stay.md"));
        assert!(
            a.engine
                .manifest_entries()
                .iter()
                .any(|e| e.path == "gone.md" && e.deleted)
        );
    }

    #[test]
    fn an_edit_beats_a_concurrent_delete() {
        let server = MemoryServer::default();
        let mut a = Device::new(&server);
        let mut b = Device::new(&server);
        a.write("a.md", "keep me\n");
        a.write("other.md", "x\n");
        converge(&mut [&mut a, &mut b]);

        a.remove("a.md");
        b.write("a.md", "keep me\nnew paragraph on B\n");
        converge(&mut [&mut a, &mut b]);

        assert_eq!(a.read("a.md"), "keep me\nnew paragraph on B\n");
        assert_eq!(b.read("a.md"), "keep me\nnew paragraph on B\n");
    }

    #[test]
    fn joining_with_identical_copies_neither_duplicates_nor_pushes() {
        let server = MemoryServer::default();
        let mut a = Device::new(&server);
        a.write("a.md", "same text\n");
        a.write("Dir/b.md", "---\nstatus: x\n---\nbody\n");
        a.sync();

        let mut b = Device::new(&server);
        b.write("a.md", "same text\n");
        b.write("Dir/b.md", "---\nstatus: x\n---\nbody\n");
        let report = b.sync();

        assert_eq!(b.files().len(), 2, "{:?}", b.files());
        assert_eq!(b.read("a.md"), "same text\n");
        assert!(report.conflict_copies.is_empty(), "{report:?}");
        assert_eq!(report.pushed_updates, 0, "{report:?}");
        converge(&mut [&mut a, &mut b]);
        assert_eq!(a.files(), b.files());
    }

    #[test]
    fn joining_with_a_different_copy_keeps_both_versions() {
        let server = MemoryServer::default();
        let mut a = Device::new(&server);
        a.write("a.md", "the vault's version\n");
        a.sync();

        let mut b = Device::new(&server);
        b.write("a.md", "an older copy from a backup\n");
        let report = b.sync();

        assert_eq!(b.read("a.md"), "the vault's version\n");
        assert_eq!(report.conflict_copies, vec!["a (conflict copy).md"]);
        assert_eq!(
            b.read("a (conflict copy).md"),
            "an older copy from a backup\n"
        );
        converge(&mut [&mut a, &mut b]);
        assert_eq!(a.files(), b.files());
        assert_eq!(a.files().len(), 2);
    }

    #[test]
    fn a_case_variant_of_a_vault_path_is_adopted_not_duplicated() {
        let server = MemoryServer::default();
        let mut a = Device::new(&server);
        a.write("Chapter One.md", "text\n");
        a.sync();

        let mut b = Device::new(&server);
        b.write("chapter one.md", "text\n");
        b.sync();

        assert_eq!(b.files().len(), 1, "{:?}", b.files());
        assert_eq!(b.read("Chapter One.md"), "text\n");
    }

    #[test]
    fn the_same_path_created_independently_keeps_both_files() {
        let server = MemoryServer::default();
        let mut a = Device::new(&server);
        let mut b = Device::new(&server);
        b.transport.set_online(false);

        a.write("x.md", "from A\n");
        b.write("x.md", "from B\n");
        a.sync();
        let _ = b.try_sync();
        b.transport.set_online(true);
        converge(&mut [&mut a, &mut b]);

        assert_eq!(a.files(), b.files());
        let mut contents: Vec<_> = a.files().into_values().collect();
        contents.sort();
        assert_eq!(contents, vec!["from A\n", "from B\n"]);
    }

    #[test]
    fn a_wrong_passphrase_is_reported_and_writes_nothing() {
        let server = MemoryServer::default();
        let mut a = Device::new(&server);
        a.write("secret.md", "chapter one\n");
        a.sync();

        let mut intruder = Device::with_passphrase(&server, "wrong passphrase");
        let result = intruder.try_sync();
        assert!(
            matches!(result, Err(SyncError::WrongPassphrase)),
            "{result:?}"
        );
        assert!(intruder.files().is_empty());
    }

    #[test]
    fn the_server_only_ever_sees_ciphertext() {
        let server = MemoryServer::default();
        let mut a = Device::new(&server);
        a.write("Chapter One.md", "TOPSECRETMANUSCRIPTTEXT\n");
        a.sync();

        let blobs = server.all_blobs();
        assert!(!blobs.is_empty());
        for blob in blobs {
            let haystack = String::from_utf8_lossy(&blob);
            assert!(!haystack.contains("TOPSECRET"));
            assert!(!haystack.contains("Chapter One"));
        }
    }

    #[test]
    fn restarting_resumes_without_duplicating_anything() {
        let server = MemoryServer::default();
        let mut a = Device::new(&server);
        let mut b = Device::new(&server);
        a.write("a.md", "one\n");
        converge(&mut [&mut a, &mut b]);

        b.restart();
        let report = b.sync();
        assert!(report.is_quiet(), "{report:?}");

        b.write("a.md", "one\ntwo\n");
        b.sync();
        a.sync();
        assert_eq!(a.read("a.md"), "one\ntwo\n");
    }

    #[test]
    fn unsafe_manifest_paths_from_a_peer_are_ignored() {
        let server = MemoryServer::default();
        let mut victim = Device::new(&server);
        victim.write("real.md", "fine\n");
        victim.sync();

        // A compromised device (it knows the vault key) publishes hostile paths.
        let evil_device = DeviceId(Uuid::new_v4());
        let evil = MemoryTransport::new(&server, evil_device);
        let key = cheap_test_key("correct horse", &SALT);
        let mut manifest = ManifestDoc::new();
        for (n, path) in ["../escape.md", "/etc/cron.d/x.md", "a/../../b.md"]
            .into_iter()
            .enumerate()
        {
            let update = manifest.add(
                DocId(Uuid::from_u128(500 + n as u128)),
                path,
                EntryKind::Doc,
            );
            let sealed = key.seal(vault(), DocId::MANIFEST, &update).unwrap();
            evil.push(DocId::MANIFEST, &sealed).unwrap();
        }

        let report = victim.sync();
        assert_eq!(report.skipped_paths.len(), 3, "{report:?}");
        assert!(
            !victim
                .dir
                .path()
                .parent()
                .unwrap()
                .join("escape.md")
                .exists()
        );
        assert_eq!(victim.files().len(), 1);
    }

    #[test]
    fn a_missing_project_folder_is_an_error_not_a_mass_delete() {
        let server = MemoryServer::default();
        let mut a = Device::new(&server);
        let mut b = Device::new(&server);
        a.write("a.md", "1\n");
        a.write("b.md", "2\n");
        converge(&mut [&mut a, &mut b]);

        std::fs::remove_dir_all(a.dir.path()).unwrap();
        assert!(matches!(a.try_sync(), Err(SyncError::ProjectMissing(_))));
        b.sync();
        assert_eq!(b.files().len(), 2);
    }

    #[test]
    fn an_emptied_project_folder_is_restored_not_synced_as_a_mass_delete() {
        let server = MemoryServer::default();
        let mut a = Device::new(&server);
        let mut b = Device::new(&server);
        for name in ["a.md", "b.md", "c.md"] {
            a.write(name, &format!("{name}\n"));
        }
        converge(&mut [&mut a, &mut b]);

        for name in ["a.md", "b.md", "c.md"] {
            a.remove(name);
        }
        let report = a.sync();
        assert_eq!(report.deletions_held_back, 3, "{report:?}");
        assert_eq!(a.files().len(), 3, "files should be restored");
        b.sync();
        assert_eq!(b.files().len(), 3);
    }

    #[test]
    fn a_non_utf8_file_is_skipped_never_treated_as_deleted() {
        let server = MemoryServer::default();
        let mut a = Device::new(&server);
        a.write("a.md", "fine\n");
        a.write("b.md", "also fine\n");
        a.sync();

        std::fs::write(a.path("a.md"), [0xff, 0xfe, 0x00]).unwrap();
        let report = a.sync();
        assert_eq!(report.files_tombstoned, 0, "{report:?}");
        assert_eq!(report.skipped_paths, vec!["a.md"]);
        assert!(
            a.engine
                .manifest_entries()
                .iter()
                .all(|entry| !entry.deleted)
        );
    }

    #[test]
    fn a_new_device_bootstraps_from_a_compaction_snapshot() {
        let server = MemoryServer::default();
        let mut a = Device::new(&server);
        for n in 1..=4 {
            a.write("long.md", &format!("revision {n}\n"));
            a.sync();
        }
        let id = a
            .engine
            .manifest_entries()
            .into_iter()
            .find(|e| e.path == "long.md")
            .unwrap()
            .doc_id;
        assert!(server.update_count(id) >= 4);

        // What the client-driven compaction will do: seal the merged state and let
        // the server drop every update it covers.
        let latest = a
            .transport
            .list_docs()
            .unwrap()
            .into_iter()
            .find(|d| d.doc_id == id)
            .unwrap()
            .latest_seq;
        let blob = a
            .engine
            .cfg
            .key
            .seal(vault(), id, &a.engine.docs[&id].doc.encode_state())
            .unwrap();
        a.transport
            .put_snapshot(
                id,
                &smaragd_sync_protocol::api::Snapshot {
                    upto_seq: latest,
                    blob,
                },
            )
            .unwrap();
        assert_eq!(server.update_count(id), 0);

        let mut b = Device::new(&server);
        converge(&mut [&mut a, &mut b]);
        assert_eq!(b.read("long.md"), "revision 4\n");

        // Edits after the snapshot still flow.
        b.write("long.md", "revision 5\n");
        converge(&mut [&mut a, &mut b]);
        assert_eq!(a.read("long.md"), "revision 5\n");
    }

    #[test]
    fn empty_and_nested_folders_reach_another_device() {
        let server = MemoryServer::default();
        let mut a = Device::new(&server);
        let mut b = Device::new(&server);
        a.mkdir("Empty");
        a.mkdir("World/Places");
        a.write("World/Places/Oslo.md", "fjord\n");
        converge(&mut [&mut a, &mut b]);

        assert!(b.dir_exists("Empty"));
        assert!(b.dir_exists("World/Places"));
        assert_eq!(b.read("World/Places/Oslo.md"), "fjord\n");
        assert_eq!(a.dirs(), b.dirs());
    }

    #[test]
    fn a_renamed_folder_moves_everywhere_and_the_old_one_disappears() {
        let server = MemoryServer::default();
        let mut a = Device::new(&server);
        let mut b = Device::new(&server);
        a.write("Notes/one.md", "1\n");
        a.write("Notes/Deep/two.md", "2\n");
        converge(&mut [&mut a, &mut b]);

        a.rename("Notes/one.md", "Ideas/one.md");
        a.rename("Notes/Deep/two.md", "Ideas/Deep/two.md");
        std::fs::remove_dir_all(a.path("Notes")).unwrap();
        converge(&mut [&mut a, &mut b]);

        assert_eq!(a.files(), b.files());
        assert_eq!(a.dirs(), b.dirs());
        assert!(!b.dir_exists("Notes"), "{:?}", b.dirs());
        assert!(b.dir_exists("Ideas/Deep"));
        // The renamed folder is the same folder, not a delete plus a create.
        let live_dirs = |d: &Device| {
            d.engine
                .manifest_entries()
                .into_iter()
                .filter(|e| e.kind == EntryKind::Dir && !e.deleted)
                .count()
        };
        assert_eq!(live_dirs(&a), 2);
        assert_eq!(live_dirs(&b), 2);
    }

    #[test]
    fn a_deleted_folder_and_its_files_are_removed_everywhere() {
        let server = MemoryServer::default();
        let mut a = Device::new(&server);
        let mut b = Device::new(&server);
        a.write("Draft/scene.md", "text\n");
        a.write("keep.md", "k\n");
        converge(&mut [&mut a, &mut b]);

        std::fs::remove_dir_all(a.path("Draft")).unwrap();
        converge(&mut [&mut a, &mut b]);

        assert!(!b.dir_exists("Draft"));
        assert!(!b.exists("Draft/scene.md"));
        assert!(b.exists("keep.md"));
    }

    #[test]
    fn joining_with_the_same_folders_adopts_them_without_duplicating() {
        let server = MemoryServer::default();
        let mut a = Device::new(&server);
        a.write("World/Places/Oslo.md", "fjord\n");
        a.mkdir("Research");
        a.sync();

        let mut b = Device::new(&server);
        b.write("World/Places/Oslo.md", "fjord\n");
        b.mkdir("Research");
        converge(&mut [&mut a, &mut b]);

        let live = |d: &Device| {
            d.engine
                .manifest_entries()
                .into_iter()
                .filter(|e| e.kind == EntryKind::Dir && !e.deleted)
                .count()
        };
        assert_eq!(live(&a), 3, "World, World/Places, Research");
        assert_eq!(live(&b), 3);
    }

    #[test]
    fn the_same_folder_created_on_two_devices_becomes_one() {
        let server = MemoryServer::default();
        let mut a = Device::new(&server);
        let mut b = Device::new(&server);
        b.transport.set_online(false);
        a.mkdir("Shared");
        b.mkdir("Shared");
        a.sync();
        let _ = b.try_sync();
        b.transport.set_online(true);
        converge(&mut [&mut a, &mut b]);

        let live: Vec<_> = a
            .engine
            .manifest_entries()
            .into_iter()
            .filter(|e| e.kind == EntryKind::Dir && !e.deleted)
            .collect();
        assert_eq!(live.len(), 1, "{live:?}");
        assert!(a.dir_exists("Shared") && b.dir_exists("Shared"));
    }

    #[test]
    fn hostile_folder_names_from_a_peer_are_ignored() {
        let server = MemoryServer::default();
        let mut victim = Device::new(&server);
        victim.write("real.md", "fine\n");
        victim.sync();

        let evil = MemoryTransport::new(&server, DeviceId(Uuid::new_v4()));
        let key = cheap_test_key("correct horse", &SALT);
        let mut manifest = ManifestDoc::new();
        for (n, path) in [".git", "../outside", "/abs", ".smaragd/plugins"]
            .into_iter()
            .enumerate()
        {
            let update = manifest.add(
                DocId(Uuid::from_u128(900 + n as u128)),
                path,
                EntryKind::Dir,
            );
            let sealed = key.seal(vault(), DocId::MANIFEST, &update).unwrap();
            evil.push(DocId::MANIFEST, &sealed).unwrap();
        }
        let report = victim.sync();
        assert_eq!(report.skipped_paths.len(), 4, "{report:?}");
        assert!(!victim.dir_exists(".git") && !victim.dir_exists(".smaragd"));
    }

    #[test]
    fn folder_state_survives_a_restart() {
        let server = MemoryServer::default();
        let mut a = Device::new(&server);
        a.mkdir("Kept");
        a.write("Kept/x.md", "x\n");
        a.sync();
        a.restart();
        let report = a.sync();
        assert!(report.is_quiet(), "{report:?}");
        assert!(a.dir_exists("Kept"));
    }

    #[test]
    fn dir_rename_pairs_come_from_the_parts_of_a_path_that_changed() {
        let pairs = |renames: &[(&str, &str)]| {
            dir_rename_pairs(
                &renames
                    .iter()
                    .map(|(a, b)| (a.to_string(), b.to_string()))
                    .collect::<Vec<_>>(),
            )
        };
        assert_eq!(
            pairs(&[("A/f.md", "B/f.md")]),
            vec![("A".into(), "B".into())]
        );
        assert_eq!(
            pairs(&[("A/S/f.md", "B/S/f.md")]),
            vec![("A".into(), "B".into())]
        );
        assert!(pairs(&[("A/f.md", "A/g.md")]).is_empty(), "a file rename");
        assert!(
            pairs(&[("f.md", "B/f.md")]).is_empty(),
            "moved out of the root"
        );
        assert_eq!(
            map_through_pairs("A/S", &[("A".into(), "B".into())]),
            Some("B/S".into())
        );
        assert_eq!(
            map_through_pairs("Other", &[("A".into(), "B".into())]),
            None
        );
    }

    fn card(n: u128, cause: &str) -> serde_json::Value {
        serde_json::json!({
            "id": Uuid::from_u128(n).to_string(), "scene_number": "1", "alpha_point": "",
            "subplot_tags": [], "cause": cause, "effect": "e", "realization": "r", "and_so": "a",
        })
    }

    fn base_meta() -> serde_json::Value {
        serde_json::json!({
            "version": 1,
            "node_order": { "": ["Draft"], "Draft": ["Ch1.md", "Ch2.md"] },
            "folder_roles": { "Draft": "Manuscript" },
            "folder_meta": { "Draft": { "status": "draft" } },
            "story_cards": [card(1, "first")],
            "logline": "A girl and a fjord.",
            "git_enabled": true,
            "plugins_enabled": true,
            "session_baseline_words": 4242,
        })
    }

    /// Two synced devices that both have the Draft folder and a project.json.
    fn synced_pair(server: &MemoryServer) -> (Device, Device) {
        let mut a = Device::new(server);
        let mut b = Device::new(server);
        a.write("Draft/Ch1.md", "one\n");
        a.write("Draft/Ch2.md", "two\n");
        a.write_meta(base_meta());
        b.write_meta(serde_json::json!({ "version": 1, "node_order": {} }));
        converge(&mut [&mut a, &mut b]);
        (a, b)
    }

    #[test]
    fn project_metadata_syncs_but_per_device_settings_stay_put() {
        let server = MemoryServer::default();
        let mut a = Device::new(&server);
        a.write("Draft/Ch1.md", "one\n");
        a.write("Draft/Ch2.md", "two\n");
        a.write_meta(base_meta());
        a.sync();

        // B has no project.json at all yet: the vault's is written for it.
        let mut b = Device::new(&server);
        converge(&mut [&mut a, &mut b]);
        let meta = b.meta();
        assert_eq!(meta.logline, "A girl and a fjord.");
        assert_eq!(meta.story_cards.len(), 1);
        assert_eq!(meta.node_order["Draft"], vec!["Ch1.md", "Ch2.md"]);
        assert!(meta.folder_roles.contains_key("Draft"));
        // A's per-device state did not follow.
        assert!(!meta.git_enabled);
        assert!(!meta.plugins_enabled, "plugin consent must stay local");
        assert_eq!(meta.session_baseline_words, 0);

        // B turns plugins on for itself; that never reaches A.
        b.edit_meta(|m| m.plugins_enabled = true);
        b.edit_meta(|m| m.logline = "Edited on B.".into());
        converge(&mut [&mut a, &mut b]);
        assert_eq!(a.meta().logline, "Edited on B.");
        assert!(a.meta().plugins_enabled, "A's own value is unchanged");
        assert_eq!(a.meta().session_baseline_words, 4242);
        let a_again = a.meta();
        assert!(a_again.git_enabled);
    }

    #[test]
    fn concurrent_metadata_edits_on_two_devices_merge() {
        let server = MemoryServer::default();
        let (mut a, mut b) = synced_pair(&server);

        a.edit_meta(|m| {
            m.logline = "A girl, a fjord and a storm.".into();
            m.status_colors.insert("final".into(), "#00ff00".into());
        });
        b.edit_meta(|m| {
            m.book_title = Some("Fjord".into());
            m.synopsis = "Once upon a time.".into();
        });
        converge(&mut [&mut a, &mut b]);

        for device in [&a, &b] {
            let meta = device.meta();
            assert_eq!(meta.logline, "A girl, a fjord and a storm.");
            assert_eq!(meta.book_title.as_deref(), Some("Fjord"));
            assert_eq!(meta.synopsis, "Once upon a time.");
            assert!(meta.status_colors.contains_key("final"));
        }
    }

    #[test]
    fn a_folder_rename_keeps_its_metadata_and_a_concurrent_edit_to_it() {
        let server = MemoryServer::default();
        let (mut a, mut b) = synced_pair(&server);

        // A renames the folder (the app rewrites the path keys as it does so)...
        a.rename("Draft/Ch1.md", "Manuscript/Ch1.md");
        a.rename("Draft/Ch2.md", "Manuscript/Ch2.md");
        std::fs::remove_dir_all(a.path("Draft")).unwrap();
        a.edit_meta(|m| {
            let mut value = serde_json::to_value(&*m).unwrap();
            let text = value.to_string().replace("\"Draft\"", "\"Manuscript\"");
            value = serde_json::from_str(&text).unwrap();
            *m = serde_json::from_value(value).unwrap();
        });
        // ...while B, still using the old name, sets the folder's status.
        b.edit_meta(|m| {
            m.folder_meta.get_mut("Draft").unwrap().status = Some("final".into());
        });
        converge(&mut [&mut a, &mut b]);

        for device in [&a, &b] {
            let meta = device.meta();
            assert!(
                !meta.node_order.contains_key("Draft"),
                "{:?}",
                meta.node_order
            );
            assert_eq!(meta.node_order["Manuscript"], vec!["Ch1.md", "Ch2.md"]);
            assert!(meta.folder_roles.contains_key("Manuscript"));
            assert_eq!(
                meta.folder_meta["Manuscript"].status.as_deref(),
                Some("final"),
                "B's edit follows the folder to its new name"
            );
        }
        assert_eq!(a.files(), b.files());
    }

    #[test]
    fn joining_a_vault_with_metadata_keeps_a_backup_of_the_local_project_json() {
        let server = MemoryServer::default();
        let mut a = Device::new(&server);
        a.write("a.md", "x\n");
        a.write_meta(
            serde_json::json!({ "version": 1, "node_order": {}, "logline": "the vault's" }),
        );
        a.sync();

        let mut b = Device::new(&server);
        b.write("a.md", "x\n");
        b.write_meta(serde_json::json!({
            "version": 1, "node_order": {}, "logline": "b's own", "story_cards": [card(9, "mine")],
        }));
        let report = b.sync();

        assert_eq!(b.meta().logline, "the vault's");
        assert!(b.meta().story_cards.is_empty());
        assert!(report.meta_written);
        assert!(
            report
                .conflict_copies
                .iter()
                .any(|c| c.ends_with("project.json.before-sync"))
        );
        let backup = b.read(".smaragd/project.json.before-sync");
        assert!(
            backup.contains("b's own") && backup.contains("mine"),
            "{backup}"
        );
    }

    #[test]
    fn a_corrupt_project_json_is_left_alone_and_never_wipes_the_vault() {
        let server = MemoryServer::default();
        let (mut a, mut b) = synced_pair(&server);

        a.write(".smaragd/project.json", "{ this is not json");
        let report = a.sync();
        assert!(
            report
                .skipped_paths
                .contains(&".smaragd/project.json".to_string()),
            "{report:?}"
        );
        assert_eq!(a.read(".smaragd/project.json"), "{ this is not json");

        b.sync();
        assert_eq!(
            b.meta().story_cards.len(),
            1,
            "the vault's metadata is intact"
        );
        assert_eq!(b.meta().logline, "A girl and a fjord.");
    }

    #[test]
    fn a_deleted_project_json_is_restored_from_the_vault_not_synced_as_empty() {
        let server = MemoryServer::default();
        let (mut a, mut b) = synced_pair(&server);

        a.remove(".smaragd/project.json");
        converge(&mut [&mut a, &mut b]);

        assert_eq!(a.meta().logline, "A girl and a fjord.");
        assert_eq!(a.meta().story_cards.len(), 1);
        assert_eq!(b.meta().story_cards.len(), 1);
    }

    #[test]
    fn metadata_state_survives_a_restart_and_a_quiet_pass_stays_quiet() {
        let server = MemoryServer::default();
        let (mut a, _b) = synced_pair(&server);
        a.restart();
        let report = a.sync();
        assert!(report.is_quiet(), "{report:?}");
    }

    #[test]
    fn three_devices_converge_after_scattered_edits() {
        let server = MemoryServer::default();
        let mut a = Device::new(&server);
        let mut b = Device::new(&server);
        let mut c = Device::new(&server);
        a.write("one.md", "1\n");
        a.write("two.md", "2\n");
        converge(&mut [&mut a, &mut b, &mut c]);

        a.write("one.md", "1 from A\n");
        b.write("two.md", "2 from B\n");
        c.write("three.md", "3 from C\n");
        b.rename("one.md", "uno.md");
        converge(&mut [&mut a, &mut b, &mut c]);

        assert_eq!(a.files(), b.files());
        assert_eq!(b.files(), c.files());
        assert_eq!(c.read("three.md"), "3 from C\n");
        assert_eq!(a.read("two.md"), "2 from B\n");
    }
}
