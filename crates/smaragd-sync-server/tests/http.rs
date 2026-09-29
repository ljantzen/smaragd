//! End-to-end tests of the HTTP API against a real server on an ephemeral port.

use serde::Serialize;
use serde::de::DeserializeOwned;
use smaragd_sync_protocol::api::{
    ADMIN_TOKEN_HEADER, ApiError, CreatePairingCodeResponse, CreateVaultRequest,
    CreateVaultResponse, HealthResponse, KDF_SALT_LEN, ListDevicesResponse, ListDocsResponse,
    MAX_BLOB_BYTES, PullUpdatesResponse, PushUpdateResponse, RedeemPairingRequest,
    RedeemPairingResponse, Snapshot, VaultInfo,
};
use smaragd_sync_protocol::envelope::Envelope;
use smaragd_sync_protocol::{DocId, VaultId};
use smaragd_sync_server::{Config, RunningServer, start_in_background};
use uuid::Uuid;

struct Harness {
    server: RunningServer,
    agent: ureq::Agent,
    _dir: tempfile::TempDir,
}

fn harness(open: bool, admin: Option<&str>, quota: u64) -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let server = start_in_background(Config {
        listen_addr: "127.0.0.1:0".into(),
        data_dir: dir.path().to_path_buf(),
        allow_open_registration: open,
        admin_token: admin.map(str::to_string),
        vault_quota_bytes: quota,
        maintenance_interval: None,
        empty_vault_retention: None,
        max_file_bytes: Some(100 * 1024 * 1024),
    })
    .unwrap();
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .build()
        .into();
    Harness {
        server,
        agent,
        _dir: dir,
    }
}

struct Reply {
    status: u16,
    body: Vec<u8>,
}

impl Reply {
    fn json<T: DeserializeOwned>(&self) -> T {
        serde_json::from_slice(&self.body)
            .unwrap_or_else(|e| panic!("bad JSON ({e}): {}", String::from_utf8_lossy(&self.body)))
    }
}

impl Harness {
    fn send(
        &self,
        method: &str,
        path: &str,
        token: Option<&str>,
        extra: &[(&str, &str)],
        body: Option<(&str, Vec<u8>)>,
    ) -> Reply {
        let url = format!("{}/v1{path}", self.server.base_url());
        macro_rules! finish {
            ($req:expr) => {{
                let mut req = $req;
                if let Some(token) = token {
                    req = req.header("Authorization", format!("Bearer {token}"));
                }
                for (name, value) in extra {
                    req = req.header(*name, *value);
                }
                req
            }};
        }
        let mut response = match (method, body) {
            ("GET", None) => finish!(self.agent.get(&url)).call(),
            ("DELETE", None) => finish!(self.agent.delete(&url)).call(),
            ("POST", Some((kind, bytes))) => finish!(self.agent.post(&url))
                .header("Content-Type", kind)
                .send(&bytes[..]),
            ("PUT", Some((kind, bytes))) => finish!(self.agent.put(&url))
                .header("Content-Type", kind)
                .send(&bytes[..]),
            ("POST", None) => finish!(self.agent.post(&url)).send_empty(),
            other => panic!("unsupported {other:?}"),
        }
        .unwrap();
        Reply {
            status: response.status().as_u16(),
            body: response.body_mut().read_to_vec().unwrap(),
        }
    }

    fn get(&self, path: &str, token: Option<&str>) -> Reply {
        self.send("GET", path, token, &[], None)
    }

    fn delete(&self, path: &str, token: &str) -> Reply {
        self.send("DELETE", path, Some(token), &[], None)
    }

    fn post_json(&self, path: &str, token: Option<&str>, value: &impl Serialize) -> Reply {
        let body = serde_json::to_vec(value).unwrap();
        self.send("POST", path, token, &[], Some(("application/json", body)))
    }

    fn create_vault_with(&self, extra: &[(&str, &str)], salt_len: usize) -> Reply {
        let body = serde_json::to_vec(&CreateVaultRequest {
            device_name: "laptop".into(),
            kdf_salt: vec![3; salt_len],
        })
        .unwrap();
        self.send(
            "POST",
            "/vaults",
            None,
            extra,
            Some(("application/json", body)),
        )
    }

    /// Creates a vault (on a server where that's allowed without an admin token).
    fn vault(&self) -> CreateVaultResponse {
        let reply = self.create_vault_with(&[], KDF_SALT_LEN);
        assert_eq!(
            reply.status,
            201,
            "{}",
            String::from_utf8_lossy(&reply.body)
        );
        reply.json()
    }

