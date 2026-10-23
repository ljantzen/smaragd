//! Binary files — images, PDFs and every other non-Markdown file in the project —
//! synced whole rather than merged, and only while the project has
//! `ProjectMeta::sync_files` switched on.
//!
//! # On the wire
//!
//! A binary file is a manifest entry of kind [`EntryKind::File`] plus a document whose
//! update log holds sealed [`Record`]s. One version of the file is a [`Header`] (a fresh
//! version id, the ids of the versions it replaced, its size, BLAKE3 hash and chunk
//! count) followed by its chunks of at most [`CHUNK_BYTES`] each, so a file of any size
//! travels in blobs the server accepts. Two devices uploading at once simply interleave
//! their records; each chunk names its version.
//!
//! # Which version wins
//!
//! The *complete* version (every chunk present, size and hash verified) whose header
//! has the highest sequence number. Every device reads the same log, so every device
//! picks the same winner, and a version that is still uploading is invisible until its
//! last chunk lands.
//!
//! Sequence numbers come from the server, which is untrusted and keeps every blob it
//! was ever sent. Replaying an old version's sealed records at new sequence numbers
//! would make it "newest", so a version the one on disk here descends from (its
//! `lineage`) never wins: a device never re-uploads a version id, so seeing an
//! ancestor again can only be a replay. That covers the last [`LINEAGE_LEN`]
//! generations. What the server can still do is withhold records, or pick which of
//! two *concurrent* versions arrives last — as it could when they were uploaded.
//!
//! # Conflicts
//!
//! Nothing is silently lost. A device that adopts a winner which doesn't descend from
//! what it had (per the winner's `lineage`) keeps its own copy as
//! `name (conflict copy).ext` — a new file that syncs like any other — if it changed the
//! file since, or made the version that lost. Only that device does so, so exactly one
//! copy appears.
//!
//! # Space
//!
//! Once a version wins, everything logged before its header is dead. The device that
//! notices replaces it with a tiny [`Record::Trimmed`] snapshot, so the server frees the
//! old versions. While a new version uploads the vault briefly needs room for both.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io;
use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use smaragd_sync_protocol::DocId;
use smaragd_sync_protocol::api::Snapshot;
use uuid::Uuid;

use super::{SyncEngine, SyncError, SyncReport, abs, ensure_parent, move_file};
use crate::project::store::ProjectStore;
use crate::sync::manifest::{EntryKind, ManifestEntry, is_safe_relative_file_path};
use crate::sync::transport::SyncTransport;

/// Largest piece of a file sent as one update: comfortably under the server's
/// per-blob limit once sealed and framed.
pub(super) const CHUNK_BYTES: usize = 4 * 1024 * 1024;
/// The largest file this device will download or upload when the server advertises no
/// limit. A file is assembled in memory, so something has to bound it; this matches the
/// server's default vault quota.
pub(super) const MAX_FILE_BYTES_CEILING: u64 = 1024 * 1024 * 1024;

/// The file-size limit this device actually applies: the server's, capped at
/// [`MAX_FILE_BYTES_CEILING`].
pub(super) fn effective_file_limit(advertised: Option<u64>) -> u64 {
    advertised.map_or(MAX_FILE_BYTES_CEILING, |limit| {
        limit.min(MAX_FILE_BYTES_CEILING)
    })
}

/// How many ancestor versions a header lists, for telling a successor from a
/// concurrent replacement even after the versions in between were trimmed away.
const LINEAGE_LEN: usize = 32;

/// A file's (size, modification time), as `ProjectStore::file_stamp` reports it.
type Stamp = (u64, u128);
/// A BLAKE3 content hash.
type Hash = [u8; 32];

pub(super) fn file_key(id: DocId) -> String {
    format!("file/{id}")
}

/// One version's description, logged before its chunks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Header {
    version: Uuid,
    /// The versions this one replaced, most recent first (at most [`LINEAGE_LEN`]).
    lineage: Vec<Uuid>,
    size: u64,
    chunks: u32,
    hash: Hash,
}

