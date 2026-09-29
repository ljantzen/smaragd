//! The SQLite layer. Everything here is synchronous and takes a plain
//! `rusqlite::Connection`, so it's unit-testable without HTTP; handlers reach it
//! through [`Db::run`], which moves the blocking work off the async runtime.
//!
//! The server is blind: `blob` columns hold sealed envelopes it cannot read, and
//! the only structure it knows is `(vault, doc, seq)`. Per-document sequence
//! numbers are gap-free and start at 1, which is what lets a client ask for
//! "everything after N".

use std::path::Path;
use std::sync::{Arc, Mutex};

use rusqlite::{Connection, OptionalExtension, params};
use smaragd_sync_protocol::api::{
    DeviceInfo, DocSummary, INITIAL_KEY_VERSION, PAIRING_CODE_LEN, PULL_PAGE_BYTES,
    PullUpdatesResponse, Snapshot, StoredUpdate, VaultInfo, normalize_pairing_code,
};
use smaragd_sync_protocol::{DeviceId, DocId, VaultId};
use uuid::Uuid;

use crate::auth::{hash_secret, new_device_token, new_pairing_code};
use crate::error::HttpError;

/// How stale `last_seen` may get before a request refreshes it, so a busy device
/// doesn't turn every request into a write.
const LAST_SEEN_GRANULARITY_SECS: i64 = 60;
/// Unredeemed pairing codes a vault may have outstanding at once.
const MAX_OUTSTANDING_PAIRING_CODES: i64 = 10;

const SCHEMA_V1: &str = "
CREATE TABLE vaults (
    id          TEXT PRIMARY KEY,
    kdf_salt    BLOB NOT NULL,
    key_version INTEGER NOT NULL,
    bytes_used  INTEGER NOT NULL DEFAULT 0,
    created_at  INTEGER NOT NULL
);
CREATE TABLE devices (
    id         TEXT PRIMARY KEY,
    vault_id   TEXT NOT NULL REFERENCES vaults(id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    token_hash TEXT NOT NULL UNIQUE,
    created_at INTEGER NOT NULL,
    last_seen  INTEGER
);
CREATE INDEX devices_by_vault ON devices(vault_id);
CREATE TABLE pairing_codes (
    code_hash  TEXT PRIMARY KEY,
    vault_id   TEXT NOT NULL REFERENCES vaults(id) ON DELETE CASCADE,
    expires_at INTEGER NOT NULL
);
CREATE TABLE docs (
    vault_id       TEXT NOT NULL REFERENCES vaults(id) ON DELETE CASCADE,
    doc_id         TEXT NOT NULL,
    latest_seq     INTEGER NOT NULL DEFAULT 0,
    snapshot_upto  INTEGER,
    snapshot_blob  BLOB,
    PRIMARY KEY (vault_id, doc_id)
);
CREATE TABLE updates (
    vault_id   TEXT NOT NULL,
    doc_id     TEXT NOT NULL,
    seq        INTEGER NOT NULL,
    device_id  TEXT NOT NULL,
    blob       BLOB NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (vault_id, doc_id, seq),
    FOREIGN KEY (vault_id, doc_id) REFERENCES docs(vault_id, doc_id) ON DELETE CASCADE
);
";

pub fn open(path: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL; PRAGMA busy_timeout = 5000;",
    )?;
    init(conn)
}

pub fn open_in_memory() -> rusqlite::Result<Connection> {
    init(Connection::open_in_memory()?)
}

/// v2 adds `vaults.last_active`, which lets maintenance tell an abandoned vault from a
/// quiet one: it's bumped by every push and when a device is revoked, so "no devices
/// and inactive for N days" means the last device left and nobody came back.
const MIGRATE_V1_TO_V2: &str = "
ALTER TABLE vaults ADD COLUMN last_active INTEGER NOT NULL DEFAULT 0;
UPDATE vaults SET last_active = COALESCE(
    (SELECT MAX(created_at) FROM updates WHERE updates.vault_id = vaults.id),
    created_at
);
";

const SCHEMA_VERSION: i64 = 2;

fn init(conn: Connection) -> rusqlite::Result<Connection> {
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version > SCHEMA_VERSION {
        return Err(rusqlite::Error::InvalidParameterName(format!(
            "database schema version {version} is newer than this server understands"
        )));
    }
    if version < 1 {
        conn.execute_batch(&format!(
            "BEGIN; {SCHEMA_V1} PRAGMA user_version = 1; COMMIT;"
        ))?;
    }
    if version < 2 {
        conn.execute_batch(&format!(
            "BEGIN; {MIGRATE_V1_TO_V2} PRAGMA user_version = 2; COMMIT;"
        ))?;
    }
    Ok(conn)
}

/// A shared connection. SQLite serializes writers anyway, and this server's load is
/// a handful of devices, so one mutex-guarded connection is the simple, correct choice.
#[derive(Clone)]
pub struct Db(Arc<Mutex<Connection>>);

impl Db {
    pub fn new(conn: Connection) -> Self {
        Self(Arc::new(Mutex::new(conn)))
    }

