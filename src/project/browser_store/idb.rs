//! IndexedDB persistence for [`super::BrowserStore`] — see that module's own
//! doc comment for the overall write-behind design (full resync on every
//! mutation, best-effort, eventually consistent). This file is the only
//! place that actually talks to `rexie`/IndexedDB.
//!
//! One database (`DB_NAME`), one object store (`STORE_NAME`), one row per
//! path — matching the plan's "single-project, browser-local" v1 scope, so
//! there's no need to namespace by project.

use std::path::PathBuf;

use rexie::{ObjectStore, Rexie, TransactionMode};
use serde::{Deserialize, Serialize};

use super::Entry;

const DB_NAME: &str = "smaragd";
const STORE_NAME: &str = "files";

/// The on-the-wire shape of one stored entry — `path` doubles as the object
/// store's key (`key_path("path")`), so a plain [`super::Entry`] value
/// wouldn't be enough on its own to round-trip through `get_all`, which
/// returns values only, not their keys.
#[derive(Serialize, Deserialize)]
struct StoredEntry {
    path: String,
    kind: StoredKind,
}

#[derive(Serialize, Deserialize)]
enum StoredKind {
    Dir,
    File(Vec<u8>),
}

impl From<(&PathBuf, &Entry)> for StoredEntry {
    fn from((path, entry): (&PathBuf, &Entry)) -> Self {
        StoredEntry {
            path: path.to_string_lossy().into_owned(),
            kind: match entry {
                Entry::Dir => StoredKind::Dir,
                Entry::File(bytes) => StoredKind::File(bytes.clone()),
            },
        }
    }
}

impl From<StoredEntry> for (PathBuf, Entry) {
    fn from(stored: StoredEntry) -> Self {
        let entry = match stored.kind {
            StoredKind::Dir => Entry::Dir,
            StoredKind::File(bytes) => Entry::File(bytes),
        };
        (PathBuf::from(stored.path), entry)
    }
}

/// Opens (creating on first use) the one database this module ever talks
/// to. Re-opening an existing database at the same version is a normal,
/// cheap IndexedDB operation — it does not re-run the "create the object
/// store" upgrade step, so calling this on every load/save is fine.
async fn open_db() -> rexie::Result<Rexie> {
    Rexie::builder(DB_NAME)
        .version(1)
        .add_object_store(ObjectStore::new(STORE_NAME).key_path("path"))
        .build()
        .await
}

/// Everything currently persisted, or an empty list if there's nothing
/// saved yet, IndexedDB isn't available, or anything about the read fails
/// — best-effort, matching `BrowserStore`'s own "never fails outright"
/// philosophy for its other operations.
pub async fn load_all() -> Vec<(PathBuf, Entry)> {
    let Ok(db) = open_db().await else {
        return Vec::new();
    };
    let Ok(transaction) = db.transaction(&[STORE_NAME], TransactionMode::ReadOnly) else {
        return Vec::new();
    };
    let Ok(store) = transaction.store(STORE_NAME) else {
        return Vec::new();
    };
    let Ok(values) = store.get_all(None, None).await else {
        return Vec::new();
    };
    values
        .into_iter()
        .filter_map(|value| serde_wasm_bindgen::from_value::<StoredEntry>(value).ok())
        .map(Into::into)
        .collect()
}

/// Clears the object store and rewrites it wholesale from `snapshot`, in
/// the background — see `BrowserStore`'s doc comment for why a full resync
/// rather than a targeted per-mutation write. Silently gives up on any
/// error (a full disk, IndexedDB unavailable, the tab closing mid-write):
/// there's no UI thread waiting on this, and the in-memory `BrowserStore`
/// this snapshot came from is already correct regardless.
pub fn spawn_save(snapshot: Vec<(PathBuf, Entry)>) {
    wasm_bindgen_futures::spawn_local(async move {
        let Ok(db) = open_db().await else { return };
        let Ok(transaction) = db.transaction(&[STORE_NAME], TransactionMode::ReadWrite) else {
            return;
        };
        let Ok(store) = transaction.store(STORE_NAME) else {
            return;
        };
        if store.clear().await.is_err() {
            return;
        }
        for (path, entry) in &snapshot {
            let stored = StoredEntry::from((path, entry));
            if let Ok(value) = serde_wasm_bindgen::to_value(&stored) {
                let _ = store.put(&value, None).await;
            }
        }
        let _ = transaction.done().await;
    });
}
