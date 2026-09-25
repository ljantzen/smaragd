//! Full-stack tests: the real sync server, the real HTTP client, real Argon2id keys
//! and several real `SyncEngine`s syncing real project folders — nothing faked.

use std::collections::BTreeMap;
use std::path::PathBuf;

use smaragd::project::store::native_store;
use smaragd::sync::client::{HttpClient, HttpTransport};
use smaragd::sync::crypto::derive_vault_key;
use smaragd::sync::engine::{EngineConfig, SyncEngine, SyncError, SyncReport};
use smaragd::sync::state::DirStateStore;
use smaragd::sync::transport::{SyncTransport, TransportError};
use smaragd_sync_protocol::ticket::ServerAddr;
use smaragd_sync_protocol::{DeviceId, VaultId};
use smaragd_sync_server::{Config, RunningServer, start_in_background};

const ADMIN: &str = "admin-secret";
const PASSPHRASE: &str = "a long, unique passphrase";

fn server_config(data_dir: PathBuf, listen: &str) -> Config {
    Config {
        listen_addr: listen.into(),
        data_dir,
        allow_open_registration: false,
        admin_token: Some(ADMIN.into()),
        vault_quota_bytes: 1 << 30,
    }
}

fn addr_of(server: &RunningServer) -> ServerAddr {
    ServerAddr {
        host: "127.0.0.1".into(),
        port: server.addr.port(),
        use_tls: false,
        path: String::new(),
    }
}

struct Device {
    project: tempfile::TempDir,
    _state: tempfile::TempDir,
    engine: SyncEngine,
    transport: HttpTransport,
    client: HttpClient,
    vault: VaultId,
    device_id: DeviceId,
}

impl Device {
    fn new(
        client: HttpClient,
        vault: &smaragd_sync_protocol::api::VaultInfo,
        device_id: DeviceId,
        passphrase: &str,
    ) -> Self {
        let project = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let key = derive_vault_key(passphrase, &vault.kdf_salt, vault.key_version).unwrap();
        let engine = SyncEngine::open(
            EngineConfig {
                vault: vault.vault_id,
                device: device_id,
                key,
                root: project.path().to_path_buf(),
            },
            native_store(),
            Box::new(DirStateStore::new(
                native_store(),
                state.path().to_path_buf(),
            )),
        )
        .unwrap();
        Self {
            transport: client.transport(vault.vault_id),
            project,
            _state: state,
            engine,
            client,
            vault: vault.vault_id,
            device_id,
        }
    }

    fn write(&self, rel: &str, text: &str) {
        let path = self.project.path().join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn read(&self, rel: &str) -> String {
        std::fs::read_to_string(self.project.path().join(rel)).unwrap()
    }

    fn files(&self) -> BTreeMap<String, String> {
        let mut out = BTreeMap::new();
        let mut stack = vec![self.project.path().to_path_buf()];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|e| e == "md") {
                    let rel = path.strip_prefix(self.project.path()).unwrap();
                    out.insert(
                        rel.to_string_lossy().replace('\\', "/"),
                        std::fs::read_to_string(&path).unwrap(),
                    );
                }
            }
        }
        out
    }

    fn sync(&mut self) -> Result<SyncReport, SyncError> {
        self.engine.sync_once(&self.transport)
    }
}

fn converge(devices: &mut [&mut Device]) {
    for _ in 0..8 {
        let mut quiet = true;
        for device in devices.iter_mut() {
            quiet &= device.sync().expect("sync").is_quiet();
        }
        if quiet {
            return;
        }
    }
    panic!("devices never settled");
}

/// Creates a vault as a first device.
fn first_device(server: &RunningServer, passphrase: &str) -> Device {
    let anonymous = HttpClient::new(&addr_of(server));
    let salt = *uuid::Uuid::new_v4().as_bytes();
    let created = anonymous
        .create_vault(Some(ADMIN), "laptop", &salt)
        .expect("admin can create a vault");
    Device::new(
        anonymous.with_token(created.device_token),
        &created.vault,
        created.device_id,
        passphrase,
    )
}

/// Pairs another device into `existing`'s vault using a pairing code.
fn pair(server: &RunningServer, existing: &Device, name: &str, passphrase: &str) -> Device {
    let code = existing
        .client
        .create_pairing_code(existing.vault)
        .expect("pairing code");
    let anonymous = HttpClient::new(&addr_of(server));
    let joined = anonymous
        .redeem_pairing_code(&code.code, name)
        .expect("redeem");
    Device::new(
        anonymous.with_token(joined.device_token),
        &joined.vault,
        joined.device_id,
        passphrase,
    )
}

