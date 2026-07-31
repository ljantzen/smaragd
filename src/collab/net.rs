//! The async iroh networking that runs on the collaboration session's
//! background thread (spawned by [`crate::collab::spawn_collab_session`]).
//!
//! Deliberately minimal: this module only ever moves already-encrypted bytes
//! in both directions (see the module doc on `src/collab/mod.rs` for why the
//! CRDT stays on the main thread instead). It doesn't know or care what the
//! plaintext means — it just derives the session key from the ticket's
//! secret once at session start and uses `crypto::encrypt`/`crypto::decrypt`
//! around every frame, including the joiner's initial handshake ping.

use std::sync::mpsc::Sender as EventSender;

use iroh::endpoint::{RecvStream, SendStream};
use tokio::sync::mpsc::UnboundedReceiver;

use crate::collab::crypto::SessionKey;
use crate::collab::ticket::CollabTicket;
use crate::collab::{CollabCommand, CollabEvent, SessionRole, crypto};

/// Identifies this application's protocol to iroh's `Endpoint` — connections
/// for any other ALPN are rejected during the handshake.
const ALPN: &[u8] = b"smaragd/collab/1";

/// A single CRDT update is never expected to approach this size; a claimed
/// frame length above it is treated as a protocol error rather than an
/// invitation to make a huge allocation.
const MAX_FRAME_LEN: u32 = 16 * 1024 * 1024;

/// Runs one collaboration session to completion: establishes the iroh
/// connection (hosting or joining, per `role`), then shuttles bytes between
/// the peer and the channels connecting back to the main thread until the
/// session ends or the connection drops. Blocks the calling thread for the
/// session's whole lifetime — call this from its own `std::thread::spawn`,
/// as [`crate::collab::spawn_collab_session`] does.
pub fn run(
    role: SessionRole,
    cmd_rx: UnboundedReceiver<CollabCommand>,
    event_tx: EventSender<CollabEvent>,
    ctx: egui::Context,
) {
    let rt = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(err) => {
            send_event(
                &event_tx,
                &ctx,
                CollabEvent::Error(format!("failed to start collaboration runtime: {err}")),
            );
            return;
        }
    };
    rt.block_on(run_session(role, cmd_rx, event_tx, ctx));
}

fn send_event(event_tx: &EventSender<CollabEvent>, ctx: &egui::Context, event: CollabEvent) {
    let _ = event_tx.send(event);
    ctx.request_repaint();
}