/// What a binary file's update log holds (plaintext; each is sealed on its own).
#[derive(Debug, Serialize, Deserialize)]
enum Record {
    Header(Header),
    Chunk {
        version: Uuid,
        index: u32,
        data: Vec<u8>,
    },
    /// A snapshot marker: nothing logged before it is needed any more.
    Trimmed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Synced {
    header: Header,
    header_seq: u64,
    /// This device made the version (rather than downloading it).
    mine: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Upload {
    header: Header,
    header_seq: u64,
    next_chunk: u32,
}

/// This device's bookkeeping for one binary file (persisted under [`file_key`]).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(super) struct FileState {
    /// Where this device has the file, `None` if it isn't materialized here.
    local_path: Option<String>,
    /// The version whose content is on disk here.
    synced: Option<Synced>,
    /// Records up to here are settled; later ones may still belong to a version
    /// that is being uploaded, so they are read again.
    scan_from: u64,
    /// The highest sequence number already read, so nothing is re-read until the
    /// log grows.
    seen_upto: u64,
    /// The server's snapshot already covers records up to here.
    trimmed_upto: u64,
    /// The file's (size, mtime) when it was last hashed, and that hash.
    stamp: Option<Stamp>,
    hash: Option<Hash>,
    /// A version this device is part-way through uploading.
    upload: Option<Upload>,
    #[serde(skip)]
    pub(super) dirty: bool,
}

impl FileState {
    pub(super) fn load(bytes: &[u8]) -> Result<Self, SyncError> {
        postcard::from_bytes(bytes).map_err(|err| SyncError::State(err.to_string()))
    }

    pub(super) fn to_bytes(&self) -> Vec<u8> {
        postcard::to_stdvec(self).expect("file state always serializes")
    }

