//! An in-memory stand-in for the sync server, for simulating several devices.
//!
//! Mirrors the real server's contract exactly where the engine depends on it:
//! per-document sequence numbers that are gap-free and start at 1, snapshots that
//! replace the updates they cover, and no ability to read any blob. Each
//! [`MemoryTransport`] is one device's connection and can be switched offline.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use smaragd_sync_protocol::api::{
    DocSummary, ListDocsResponse, PULL_PAGE_BYTES, PullUpdatesResponse, Snapshot, StoredUpdate,
};
use smaragd_sync_protocol::{DeviceId, DocId};

use super::transport::{SyncTransport, TransportError};

#[derive(Debug, Default)]
struct DocLog {
    snapshot: Option<Snapshot>,
    updates: Vec<StoredUpdate>,
    latest_seq: u64,
}

#[derive(Debug, Clone, Default)]
pub struct MemoryServer {
    docs: Arc<Mutex<BTreeMap<DocId, DocLog>>>,
    max_file_bytes: Arc<Mutex<Option<u64>>>,
}

impl MemoryServer {
    /// What the listing advertises as the largest file to sync (`None`: no limit).
    pub fn set_max_file_bytes(&self, limit: Option<u64>) {
        *self.max_file_bytes.lock().unwrap() = limit;
    }

    /// Every blob the server currently stores, for asserting it's all ciphertext.
    pub fn all_blobs(&self) -> Vec<Vec<u8>> {
        let docs = self.docs.lock().unwrap();
        docs.values()
            .flat_map(|log| {
                log.snapshot
                    .iter()
                    .map(|s| s.blob.clone())
                    .chain(log.updates.iter().map(|u| u.blob.clone()))
            })
            .collect()
    }

    /// The sealed blobs of `doc`'s current updates, oldest first.
    pub fn update_blobs(&self, doc: DocId) -> Vec<Vec<u8>> {
        self.docs
            .lock()
            .unwrap()
            .get(&doc)
            .map_or_else(Vec::new, |log| {
                log.updates.iter().map(|u| u.blob.clone()).collect()
            })
    }

    /// A hostile server replaying blobs it kept: appends them to `doc` as fresh updates
    /// from `device`, as if newly pushed. It can't make new ones — they're sealed.
    pub fn replay(&self, doc: DocId, device: DeviceId, blobs: &[Vec<u8>]) {
        let mut docs = self.docs.lock().unwrap();
        let log = docs.entry(doc).or_default();
        for blob in blobs {
            log.latest_seq += 1;
            log.updates.push(StoredUpdate {
                seq: log.latest_seq,
                device_id: device,
                blob: blob.clone(),
            });
        }
    }

    pub fn update_count(&self, doc: DocId) -> usize {
        self.docs
            .lock()
            .unwrap()
            .get(&doc)
            .map_or(0, |log| log.updates.len())
    }
}

#[derive(Debug)]
pub struct MemoryTransport {
    server: MemoryServer,
    device: DeviceId,
    online: Arc<AtomicBool>,
}

impl MemoryTransport {
    pub fn new(server: &MemoryServer, device: DeviceId) -> Self {
        Self {
            server: server.clone(),
            device,
            online: Arc::new(AtomicBool::new(true)),
        }
    }

    /// The switch behind [`Self::set_online`], for tests that hand the transport away.
    pub fn online_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.online)
    }

    pub fn set_online(&self, online: bool) {
        self.online.store(online, Ordering::SeqCst);
    }

    fn check_online(&self) -> Result<(), TransportError> {
        if self.online.load(Ordering::SeqCst) {
            Ok(())
        } else {
            Err(TransportError::Offline("simulated outage".into()))
        }
    }
}

impl SyncTransport for MemoryTransport {
    fn list_docs(&self) -> Result<ListDocsResponse, TransportError> {
        self.check_online()?;
        let docs = self.server.docs.lock().unwrap();
        Ok(ListDocsResponse {
            docs: docs
                .iter()
                .map(|(doc_id, log)| DocSummary {
                    doc_id: *doc_id,
                    latest_seq: log.latest_seq,
                })
                .collect(),
            max_file_bytes: *self.server.max_file_bytes.lock().unwrap(),
        })
    }

    fn pull(&self, doc: DocId, since: u64) -> Result<PullUpdatesResponse, TransportError> {
        self.check_online()?;
        let docs = self.server.docs.lock().unwrap();
        let Some(log) = docs.get(&doc) else {
            return Ok(PullUpdatesResponse {
                snapshot: None,
                updates: vec![],
                more: false,
            });
        };
        let snapshot = log.snapshot.clone().filter(|s| s.upto_seq > since);
        let floor = snapshot.as_ref().map_or(since, |s| s.upto_seq.max(since));
        // Paged exactly like the real server (`db::pull_updates`).
        let mut budget =
            PULL_PAGE_BYTES.saturating_sub(snapshot.as_ref().map_or(0, |s| s.blob.len()));
        let mut must_take_one = snapshot.is_none();
        let (mut updates, mut more) = (Vec::new(), false);
        for update in log.updates.iter().filter(|u| u.seq > floor) {
            if update.blob.len() > budget && !must_take_one {
                more = true;
                break;
            }
            budget = budget.saturating_sub(update.blob.len());
            must_take_one = false;
            updates.push(update.clone());
        }
        Ok(PullUpdatesResponse {
            snapshot,
            updates,
            more,
        })
    }

    fn push(&self, doc: DocId, sealed: &[u8]) -> Result<u64, TransportError> {
        self.check_online()?;
        let mut docs = self.server.docs.lock().unwrap();
        let log = docs.entry(doc).or_default();
        log.latest_seq += 1;
        log.updates.push(StoredUpdate {
            seq: log.latest_seq,
            device_id: self.device,
            blob: sealed.to_vec(),
        });
        Ok(log.latest_seq)
    }

    fn put_snapshot(&self, doc: DocId, snapshot: &Snapshot) -> Result<(), TransportError> {
        self.check_online()?;
        let mut docs = self.server.docs.lock().unwrap();
        let log = docs.entry(doc).or_default();
        // Like the real server: an older or equal snapshot is ignored.
        if log
            .snapshot
            .as_ref()
            .is_some_and(|s| s.upto_seq >= snapshot.upto_seq)
        {
            return Ok(());
        }
        log.updates.retain(|u| u.seq > snapshot.upto_seq);
        log.snapshot = Some(snapshot.clone());
        Ok(())
    }
}