async fn run_session(
    role: SessionRole,
    mut cmd_rx: UnboundedReceiver<CollabCommand>,
    event_tx: EventSender<CollabEvent>,
    ctx: egui::Context,
) {
    let role_label = match &role {
        SessionRole::Host => "host",
        SessionRole::Join(_) => "joiner",
    };
    println!("[collab:{role_label}] starting session");

    let secret_key = iroh::SecretKey::generate();
    let endpoint = match iroh::Endpoint::builder(iroh::endpoint::presets::N0)
        .secret_key(secret_key)
        .alpns(vec![ALPN.to_vec()])
        .bind()
        .await
    {
        Ok(endpoint) => endpoint,
        Err(err) => {
            println!("[collab:{role_label}] failed to bind endpoint: {err}");
            send_event(
                &event_tx,
                &ctx,
                CollabEvent::Error(format!("failed to start networking: {err}")),
            );
            return;
        }
    };
    println!(
        "[collab:{role_label}] endpoint bound, id={}",
        endpoint.id().fmt_short()
    );

    let (mut send_stream, mut recv_stream, key, peer_fingerprint) =
        match establish(&role, &endpoint, &event_tx, &ctx).await {
            Some(result) => result,
            None => {
                println!("[collab:{role_label}] establish() failed — see CollabEvent::Error above");
                endpoint.close().await;
                return;
            }
        };
    println!("[collab:{role_label}] connected to peer {peer_fingerprint}");

    send_event(
        &event_tx,
        &ctx,
        CollabEvent::PeerConnected(peer_fingerprint),
    );

    // The reader runs as its own task so it can be blocked on `read_encrypted`
    // (an inherently async wait for the peer's next message) concurrently
    // with this thread's loop waiting on local commands — a single
    // `tokio::select!` can't hold two separate `&mut` borrows of the one
    // stream pair across loop iterations the way two tasks naturally can.
    let reader_event_tx = event_tx.clone();
    let reader_ctx = ctx.clone();
    let reader_role_label = role_label;
    let mut reader = tokio::spawn(async move {
        loop {
            match read_encrypted(&mut recv_stream, &key).await {
                // An empty frame is the joiner's handshake ping (see
                // `establish`), not a real update — swallow it rather than
                // surfacing a meaningless empty `RemoteUpdate`.
                Ok(bytes) if bytes.is_empty() => {}
                Ok(bytes) => {
                    send_event(
                        &reader_event_tx,
                        &reader_ctx,
                        CollabEvent::RemoteUpdate(bytes),
                    );
                }
                Err(err) => {
                    println!(
                        "[collab:{reader_role_label}] read from peer failed, reporting disconnect: {err}"
                    );
                    send_event(&reader_event_tx, &reader_ctx, CollabEvent::PeerDisconnected);
                    return;
                }
            }
        }
    });

    // Races the command loop against the reader task itself: without this,
    // a reader failure (the peer's connection actually dying) would only
    // ever end the reader sub-task, leaving this loop waiting forever for
    // commands on a connection nothing is receiving from any more — a
    // zombie session that looks alive to the main thread (the command
    // channel is still open) but can never do anything useful again.
    loop {
        tokio::select! {
            command = cmd_rx.recv() => {
                match command {
                    Some(CollabCommand::LocalEdit(bytes)) => {
                        if let Err(err) = write_encrypted(&mut send_stream, &key, &bytes).await {
                            println!("[collab:{role_label}] write to peer failed: {err}");
                            send_event(
                                &event_tx,
                                &ctx,
                                CollabEvent::Error(format!("failed to send edit to peer: {err}")),
                            );
                            break;
                        }
                    }
                    Some(CollabCommand::EndSession) => {
                        println!("[collab:{role_label}] EndSession command received");
                        break;
                    }
                    None => {
                        println!(
                            "[collab:{role_label}] command channel closed (CollabSession dropped)"
                        );
                        break;
                    }
                }
            }
            _ = &mut reader => {
                println!("[collab:{role_label}] reader task ended, tearing down session");
                break;
            }
        }
    }
    println!("[collab:{role_label}] session ending, tearing down");

    reader.abort();
    endpoint.close().await;
}

/// The endpoint's address immediately after `bind()` only carries local
/// interface addresses — home-relay assignment happens asynchronously
/// afterwards. Waits (up to a few seconds) for a relay address to appear, so
/// a ticket handed to a peer that can't be reached directly (a different
/// network entirely, symmetric NAT, etc.) still has a relay fallback to use,
/// rather than only ever the addresses available at the instant of binding.
async fn wait_for_relay_addr(endpoint: &iroh::Endpoint) -> iroh::EndpointAddr {
    use iroh::Watcher;

    let mut watcher = endpoint.watch_addr();
    let mut addr = watcher.get();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    while !addr.addrs.iter().any(|a| a.is_relay()) {
        let Some(remaining) = deadline.checked_duration_since(tokio::time::Instant::now()) else {
            break;
        };
        match tokio::time::timeout(remaining, watcher.updated()).await {
            Ok(Ok(next)) => addr = next,
            _ => break,
        }
    }
    addr
}

