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
use super::manifest::{ManifestDoc, ManifestEntry, is_safe_relative_path};
use super::state::StateStore;
use super::transport::{SyncTransport, TransportError};
use crate::project::store::{ProjectStore, TreeEntryKind};

const MANIFEST_KEY: &str = "manifest";

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
    docs: BTreeMap<DocId, Tracked<FileDoc>>,
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
        Ok(Self {
            cfg,
            files,
            state,
            manifest,
            docs,
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
        for (id, tracked) in &mut self.docs {
            if tracked.dirty {
                self.state.put(&doc_key(*id), &persist_bytes(tracked))?;
                tracked.dirty = false;
            }
        }
        Ok(())
    }

    fn queue_manifest_update(&mut self, update: Option<Vec<u8>>) {
        if let Some(update) = update {
            self.manifest.pending.push(update);
            self.manifest.dirty = true;
        }
    }

    /// Live entries with usable paths; unsafe paths are reported and ignored.
    fn live_entries(&self, report: &mut SyncReport) -> Vec<ManifestEntry> {
        let mut out = Vec::new();
        for entry in self.manifest.doc.entries() {
            if entry.deleted {
                continue;
            }
            if is_safe_relative_path(&entry.path) {
                out.push(entry);
            } else if !report.skipped_paths.contains(&entry.path) {
                report.skipped_paths.push(entry.path);
            }
        }
        out
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
        missing.retain(|(id, _)| {
            let Some(tracked) = self.docs.get(id) else {
                return true;
            };
            let rendered = tracked.doc.render();
            match new_files.iter().position(|p| new_texts[p] == rendered) {
                Some(position) => {
                    renames.push((*id, new_files.remove(position)));
                    false
                }
                None => true,
            }
        });
        for (id, new_path) in renames {
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

        self.apply_tombstones(&scan, report)
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
        let update = self.manifest.doc.add(id, path);
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
        for _ in 0..8 {
            let mut quiet = true;
            for device in devices.iter_mut() {
                quiet &= device.sync().is_quiet();
            }
            if quiet {
                return;
            }
        }
        panic!("devices never settled");
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
            let update = manifest.add(DocId(Uuid::from_u128(500 + n as u128)), path);
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