    /// Runs blocking database work on the blocking pool.
    pub async fn run<T, F>(&self, work: F) -> Result<T, HttpError>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T, HttpError> + Send + 'static,
    {
        let conn = Arc::clone(&self.0);
        tokio::task::spawn_blocking(move || {
            let mut guard = conn
                .lock()
                .map_err(|_| HttpError::Internal("database mutex poisoned".into()))?;
            work(&mut guard)
        })
        .await
        .map_err(|err| HttpError::Internal(format!("database task failed: {err}")))?
    }
}

/// Who a request's bearer token belongs to.
#[derive(Debug, Clone, Copy)]
pub struct Authed {
    pub device_id: DeviceId,
    pub vault_id: VaultId,
}

fn parse_id<T: std::str::FromStr>(text: &str, what: &str) -> Result<T, HttpError> {
    text.parse()
        .map_err(|_| HttpError::Internal(format!("corrupt {what} id in database: {text:?}")))
}

pub fn authenticate(
    conn: &Connection,
    token_hash: &str,
    now: i64,
) -> Result<Option<Authed>, HttpError> {
    let row: Option<(String, String, Option<i64>)> = conn
        .query_row(
            "SELECT id, vault_id, last_seen FROM devices WHERE token_hash = ?1",
            [token_hash],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    let Some((device, vault, last_seen)) = row else {
        return Ok(None);
    };
    if last_seen.is_none_or(|seen| now - seen >= LAST_SEEN_GRANULARITY_SECS) {
        conn.execute(
            "UPDATE devices SET last_seen = ?2 WHERE id = ?1",
            params![device, now],
        )?;
    }
    Ok(Some(Authed {
        device_id: parse_id(&device, "device")?,
        vault_id: parse_id(&vault, "vault")?,
    }))
}

/// A newly created device and the one-time plaintext of its token.
pub struct NewDevice {
    pub vault: VaultInfo,
    pub device_id: DeviceId,
    pub token: String,
}

fn insert_device(
    conn: &Connection,
    vault: &VaultInfo,
    name: &str,
    now: i64,
) -> Result<NewDevice, HttpError> {
    let device_id = DeviceId(Uuid::new_v4());
    let token = new_device_token();
    conn.execute(
        "INSERT INTO devices (id, vault_id, name, token_hash, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            device_id.to_string(),
            vault.vault_id.to_string(),
            name,
            hash_secret(&token),
            now
        ],
    )?;
    Ok(NewDevice {
        vault: vault.clone(),
        device_id,
        token,
    })
}

pub fn create_vault(
    conn: &mut Connection,
    salt: &[u8],
    device_name: &str,
    now: i64,
) -> Result<NewDevice, HttpError> {
    let vault = VaultInfo {
        vault_id: VaultId(Uuid::new_v4()),
        kdf_salt: salt.to_vec(),
        key_version: INITIAL_KEY_VERSION,
    };
    let tx = conn.transaction()?;
    tx.execute(
        "INSERT INTO vaults (id, kdf_salt, key_version, created_at, last_active)
         VALUES (?1, ?2, ?3, ?4, ?4)",
        params![
            vault.vault_id.to_string(),
            vault.kdf_salt,
            vault.key_version,
            now
        ],
    )?;
    let device = insert_device(&tx, &vault, device_name, now)?;
    tx.commit()?;
    Ok(device)
}

pub fn get_vault(conn: &Connection, vault: VaultId) -> Result<Option<VaultInfo>, HttpError> {
    Ok(conn
        .query_row(
            "SELECT kdf_salt, key_version FROM vaults WHERE id = ?1",
            [vault.to_string()],
            |row| {
                Ok(VaultInfo {
                    vault_id: vault,
                    kdf_salt: row.get(0)?,
                    key_version: row.get(1)?,
                })
            },
        )
        .optional()?)
}

pub fn delete_vault(conn: &Connection, vault: VaultId) -> Result<bool, HttpError> {
    let id = vault.to_string();
    conn.execute("DELETE FROM updates WHERE vault_id = ?1", [&id])?;
    Ok(conn.execute("DELETE FROM vaults WHERE id = ?1", [&id])? > 0)
}

pub fn create_pairing_code(
    conn: &mut Connection,
    vault: VaultId,
    now: i64,
    ttl_secs: u64,
) -> Result<String, HttpError> {
    let tx = conn.transaction()?;
    tx.execute("DELETE FROM pairing_codes WHERE expires_at < ?1", [now])?;
    let outstanding: i64 = tx.query_row(
        "SELECT COUNT(*) FROM pairing_codes WHERE vault_id = ?1",
        [vault.to_string()],
        |row| row.get(0),
    )?;
    if outstanding >= MAX_OUTSTANDING_PAIRING_CODES {
        return Err(HttpError::BadRequest(
            "too many unredeemed pairing codes; wait for them to expire".into(),
        ));
    }
    let code = new_pairing_code();
    tx.execute(
        "INSERT INTO pairing_codes (code_hash, vault_id, expires_at) VALUES (?1, ?2, ?3)",
        params![
            hash_secret(&normalize_pairing_code(&code)),
            vault.to_string(),
            now + ttl_secs as i64
        ],
    )?;
    tx.commit()?;
    Ok(code)
}

