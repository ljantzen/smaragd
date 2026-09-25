//! The user-facing setup path against a real server: create a vault (admin token flow),
//! join with a ticket, manage devices, sync in the background with the real runner, get
//! revoked, and leave.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use smaragd::project::store::{ProjectStore, native_store};
use smaragd::sync::link::{DeviceCredentials, ProjectLink, state_dir};
use smaragd::sync::pairing::{self, PairError};
use smaragd::sync::runner::{self, HaltReason, StartParams, SyncEvent, SyncRunner};
use smaragd::sync::state::DirStateStore;
use smaragd_sync_protocol::ticket::{ServerAddr, SyncTicket};
use smaragd_sync_server::{Config, RunningServer, start_in_background};

const ADMIN: &str = "admin-secret";
const PASSPHRASE: &str = "correct horse battery";

fn server() -> (RunningServer, ServerAddr, tempfile::TempDir) {
    let data = tempfile::tempdir().unwrap();
    let server = start_in_background(Config {
        listen_addr: "127.0.0.1:0".into(),
        data_dir: data.path().to_path_buf(),
        allow_open_registration: false,
        admin_token: Some(ADMIN.into()),
        vault_quota_bytes: 1 << 30,
    })
    .unwrap();
    let addr = ServerAddr {
        host: "127.0.0.1".into(),
        port: server.addr.port(),
        use_tls: false,
        path: String::new(),
    };
    (server, addr, data)
}

struct Project {
    dir: tempfile::TempDir,
    data_root: tempfile::TempDir,
    files: Arc<dyn ProjectStore>,
}

impl Project {
    fn new() -> Self {
        Self {
            dir: tempfile::tempdir().unwrap(),
            data_root: tempfile::tempdir().unwrap(),
            files: native_store(),
        }
    }

    fn root(&self) -> &Path {
        self.dir.path()
    }

    fn write(&self, rel: &str, text: &str) {
        let path = self.root().join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn read(&self, rel: &str) -> Option<String> {
        std::fs::read_to_string(self.root().join(rel)).ok()
    }

    fn start(&self, paired: &pairing::Paired, passphrase: &str, interval: Duration) -> SyncRunner {
        runner::start(
            StartParams {
                link: paired.link.clone(),
                credentials: paired.credentials.clone(),
                passphrase: passphrase.into(),
                project_root: self.root().to_path_buf(),
                files: self.files.clone(),
                state_dir: state_dir(self.data_root.path(), paired.link.vault_id),
            },
            interval,
            Box::new(|| {}),
        )
    }
}

/// Waits for an event matching `wanted`, ignoring others.
fn wait_for(runner: &SyncRunner, what: &str, wanted: impl Fn(&SyncEvent) -> bool) -> SyncEvent {
    let deadline = Instant::now() + Duration::from_secs(60);
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        if let Some(event) = runner.recv_timeout(left)
            && wanted(&event)
        {
            return event;
        }
    }
    panic!("timed out waiting for: {what}");
}

/// Waits until `check` holds, syncing on request each time it doesn't.
fn eventually(runner: &SyncRunner, what: &str, check: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(60);
    while Instant::now() < deadline {
        if check() {
            return;
        }
        runner.sync_now();
        std::thread::sleep(Duration::from_millis(150));
    }
    panic!("timed out waiting for: {what}");
}

