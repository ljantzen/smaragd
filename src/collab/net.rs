//! The async iroh networking that runs on the collaboration session's
//! background thread (spawned by [`crate::collab::spawn_collab_session`]).
//!
//! Deliberately minimal: this module only ever moves already-encrypted bytes
//! in both directions (see the module doc on `src/collab/mod.rs` for why the
//! CRDT stays on the main thread instead). It doesn't know or care what the
//! plaintext means — it derives the session's directional frame keys from
//! the ticket's secret once at session start and runs every frame through
//! `crypto::SealCipher`/`crypto::OpenCipher`, starting with the empty
//! handshake ping/ack pair each side must successfully decrypt before
//! either reports the peer as connected.

use std::sync::mpsc::Sender as EventSender;
use std::time::Duration;

use iroh::endpoint::{RecvStream, SendStream};
use tokio::sync::mpsc::UnboundedReceiver;

use crate::collab::crypto::{Direction, OpenCipher, SealCipher};
use crate::collab::ticket::CollabTicket;
use crate::collab::{CollabCommand, CollabEvent, SessionRole, crypto};

/// Identifies this application's protocol to iroh's `Endpoint` — connections
/// for any other ALPN are rejected during the handshake.
const ALPN: &[u8] = b"smaragd/collab/1";

/// A single CRDT update is never expected to approach this size; a claimed
/// frame length above it is treated as a protocol error rather than an
/// invitation to make a huge allocation.
const MAX_FRAME_LEN: u32 = 16 * 1024 * 1024;