/// Exchanges a pairing code for a new device. The code is consumed whether or not
/// it had expired, so it can never be tried twice.
pub fn redeem_pairing_code(
    conn: &mut Connection,
    code: &str,
    device_name: &str,
    now: i64,
) -> Result<Option<NewDevice>, HttpError> {
    let normalized = normalize_pairing_code(code);
    if normalized.len() != PAIRING_CODE_LEN {
        return Ok(None);
    }
    let hash = hash_secret(&normalized);
    let tx = conn.transaction()?;
    let row: Option<(String, i64)> = tx
        .query_row(
            "SELECT vault_id, expires_at FROM pairing_codes WHERE code_hash = ?1",
            [&hash],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((vault_id, expires_at)) = row else {
        return Ok(None);
    };
    tx.execute("DELETE FROM pairing_codes WHERE code_hash = ?1", [&hash])?;
    if expires_at < now {
        tx.commit()?;
        return Ok(None);
    }
    let Some(vault) = get_vault(&tx, parse_id(&vault_id, "vault")?)? else {
        return Ok(None);
    };
    let device = insert_device(&tx, &vault, device_name, now)?;
    tx.commit()?;
    Ok(Some(device))
}

pub fn list_devices(conn: &Connection, vault: VaultId) -> Result<Vec<DeviceInfo>, HttpError> {
    let mut stmt = conn.prepare(
        "SELECT id, name, created_at, last_seen FROM devices WHERE vault_id = ?1 ORDER BY created_at, id",
    )?;
    let rows = stmt.query_map([vault.to_string()], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, Option<i64>>(3)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, name, created, seen) = row?;
        out.push(DeviceInfo {
            device_id: parse_id(&id, "device")?,
            name,
            created_at_unix: created.max(0) as u64,
            last_seen_unix: seen.map(|s| s.max(0) as u64),
        });
    }
    Ok(out)
}

pub fn revoke_device(
    conn: &Connection,
    vault: VaultId,
    device: DeviceId,
    now: i64,
) -> Result<bool, HttpError> {
    let removed = conn.execute(
        "DELETE FROM devices WHERE id = ?1 AND vault_id = ?2",
        params![device.to_string(), vault.to_string()],
    )? > 0;
    if removed {
        // Starts the clock for a vault whose last device just left.
        conn.execute(
            "UPDATE vaults SET last_active = ?2 WHERE id = ?1",
            params![vault.to_string(), now],
        )?;
    }
    Ok(removed)
}

pub fn list_docs(conn: &Connection, vault: VaultId) -> Result<Vec<DocSummary>, HttpError> {
    let mut stmt =
        conn.prepare("SELECT doc_id, latest_seq FROM docs WHERE vault_id = ?1 ORDER BY doc_id")?;
    let rows = stmt.query_map([vault.to_string()], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (doc, seq) = row?;
        out.push(DocSummary {
            doc_id: parse_id(&doc, "doc")?,
            latest_seq: seq.max(0) as u64,
        });
    }
    Ok(out)
}

fn bytes_used(conn: &Connection, vault: VaultId) -> Result<u64, HttpError> {
    let used: Option<i64> = conn
        .query_row(
            "SELECT bytes_used FROM vaults WHERE id = ?1",
            [vault.to_string()],
            |row| row.get(0),
        )
        .optional()?;
    used.map(|u| u.max(0) as u64)
        .ok_or(HttpError::NotFound("no such vault"))
}

pub fn push_update(
    conn: &mut Connection,
    vault: VaultId,
    doc: DocId,
    device: DeviceId,
    blob: &[u8],
    quota: u64,
    now: i64,
) -> Result<u64, HttpError> {
    let tx = conn.transaction()?;
    if bytes_used(&tx, vault)?.saturating_add(blob.len() as u64) > quota {
        return Err(HttpError::QuotaExceeded);
    }
    let (v, d) = (vault.to_string(), doc.to_string());
    tx.execute(
        "INSERT INTO docs (vault_id, doc_id, latest_seq) VALUES (?1, ?2, 0)
         ON CONFLICT (vault_id, doc_id) DO NOTHING",
        params![v, d],
    )?;
    let seq: i64 = tx.query_row(
        "UPDATE docs SET latest_seq = latest_seq + 1 WHERE vault_id = ?1 AND doc_id = ?2
         RETURNING latest_seq",
        params![v, d],
        |row| row.get(0),
    )?;
    tx.execute(
        "INSERT INTO updates (vault_id, doc_id, seq, device_id, blob, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![v, d, seq, device.to_string(), blob, now],
    )?;
    tx.execute(
        "UPDATE vaults SET bytes_used = bytes_used + ?2, last_active = ?3 WHERE id = ?1",
        params![v, blob.len() as i64, now],
    )?;
    tx.commit()?;
    Ok(seq as u64)
}