    fn synced_hash(&self) -> Option<Hash> {
        self.synced.as_ref().map(|s| s.header.hash)
    }
}

/// The non-Markdown files under `root` that may sync, as `/`-separated relative paths.
fn scan_other_files(files: &dyn ProjectStore, root: &Path) -> BTreeSet<String> {
    files
        .list_other_files(root)
        .into_iter()
        .filter_map(|path| super::rel_key(root, &path))
        .filter(|rel| is_safe_relative_file_path(rel))
        .collect()
}

/// `dir/name.ext` -> (`dir/name`, `.ext`); a name without an extension keeps it all.
fn split_ext(path: &str) -> (&str, &str) {
    let name_start = path.rfind('/').map_or(0, |i| i + 1);
    match path[name_start..].rfind('.') {
        Some(dot) if dot > 0 => path.split_at(name_start + dot),
        _ => (path, ""),
    }
}

fn conflict_copy_path(path: &str) -> String {
    let (stem, ext) = split_ext(path);
    format!("{stem} (conflict copy){ext}")
}

fn collision_path(path: &str, id: DocId) -> String {
    let (stem, ext) = split_ext(path);
    let short: String = id.as_uuid().simple().to_string().chars().take(8).collect();
    format!("{stem} (conflict {short}){ext}")
}

/// Hashes the file at `rel`, reusing the cached hash while its size and mtime are
/// unchanged. `None` if there's no file.
fn current_hash(
    files: &dyn ProjectStore,
    root: &Path,
    rel: &str,
    state: &mut FileState,
) -> io::Result<Option<Hash>> {
    let path = abs(root, rel);
    if !files.exists(&path) {
        return Ok(None);
    }
    let stamp = files.file_stamp(&path);
    if stamp.is_some() && stamp == state.stamp && state.hash.is_some() {
        return Ok(state.hash);
    }
    let hash: Hash = blake3::hash(&files.read_bytes(&path)?).into();
    state.stamp = stamp;
    state.hash = Some(hash);
    state.dirty = true;
    Ok(Some(hash))
}

/// Writes `bytes` to `rel` through a temporary file, so a reader never sees half a file.
fn write_bytes(files: &dyn ProjectStore, root: &Path, rel: &str, bytes: &[u8]) -> io::Result<()> {
    let path = abs(root, rel);
    ensure_parent(files, &path)?;
    let tmp = abs(root, &format!("{rel}.sync-tmp"));
    files.write(&tmp, bytes)?;
    files.rename(&tmp, &path)
}

/// What a stretch of a file's log amounts to.
struct Reading {
    /// The winning version, its header's sequence number and its verified content.
    winner: Option<(Header, u64, Vec<u8>)>,
    /// Where the next read should start (see `FileState::scan_from`).
    scan_from: u64,
    /// Records from other devices that were read.
    records: usize,
    /// A version was left alone for being over the size limit.
    too_large: bool,
}

/// Reads `id`'s log from `since` on (every page) and works out the winner. `current` is
/// the version on disk here; neither it nor any of its ancestors can win (see the
/// module docs on replays).
///
/// Only chunks that can matter are kept in memory: those of a version whose header was
/// read, that isn't superseded, and whose size is within `limit` (see
/// [`effective_file_limit`]) and consistent with its chunk count. A version over the
/// limit is skipped and flagged. So the log — which a peer or the server could have
/// filled with anything sealed under the vault key — can't make this device buffer more
/// than about `limit` per live candidate version.
fn read_log(
    engine: &SyncEngine,
    transport: &dyn SyncTransport,
    id: DocId,
    since: u64,
    current: Option<&Header>,
    limit: u64,
) -> Result<(Reading, u64), SyncError> {
    let (key, vault) = (&engine.cfg.key, engine.cfg.vault);
    let superseded = |header: &Header| current.is_some_and(|c| c.lineage.contains(&header.version));
    let mut headers: Vec<(u64, Header)> = Vec::new();
    let mut wanted: HashMap<Uuid, u32> = HashMap::new();
    let mut chunks: HashMap<Uuid, BTreeMap<u32, Vec<u8>>> = HashMap::new();
    let (mut last, mut records, mut too_large) = (since, 0, false);
    loop {
        let page = transport.pull(id, last)?;
        if let Some(snapshot) = page.snapshot {
            key.open(vault, id, &snapshot.blob)?;
            last = last.max(snapshot.upto_seq);
        }
        let progressed = !page.updates.is_empty();
        for update in page.updates {
            let plain = key.open(vault, id, &update.blob)?;
            if update.device_id != engine.cfg.device {
                records += 1;
            }
            // A record this version doesn't understand is skipped, not fatal.
            match postcard::from_bytes::<Record>(&plain) {
                Ok(Record::Header(header)) => {
                    let consistent =
                        u64::from(header.chunks) == header.size.div_ceil(CHUNK_BYTES as u64);
                    if header.size > limit {
                        too_large = true;
                    } else if consistent && !superseded(&header) {
                        wanted.insert(header.version, header.chunks);
                        headers.push((update.seq, header));
                    }
                }
                Ok(Record::Chunk {
                    version,
                    index,
                    data,
                }) => {
                    if wanted.get(&version).is_some_and(|&count| index < count)
                        && data.len() <= CHUNK_BYTES
                    {
                        chunks.entry(version).or_default().insert(index, data);
                    }
                }
                Ok(Record::Trimmed) | Err(_) => {}
            }
            last = update.seq;
        }
        if !page.more || !progressed {
            break;
        }
    }

    let complete: BTreeSet<Uuid> = headers
        .iter()
        .map(|(_, header)| header)
        .filter(|header| {
            chunks
                .get(&header.version)
                .map_or(header.chunks == 0, |got| {
                    (0..header.chunks).all(|i| got.contains_key(&i))
                })
        })
        .map(|header| header.version)
        .collect();
    let mut winner = None;
    for (seq, header) in headers.iter().rev() {
        if !complete.contains(&header.version) {
            continue;
        }
        let got = chunks.remove(&header.version).unwrap_or_default();
        let content: Vec<u8> = got
            .into_values()
            .take(header.chunks as usize)
            .flatten()
            .collect();
        // A version whose content doesn't check out is ignored, never written.
        if content.len() as u64 == header.size && blake3::hash(&content) == header.hash {
            winner = Some((header.clone(), *seq, content));
            break;
        }
    }
    // Keep re-reading from the first version after the winner that is still uploading.
    let after = winner.as_ref().map_or(0, |(_, seq, _)| *seq);
    let uploading = headers
        .iter()
        .filter(|(seq, header)| *seq > after && !complete.contains(&header.version))
        .map(|(seq, _)| *seq)
        .min();
    let scan_from = uploading.map_or(last, |seq| seq - 1);
    Ok((
        Reading {
            winner,
            scan_from,
            records,
            too_large,
        },
        last,
    ))
}

impl SyncEngine {
    /// Whether this project syncs its non-Markdown files, per its own `project.json`.
    pub(super) fn sync_files_enabled(&self) -> bool {
        self.files
            .read_to_string(&abs(&self.cfg.root, super::META_PATH))
            .ok()
            .and_then(|text| serde_json::from_str::<crate::project::ProjectMeta>(&text).ok())
            .is_some_and(|meta| meta.sync_files)
    }