    fn push(&self, vault: VaultId, doc: DocId, token: &str, blob: Vec<u8>) -> Reply {
        self.send(
            "POST",
            &format!("/vaults/{vault}/docs/{doc}/updates"),
            Some(token),
            &[],
            Some(("application/octet-stream", blob)),
        )
    }
}

fn envelope(payload: usize) -> Vec<u8> {
    Envelope::new(1, [0; 24], vec![7; 16 + payload]).to_bytes()
}

fn doc(n: u128) -> DocId {
    DocId(Uuid::from_u128(n))
}

const GB: u64 = 1 << 30;

#[test]
fn health_needs_no_credentials() {
    let h = harness(true, None, GB);
    let reply = h.get("/health", None);
    assert_eq!(reply.status, 200);
    assert_eq!(reply.json::<HealthResponse>().status, "ok");
}

#[test]
fn vault_creation_is_closed_unless_open_or_given_the_admin_token() {
    let h = harness(false, Some("letmein"), GB);
    assert_eq!(h.create_vault_with(&[], KDF_SALT_LEN).status, 403);
    assert_eq!(
        h.create_vault_with(&[(ADMIN_TOKEN_HEADER, "wrong")], KDF_SALT_LEN)
            .status,
        403
    );
    assert_eq!(
        h.create_vault_with(&[(ADMIN_TOKEN_HEADER, "letmein")], KDF_SALT_LEN)
            .status,
        201
    );

    let no_admin_configured = harness(false, None, GB);
    assert_eq!(
        no_admin_configured
            .create_vault_with(&[(ADMIN_TOKEN_HEADER, "")], KDF_SALT_LEN)
            .status,
        403
    );

    let open = harness(true, None, GB);
    assert_eq!(open.create_vault_with(&[], KDF_SALT_LEN).status, 201);
}

#[test]
fn a_salt_of_the_wrong_length_is_rejected() {
    let h = harness(true, None, GB);
    assert_eq!(h.create_vault_with(&[], 5).status, 400);
}

#[test]
fn every_vault_endpoint_requires_a_valid_token_for_that_vault() {
    let h = harness(true, None, GB);
    let a = h.vault();
    let b = h.vault();
    let vault = a.vault.vault_id;

    for path in [
        format!("/vaults/{vault}"),
        format!("/vaults/{vault}/docs"),
        format!("/vaults/{vault}/devices"),
        format!("/vaults/{vault}/docs/{}/updates", doc(1)),
    ] {
        assert_eq!(h.get(&path, None).status, 401, "{path} without a token");
        assert_eq!(
            h.get(&path, Some("bogus")).status,
            401,
            "{path} bogus token"
        );
        assert_eq!(
            h.get(&path, Some(&b.device_token)).status,
            403,
            "{path} with another vault's token"
        );
        assert_eq!(h.get(&path, Some(&a.device_token)).status, 200, "{path}");
    }
    assert_eq!(
        h.push(vault, doc(1), &b.device_token, envelope(1)).status,
        403
    );
    assert_eq!(
        h.delete(&format!("/vaults/{vault}"), &b.device_token)
            .status,
        403
    );
}

#[test]
fn pushed_updates_come_back_in_order_with_gap_free_sequence_numbers() {
    let h = harness(true, None, GB);
    let v = h.vault();
    let (vault, token) = (v.vault.vault_id, v.device_token.as_str());

    for expected in 1..=3u64 {
        let reply = h.push(vault, doc(1), token, envelope(expected as usize));
        assert_eq!(reply.status, 200);
        assert_eq!(reply.json::<PushUpdateResponse>().seq, expected);
    }
    let docs: ListDocsResponse = h.get(&format!("/vaults/{vault}/docs"), Some(token)).json();
    assert_eq!(docs.docs.len(), 1);
    assert_eq!(docs.docs[0].latest_seq, 3);
    assert_eq!(docs.max_file_bytes, Some(100 * 1024 * 1024));

    let pulled: PullUpdatesResponse = h
        .get(
            &format!("/vaults/{vault}/docs/{}/updates?since=1", doc(1)),
            Some(token),
        )
        .json();
    assert_eq!(
        pulled.updates.iter().map(|u| u.seq).collect::<Vec<_>>(),
        [2, 3]
    );
    assert_eq!(pulled.updates[0].device_id, v.device_id);
    assert_eq!(pulled.updates[0].blob, envelope(2));
}