/// Hosts or joins the session per `role`, returning the resulting
/// bidirectional stream pair, the session's derived encryption key, and a
/// short display fingerprint for the peer (see `iroh::PublicKey::fmt_short`),
/// or `None` if an event was already sent reporting why establishment failed.
async fn establish(
    role: &SessionRole,
    endpoint: &iroh::Endpoint,
    event_tx: &EventSender<CollabEvent>,
    ctx: &egui::Context,
) -> Option<(SendStream, RecvStream, SessionKey, String)> {
    match role {
        SessionRole::Host => {
            let session_secret: [u8; 32] = rand::random();
            let key = crypto::derive_key(&session_secret);
            println!("[collab:host] waiting for a relay address...");
            let addr = wait_for_relay_addr(endpoint).await;
            println!(
                "[collab:host] addr resolved: {} addr(s), relay={}",
                addr.addrs.len(),
                addr.addrs.iter().any(|a| a.is_relay())
            );
            let ticket = CollabTicket::new(addr, session_secret);
            send_event(event_tx, ctx, CollabEvent::HostReady(ticket.encode()));

            println!("[collab:host] waiting for an incoming connection...");
            let incoming = endpoint.accept().await.or_else(|| {
                println!("[collab:host] endpoint.accept() returned None (endpoint closed)");
                send_event(
                    event_tx,
                    ctx,
                    CollabEvent::Error(
                        "networking endpoint closed before a peer connected".to_string(),
                    ),
                );
                None
            })?;
            let conn = match incoming.await {
                Ok(conn) => conn,
                Err(err) => {
                    println!("[collab:host] incoming connection failed: {err}");
                    send_event(
                        event_tx,
                        ctx,
                        CollabEvent::Error(format!("incoming connection failed: {err}")),
                    );
                    return None;
                }
            };
            let peer_fingerprint = conn.remote_id().fmt_short().to_string();
            println!(
                "[collab:host] connection accepted from {peer_fingerprint}, waiting for the peer's bi-stream..."
            );
            match conn.accept_bi().await {
                Ok((send_stream, recv_stream)) => {
                    println!("[collab:host] accept_bi() succeeded");
                    Some((send_stream, recv_stream, key, peer_fingerprint))
                }
                Err(err) => {
                    println!("[collab:host] accept_bi() failed: {err}");
                    send_event(
                        event_tx,
                        ctx,
                        CollabEvent::Error(format!("failed to accept peer stream: {err}")),
                    );
                    None
                }
            }
        }
        SessionRole::Join(code) => {
            let ticket = match CollabTicket::decode(code) {
                Ok(ticket) => ticket,
                Err(err) => {
                    println!("[collab:joiner] invalid connection code: {err}");
                    send_event(
                        event_tx,
                        ctx,
                        CollabEvent::Error(format!("invalid connection code: {err}")),
                    );
                    return None;
                }
            };
            let key = crypto::derive_key(&ticket.session_secret);
            println!(
                "[collab:joiner] connecting to host ({} addr(s), relay={})...",
                ticket.endpoint_addr.addrs.len(),
                ticket.endpoint_addr.addrs.iter().any(|a| a.is_relay())
            );
            let conn = match endpoint.connect(ticket.endpoint_addr, ALPN).await {
                Ok(conn) => conn,
                Err(err) => {
                    println!("[collab:joiner] failed to connect to peer: {err}");
                    send_event(
                        event_tx,
                        ctx,
                        CollabEvent::Error(format!("failed to connect to peer: {err}")),
                    );
                    return None;
                }
            };
            let peer_fingerprint = conn.remote_id().fmt_short().to_string();
            println!("[collab:joiner] connected to {peer_fingerprint}, opening bi-stream...");
            match conn.open_bi().await {
                Ok((mut send_stream, recv_stream)) => {
                    println!("[collab:joiner] open_bi() succeeded, sending handshake ping...");
                    // `accept_bi` on the host's side does not resolve just
                    // because we opened the stream — the host only unblocks
                    // once actual data arrives on it (this is a documented
                    // quirk of iroh/QUIC bidirectional streams, not
                    // optional). Ping it explicitly with an empty frame so
                    // the host reports `PeerConnected` immediately, rather
                    // than staying blocked in `accept_bi` until the first
                    // real edit happens to be typed.
                    if let Err(err) = write_encrypted(&mut send_stream, &key, &[]).await {
                        println!("[collab:joiner] handshake ping failed: {err}");
                        send_event(
                            event_tx,
                            ctx,
                            CollabEvent::Error(format!(
                                "failed to complete handshake with peer: {err}"
                            )),
                        );
                        return None;
                    }
                    println!("[collab:joiner] handshake ping sent");
                    Some((send_stream, recv_stream, key, peer_fingerprint))
                }
                Err(err) => {
                    println!("[collab:joiner] open_bi() failed: {err}");
                    send_event(
                        event_tx,
                        ctx,
                        CollabEvent::Error(format!("failed to open stream to peer: {err}")),
                    );
                    None
                }
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
enum FrameError {
    #[error("write failed: {0}")]
    Write(#[from] iroh::endpoint::WriteError),
    #[error("read failed: {0}")]
    Read(#[from] iroh::endpoint::ReadExactError),
    #[error("peer claimed an implausibly large frame ({0} bytes)")]
    TooLarge(u32),
    #[error("{0}")]
    Decrypt(#[from] crypto::DecryptError),
}

async fn write_frame(stream: &mut SendStream, payload: &[u8]) -> Result<(), FrameError> {
    let len = (payload.len() as u32).to_be_bytes();
    stream.write_all(&len).await?;
    stream.write_all(payload).await?;
    Ok(())
}

async fn read_frame(stream: &mut RecvStream) -> Result<Vec<u8>, FrameError> {
    let mut len_bytes = [0u8; 4];
    stream.read_exact(&mut len_bytes).await?;
    let len = u32::from_be_bytes(len_bytes);
    if len > MAX_FRAME_LEN {
        return Err(FrameError::TooLarge(len));
    }
    let mut payload = vec![0u8; len as usize];
    stream.read_exact(&mut payload).await?;
    Ok(payload)
}

/// Encrypts `plaintext` and sends it as one frame — every frame on the wire
/// is ciphertext, including the joiner's empty handshake ping, so there is
/// no plaintext-vs-ciphertext special case to get wrong.
async fn write_encrypted(
    stream: &mut SendStream,
    key: &SessionKey,
    plaintext: &[u8],
) -> Result<(), FrameError> {
    let ciphertext = crypto::encrypt(key, plaintext);
    write_frame(stream, &ciphertext).await
}

async fn read_encrypted(stream: &mut RecvStream, key: &SessionKey) -> Result<Vec<u8>, FrameError> {
    let ciphertext = read_frame(stream).await?;
    let plaintext = crypto::decrypt(key, &ciphertext)?;
    Ok(plaintext)
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::Receiver;
    use std::time::Duration;

    use iroh::EndpointAddr;

    use crate::collab::ticket::CollabTicket;
    use crate::collab::{CollabCommand, CollabEvent, SessionRole, spawn_collab_session};

    /// These tests establish a real iroh connection between two in-process
    /// endpoints over the live public network (iroh's default relay
    /// infrastructure), so they're excluded from the default `cargo test`
    /// run that CI executes on every push — a flaky external dependency has
    /// no business gating every commit. Run them manually with
    /// `cargo test --lib collab::net -- --ignored` when you want to verify
    /// the real networking path end to end.
    fn recv_event(rx: &Receiver<CollabEvent>, timeout: Duration) -> CollabEvent {
        rx.recv_timeout(timeout)
            .expect("expected a collab event before the timeout")
    }

    #[test]
    #[ignore = "requires real internet access to iroh's public relay infrastructure"]
    fn two_instances_exchange_bytes_via_a_pasted_connection_code() {
        let ctx = egui::Context::default();
        let timeout = Duration::from_secs(30);

        let host = spawn_collab_session(SessionRole::Host, ctx.clone());
        let code = match recv_event(&host.event_rx, timeout) {
            CollabEvent::HostReady(code) => code,
            other => panic!("expected HostReady, got {other:?}"),
        };

        let joiner = spawn_collab_session(SessionRole::Join(code), ctx.clone());

        assert!(matches!(
            recv_event(&joiner.event_rx, timeout),
            CollabEvent::PeerConnected(_)
        ));
        assert!(matches!(
            recv_event(&host.event_rx, timeout),
            CollabEvent::PeerConnected(_)
        ));

        joiner
            .cmd_tx
            .send(CollabCommand::LocalEdit(b"hello from joiner".to_vec()))
            .unwrap();
        match recv_event(&host.event_rx, timeout) {
            CollabEvent::RemoteUpdate(bytes) => assert_eq!(bytes, b"hello from joiner"),
            other => panic!("expected RemoteUpdate, got {other:?}"),
        }

        host.cmd_tx
            .send(CollabCommand::LocalEdit(b"hello from host".to_vec()))
            .unwrap();
        match recv_event(&joiner.event_rx, timeout) {
            CollabEvent::RemoteUpdate(bytes) => assert_eq!(bytes, b"hello from host"),
            other => panic!("expected RemoteUpdate, got {other:?}"),
        }

        host.cmd_tx.send(CollabCommand::EndSession).unwrap();
        joiner.cmd_tx.send(CollabCommand::EndSession).unwrap();
    }

    #[test]
    #[ignore = "requires real internet access to iroh's public relay infrastructure"]
    fn relay_only_addressing_still_connects() {
        // Strips every direct IP address from the host's ticket before the
        // joiner uses it, so the only way to reach the host is via iroh's
        // public relay — proving the relay fallback path actually works,
        // not just direct same-host/same-LAN connectivity.
        let ctx = egui::Context::default();
        let timeout = Duration::from_secs(30);

        let host = spawn_collab_session(SessionRole::Host, ctx.clone());
        let code = match recv_event(&host.event_rx, timeout) {
            CollabEvent::HostReady(code) => code,
            other => panic!("expected HostReady, got {other:?}"),
        };

        let ticket = CollabTicket::decode(&code).unwrap();
        let relay_addrs: Vec<_> = ticket
            .endpoint_addr
            .addrs
            .iter()
            .filter(|addr| addr.is_relay())
            .cloned()
            .collect();
        assert!(
            !relay_addrs.is_empty(),
            "host's ticket had no relay address to fall back to"
        );
        let relay_only_addr = EndpointAddr::from_parts(ticket.endpoint_addr.id, relay_addrs);
        let relay_only_ticket = CollabTicket::new(relay_only_addr, ticket.session_secret);

        let joiner =
            spawn_collab_session(SessionRole::Join(relay_only_ticket.encode()), ctx.clone());

        assert!(matches!(
            recv_event(&joiner.event_rx, timeout),
            CollabEvent::PeerConnected(_)
        ));
        assert!(matches!(
            recv_event(&host.event_rx, timeout),
            CollabEvent::PeerConnected(_)
        ));

        host.cmd_tx.send(CollabCommand::EndSession).unwrap();
        joiner.cmd_tx.send(CollabCommand::EndSession).unwrap();
    }

    #[test]
    #[ignore = "requires real internet access to iroh's public relay infrastructure"]
    fn a_mismatched_session_secret_fails_closed_instead_of_producing_garbage() {
        // Corrupts the session secret before the joiner uses it, so the two
        // sides derive different encryption keys despite using "the same"
        // connection code — proving the encryption layer fails closed
        // rather than one side silently accepting garbage as a real update.
        let ctx = egui::Context::default();
        let timeout = Duration::from_secs(30);

        let host = spawn_collab_session(SessionRole::Host, ctx.clone());
        let code = match recv_event(&host.event_rx, timeout) {
            CollabEvent::HostReady(code) => code,
            other => panic!("expected HostReady, got {other:?}"),
        };

        let mut ticket = CollabTicket::decode(&code).unwrap();
        ticket.session_secret[0] ^= 0xFF;
        let wrong_key_code = ticket.encode();

        let joiner = spawn_collab_session(SessionRole::Join(wrong_key_code), ctx.clone());

        // The QUIC-level connection still succeeds on both sides — a session
        // key mismatch is invisible below the encryption layer.
        assert!(matches!(
            recv_event(&joiner.event_rx, timeout),
            CollabEvent::PeerConnected(_)
        ));

        // The host also reports `PeerConnected` immediately (the QUIC-level
        // handshake succeeded before any encrypted payload was involved)...
        assert!(matches!(
            recv_event(&host.event_rx, timeout),
            CollabEvent::PeerConnected(_)
        ));

        // ...but it can never decrypt anything the joiner sends, starting
        // with the joiner's own handshake ping, so the session fails closed
        // instead of proceeding on garbage.
        match recv_event(&host.event_rx, timeout) {
            CollabEvent::PeerDisconnected => {}
            other => panic!("expected the mismatched key to fail closed, got {other:?}"),
        }

        joiner.cmd_tx.send(CollabCommand::EndSession).unwrap();
    }
}