#[test]
fn two_devices_sync_a_project_through_the_real_server() {
    let data = tempfile::tempdir().unwrap();
    let server = start_in_background(server_config(data.path().into(), "127.0.0.1:0")).unwrap();

    // Creating a vault without the admin token is refused.
    let anonymous = HttpClient::new(&addr_of(&server));
    assert!(anonymous.health().is_ok());
    assert!(matches!(
        anonymous.create_vault(None, "intruder", &[1; 16]),
        Err(TransportError::Rejected(_))
    ));

    let mut a = first_device(&server, PASSPHRASE);
    a.write("Chapter One.md", "TOPSECRETPROSE opens the story.\n");
    a.write(
        "Notes/Idea.md",
        "---\nstatus: draft\npov: Anna\n---\nWhat if?\n",
    );
    let report = a.sync().unwrap();
    assert!(report.pushed_updates >= 3, "{report:?}");

    let mut b = pair(&server, &a, "desktop", PASSPHRASE);
    converge(&mut [&mut a, &mut b]);
    assert_eq!(a.files(), b.files());
    assert_eq!(
        b.read("Chapter One.md"),
        "TOPSECRETPROSE opens the story.\n"
    );

    // Concurrent edits — including frontmatter keys — merge across the wire.
    a.write(
        "Chapter One.md",
        "TOPSECRETPROSE opens the story.\nA adds a line.\n",
    );
    b.write(
        "Notes/Idea.md",
        "---\nstatus: draft\npov: Bo\n---\nWhat if?\n",
    );
    a.write(
        "Notes/Idea.md",
        "---\nstatus: final\npov: Anna\n---\nWhat if?\n",
    );
    converge(&mut [&mut a, &mut b]);
    assert_eq!(a.files(), b.files());
    let idea = a.read("Notes/Idea.md");
    assert!(
        idea.contains("status: final") && idea.contains("pov: Bo"),
        "{idea}"
    );

    // A rename and a delete propagate.
    std::fs::rename(
        a.project.path().join("Chapter One.md"),
        a.project.path().join("Chapter 1.md"),
    )
    .unwrap();
    std::fs::remove_file(b.project.path().join("Notes/Idea.md")).unwrap();
    converge(&mut [&mut a, &mut b]);
    assert_eq!(a.files(), b.files());
    assert!(a.files().contains_key("Chapter 1.md"));
    assert!(!a.files().contains_key("Notes/Idea.md"));

    // Everything the server holds is unreadable ciphertext.
    for summary in a.transport.list_docs().unwrap() {
        let pulled = a.transport.pull(summary.doc_id, 0).unwrap();
        for blob in pulled
            .updates
            .iter()
            .map(|u| &u.blob)
            .chain(pulled.snapshot.iter().map(|s| &s.blob))
        {
            let text = String::from_utf8_lossy(blob);
            assert!(
                !text.contains("TOPSECRET"),
                "plaintext leaked to the server"
            );
            assert!(!text.contains("Chapter"), "file name leaked to the server");
        }
    }

    // Device management: both listed, revoking B cuts it off, A carries on.
    let devices = a.client.list_devices(a.vault).unwrap();
    assert_eq!(devices.len(), 2);
    a.client.revoke_device(a.vault, b.device_id).unwrap();
    assert!(matches!(
        b.sync(),
        Err(SyncError::Transport(TransportError::Unauthorized))
    ));
    a.write("After.md", "still working\n");
    assert!(a.sync().is_ok());
}

#[test]
fn a_device_with_the_wrong_passphrase_is_told_so_and_writes_nothing() {
    let data = tempfile::tempdir().unwrap();
    let server = start_in_background(server_config(data.path().into(), "127.0.0.1:0")).unwrap();

    let mut a = first_device(&server, PASSPHRASE);
    a.write("secret.md", "chapter one\n");
    a.sync().unwrap();

    let mut mistyped = pair(&server, &a, "phone", "a different passphrase");
    assert!(matches!(mistyped.sync(), Err(SyncError::WrongPassphrase)));
    assert!(mistyped.files().is_empty());
}

#[test]
fn pairing_codes_work_once() {
    let data = tempfile::tempdir().unwrap();
    let server = start_in_background(server_config(data.path().into(), "127.0.0.1:0")).unwrap();
    let a = first_device(&server, PASSPHRASE);

    let code = a.client.create_pairing_code(a.vault).unwrap().code;
    let anonymous = HttpClient::new(&addr_of(&server));
    assert!(anonymous.redeem_pairing_code(&code, "one").is_ok());
    assert!(matches!(
        anonymous.redeem_pairing_code(&code, "two"),
        Err(TransportError::Rejected(_))
    ));
}

#[test]
fn edits_made_while_the_server_is_down_sync_when_it_returns() {
    let data = tempfile::tempdir().unwrap();
    let first = start_in_background(server_config(data.path().into(), "127.0.0.1:0")).unwrap();
    let port = first.addr.port();

    let mut a = first_device(&first, PASSPHRASE);
    a.write("a.md", "start\n");
    a.sync().unwrap();
    let mut b = pair(&first, &a, "desktop", PASSPHRASE);
    converge(&mut [&mut a, &mut b]);

    drop(first);
    a.write("a.md", "start\nwritten while the server was down\n");
    a.write("new.md", "created while offline\n");
    assert!(matches!(
        a.sync(),
        Err(SyncError::Transport(TransportError::Offline(_)))
    ));

    // Same data directory, same port: the server comes back with its state intact.
    let _second = start_in_background(server_config(
        data.path().into(),
        &format!("127.0.0.1:{port}"),
    ))
    .unwrap();
    converge(&mut [&mut a, &mut b]);
    assert_eq!(b.read("a.md"), "start\nwritten while the server was down\n");
    assert_eq!(b.read("new.md"), "created while offline\n");
    assert_eq!(a.files(), b.files());
}