#[test]
fn set_up_two_projects_sync_them_revoke_one_and_leave() {
    let (_server, addr, _data) = server();
    let (a, b) = (Project::new(), Project::new());
    a.write("Chapter One.md", "It was a dark night.\n");

    // A server that only lets its administrator create vaults says so, and the admin
    // token then works.
    assert_eq!(
        pairing::create_vault(
            &a.files,
            a.root(),
            a.data_root.path(),
            &addr,
            None,
            "laptop"
        )
        .unwrap_err(),
        PairError::NeedsAdminToken
    );
    assert!(matches!(
        pairing::create_vault(
            &a.files,
            a.root(),
            a.data_root.path(),
            &addr,
            Some("wrong"),
            "laptop"
        ),
        Err(PairError::Rejected(_))
    ));
    let paired_a = pairing::create_vault(
        &a.files,
        a.root(),
        a.data_root.path(),
        &addr,
        Some(ADMIN),
        "laptop",
    )
    .unwrap();

    // The link (non-secret) lives in the project; the token lives outside it.
    assert_eq!(
        ProjectLink::load(&*a.files, a.root()),
        Some(paired_a.link.clone())
    );
    let project_text = std::fs::read_to_string(a.root().join(".smaragd/sync.json")).unwrap();
    assert!(!project_text.contains(&paired_a.credentials.token));
    let state = DirStateStore::new(
        a.files.clone(),
        state_dir(a.data_root.path(), paired_a.link.vault_id),
    );
    assert_eq!(
        DeviceCredentials::load(&state),
        Some(paired_a.credentials.clone())
    );

    // A ticket pairs a second project; it works exactly once and survives a round trip
    // through its pasteable form.
    let ticket = pairing::make_ticket(&paired_a.link, &paired_a.credentials).unwrap();
    let pasted = SyncTicket::decode(&ticket.encode()).unwrap();
    let paired_b =
        pairing::join_vault(&b.files, b.root(), b.data_root.path(), &pasted, "desktop").unwrap();
    assert_eq!(paired_b.link.vault_id, paired_a.link.vault_id);
    assert_eq!(paired_b.link.kdf_salt, paired_a.link.kdf_salt);
    assert!(matches!(
        pairing::join_vault(&b.files, b.root(), b.data_root.path(), &pasted, "again"),
        Err(PairError::Rejected(_))
    ));

    let devices = pairing::list_devices(&paired_a.link, &paired_a.credentials).unwrap();
    assert_eq!(devices.len(), 2);

    // Background sync, both directions, through the real runner.
    let runner_a = a.start(&paired_a, PASSPHRASE, Duration::from_millis(200));
    let runner_b = b.start(&paired_b, PASSPHRASE, Duration::from_millis(200));
    eventually(&runner_b, "B receives A's chapter", || {
        b.read("Chapter One.md").as_deref() == Some("It was a dark night.\n")
    });
    b.write("Chapter One.md", "It was a dark night.\nB adds a line.\n");
    eventually(&runner_a, "A receives B's edit", || {
        a.read("Chapter One.md").as_deref() == Some("It was a dark night.\nB adds a line.\n")
    });

    // Revoking B halts its runner for good.
    let b_device = paired_b.credentials.device_id;
    pairing::revoke_device(&paired_a.link, &paired_a.credentials, b_device).unwrap();
    runner_b.sync_now();
    let halted = wait_for(&runner_b, "B halts as revoked", |e| {
        matches!(e, SyncEvent::Halted(_))
    });
    assert!(
        matches!(halted, SyncEvent::Halted(HaltReason::Revoked)),
        "{halted:?}"
    );

    // Leaving removes the link and local state but never touches the files.
    drop(runner_a);
    let removed = pairing::leave_vault(
        &a.files,
        a.root(),
        a.data_root.path(),
        &paired_a.link,
        &paired_a.credentials,
    )
    .unwrap();
    assert!(
        removed,
        "the server was reachable, so the device was removed"
    );
    assert_eq!(ProjectLink::load(&*a.files, a.root()), None);
    assert!(!state_dir(a.data_root.path(), paired_a.link.vault_id).exists());
    assert_eq!(
        a.read("Chapter One.md").as_deref(),
        Some("It was a dark night.\nB adds a line.\n")
    );
}

#[test]
fn a_mistyped_passphrase_halts_the_runner_with_a_clear_reason() {
    let (_server, addr, _data) = server();
    let (a, b) = (Project::new(), Project::new());
    a.write("secret.md", "chapter one\n");
    let paired_a = pairing::create_vault(
        &a.files,
        a.root(),
        a.data_root.path(),
        &addr,
        Some(ADMIN),
        "laptop",
    )
    .unwrap();
    let runner_a = a.start(&paired_a, PASSPHRASE, Duration::from_secs(3600));
    wait_for(
        &runner_a,
        "A's first pass",
        |e| matches!(e, SyncEvent::Synced(r) if r.pushed_updates > 0),
    );

    let ticket = pairing::make_ticket(&paired_a.link, &paired_a.credentials).unwrap();
    let paired_b =
        pairing::join_vault(&b.files, b.root(), b.data_root.path(), &ticket, "phone").unwrap();
    let runner_b = b.start(
        &paired_b,
        "a different passphrase",
        Duration::from_secs(3600),
    );
    let halted = wait_for(&runner_b, "B halts", |e| matches!(e, SyncEvent::Halted(_)));
    assert!(matches!(
        halted,
        SyncEvent::Halted(HaltReason::WrongPassphrase)
    ));
    assert_eq!(
        b.read("secret.md"),
        None,
        "nothing is written with the wrong key"
    );
}

#[test]
fn test_connection_tells_a_real_server_from_a_dead_one() {
    let (_server, addr, _data) = server();
    assert_eq!(pairing::test_connection(&addr).unwrap().status, "ok");

    let dead = ServerAddr { port: 1, ..addr };
    assert!(pairing::test_connection(&dead).is_err());
}