    fn file_path_taken(&self, candidate: &str) -> bool {
        self.files.exists(&abs(&self.cfg.root, candidate))
            || self
                .manifest
                .doc
                .entries()
                .iter()
                .any(|e| !e.deleted && e.path.eq_ignore_ascii_case(candidate))
    }

    fn unique_file_path(&self, wanted: &str) -> String {
        if !self.file_path_taken(wanted) {
            return wanted.to_string();
        }
        let (stem, ext) = split_ext(wanted);
        (2..)
            .map(|n| format!("{stem} {n}{ext}"))
            .find(|candidate| !self.file_path_taken(candidate))
            .expect("an unused name exists")
    }

    /// Registers a file that exists at `path` as a new binary document.
    fn add_local_file(&mut self, path: &str, stamp: Option<Stamp>, hash: Hash) {
        let id = DocId(Uuid::new_v4());
        let update = self.manifest.doc.add(id, path, EntryKind::File);
        self.queue_manifest_update(Some(update));
        self.bins.insert(
            id,
            FileState {
                local_path: Some(path.to_string()),
                stamp,
                hash: Some(hash),
                dirty: true,
                ..FileState::default()
            },
        );
    }

    /// Two live files at the same path (each device registered its own): the larger id
    /// moves to a deterministic name, so every device agrees.
    fn fix_file_collisions(&mut self, report: &mut SyncReport) {
        let mut by_path: BTreeMap<String, Vec<DocId>> = BTreeMap::new();
        for entry in self.live_of_kind(EntryKind::File, report) {
            by_path
                .entry(entry.path.to_lowercase())
                .or_default()
                .push(entry.doc_id);
        }
        for mut ids in by_path.into_values().filter(|ids| ids.len() > 1) {
            ids.sort();
            for loser in ids.split_off(1) {
                if let Some(entry) = self.manifest.doc.get(loser) {
                    let update = self
                        .manifest
                        .doc
                        .rename(loser, &collision_path(&entry.path, loser));
                    self.queue_manifest_update(update);
                }
            }
        }
    }

    /// The local half of syncing binary files, run with the rest of `reconcile`: follow
    /// remote renames, recognise local renames (same content under a new name), announce
    /// deletions, register new files and apply other devices' deletions. Returns whether
    /// the project has no such files at all (for the "looks unmounted" guard).
    pub(super) fn reconcile_files(
        &mut self,
        renamed_files: &mut Vec<(String, String)>,
        no_docs: bool,
        report: &mut SyncReport,
    ) -> Result<bool, SyncError> {
        self.fix_file_collisions(report);
        let files = Arc::clone(&self.files);
        let root = self.cfg.root.clone();
        let mut scan = scan_other_files(&*files, &root);
        let entries = self.live_of_kind(EntryKind::File, report);

        for entry in &entries {
            let Some(state) = self.bins.get_mut(&entry.doc_id) else {
                continue;
            };
            let Some(local) = state.local_path.clone() else {
                continue;
            };
            if local != entry.path && scan.contains(&local) && !scan.contains(&entry.path) {
                move_file(&*files, &root, &local, &entry.path)?;
                scan.remove(&local);
                scan.insert(entry.path.clone());
                renamed_files.push((local, entry.path.clone()));
                state.local_path = Some(entry.path.clone());
                state.dirty = true;
                report.files_renamed += 1;
            }
        }

        let materialized: BTreeSet<String> = self
            .bins
            .values()
            .filter_map(|state| state.local_path.clone())
            .collect();
        let never_here = |id: DocId| {
            self.bins
                .get(&id)
                .is_none_or(|state| state.local_path.is_none())
        };
        // A file at the path of an entry this device never materialized is adopted by
        // the transfer step (identical: nothing to do; different: a conflict copy).
        let mut new_files: Vec<(String, Option<Stamp>, Hash)> = Vec::new();
        for path in scan.iter().filter(|path| !materialized.contains(*path)) {
            if entries
                .iter()
                .any(|e| e.path == *path && never_here(e.doc_id))
            {
                continue;
            }
            let full = abs(&root, path);
            match files.read_bytes(&full) {
                Ok(bytes) => new_files.push((
                    path.clone(),
                    files.file_stamp(&full),
                    blake3::hash(&bytes).into(),
                )),
                Err(_) => report.skipped_paths.push(path.clone()),
            }
        }

        let mut missing: Vec<(DocId, String)> = entries
            .iter()
            .filter_map(|entry| {
                let local = self.bins.get(&entry.doc_id)?.local_path.clone()?;
                (!scan.contains(&local)).then_some((entry.doc_id, local))
            })
            .collect();
        // A missing file whose content reappears under a new name was renamed.
        missing.retain(|(id, old)| {
            let Some(known) = self.bins.get(id).and_then(|state| state.hash) else {
                return true;
            };
            let Some(position) = new_files.iter().position(|(_, _, hash)| *hash == known) else {
                return true;
            };
            let (new, stamp, _) = new_files.remove(position);
            let update = self.manifest.doc.rename(*id, &new);
            self.queue_manifest_update(update);
            let state = self.bins.get_mut(id).expect("checked above");
            state.local_path = Some(new.clone());
            state.stamp = stamp;
            state.dirty = true;
            renamed_files.push((old.clone(), new));
            report.files_renamed += 1;
            false
        });

        if scan.is_empty() && no_docs && !missing.is_empty() {
            report.deletions_held_back += missing.len();
        } else {
            for (id, _) in missing {
                let update = self.manifest.doc.set_deleted(id, true);
                self.queue_manifest_update(update);
                if let Some(state) = self.bins.get_mut(&id) {
                    state.local_path = None;
                    state.upload = None;
                    state.dirty = true;
                }
                report.files_tombstoned += 1;
            }
        }

        for (path, stamp, hash) in new_files {
            self.add_local_file(&path, stamp, hash);
            report.files_created += 1;
        }

        self.apply_file_tombstones(&scan, report)?;
        Ok(scan.is_empty())
    }