pub fn pull_updates(
    conn: &Connection,
    vault: VaultId,
    doc: DocId,
    since: u64,
) -> Result<PullUpdatesResponse, HttpError> {
    let (v, d) = (vault.to_string(), doc.to_string());
    let stored: Option<(Option<i64>, Option<Vec<u8>>)> = conn
        .query_row(
            "SELECT snapshot_upto, snapshot_blob FROM docs WHERE vault_id = ?1 AND doc_id = ?2",
            params![v, d],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let snapshot = match stored {
        Some((Some(upto), Some(blob))) if upto.max(0) as u64 > since => Some(Snapshot {
            upto_seq: upto as u64,
            blob,
        }),
        _ => None,
    };
    let floor = snapshot.as_ref().map_or(since, |s| s.upto_seq.max(since));
    // One page: blobs (the snapshot included) up to `PULL_PAGE_BYTES`, but always at
    // least one, so any single blob can be fetched however large.
    let mut budget = PULL_PAGE_BYTES.saturating_sub(snapshot.as_ref().map_or(0, |s| s.blob.len()));
    let mut must_take_one = snapshot.is_none();

    let mut stmt = conn.prepare(
        "SELECT seq, device_id, blob FROM updates
         WHERE vault_id = ?1 AND doc_id = ?2 AND seq > ?3 ORDER BY seq",
    )?;
    let rows = stmt.query_map(params![v, d, floor as i64], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Vec<u8>>(2)?,
        ))
    })?;
    let mut updates = Vec::new();
    let mut more = false;
    for row in rows {
        let (seq, device, blob) = row?;
        if blob.len() > budget && !must_take_one {
            more = true;
            break;
        }
        budget = budget.saturating_sub(blob.len());
        must_take_one = false;
        updates.push(StoredUpdate {
            seq: seq as u64,
            device_id: parse_id(&device, "device")?,
            blob,
        });
    }
    Ok(PullUpdatesResponse {
        snapshot,
        updates,
        more,
    })
}

/// Stores a client-made snapshot and drops the updates it covers. Idempotent, and a
/// snapshot older than the current one is ignored.
pub fn put_snapshot(
    conn: &mut Connection,
    vault: VaultId,
    doc: DocId,
    upto: u64,
    blob: &[u8],
    quota: u64,
) -> Result<(), HttpError> {
    let tx = conn.transaction()?;
    let (v, d) = (vault.to_string(), doc.to_string());
    let row: Option<(i64, Option<i64>, Option<i64>)> = tx
        .query_row(
            "SELECT latest_seq, snapshot_upto, length(snapshot_blob) FROM docs
             WHERE vault_id = ?1 AND doc_id = ?2",
            params![v, d],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    let Some((latest, current_upto, current_len)) = row else {
        return Err(HttpError::NotFound("no such document"));
    };
    if upto == 0 || upto > latest.max(0) as u64 {
        return Err(HttpError::BadRequest(
            "snapshot covers updates the server doesn't have".into(),
        ));
    }
    if current_upto.is_some_and(|c| c.max(0) as u64 >= upto) {
        return Ok(());
    }
    let freed: i64 = tx.query_row(
        "SELECT COALESCE(SUM(length(blob)), 0) FROM updates
         WHERE vault_id = ?1 AND doc_id = ?2 AND seq <= ?3",
        params![v, d, upto as i64],
        |row| row.get(0),
    )?;
    let freed = freed + current_len.unwrap_or(0);
    let after = (bytes_used(&tx, vault)? as i64 - freed).max(0) as u64 + blob.len() as u64;
    if after > quota {
        return Err(HttpError::QuotaExceeded);
    }
    tx.execute(
        "DELETE FROM updates WHERE vault_id = ?1 AND doc_id = ?2 AND seq <= ?3",
        params![v, d, upto as i64],
    )?;
    tx.execute(
        "UPDATE docs SET snapshot_upto = ?3, snapshot_blob = ?4 WHERE vault_id = ?1 AND doc_id = ?2",
        params![v, d, upto as i64, blob],
    )?;
    tx.execute(
        "UPDATE vaults SET bytes_used = ?2 WHERE id = ?1",
        params![v, after as i64],
    )?;
    tx.commit()?;
    Ok(())
}

/// One vault's footprint, for the admin listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultSummary {
    pub vault_id: VaultId,
    pub devices: u64,
    pub docs: u64,
    pub bytes_used: u64,
    pub created_at: i64,
    pub last_active: i64,
}

pub fn list_vaults(conn: &Connection) -> Result<Vec<VaultSummary>, HttpError> {
    let mut stmt = conn.prepare(
        "SELECT v.id, v.bytes_used, v.created_at, v.last_active,
                (SELECT COUNT(*) FROM devices d WHERE d.vault_id = v.id),
                (SELECT COUNT(*) FROM docs x WHERE x.vault_id = v.id)
         FROM vaults v ORDER BY v.created_at, v.id",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, i64>(3)?,
            row.get::<_, i64>(4)?,
            row.get::<_, i64>(5)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, bytes, created, active, devices, docs) = row?;
        out.push(VaultSummary {
            vault_id: parse_id(&id, "vault")?,
            devices: devices.max(0) as u64,
            docs: docs.max(0) as u64,
            bytes_used: bytes.max(0) as u64,
            created_at: created,
            last_active: active,
        });
    }
    Ok(out)
}