#[test]
fn malformed_and_oversized_pushes_are_refused() {
    let h = harness(true, None, GB);
    let v = h.vault();
    let (vault, token) = (v.vault.vault_id, v.device_token.as_str());

    assert_eq!(
        h.push(vault, doc(1), token, b"not an envelope".to_vec())
            .status,
        400
    );
    assert_eq!(h.push(vault, doc(1), token, vec![]).status, 400);
    assert_eq!(
        h.push(vault, doc(1), token, vec![1; MAX_BLOB_BYTES + 1])
            .status,
        413
    );
    let none: ListDocsResponse = h.get(&format!("/vaults/{vault}/docs"), Some(token)).json();
    assert!(none.docs.is_empty(), "rejected pushes must store nothing");
}

#[test]
fn the_vault_quota_returns_507() {
    let h = harness(true, None, 200);
    let v = h.vault();
    let (vault, token) = (v.vault.vault_id, v.device_token.as_str());
    let first = envelope(60); // 2 + 24 + 16 + 60 = 102 bytes
    assert_eq!(h.push(vault, doc(1), token, first.clone()).status, 200);
    assert_eq!(h.push(vault, doc(1), token, first).status, 507);
}

#[test]
fn a_second_device_pairs_with_a_single_use_code_and_can_be_revoked() {
    let h = harness(true, None, GB);
    let first = h.vault();
    let vault = first.vault.vault_id;
    let token = first.device_token.as_str();
    h.push(vault, doc(1), token, envelope(4));

    let code: CreatePairingCodeResponse = h
        .send(
            "POST",
            &format!("/vaults/{vault}/pairing-codes"),
            Some(token),
            &[],
            None,
        )
        .json();
    assert_eq!(code.expires_in_secs, 600);

    let redeem = |code: &str| {
        h.post_json(
            "/pairing/redeem",
            None,
            &RedeemPairingRequest {
                code: code.into(),
                device_name: "phone".into(),
            },
        )
    };
    // Sloppy typing is fine.
    let reply = redeem(&code.code.to_lowercase().replace('-', " "));
    assert_eq!(reply.status, 200);
    let second: RedeemPairingResponse = reply.json();
    assert_eq!(second.vault.vault_id, vault);
    assert_eq!(second.vault.kdf_salt, first.vault.kdf_salt);
    assert_ne!(second.device_token, first.device_token);

    // The code is spent; a wrong one gets the same generic answer.
    assert_eq!(redeem(&code.code).status, 404);
    assert_eq!(redeem("AAAA-BBBB-CCCC").status, 404);

    // The new device sees the vault's data, and both are listed.
    let pulled: PullUpdatesResponse = h
        .get(
            &format!("/vaults/{vault}/docs/{}/updates", doc(1)),
            Some(&second.device_token),
        )
        .json();
    assert_eq!(pulled.updates.len(), 1);
    let devices: ListDevicesResponse = h
        .get(&format!("/vaults/{vault}/devices"), Some(token))
        .json();
    assert_eq!(devices.devices.len(), 2);

    // Revoking cuts the second device off immediately.
    let revoke = h.delete(
        &format!("/vaults/{vault}/devices/{}", second.device_id),
        token,
    );
    assert_eq!(revoke.status, 204);
    assert_eq!(
        h.get(&format!("/vaults/{vault}/docs"), Some(&second.device_token))
            .status,
        401
    );
    assert_eq!(
        h.delete(
            &format!("/vaults/{vault}/devices/{}", second.device_id),
            token
        )
        .status,
        404
    );
}

#[test]
fn compaction_over_http_replaces_covered_updates() {
    let h = harness(true, None, GB);
    let v = h.vault();
    let (vault, token) = (v.vault.vault_id, v.device_token.as_str());
    for n in 1..=3 {
        h.push(vault, doc(1), token, envelope(n));
    }
    let path = format!("/vaults/{vault}/docs/{}/snapshot", doc(1));
    let put = |upto: u64, blob: Vec<u8>| {
        h.send(
            "PUT",
            &path,
            Some(token),
            &[],
            Some((
                "application/json",
                serde_json::to_vec(&Snapshot {
                    upto_seq: upto,
                    blob,
                })
                .unwrap(),
            )),
        )
    };
    assert_eq!(
        put(2, b"garbage".to_vec()).status,
        400,
        "must look like an envelope"
    );
    assert_eq!(
        put(9, envelope(1)).status,
        400,
        "can't cover updates that don't exist"
    );
    assert_eq!(put(2, envelope(5)).status, 204);

    let pulled: PullUpdatesResponse = h
        .get(
            &format!("/vaults/{vault}/docs/{}/updates", doc(1)),
            Some(token),
        )
        .json();
    assert_eq!(pulled.snapshot.unwrap().upto_seq, 2);
    assert_eq!(pulled.updates.len(), 1);
    assert_eq!(pulled.updates[0].seq, 3);
}