    /// Removes files another device deleted — unless this device changed them since,
    /// in which case the change wins and the file is revived.
    fn apply_file_tombstones(
        &mut self,
        scan: &BTreeSet<String>,
        report: &mut SyncReport,
    ) -> Result<(), SyncError> {
        let files = Arc::clone(&self.files);
        let root = self.cfg.root.clone();
        for entry in self.manifest.doc.entries() {
            if !entry.deleted || entry.kind != EntryKind::File {
                continue;
            }
            let Some(state) = self.bins.get_mut(&entry.doc_id) else {
                continue;
            };
            let Some(local) = state.local_path.clone() else {
                continue;
            };
            if !scan.contains(&local) {
                state.local_path = None;
                state.dirty = true;
                continue;
            }
            let current = current_hash(&*files, &root, &local, state)?;
            if current.is_some() && current == state.synced_hash() {
                files.remove_file(&abs(&root, &local))?;
                state.local_path = None;
                state.dirty = true;
                report.files_removed += 1;
            } else {
                let update = self.manifest.doc.set_deleted(entry.doc_id, false);
                self.queue_manifest_update(update);
                report.resurrected += 1;
            }
        }
        Ok(())
    }

    /// The network half: for each binary file, download a newer winning version, upload
    /// a local change, and trim versions nobody needs any more. `max_file_bytes` is the
    /// server's advertised limit; larger local files are reported, not uploaded.
    pub(super) fn transfer_files(
        &mut self,
        transport: &dyn SyncTransport,
        remote: &HashMap<DocId, u64>,
        max_file_bytes: Option<u64>,
        report: &mut SyncReport,
    ) -> Result<(), SyncError> {
        for entry in self.live_of_kind(EntryKind::File, report) {
            let latest = remote.get(&entry.doc_id).copied().unwrap_or(0);
            self.transfer_file(&entry, transport, latest, max_file_bytes, report)?;
        }
        // A deleted file that got a new version meanwhile was edited concurrently with
        // (or after) the deletion: the edit wins, and the next pass downloads it.
        for entry in self.manifest.doc.entries() {
            if !entry.deleted || entry.kind != EntryKind::File {
                continue;
            }
            let Some(state) = self.bins.get(&entry.doc_id) else {
                continue;
            };
            let latest = remote.get(&entry.doc_id).copied().unwrap_or(0);
            if latest <= state.seen_upto {
                continue;
            }
            let current = state.synced.as_ref().map(|s| s.header.clone());
            let (reading, _) = read_log(
                self,
                transport,
                entry.doc_id,
                state.scan_from,
                current.as_ref(),
                effective_file_limit(max_file_bytes),
            )?;
            if reading.winner.is_some_and(|(header, _, _)| {
                Some(header.version) != current.as_ref().map(|c| c.version)
            }) {
                let update = self.manifest.doc.set_deleted(entry.doc_id, false);
                self.queue_manifest_update(update);
                report.resurrected += 1;
            }
        }
        Ok(())
    }