/// Pairing codes only matter for ten minutes; this drops the ones that are past it.
pub fn purge_expired_pairing_codes(conn: &Connection, now: i64) -> Result<usize, HttpError> {
    Ok(conn.execute("DELETE FROM pairing_codes WHERE expires_at < ?1", [now])?)
}

/// Vaults with no devices left whose last activity is older than `older_than_secs`:
/// the last device left (or was revoked) and nobody has pushed since. Not deleted — see
/// [`purge_empty_vaults`].
pub fn find_empty_vaults(
    conn: &Connection,
    now: i64,
    older_than_secs: i64,
) -> Result<Vec<VaultSummary>, HttpError> {
    let cutoff = now.saturating_sub(older_than_secs);
    Ok(list_vaults(conn)?
        .into_iter()
        .filter(|vault| vault.devices == 0 && vault.last_active < cutoff)
        .collect())
}

/// Deletes the vaults [`find_empty_vaults`] finds, returning them.
pub fn purge_empty_vaults(
    conn: &Connection,
    now: i64,
    older_than_secs: i64,
) -> Result<Vec<VaultSummary>, HttpError> {
    let found = find_empty_vaults(conn, now, older_than_secs)?;
    for vault in &found {
        delete_vault(conn, vault.vault_id)?;
    }
    Ok(found)
}

/// How much of the database file is unused space that a `VACUUM` would give back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fragmentation {
    pub free_bytes: u64,
    pub total_bytes: u64,
}

pub fn fragmentation(conn: &Connection) -> Result<Fragmentation, HttpError> {
    let page_size: i64 = conn.query_row("PRAGMA page_size", [], |r| r.get(0))?;
    let pages: i64 = conn.query_row("PRAGMA page_count", [], |r| r.get(0))?;
    let free: i64 = conn.query_row("PRAGMA freelist_count", [], |r| r.get(0))?;
    Ok(Fragmentation {
        free_bytes: (free.max(0) * page_size) as u64,
        total_bytes: (pages.max(0) * page_size) as u64,
    })
}

