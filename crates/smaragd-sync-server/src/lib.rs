//! Smaragd's self-hostable sync server.
//!
//! A **blind** store: it keeps sealed (end-to-end encrypted) CRDT updates per
//! `(vault, document)` and hands them to the vault's other devices. It can't read,
//! merge or forge anything — all of that is the clients' job — so it needs no
//! knowledge of markdown, projects or users, only vaults, devices and sequence
//! numbers. See `smaragd_sync_protocol` for the wire format and endpoint table.

pub mod admin;
pub mod auth;
pub mod config;
pub mod db;
pub mod error;
pub mod maintenance;
pub mod routes;

use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::thread::JoinHandle;

use tokio::net::TcpListener;
use tokio::sync::oneshot;

pub use config::Config;
pub use routes::router;

/// Shared by every request.
pub struct AppState {
    pub db: db::Db,
    pub config: Config,
}

/// Opens (creating if needed) the database under `config.data_dir`.
pub fn open_state(config: Config) -> io::Result<Arc<AppState>> {
    std::fs::create_dir_all(&config.data_dir)?;
    let conn = db::open(&config.data_dir.join("sync.sqlite3"))
        .map_err(|err| io::Error::other(format!("opening the database: {err}")))?;
    Ok(Arc::new(AppState {
        db: db::Db::new(conn),
        config,
    }))
}

/// Serves until `shutdown` resolves, letting in-flight requests finish.
pub async fn serve(
    state: Arc<AppState>,
    listener: TcpListener,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> io::Result<()> {
    if let Some(interval) = state.config.maintenance_interval {
        tokio::spawn(maintenance::run_forever(Arc::clone(&state), interval));
    }
    axum::serve(listener, router(state))
        .with_graceful_shutdown(shutdown)
        .await
}

/// A server running on its own thread and runtime, for tests and embedding.
/// Stops (and joins) when dropped.
pub struct RunningServer {
    pub addr: SocketAddr,
    pub state: Arc<AppState>,
    stop: Option<oneshot::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl RunningServer {
    /// Root URL without a trailing slash, e.g. `http://127.0.0.1:41234`.
    pub fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }
}

impl Drop for RunningServer {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Starts a server on `config.listen_addr` (use port 0 for an ephemeral one).
pub fn start_in_background(config: Config) -> io::Result<RunningServer> {
    let state = open_state(config.clone())?;
    let (addr_tx, addr_rx) = std::sync::mpsc::channel();
    let (stop_tx, stop_rx) = oneshot::channel::<()>();
    let served = Arc::clone(&state);
    let thread = std::thread::spawn(move || {
        let runtime = match tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(err) => {
                let _ = addr_tx.send(Err(err));
                return;
            }
        };
        runtime.block_on(async move {
            let listener = match TcpListener::bind(&config.listen_addr).await {
                Ok(listener) => listener,
                Err(err) => {
                    let _ = addr_tx.send(Err(err));
                    return;
                }
            };
            let _ = addr_tx.send(listener.local_addr());
            let _ = serve(served, listener, async {
                let _ = stop_rx.await;
            })
            .await;
        });
    });
    let addr = addr_rx
        .recv()
        .map_err(|_| io::Error::other("the server thread exited before binding"))??;
    Ok(RunningServer {
        addr,
        state,
        stop: Some(stop_tx),
        thread: Some(thread),
    })
}
