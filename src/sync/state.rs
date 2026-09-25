//! Where the engine keeps its local CRDT state between runs.
//!
//! This is a plain key → bytes store. The production implementation ([`DirStateStore`])
//! writes through the [`ProjectStore`] trait, so the same code works on native
//! (real files, in the OS data dir — never inside the project folder, since the
//! project may be under git or copied around) and, later, in the browser
//! (IndexedDB). Tests use [`MemoryStateStore`], which can be cloned to simulate a
//! restart.

use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;

use crate::project::store::ProjectStore;

pub trait StateStore: Send {
    fn get(&self, key: &str) -> io::Result<Option<Vec<u8>>>;
    fn put(&mut self, key: &str, value: &[u8]) -> io::Result<()>;
}

/// An in-memory store. Clones share the same map, so a clone handed to a second
/// engine simulates the same device restarting.
#[derive(Debug, Clone, Default)]
pub struct MemoryStateStore {
    map: Arc<Mutex<HashMap<String, Vec<u8>>>>,
}

impl StateStore for MemoryStateStore {
    fn get(&self, key: &str) -> io::Result<Option<Vec<u8>>> {
        Ok(self.map.lock().unwrap().get(key).cloned())
    }

    fn put(&mut self, key: &str, value: &[u8]) -> io::Result<()> {
        self.map
            .lock()
            .unwrap()
            .insert(key.to_string(), value.to_vec());
        Ok(())
    }
}

/// Files in one directory, one per key. Values are base64 text because
/// [`ProjectStore`] only exposes `read_to_string`; each write goes to a temp name
/// and is renamed into place so a crash never leaves a half-written state file.
#[derive(Debug)]
pub struct DirStateStore {
    files: Arc<dyn ProjectStore>,
    dir: PathBuf,
}

impl DirStateStore {
    pub fn new(files: Arc<dyn ProjectStore>, dir: PathBuf) -> Self {
        Self { files, dir }
    }

    fn path_for(&self, key: &str) -> PathBuf {
        self.dir.join(format!("{}.state", key.replace('/', "_")))
    }
}

impl StateStore for DirStateStore {
    fn get(&self, key: &str) -> io::Result<Option<Vec<u8>>> {
        match self.files.read_to_string(&self.path_for(key)) {
            Ok(text) => STANDARD
                .decode(text.trim())
                .map(Some)
                .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err)),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err),
        }
    }

    fn put(&mut self, key: &str, value: &[u8]) -> io::Result<()> {
        self.files.create_dir_all(&self.dir)?;
        let path = self.path_for(key);
        let tmp = path.with_extension("state.tmp");
        self.files.write(&tmp, STANDARD.encode(value).as_bytes())?;
        self.files.rename(&tmp, &path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::store::native_store;

    #[test]
    fn memory_clones_share_state() {
        let mut a = MemoryStateStore::default();
        let b = a.clone();
        a.put("k", b"v").unwrap();
        assert_eq!(b.get("k").unwrap().as_deref(), Some(&b"v"[..]));
        assert_eq!(b.get("missing").unwrap(), None);
    }

    #[test]
    fn dir_store_round_trips_binary_values_and_overwrites() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = DirStateStore::new(native_store(), tmp.path().join("sync/vault"));
        assert_eq!(store.get("doc/abc").unwrap(), None);

        let bytes = vec![0u8, 255, 10, 13, 200];
        store.put("doc/abc", &bytes).unwrap();
        assert_eq!(store.get("doc/abc").unwrap(), Some(bytes));

        store.put("doc/abc", b"second").unwrap();
        assert_eq!(
            store.get("doc/abc").unwrap().as_deref(),
            Some(&b"second"[..])
        );
    }

    #[test]
    fn dir_store_rejects_corrupt_files_instead_of_returning_garbage() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = DirStateStore::new(native_store(), tmp.path().to_path_buf());
        store.put("k", b"ok").unwrap();
        std::fs::write(tmp.path().join("k.state"), "!!! not base64 !!!").unwrap();
        assert!(store.get("k").is_err());
    }
}