/// How long a connected peer gets to complete the encrypted handshake (the
/// empty ping/ack frames proving each side holds the session key) before
/// the attempt is abandoned. Generous enough for a relay round trip; short
/// enough that a connector who never speaks can't park the session.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(15);

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

    // `establish` can wait indefinitely (a host has no deadline for a peer
    // to show up), so it races the command channel here: an EndSession (or
    // the handle being dropped) while still waiting must tear the endpoint
    // down immediately, not leave it listening in the background. Local
    // edits that arrive meanwhile — the host's initial bootstrap diff,
    // above all — are queued and flushed right after the handshake.
    let mut pending_edits: Vec<Vec<u8>> = Vec::new();
    let outcome = {
        let establish_fut = establish(&role, &endpoint, &event_tx, &ctx);
        tokio::pin!(establish_fut);
        loop {
            tokio::select! {
                result = &mut establish_fut => break Some(result),
                command = cmd_rx.recv() => match command {
                    Some(CollabCommand::LocalEdit(bytes)) => pending_edits.push(bytes),
                    Some(CollabCommand::EndSession) | None => break None,
                }
            }
        }
    };
    let Some(result) = outcome else {
        println!("[collab:{role_label}] session ended before a peer connected, tearing down");
        endpoint.close().await;
        return;
    };
    let Some(established) = result else {
        println!("[collab:{role_label}] establish() failed — see CollabEvent::Error above");
        endpoint.close().await;
        return;
    };
    let Established {
        mut send_stream,
        recv_stream,
        mut sealer,
        opener,
        peer_fingerprint,
    } = established;
    println!("[collab:{role_label}] connected to peer {peer_fingerprint}");

    send_event(
        &event_tx,
        &ctx,
        CollabEvent::PeerConnected(peer_fingerprint),
    );

    for bytes in pending_edits {
        if let Err(err) = write_encrypted(&mut send_stream, &mut sealer, &bytes).await {
            println!("[collab:{role_label}] flushing queued edit to peer failed: {err}");
            send_event(
                &event_tx,
                &ctx,
                CollabEvent::Error(format!("failed to send edit to peer: {err}")),
            );
            endpoint.close().await;
            return;
        }
    }

    // The reader runs as its own task so it can be blocked on `read_encrypted`
    // (an inherently async wait for the peer's next message) concurrently
    // with this thread's loop waiting on local commands — a single
    // `tokio::select!` can't hold two separate `&mut` borrows of the one
    // stream pair across loop iterations the way two tasks naturally can.
    let reader_event_tx = event_tx.clone();
    let reader_ctx = ctx.clone();
    let reader_role_label = role_label;
    let mut reader = tokio::spawn(async move {
        let mut recv_stream = recv_stream;
        let mut opener = opener;
        loop {
            match read_encrypted(&mut recv_stream, &mut opener).await {
                // The handshake ping/ack are consumed inside `establish`,
                // but tolerate an empty frame here too — it carries no
                // update, so swallow it rather than surfacing a meaningless
                // empty `RemoteUpdate`.
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
                        if let Err(err) =
                            write_encrypted(&mut send_stream, &mut sealer, &bytes).await
                        {
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

/// The result of a completed `establish`: the stream pair, the stateful
/// directional frame ciphers (see `crypto`), and a short display fingerprint
/// for the peer (see `iroh::PublicKey::fmt_short`).
struct Established {
    send_stream: SendStream,
    recv_stream: RecvStream,
    sealer: SealCipher,
    opener: OpenCipher,
    peer_fingerprint: String,
}

/// Hosts or joins the session per `role`, completing the encrypted handshake
/// (joiner's ping, host's ack — each side must decrypt the other's before a
/// peer is ever reported as connected), or returns `None` if an event was
/// already sent reporting why establishment failed.
async fn establish(
    role: &SessionRole,
    endpoint: &iroh::Endpoint,
    event_tx: &EventSender<CollabEvent>,
    ctx: &egui::Context,
) -> Option<Established> {
    match role {
        SessionRole::Host => {
            let session_secret: [u8; 32] = rand::random();
            let host_id = *endpoint.id().as_bytes();
            let send_key =
                crypto::derive_directional_key(&session_secret, &host_id, Direction::HostToJoiner);
            let recv_key =
                crypto::derive_directional_key(&session_secret, &host_id, Direction::JoinerToHost);
            println!("[collab:host] waiting for a relay address...");
            let addr = wait_for_relay_addr(endpoint).await;
            println!(
                "[collab:host] addr resolved: {} addr(s), relay={}",
                addr.addrs.len(),
                addr.addrs.iter().any(|a| a.is_relay())
            );
            let ticket = CollabTicket::new(addr, session_secret);
            send_event(event_tx, ctx, CollabEvent::HostReady(ticket.encode()));

            // Keep accepting until a connector proves — within
            // `HANDSHAKE_TIMEOUT` — that it holds the session key. Anyone
            // else (whoever reached this endpoint id without the code)
            // burns only its own attempt, not the session: the genuine
            // collaborator can still pair with the same code afterwards.
            loop {
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
                        println!(
                            "[collab:host] incoming connection failed ({err}), still listening"
                        );
                        continue;
                    }
                };
                let peer_fingerprint = conn.remote_id().fmt_short().to_string();
                println!(
                    "[collab:host] connection from {peer_fingerprint}, awaiting its handshake..."
                );

                // Fresh ciphers per attempt: frame counters are
                // per-connection, and a rejected attempt never produced a
                // frame under these keys anyway (nothing it sent decrypted).
                let mut opener = OpenCipher::new(&recv_key);
                let handshake = tokio::time::timeout(HANDSHAKE_TIMEOUT, async {
                    let (send_stream, mut recv_stream) = conn
                        .accept_bi()
                        .await
                        .map_err(|err| format!("accepting the peer's stream failed: {err}"))?;
                    let ping = read_frame(&mut recv_stream)
                        .await
                        .map_err(|err| format!("reading the handshake frame failed: {err}"))?;
                    opener
                        .open(&ping)
                        .map_err(|err| format!("the handshake frame did not decrypt: {err}"))?;
                    Ok::<_, String>((send_stream, recv_stream))
                })
                .await;
                let (mut send_stream, recv_stream) = match handshake {
                    Ok(Ok(streams)) => streams,
                    Ok(Err(reason)) => {
                        println!(
                            "[collab:host] pairing attempt by {peer_fingerprint} rejected ({reason}), still listening"
                        );
                        conn.close(1u32.into(), b"pairing failed");
                        continue;
                    }
                    Err(_) => {
                        println!(
                            "[collab:host] pairing attempt by {peer_fingerprint} timed out, still listening"
                        );
                        conn.close(1u32.into(), b"pairing timed out");
                        continue;
                    }
                };

                let mut sealer = SealCipher::new(&send_key);
                // The ack is the joiner's mirror-image proof that this side
                // holds the key too — it won't report the peer as connected
                // (or send anything) until this decrypts.
                if let Err(err) = write_frame(&mut send_stream, &sealer.seal(&[])).await {
                    println!(
                        "[collab:host] sending the handshake ack failed ({err}), still listening"
                    );
                    continue;
                }
                println!("[collab:host] handshake complete with {peer_fingerprint}");
                return Some(Established {
                    send_stream,
                    recv_stream,
                    sealer,
                    opener,
                    peer_fingerprint,
                });
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
            let host_id = *ticket.endpoint_addr.id.as_bytes();
            let mut sealer = SealCipher::new(&crypto::derive_directional_key(
                &ticket.session_secret,
                &host_id,
                Direction::JoinerToHost,
            ));
            let mut opener = OpenCipher::new(&crypto::derive_directional_key(
                &ticket.session_secret,
                &host_id,
                Direction::HostToJoiner,
            ));
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
            let (mut send_stream, mut recv_stream) = match conn.open_bi().await {
                Ok(streams) => streams,
                Err(err) => {
                    println!("[collab:joiner] open_bi() failed: {err}");
                    send_event(
                        event_tx,
                        ctx,
                        CollabEvent::Error(format!("failed to open stream to peer: {err}")),
                    );
                    return None;
                }
            };
            // The encrypted ping both proves this side holds the session
            // key (the host rejects the pairing otherwise) and unblocks the
            // host's `accept_bi` in the first place — that call only
            // resolves once actual data arrives on the stream (a documented
            // quirk of iroh/QUIC bidirectional streams, not optional).
            println!("[collab:joiner] open_bi() succeeded, sending handshake ping...");
            if let Err(err) = write_frame(&mut send_stream, &sealer.seal(&[])).await {
                println!("[collab:joiner] handshake ping failed: {err}");
                send_event(
                    event_tx,
                    ctx,
                    CollabEvent::Error(format!("failed to complete handshake with peer: {err}")),
                );
                return None;
            }
            // ...and the host's ack is the mirror-image proof: nothing is
            // reported as connected here until the host has shown it can
            // produce a frame under the session key too.
            println!("[collab:joiner] handshake ping sent, waiting for the host's ack...");
            let ack = tokio::time::timeout(HANDSHAKE_TIMEOUT, async {
                let frame = read_frame(&mut recv_stream).await?;
                opener.open(&frame)?;
                Ok::<(), FrameError>(())
            })
            .await;
            match ack {
                Ok(Ok(())) => {
                    println!("[collab:joiner] handshake complete with {peer_fingerprint}");
                    Some(Established {
                        send_stream,
                        recv_stream,
                        sealer,
                        opener,
                        peer_fingerprint,
                    })
                }
                Ok(Err(err)) => {
                    println!("[collab:joiner] handshake with host failed: {err}");
                    send_event(
                        event_tx,
                        ctx,
                        CollabEvent::Error(format!(
                            "handshake with the host failed: {err} — check the connection code"
                        )),
                    );
                    None
                }
                Err(_) => {
                    println!("[collab:joiner] host did not answer the handshake in time");
                    send_event(
                        event_tx,
                        ctx,
                        CollabEvent::Error(
                            "the host did not answer the handshake in time — check the connection code"
                                .to_string(),
                        ),
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

/// Seals `plaintext` as the next outgoing frame — every frame on the wire
/// is ciphertext, including the empty handshake ping/ack, so there is no
/// plaintext-vs-ciphertext special case to get wrong.
async fn write_encrypted(
    stream: &mut SendStream,
    sealer: &mut SealCipher,
    plaintext: &[u8],
) -> Result<(), FrameError> {
    let ciphertext = sealer.seal(plaintext);
    write_frame(stream, &ciphertext).await
}

async fn read_encrypted(
    stream: &mut RecvStream,
    opener: &mut OpenCipher,
) -> Result<Vec<u8>, FrameError> {
    let ciphertext = read_frame(stream).await?;
    Ok(opener.open(&ciphertext)?)
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
    fn canceling_a_waiting_host_tears_the_whole_session_down() {
        // Regression test: EndSession while the host is still waiting for a
        // peer must tear down the background thread (and with it the
        // endpoint) immediately — not leave it listening until a peer
        // happens to connect. Observable from outside as the event channel
        // disconnecting once the thread exits and drops its sender.
        let ctx = egui::Context::default();
        let timeout = Duration::from_secs(30);

        let host = spawn_collab_session(SessionRole::Host, ctx.clone());
        match recv_event(&host.event_rx, timeout) {
            CollabEvent::HostReady(_) => {}
            other => panic!("expected HostReady, got {other:?}"),
        }

        host.cmd_tx.send(CollabCommand::EndSession).unwrap();

        let deadline = std::time::Instant::now() + timeout;
        loop {
            match host.event_rx.recv_timeout(Duration::from_millis(100)) {
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Ok(other) => panic!("expected no further events, got {other:?}"),
            }
            assert!(
                std::time::Instant::now() < deadline,
                "background thread did not exit after EndSession while waiting for a peer"
            );
        }
    }

    #[test]
    #[ignore = "requires real internet access to iroh's public relay infrastructure"]
    fn a_mismatched_session_secret_is_rejected_and_the_host_keeps_listening() {
        // Corrupts the session secret before the joiner uses it, so the two
        // sides derive different frame keys despite using "the same"
        // connection code. The wrong-key joiner must be rejected during the
        // encrypted handshake — neither side ever reports it as a connected
        // peer — and the host must survive the attempt: a joiner with the
        // genuine code still pairs afterwards.
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

        let wrong_joiner = spawn_collab_session(SessionRole::Join(wrong_key_code), ctx.clone());

        // The host can't decrypt the wrong-key ping and closes the attempt,
        // so the joiner's handshake fails with an error — it must never see
        // `PeerConnected`.
        match recv_event(&wrong_joiner.event_rx, timeout) {
            CollabEvent::Error(_) => {}
            other => panic!("expected the wrong key to fail the handshake, got {other:?}"),
        }

        // The host is still listening on the same code: the genuine secret
        // pairs successfully afterwards.
        let good_joiner = spawn_collab_session(SessionRole::Join(code), ctx.clone());
        assert!(matches!(
            recv_event(&good_joiner.event_rx, timeout),
            CollabEvent::PeerConnected(_)
        ));

        // And this is the host's *first* connection event of any kind — the
        // rejected attempt produced none.
        assert!(matches!(
            recv_event(&host.event_rx, timeout),
            CollabEvent::PeerConnected(_)
        ));

        host.cmd_tx.send(CollabCommand::EndSession).unwrap();
        good_joiner.cmd_tx.send(CollabCommand::EndSession).unwrap();
    }
}