/// Rewrites the database file without its unused space. SQLite never shrinks the file by
/// itself, so after a lot of deletion (compaction, purged vaults) this is what returns
/// the disk space. Blocks other work for as long as it takes and needs free disk space of
/// about the database's size.
pub fn vacuum(conn: &Connection) -> Result<(), HttpError> {
    conn.execute_batch("VACUUM")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_000_000;
    const SALT: [u8; 16] = [7; 16];

    fn setup() -> (Connection, NewDevice) {
        let mut conn = open_in_memory().unwrap();
        let device = create_vault(&mut conn, &SALT, "laptop", NOW).unwrap();
        (conn, device)
    }

    fn doc(n: u128) -> DocId {
        DocId(Uuid::from_u128(n))
    }

    const BIG: u64 = 1 << 30;

    #[test]
    fn a_created_vault_authenticates_its_first_device_only_by_token() {
        let (conn, device) = setup();
        let authed = authenticate(&conn, &hash_secret(&device.token), NOW)
            .unwrap()
            .unwrap();
        assert_eq!(authed.vault_id, device.vault.vault_id);
        assert_eq!(authed.device_id, device.device_id);
        assert!(
            authenticate(&conn, &hash_secret("wrong"), NOW)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            get_vault(&conn, device.vault.vault_id)
                .unwrap()
                .unwrap()
                .kdf_salt,
            SALT
        );
    }

    #[test]
    fn tokens_are_stored_only_as_hashes() {
        let (conn, device) = setup();
        let stored: String = conn
            .query_row("SELECT token_hash FROM devices", [], |r| r.get(0))
            .unwrap();
        assert_ne!(stored, device.token);
        assert_eq!(stored, hash_secret(&device.token));
    }

    #[test]
    fn sequence_numbers_are_gap_free_per_document_from_one() {
        let (mut conn, device) = setup();
        let (v, dev) = (device.vault.vault_id, device.device_id);
        assert_eq!(
            push_update(&mut conn, v, doc(1), dev, b"a", BIG, NOW).unwrap(),
            1
        );
        assert_eq!(
            push_update(&mut conn, v, doc(1), dev, b"b", BIG, NOW).unwrap(),
            2
        );
        assert_eq!(
            push_update(&mut conn, v, doc(2), dev, b"c", BIG, NOW).unwrap(),
            1
        );

        let pulled = pull_updates(&conn, v, doc(1), 0).unwrap();
        assert_eq!(
            pulled.updates.iter().map(|u| u.seq).collect::<Vec<_>>(),
            [1, 2]
        );
        assert!(pulled.snapshot.is_none());
        assert_eq!(pull_updates(&conn, v, doc(1), 1).unwrap().updates.len(), 1);
        assert!(
            pull_updates(&conn, v, doc(9), 0)
                .unwrap()
                .updates
                .is_empty()
        );

        let docs = list_docs(&conn, v).unwrap();
        assert_eq!(docs.len(), 2);
        assert_eq!(
            docs.iter().find(|d| d.doc_id == doc(1)).unwrap().latest_seq,
            2
        );
    }

    #[test]
    fn large_histories_are_pulled_in_pages() {
        let (mut conn, device) = setup();
        let (v, dev) = (device.vault.vault_id, device.device_id);
        let third = vec![7u8; PULL_PAGE_BYTES / 3 + 1];
        for _ in 0..5 {
            push_update(&mut conn, v, doc(1), dev, &third, BIG, NOW).unwrap();
        }

        // Two blobs fit a page, a third wouldn't: the client is told to ask again.
        let mut since = 0;
        let mut pages = Vec::new();
        loop {
            let page = pull_updates(&conn, v, doc(1), since).unwrap();
            let seqs: Vec<u64> = page.updates.iter().map(|u| u.seq).collect();
            since = *seqs.last().unwrap();
            pages.push(seqs);
            if !page.more {
                break;
            }
        }
        assert_eq!(pages, [vec![1, 2], vec![3, 4], vec![5]]);

        // A single blob bigger than the page budget still comes through on its own.
        let huge = vec![1u8; PULL_PAGE_BYTES + 10];
        push_update(&mut conn, v, doc(2), dev, &huge, BIG, NOW).unwrap();
        push_update(&mut conn, v, doc(2), dev, b"small", BIG, NOW).unwrap();
        let page = pull_updates(&conn, v, doc(2), 0).unwrap();
        assert_eq!(page.updates.len(), 1);
        assert!(page.more);
    }

    #[test]
    fn a_snapshot_replaces_the_updates_it_covers() {
        let (mut conn, device) = setup();
        let (v, dev) = (device.vault.vault_id, device.device_id);
        for blob in [b"one", b"two", b"333"] {
            push_update(&mut conn, v, doc(1), dev, blob, BIG, NOW).unwrap();
        }
        put_snapshot(&mut conn, v, doc(1), 2, b"SNAP", BIG).unwrap();

        let fresh = pull_updates(&conn, v, doc(1), 0).unwrap();
        assert_eq!(fresh.snapshot.unwrap().upto_seq, 2);
        assert_eq!(fresh.updates.len(), 1);
        assert_eq!(fresh.updates[0].seq, 3);

        // A device already at seq 2 needs no snapshot, just the tail.
        let caught_up = pull_updates(&conn, v, doc(1), 2).unwrap();
        assert!(caught_up.snapshot.is_none());
        assert_eq!(caught_up.updates.len(), 1);

        // Sequence numbers keep counting after compaction.
        assert_eq!(
            push_update(&mut conn, v, doc(1), dev, b"x", BIG, NOW).unwrap(),
            4
        );
    }

    #[test]
    fn snapshots_beyond_the_latest_seq_or_stale_ones_are_handled() {
        let (mut conn, device) = setup();
        let (v, dev) = (device.vault.vault_id, device.device_id);
        push_update(&mut conn, v, doc(1), dev, b"a", BIG, NOW).unwrap();
        assert!(matches!(
            put_snapshot(&mut conn, v, doc(1), 5, b"S", BIG),
            Err(HttpError::BadRequest(_))
        ));
        assert!(matches!(
            put_snapshot(&mut conn, v, doc(7), 1, b"S", BIG),
            Err(HttpError::NotFound(_))
        ));
        put_snapshot(&mut conn, v, doc(1), 1, b"NEW", BIG).unwrap();
        // An older/equal snapshot must not overwrite the newer one.
        put_snapshot(&mut conn, v, doc(1), 1, b"OLD", BIG).unwrap();
        let blob = pull_updates(&conn, v, doc(1), 0)
            .unwrap()
            .snapshot
            .unwrap()
            .blob;
        assert_eq!(blob, b"NEW");
    }

    #[test]
    fn the_quota_caps_pushes_and_compaction_frees_space() {
        let (mut conn, device) = setup();
        let (v, dev) = (device.vault.vault_id, device.device_id);
        push_update(&mut conn, v, doc(1), dev, &[0; 60], 100, NOW).unwrap();
        assert!(matches!(
            push_update(&mut conn, v, doc(1), dev, &[0; 60], 100, NOW),
            Err(HttpError::QuotaExceeded)
        ));
        // Compacting 60 bytes down to a 10-byte snapshot leaves room again.
        put_snapshot(&mut conn, v, doc(1), 1, &[1; 10], 100).unwrap();
        push_update(&mut conn, v, doc(1), dev, &[0; 60], 100, NOW).unwrap();
        assert_eq!(bytes_used(&conn, v).unwrap(), 70);
    }

    #[test]
    fn pairing_codes_are_single_use_expire_and_are_stored_hashed() {
        let (mut conn, device) = setup();
        let vault = device.vault.vault_id;
        let code = create_pairing_code(&mut conn, vault, NOW, 600).unwrap();
        let stored: String = conn
            .query_row("SELECT code_hash FROM pairing_codes", [], |r| r.get(0))
            .unwrap();
        assert_ne!(stored, code);

        // Case, dashes and spacing don't matter when redeeming.
        let sloppy = code.to_lowercase().replace('-', " ");
        let second = redeem_pairing_code(&mut conn, &sloppy, "desktop", NOW + 5)
            .unwrap()
            .expect("valid code redeems");
        assert_eq!(second.vault.vault_id, vault);
        assert_ne!(second.device_id, device.device_id);
        assert_eq!(list_devices(&conn, vault).unwrap().len(), 2);

        assert!(
            redeem_pairing_code(&mut conn, &code, "again", NOW + 6)
                .unwrap()
                .is_none()
        );

        let expiring = create_pairing_code(&mut conn, vault, NOW, 600).unwrap();
        assert!(
            redeem_pairing_code(&mut conn, &expiring, "late", NOW + 601)
                .unwrap()
                .is_none()
        );
        assert!(
            redeem_pairing_code(&mut conn, &expiring, "later", NOW + 1)
                .unwrap()
                .is_none(),
            "an expired code is consumed too"
        );
        assert!(
            redeem_pairing_code(&mut conn, "nope", "x", NOW)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn too_many_outstanding_pairing_codes_are_refused() {
        let (mut conn, device) = setup();
        for _ in 0..MAX_OUTSTANDING_PAIRING_CODES {
            create_pairing_code(&mut conn, device.vault.vault_id, NOW, 600).unwrap();
        }
        assert!(create_pairing_code(&mut conn, device.vault.vault_id, NOW, 600).is_err());
        // Expired ones don't count.
        create_pairing_code(&mut conn, device.vault.vault_id, NOW + 601, 600).unwrap();
    }

    #[test]
    fn revoking_a_device_cuts_off_its_token() {
        let (mut conn, device) = setup();
        let vault = device.vault.vault_id;
        let code = create_pairing_code(&mut conn, vault, NOW, 600).unwrap();
        let other = redeem_pairing_code(&mut conn, &code, "phone", NOW)
            .unwrap()
            .unwrap();

        assert!(revoke_device(&conn, vault, other.device_id, NOW).unwrap());
        assert!(
            authenticate(&conn, &hash_secret(&other.token), NOW)
                .unwrap()
                .is_none()
        );
        assert!(
            authenticate(&conn, &hash_secret(&device.token), NOW)
                .unwrap()
                .is_some()
        );
        assert!(!revoke_device(&conn, vault, other.device_id, NOW).unwrap());
    }

    #[test]
    fn deleting_a_vault_removes_everything_it_owned() {
        let (mut conn, device) = setup();
        let (v, dev) = (device.vault.vault_id, device.device_id);
        push_update(&mut conn, v, doc(1), dev, b"a", BIG, NOW).unwrap();
        assert!(delete_vault(&conn, v).unwrap());
        for table in ["vaults", "devices", "docs", "updates", "pairing_codes"] {
            let n: i64 = conn
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
                .unwrap();
            assert_eq!(n, 0, "{table} should be empty");
        }
        assert!(
            authenticate(&conn, &hash_secret(&device.token), NOW)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn vaults_are_isolated_from_each_other() {
        let mut conn = open_in_memory().unwrap();
        let a = create_vault(&mut conn, &SALT, "a", NOW).unwrap();
        let b = create_vault(&mut conn, &SALT, "b", NOW).unwrap();
        push_update(
            &mut conn,
            a.vault.vault_id,
            doc(1),
            a.device_id,
            b"secret-a",
            BIG,
            NOW,
        )
        .unwrap();
        assert!(list_docs(&conn, b.vault.vault_id).unwrap().is_empty());
        assert!(
            pull_updates(&conn, b.vault.vault_id, doc(1), 0)
                .unwrap()
                .updates
                .is_empty()
        );
    }

    #[test]
    fn last_seen_is_refreshed_but_not_on_every_request() {
        let (conn, device) = setup();
        let hash = hash_secret(&device.token);
        authenticate(&conn, &hash, NOW).unwrap();
        authenticate(&conn, &hash, NOW + 5).unwrap();
        let seen: i64 = conn
            .query_row("SELECT last_seen FROM devices", [], |r| r.get(0))
            .unwrap();
        assert_eq!(seen, NOW);
        authenticate(&conn, &hash, NOW + LAST_SEEN_GRANULARITY_SECS).unwrap();
        let seen: i64 = conn
            .query_row("SELECT last_seen FROM devices", [], |r| r.get(0))
            .unwrap();
        assert_eq!(seen, NOW + LAST_SEEN_GRANULARITY_SECS);
    }

    fn add_device(conn: &mut Connection, vault: VaultId) -> NewDevice {
        let code = create_pairing_code(conn, vault, NOW, 600).unwrap();
        redeem_pairing_code(conn, &code, "extra", NOW)
            .unwrap()
            .unwrap()
    }

    const DAY: i64 = 86_400;

    #[test]
    fn an_old_v1_database_is_migrated_in_place() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(&format!(
            "BEGIN; {SCHEMA_V1} PRAGMA user_version = 1; COMMIT;"
        ))
        .unwrap();
        conn.execute(
            "INSERT INTO vaults (id, kdf_salt, key_version, created_at) VALUES ('v', x'00', 1, 500)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO docs (vault_id, doc_id, latest_seq) VALUES ('v', 'd', 1)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO updates (vault_id, doc_id, seq, device_id, blob, created_at) VALUES ('v', 'd', 1, 'x', x'00', 900)",
            [],
        )
        .unwrap();

        let conn = init(conn).unwrap();
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 2);
        let last_active: i64 = conn
            .query_row("SELECT last_active FROM vaults WHERE id = 'v'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(last_active, 900, "seeded from the latest update");
    }

    #[test]
    fn last_active_moves_on_push_and_when_a_device_leaves() {
        let (mut conn, device) = setup();
        let (v, dev) = (device.vault.vault_id, device.device_id);
        let active = |conn: &Connection| list_vaults(conn).unwrap()[0].last_active;
        assert_eq!(active(&conn), NOW);
        push_update(&mut conn, v, doc(1), dev, b"a", BIG, NOW + 100).unwrap();
        assert_eq!(active(&conn), NOW + 100);
        revoke_device(&conn, v, dev, NOW + 200).unwrap();
        assert_eq!(active(&conn), NOW + 200);
    }

    #[test]
    fn only_vaults_with_no_devices_and_past_the_retention_are_purged() {
        let mut conn = open_in_memory().unwrap();
        let keep_with_device = create_vault(&mut conn, &SALT, "a", NOW).unwrap();
        let keep_recent = create_vault(&mut conn, &SALT, "b", NOW).unwrap();
        let stale = create_vault(&mut conn, &SALT, "c", NOW).unwrap();
        push_update(
            &mut conn,
            stale.vault.vault_id,
            doc(1),
            stale.device_id,
            b"x",
            BIG,
            NOW,
        )
        .unwrap();
        revoke_device(
            &conn,
            keep_recent.vault.vault_id,
            keep_recent.device_id,
            NOW + 29 * DAY,
        )
        .unwrap();
        revoke_device(&conn, stale.vault.vault_id, stale.device_id, NOW + DAY).unwrap();

        let now = NOW + 40 * DAY;
        let found = find_empty_vaults(&conn, now, 30 * DAY).unwrap();
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].vault_id, stale.vault.vault_id);
        assert_eq!(
            list_vaults(&conn).unwrap().len(),
            3,
            "finding deletes nothing"
        );

        let purged = purge_empty_vaults(&conn, now, 30 * DAY).unwrap();
        assert_eq!(purged.len(), 1);
        let remaining: Vec<_> = list_vaults(&conn)
            .unwrap()
            .into_iter()
            .map(|v| v.vault_id)
            .collect();
        assert!(remaining.contains(&keep_with_device.vault.vault_id));
        assert!(remaining.contains(&keep_recent.vault.vault_id));
        assert!(!remaining.contains(&stale.vault.vault_id));
        // Its data went with it.
        let updates: i64 = conn
            .query_row("SELECT COUNT(*) FROM updates", [], |r| r.get(0))
            .unwrap();
        assert_eq!(updates, 0);
    }

    #[test]
    fn a_vault_that_still_has_a_device_is_never_purged_however_old() {
        let (mut conn, first) = setup();
        let vault = first.vault.vault_id;
        let second = add_device(&mut conn, vault);
        // One device leaves; the other remains.
        revoke_device(&conn, vault, first.device_id, NOW).unwrap();
        assert!(
            find_empty_vaults(&conn, NOW + 999 * DAY, DAY)
                .unwrap()
                .is_empty()
        );
        // Once the last one leaves, the clock starts from then.
        revoke_device(&conn, vault, second.device_id, NOW + 10 * DAY).unwrap();
        assert!(
            find_empty_vaults(&conn, NOW + 10 * DAY + 100, DAY)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            find_empty_vaults(&conn, NOW + 12 * DAY, DAY).unwrap().len(),
            1
        );
    }

    #[test]
    fn expired_pairing_codes_are_purged_and_live_ones_kept() {
        let (mut conn, device) = setup();
        create_pairing_code(&mut conn, device.vault.vault_id, NOW, 600).unwrap();
        create_pairing_code(&mut conn, device.vault.vault_id, NOW + 1000, 600).unwrap();
        // The second creation already purged the first (it had expired by then).
        assert_eq!(purge_expired_pairing_codes(&conn, NOW + 1000).unwrap(), 0);
        assert_eq!(purge_expired_pairing_codes(&conn, NOW + 5000).unwrap(), 1);
        assert_eq!(purge_expired_pairing_codes(&conn, NOW + 5000).unwrap(), 0);
    }

    #[test]
    fn vacuum_returns_space_after_data_is_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = open(&dir.path().join("t.sqlite3")).unwrap();
        let device = create_vault(&mut conn, &SALT, "a", NOW).unwrap();
        let (v, dev) = (device.vault.vault_id, device.device_id);
        for n in 0..40u128 {
            push_update(&mut conn, v, doc(n), dev, &vec![7u8; 100_000], BIG, NOW).unwrap();
        }
        assert!(delete_vault(&conn, v).unwrap());
        let before = fragmentation(&conn).unwrap();
        assert!(before.free_bytes > 1_000_000, "{before:?}");

        vacuum(&conn).unwrap();
        let after = fragmentation(&conn).unwrap();
        assert!(after.free_bytes < before.free_bytes / 10, "{after:?}");
        assert!(after.total_bytes < before.total_bytes);
    }

    #[test]
    fn a_newer_schema_version_is_refused_rather_than_misread() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA user_version = 99;").unwrap();
        assert!(init(conn).is_err());
    }
}