    fn transfer_file(
        &mut self,
        entry: &ManifestEntry,
        transport: &dyn SyncTransport,
        remote_latest: u64,
        max_file_bytes: Option<u64>,
        report: &mut SyncReport,
    ) -> Result<(), SyncError> {
        let id = entry.doc_id;
        let files = Arc::clone(&self.files);
        let root = self.cfg.root.clone();
        let path = entry.path.clone();
        self.bins.entry(id).or_default();

        // 1. Anything new on the server.
        let seen_upto = self.bins[&id].seen_upto;
        if remote_latest > seen_upto {
            let since = self.bins[&id].scan_from;
            let current = self.bins[&id].synced.as_ref().map(|s| s.header.clone());
            let limit = effective_file_limit(max_file_bytes);
            let (reading, last) = read_log(self, transport, id, since, current.as_ref(), limit)?;
            report.pulled_updates += reading.records;
            if reading.too_large {
                report.files_too_large_to_download.push(path.clone());
            }
            if let Some((winner, winner_seq, content)) = reading.winner {
                self.adopt(&path, id, &winner, winner_seq, &content, report)?;
                self.trim(transport, id, winner_seq, report)?;
            }
            let state = self.bins.get_mut(&id).expect("inserted above");
            state.scan_from = reading.scan_from;
            state.seen_upto = last;
            state.dirty = true;
        }

        // 2. A local change to upload.
        let state = self.bins.get_mut(&id).expect("inserted above");
        if state.local_path.is_none() && !files.exists(&abs(&root, &path)) {
            return Ok(());
        }
        let Some(local) = current_hash(&*files, &root, &path, state)? else {
            return Ok(());
        };
        if Some(local) == state.synced_hash() {
            state.local_path = Some(path);
            return Ok(());
        }
        let bytes = files.read_bytes(&abs(&root, &path))?;
        if bytes.len() as u64 > effective_file_limit(max_file_bytes) {
            report.files_over_limit.push(path);
            return Ok(());
        }
        let hash: Hash = blake3::hash(&bytes).into();
        if hash != local {
            // Changed while we looked; the next pass picks the new content up.
            state.hash = None;
            return Ok(());
        }
        self.upload(transport, id, &path, &bytes, hash, report)
    }

    /// Makes `winner` the version on disk at `path`, unless it already is. This device's
    /// own version is kept as a conflict copy when it would otherwise be lost: when the
    /// file changed here since the last sync, or this device made the version that the
    /// winner doesn't descend from.
    fn adopt(
        &mut self,
        path: &str,
        id: DocId,
        winner: &Header,
        winner_seq: u64,
        content: &[u8],
        report: &mut SyncReport,
    ) -> Result<(), SyncError> {
        let files = Arc::clone(&self.files);
        let root = self.cfg.root.clone();
        let state = self.bins.get_mut(&id).expect("present");
        if state.synced.as_ref().map(|s| s.header.version) == Some(winner.version) {
            return Ok(());
        }
        let local = current_hash(&*files, &root, path, state)?;
        let descends = state
            .synced
            .as_ref()
            .is_none_or(|s| winner.lineage.contains(&s.header.version));
        let changed_here = local.is_some() && local != state.synced_hash();
        let lost_mine = state.synced.as_ref().is_some_and(|s| s.mine) && !descends;
        let keep_ours = local
            .filter(|hash| *hash != winner.hash && (changed_here || lost_mine))
            .map(|hash| (hash, state.stamp));
        state.synced = Some(Synced {
            header: winner.clone(),
            header_seq: winner_seq,
            mine: false,
        });
        state.upload = None;
        state.dirty = true;

        if let Some((hash, stamp)) = keep_ours {
            let copy = self.unique_file_path(&conflict_copy_path(path));
            move_file(&*files, &root, path, &copy)?;
            self.add_local_file(&copy, stamp, hash);
            report.conflict_copies.push(copy);
        }
        if local != Some(winner.hash) {
            write_bytes(&*files, &root, path, content)?;
            report.files_written += 1;
        }
        let state = self.bins.get_mut(&id).expect("present");
        state.local_path = Some(path.to_string());
        state.stamp = files.file_stamp(&abs(&root, path));
        state.hash = Some(winner.hash);
        Ok(())
    }