#[test]
fn deleting_a_vault_removes_its_data_and_credentials() {
    let h = harness(true, None, GB);
    let v = h.vault();
    let (vault, token) = (v.vault.vault_id, v.device_token.as_str());
    h.push(vault, doc(1), token, envelope(4));

    assert_eq!(h.delete(&format!("/vaults/{vault}"), token).status, 204);
    assert_eq!(h.get(&format!("/vaults/{vault}"), Some(token)).status, 401);
    let info = h.vault();
    assert_ne!(info.vault.vault_id, vault);
}

#[test]
fn errors_are_json_and_never_leak_internals() {
    let h = harness(true, None, GB);
    let reply = h.get("/vaults/not-a-uuid", Some("x"));
    assert!(reply.status == 400 || reply.status == 401 || reply.status == 404);
    let reply = h.get(&format!("/vaults/{}", Uuid::nil()), Some("bogus"));
    assert_eq!(reply.status, 401);
    let error: ApiError = reply.json();
    assert!(!error.error.to_lowercase().contains("sql"));
}

#[test]
fn state_survives_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let config = || Config {
        listen_addr: "127.0.0.1:0".into(),
        data_dir: dir.path().to_path_buf(),
        allow_open_registration: true,
        admin_token: None,
        vault_quota_bytes: GB,
        maintenance_interval: None,
        empty_vault_retention: None,
        max_file_bytes: Some(100 * 1024 * 1024),
    };
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .build()
        .into();

    let first = start_in_background(config()).unwrap();
    let body = serde_json::to_vec(&CreateVaultRequest {
        device_name: "d".into(),
        kdf_salt: vec![1; KDF_SALT_LEN],
    })
    .unwrap();
    let created: CreateVaultResponse = serde_json::from_slice(
        &agent
            .post(&format!("{}/v1/vaults", first.base_url()))
            .header("Content-Type", "application/json")
            .send(&body[..])
            .unwrap()
            .body_mut()
            .read_to_vec()
            .unwrap(),
    )
    .unwrap();
    drop(first);

    let second = start_in_background(config()).unwrap();
    let mut response = agent
        .get(&format!(
            "{}/v1/vaults/{}",
            second.base_url(),
            created.vault.vault_id
        ))
        .header("Authorization", format!("Bearer {}", created.device_token))
        .call()
        .unwrap();
    assert_eq!(response.status().as_u16(), 200);
    let info: VaultInfo =
        serde_json::from_slice(&response.body_mut().read_to_vec().unwrap()).unwrap();
    assert_eq!(info, created.vault);
}

#[test]
fn the_background_task_deletes_a_vault_whose_last_device_left() {
    let dir = tempfile::tempdir().unwrap();
    let server = start_in_background(Config {
        listen_addr: "127.0.0.1:0".into(),
        data_dir: dir.path().to_path_buf(),
        allow_open_registration: true,
        admin_token: None,
        vault_quota_bytes: GB,
        maintenance_interval: Some(std::time::Duration::from_secs(1)),
        empty_vault_retention: Some(std::time::Duration::from_secs(1)),
        max_file_bytes: Some(100 * 1024 * 1024),
    })
    .unwrap();
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .build()
        .into();
    let create = |name: &str| -> CreateVaultResponse {
        let body = serde_json::to_vec(&CreateVaultRequest {
            device_name: name.into(),
            kdf_salt: vec![1; KDF_SALT_LEN],
        })
        .unwrap();
        serde_json::from_slice(
            &agent
                .post(&format!("{}/v1/vaults", server.base_url()))
                .header("Content-Type", "application/json")
                .send(&body[..])
                .unwrap()
                .body_mut()
                .read_to_vec()
                .unwrap(),
        )
        .unwrap()
    };
    let abandoned = create("leaving");
    let kept = create("staying");

    // The first vault's only device removes itself.
    let response = agent
        .delete(&format!(
            "{}/v1/vaults/{}/devices/{}",
            server.base_url(),
            abandoned.vault.vault_id,
            abandoned.device_id
        ))
        .header(
            "Authorization",
            format!("Bearer {}", abandoned.device_token),
        )
        .call()
        .unwrap();
    assert_eq!(response.status().as_u16(), 204);

    // Within a few seconds maintenance removes it (and only it).
    let db_path = dir.path().join("sync.sqlite3");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        let conn = smaragd_sync_server::db::open(&db_path).unwrap();
        let ids: Vec<_> = smaragd_sync_server::db::list_vaults(&conn)
            .unwrap()
            .into_iter()
            .map(|v| v.vault_id)
            .collect();
        if !ids.contains(&abandoned.vault.vault_id) {
            assert_eq!(ids, vec![kept.vault.vault_id]);
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the abandoned vault was never purged"
        );
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
}
