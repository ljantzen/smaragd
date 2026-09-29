use std::process::ExitCode;

use smaragd_sync_server::{Config, open_state, serve};
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

/// Resolves on Ctrl-C or (on Unix) SIGTERM, which is what `docker stop` sends.
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        match signal(SignalKind::terminate()) {
            Ok(mut term) => {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {}
                    _ = term.recv() => {}
                }
            }
            Err(_) => {
                let _ = tokio::signal::ctrl_c().await;
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
    tracing::info!("shutting down");
}

/// `smaragd-sync-server healthcheck`: asks the locally running server for
/// `/v1/health` and exits 0 only on a 200. This is what the Docker `HEALTHCHECK`
/// runs, so the slim runtime image needs no `curl`/`wget`.
fn healthcheck() -> ExitCode {
    use std::io::{Read, Write};
    use std::net::{TcpStream, ToSocketAddrs};
    use std::time::Duration;

    let listen = std::env::var("SMARAGD_SYNC_LISTEN_ADDR")
        .ok()
        .filter(|addr| !addr.trim().is_empty())
        .unwrap_or_else(|| "0.0.0.0:8080".to_string());
    let port = listen.rsplit(':').next().unwrap_or("8080");
    let Some(addr) = format!("127.0.0.1:{port}")
        .to_socket_addrs()
        .ok()
        .and_then(|mut addrs| addrs.next())
    else {
        return ExitCode::FAILURE;
    };
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_secs(3)) else {
        return ExitCode::FAILURE;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
    let request = "GET /v1/health HTTP/1.0\r\nHost: localhost\r\n\r\n";
    let mut reply = String::new();
    if stream.write_all(request.as_bytes()).is_err()
        || stream.take(4096).read_to_string(&mut reply).is_err()
    {
        return ExitCode::FAILURE;
    }
    if reply.starts_with("HTTP/1.1 200") || reply.starts_with("HTTP/1.0 200") {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// `smaragd-sync-server admin <command>`: operator tools on the database, see `admin.rs`.
fn admin() -> ExitCode {
    let config = match Config::from_env() {
        Ok(config) => config,
        Err(message) => {
            eprintln!("configuration error: {message}");
            return ExitCode::FAILURE;
        }
    };
    let args: Vec<String> = std::env::args().skip(2).collect();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    match smaragd_sync_server::admin::run(&args, &config, now, &mut std::io::stdout()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    if std::env::args().nth(1).as_deref() == Some("healthcheck") {
        return healthcheck();
    }
    if std::env::args().nth(1).as_deref() == Some("admin") {
        return admin();
    }
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let config = match Config::from_env() {
        Ok(config) => config,
        Err(message) => {
            eprintln!("configuration error: {message}");
            return ExitCode::FAILURE;
        }
    };
    if !config.allow_open_registration && config.admin_token.is_none() {
        tracing::warn!(
            "open registration is off and no SMARAGD_SYNC_ADMIN_TOKEN is set: \
             no one will be able to create a vault"
        );
    }

    let listen_addr = config.listen_addr.clone();
    let data_dir = config.data_dir.clone();
    let state = match open_state(config) {
        Ok(state) => state,
        Err(err) => {
            eprintln!(
                "can't open the data directory {}: {err}",
                data_dir.display()
            );
            return ExitCode::FAILURE;
        }
    };
    let listener = match TcpListener::bind(&listen_addr).await {
        Ok(listener) => listener,
        Err(err) => {
            eprintln!("can't listen on {listen_addr}: {err}");
            return ExitCode::FAILURE;
        }
    };
    tracing::info!(
        "smaragd-sync-server {} listening on {listen_addr}, data in {}",
        env!("CARGO_PKG_VERSION"),
        data_dir.display()
    );

    match serve(state, listener, shutdown_signal()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("server error: {err}");
            ExitCode::FAILURE
        }
    }
}