    fn upload(
        &mut self,
        transport: &dyn SyncTransport,
        id: DocId,
        path: &str,
        bytes: &[u8],
        hash: Hash,
        report: &mut SyncReport,
    ) -> Result<(), SyncError> {
        let (key, vault) = (&self.cfg.key, self.cfg.vault);
        let state = self.bins.get_mut(&id).expect("present");
        state.local_path = Some(path.to_string());
        let seal = |record: &Record| {
            let plain = postcard::to_stdvec(record).expect("records always serialize");
            key.seal(vault, id, &plain)
        };

        let mut upload = match state.upload.take() {
            Some(upload) if upload.header.hash == hash => upload,
            _ => {
                let mut lineage = Vec::new();
                if let Some(synced) = &state.synced {
                    lineage.push(synced.header.version);
                    lineage.extend(synced.header.lineage.iter().take(LINEAGE_LEN - 1));
                }
                let header = Header {
                    version: Uuid::new_v4(),
                    lineage,
                    size: bytes.len() as u64,
                    chunks: bytes.len().div_ceil(CHUNK_BYTES) as u32,
                    hash,
                };
                let header_seq = transport.push(id, &seal(&Record::Header(header.clone()))?)?;
                report.pushed_updates += 1;
                Upload {
                    header,
                    header_seq,
                    next_chunk: 0,
                }
            }
        };
        // Our own records need not be read back, as long as nobody else's came between.
        let mut contiguous = state.seen_upto + 1 == upload.header_seq && upload.next_chunk == 0;
        let mut last_seq = upload.header_seq;
        for index in upload.next_chunk..upload.header.chunks {
            let start = index as usize * CHUNK_BYTES;
            let data = bytes[start..(start + CHUNK_BYTES).min(bytes.len())].to_vec();
            let chunk = Record::Chunk {
                version: upload.header.version,
                index,
                data,
            };
            let sealed = seal(&chunk)?;
            // Recorded before pushing, so an interrupted upload resumes where it stopped.
            state.upload = Some(upload.clone());
            state.dirty = true;
            let seq = transport.push(id, &sealed)?;
            contiguous &= seq == last_seq + 1;
            last_seq = seq;
            upload.next_chunk = index + 1;
            report.pushed_updates += 1;
        }
        state.upload = None;
        state.synced = Some(Synced {
            header: upload.header.clone(),
            header_seq: upload.header_seq,
            mine: true,
        });
        state.hash = Some(hash);
        if contiguous {
            state.seen_upto = last_seq;
            state.scan_from = last_seq;
        }
        state.dirty = true;
        self.trim(transport, id, upload.header_seq, report)
    }

    /// Drops everything logged before the winning version's header from the server.
    fn trim(
        &mut self,
        transport: &dyn SyncTransport,
        id: DocId,
        header_seq: u64,
        report: &mut SyncReport,
    ) -> Result<(), SyncError> {
        let upto = header_seq.saturating_sub(1);
        let state = self.bins.get_mut(&id).expect("present");
        if upto == 0 || upto <= state.trimmed_upto {
            return Ok(());
        }
        let plain = postcard::to_stdvec(&Record::Trimmed).expect("records always serialize");
        let blob = self.cfg.key.seal(self.cfg.vault, id, &plain)?;
        transport.put_snapshot(
            id,
            &Snapshot {
                upto_seq: upto,
                blob,
            },
        )?;
        state.trimmed_upto = upto;
        state.dirty = true;
        report.snapshots_uploaded += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conflict_names_keep_the_extension() {
        assert_eq!(
            conflict_copy_path("Art/cover.png"),
            "Art/cover (conflict copy).png"
        );
        assert_eq!(conflict_copy_path("README"), "README (conflict copy)");
        assert_eq!(conflict_copy_path("a.b/notes"), "a.b/notes (conflict copy)");
        assert_eq!(conflict_copy_path(".hidden"), ".hidden (conflict copy)");
        let id = DocId(Uuid::from_u128(0xabcdef12_0000_0000_0000_000000000000));
        assert_eq!(collision_path("map.pdf", id), "map (conflict abcdef12).pdf");
    }
}
