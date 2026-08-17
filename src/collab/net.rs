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
//!
//! A connection that drops mid-session (network loss, sleep/wake, a phone
//! switching networks) doesn't end the session outright: [`run_session`]'s
//! outer loop falls back to [`reconnect`], bounded by [`RECONNECT_TIMEOUT`],
//! reusing the *same* directional keys (and, for a joiner, the same pasted
//! ticket) rather than generating new ones — which is what lets the original
//! connection code keep working to get the peer back, with no fresh Host/Join
//! needed on either side. Only once reconnection itself is exhausted (or the
//! caller sends `EndSession`) does the session actually end.

use std::future::Future;
use std::sync::mpsc::Sender as EventSender;
use std::time::Duration;

use iroh::endpoint::{RecvStream, SendStream};
use tokio::sync::mpsc::UnboundedReceiver;

use crate::collab::crypto::{Direction, OpenCipher, SealCipher, SessionKey};
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

/// How long the background thread keeps trying to get the peer back after an
/// established connection drops, before giving up and ending the session for
/// good. Generous enough to survive a laptop going to sleep and waking, or a
/// phone switching networks — but still bounded, so a peer that's gone for
/// good doesn't leave the session "reconnecting" forever.
const RECONNECT_TIMEOUT: Duration = Duration::from_secs(60);

/// How long a joiner waits between reconnect attempts. The host side doesn't
/// need an equivalent: `endpoint.accept()` just blocks until someone
/// connects, so its retry loop is naturally paced by incoming connections
/// rather than a timer.
const RECONNECT_RETRY_INTERVAL: Duration = Duration::from_secs(2);

/// Session-lifecycle tracing for local debugging of the handshake/session-loop
/// state machine — compiled out of release builds. Real errors are always
/// surfaced to the UI via `CollabEvent::Error` regardless of this; these calls
/// are extra, redundant console notes about *when* in the state machine
/// something happened, not the only place a failure is reported.
macro_rules! debug_log {
    ($($arg:tt)*) => {{
        #[cfg(debug_assertions)]
        println!($($arg)*);
        // Reference the interpolated variables even when the print above is
        // compiled out, so a release build doesn't warn that (e.g.) an `err`
        // bound only for a debug_log! call is unused.
        #[cfg(not(debug_assertions))]
        let _ = format_args!($($arg)*);
    }};
}

/// Runs one collaboration session to completion: establishes the iroh
/// connection (hosting or joining, per `role`), then shuttles bytes between
/// the peer and the channels connecting back to the main thread until the
/// session ends for good (see the module doc for what "ends" means now that
/// a dropped connection tries to reconnect first). Blocks the calling thread
/// for the session's whole lifetime — call this from its own
/// `std::thread::spawn`, as [`crate::collab::spawn_collab_session`] does.
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

/// Connection-attempt-invariant material for a session: the session's
/// directional frame keys (see `crypto::derive_directional_key`), computed
/// once regardless of how many physical connections this logical session
/// goes through. Reconnecting rederives fresh *ciphers* from these same keys
/// (frame counters must restart per QUIC connection, see `crypto`'s module
/// doc) but never regenerates the keys themselves — which is exactly what
/// lets the original connection code keep working across a reconnect.
enum RoleMaterial {
    Host {
        send_key: SessionKey,
        recv_key: SessionKey,
    },
    Join {
        ticket: CollabTicket,
        send_key: SessionKey,
        recv_key: SessionKey,
    },
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
    debug_log!("[collab:{role_label}] starting session");

    let secret_key = iroh::SecretKey::generate();
    let endpoint = match iroh::Endpoint::builder(iroh::endpoint::presets::N0)
        .secret_key(secret_key)
        .alpns(vec![ALPN.to_vec()])
        .bind()
        .await
    {
        Ok(endpoint) => endpoint,
        Err(err) => {
            debug_log!("[collab:{role_label}] failed to bind endpoint: {err}");
            send_event(
                &event_tx,
                &ctx,
                CollabEvent::Error(format!("failed to start networking: {err}")),
            );
            return;
        }
    };
    debug_log!(
        "[collab:{role_label}] endpoint bound, id={}",
        endpoint.id().fmt_short()
    );

    // Local edits that arrive while there's no live connection to send them
    // over — waiting for the first peer, or waiting to reconnect after one
    // drops — are queued here and flushed right after the next successful
    // (re)connection, rather than dropped. `pending_edits` is threaded
    // through the whole function precisely so this applies uniformly to
    // both cases: a fresh session's bootstrap diff and a mid-session edit
    // typed during an outage are handled identically.
    let mut pending_edits: Vec<Vec<u8>> = Vec::new();

    let outcome = race_with_commands(
        establish_initial(&role, &endpoint, &event_tx, &ctx),
        &mut cmd_rx,
        &mut pending_edits,
    )
    .await;
    let Some(outcome) = outcome else {
        debug_log!("[collab:{role_label}] session ended before a peer connected, tearing down");
        endpoint.close().await;
        return;
    };
    let Some((mut established, material)) = outcome else {
        debug_log!(
            "[collab:{role_label}] establish_initial() failed — see CollabEvent::Error above"
        );
        endpoint.close().await;
        return;
    };

    loop {
        debug_log!(
            "[collab:{role_label}] connected to peer {}",
            established.peer_fingerprint
        );
        send_event(
            &event_tx,
            &ctx,
            CollabEvent::PeerConnected(established.peer_fingerprint.clone()),
        );

        for bytes in pending_edits.drain(..) {
            if let Err(err) = write_encrypted(
                &mut established.send_stream,
                &mut established.sealer,
                &bytes,
            )
            .await
            {
                debug_log!("[collab:{role_label}] flushing queued edit to peer failed: {err}");
                send_event(
                    &event_tx,
                    &ctx,
                    CollabEvent::Error(format!("failed to send edit to peer: {err}")),
                );
                endpoint.close().await;
                return;
            }
        }

        match run_connected(established, &mut cmd_rx, &event_tx, &ctx, role_label).await {
            ConnectedOutcome::Ended => {
                debug_log!("[collab:{role_label}] session ending, tearing down");
                endpoint.close().await;
                return;
            }
            ConnectedOutcome::ConnectionLost => {
                debug_log!(
                    "[collab:{role_label}] connection lost, attempting to reconnect (up to {RECONNECT_TIMEOUT:?})"
                );
                send_event(&event_tx, &ctx, CollabEvent::Reconnecting);
            }
        }

        let reconnected = race_with_commands(
            tokio::time::timeout(
                RECONNECT_TIMEOUT,
                reconnect(role_label, &material, &endpoint),
            ),
            &mut cmd_rx,
            &mut pending_edits,
        )
        .await;
        established = match reconnected {
            None => {
                debug_log!(
                    "[collab:{role_label}] EndSession received while reconnecting, tearing down"
                );
                endpoint.close().await;
                return;
            }
            Some(Ok(ReconnectOutcome::Established(established))) => {
                debug_log!("[collab:{role_label}] reconnected");
                established
            }
            Some(Ok(ReconnectOutcome::EndpointClosed)) => {
                debug_log!("[collab:{role_label}] endpoint closed while reconnecting, giving up");
                send_event(&event_tx, &ctx, CollabEvent::PeerDisconnected);
                endpoint.close().await;
                return;
            }
            Some(Err(_timed_out)) => {
                debug_log!(
                    "[collab:{role_label}] gave up reconnecting after {RECONNECT_TIMEOUT:?}"
                );
                send_event(&event_tx, &ctx, CollabEvent::PeerDisconnected);
                endpoint.close().await;
                return;
            }
        };
    }
}

/// Runs `fut` to completion, or returns `None` if `EndSession` arrives (or
/// the command channel closes) first — the shape both the initial connection
/// attempt and every later reconnect attempt need, so this replaces what
/// used to be a one-off `tokio::select!` loop inlined just for the first
/// connect. Any `LocalEdit` arriving meanwhile is queued into
/// `pending_edits` rather than dropped or blocked on, so an edit typed
/// during a (re)connection wait isn't lost — it's flushed once `fut`
/// actually completes (see `run_session`).
async fn race_with_commands<T>(
    fut: impl Future<Output = T>,
    cmd_rx: &mut UnboundedReceiver<CollabCommand>,
    pending_edits: &mut Vec<Vec<u8>>,
) -> Option<T> {
    tokio::pin!(fut);
    loop {
        tokio::select! {
            result = &mut fut => return Some(result),
            command = cmd_rx.recv() => match command {
                Some(CollabCommand::LocalEdit(bytes)) => pending_edits.push(bytes),
                Some(CollabCommand::EndSession) | None => return None,
            }
        }
    }
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

/// The result of a completed connection attempt: the stream pair, the
/// stateful directional frame ciphers (see `crypto`), and a short display
/// fingerprint for the peer (see `iroh::PublicKey::fmt_short`).
struct Established {
    send_stream: SendStream,
    recv_stream: RecvStream,
    sealer: SealCipher,
    opener: OpenCipher,
    peer_fingerprint: String,
}

/// Establishes the *first* connection of a session, per `role`: generates
/// (host) or decodes (joiner) the session's directional keys, then performs
/// one successful handshake — waiting indefinitely for a host, a single
/// attempt for a joiner (unlike [`reconnect`], the first connection isn't
/// retried; a joiner who can't reach the host at all gets a prompt,
/// immediate error). Returns the established connection alongside the
/// [`RoleMaterial`] a later [`reconnect`] needs, or `None` if an event was
/// already sent reporting why establishment failed.
async fn establish_initial(
    role: &SessionRole,
    endpoint: &iroh::Endpoint,
    event_tx: &EventSender<CollabEvent>,
    ctx: &egui::Context,
) -> Option<(Established, RoleMaterial)> {
    match role {
        SessionRole::Host => {
            let session_secret: [u8; 32] = rand::random();
            let host_id = *endpoint.id().as_bytes();
            let send_key =
                crypto::derive_directional_key(&session_secret, &host_id, Direction::HostToJoiner);
            let recv_key =
                crypto::derive_directional_key(&session_secret, &host_id, Direction::JoinerToHost);
            debug_log!("[collab:host] waiting for a relay address...");
            let addr = wait_for_relay_addr(endpoint).await;
            debug_log!(
                "[collab:host] addr resolved: {} addr(s), relay={}",
                addr.addrs.len(),
                addr.addrs.iter().any(|a| a.is_relay())
            );
            let ticket = CollabTicket::new(addr, session_secret);
            send_event(event_tx, ctx, CollabEvent::HostReady(ticket.encode()));

            // Keep accepting until a connector proves it holds the session
            // key. Anyone else (whoever reached this endpoint id without the
            // code) burns only its own attempt, not the session: the genuine
            // collaborator can still pair with the same code afterwards.
            loop {
                match accept_one_host(endpoint, &send_key, &recv_key, "host").await {
                    HostAcceptOutcome::Established(established) => {
                        return Some((established, RoleMaterial::Host { send_key, recv_key }));
                    }
                    HostAcceptOutcome::Retry => continue,
                    HostAcceptOutcome::EndpointClosed => {
                        send_event(
                            event_tx,
                            ctx,
                            CollabEvent::Error(
                                "networking endpoint closed before a peer connected".to_string(),
                            ),
                        );
                        return None;
                    }
                }
            }
        }
        SessionRole::Join(code) => {
            let ticket = match CollabTicket::decode(code) {
                Ok(ticket) => ticket,
                Err(err) => {
                    debug_log!("[collab:joiner] invalid connection code: {err}");
                    send_event(
                        event_tx,
                        ctx,
                        CollabEvent::Error(format!("invalid connection code: {err}")),
                    );
                    return None;
                }
            };
            let host_id = *ticket.endpoint_addr.id.as_bytes();
            let send_key = crypto::derive_directional_key(
                &ticket.session_secret,
                &host_id,
                Direction::JoinerToHost,
            );
            let recv_key = crypto::derive_directional_key(
                &ticket.session_secret,
                &host_id,
                Direction::HostToJoiner,
            );
            match connect_one_joiner(endpoint, &ticket, &send_key, &recv_key, "joiner").await {
                Ok(established) => Some((
                    established,
                    RoleMaterial::Join {
                        ticket,
                        send_key,
                        recv_key,
                    },
                )),
                Err(reason) => {
                    send_event(event_tx, ctx, CollabEvent::Error(reason));
                    None
                }
            }
        }
    }
}

/// One host-side accept-and-handshake attempt: waits for exactly one
/// incoming connection and tries the encrypted handshake against it.
/// Returns `Retry` for anything specific to that one attempt (a stray
/// connector, a rejected/timed-out handshake) — the caller keeps listening
/// either way, whether this is the first connection ([`establish_initial`])
/// or trying to get a dropped peer back ([`reconnect`]) — or
/// `EndpointClosed` if the endpoint itself is gone, which neither caller can
/// recover from.
async fn accept_one_host(
    endpoint: &iroh::Endpoint,
    send_key: &SessionKey,
    recv_key: &SessionKey,
    role_label: &'static str,
) -> HostAcceptOutcome {
    debug_log!("[collab:{role_label}] waiting for an incoming connection...");
    let Some(incoming) = endpoint.accept().await else {
        debug_log!("[collab:{role_label}] endpoint.accept() returned None (endpoint closed)");
        return HostAcceptOutcome::EndpointClosed;
    };
    let conn = match incoming.await {
        Ok(conn) => conn,
        Err(err) => {
            debug_log!("[collab:{role_label}] incoming connection failed ({err}), still listening");
            return HostAcceptOutcome::Retry;
        }
    };
    let peer_fingerprint = conn.remote_id().fmt_short().to_string();
    debug_log!(
        "[collab:{role_label}] connection from {peer_fingerprint}, awaiting its handshake..."
    );

    // Fresh ciphers per attempt: frame counters are per-connection, and a
    // rejected attempt never produced a frame under these keys anyway
    // (nothing it sent decrypted).
    let mut opener = OpenCipher::new(recv_key);
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
            debug_log!(
                "[collab:{role_label}] pairing attempt by {peer_fingerprint} rejected ({reason}), still listening"
            );
            conn.close(1u32.into(), b"pairing failed");
            return HostAcceptOutcome::Retry;
        }
        Err(_) => {
            debug_log!(
                "[collab:{role_label}] pairing attempt by {peer_fingerprint} timed out, still listening"
            );
            conn.close(1u32.into(), b"pairing timed out");
            return HostAcceptOutcome::Retry;
        }
    };

    let mut sealer = SealCipher::new(send_key);
    // The ack is the joiner's mirror-image proof that this side holds the
    // key too — it won't report the peer as connected (or send anything)
    // until this decrypts.
    if let Err(err) = write_frame(&mut send_stream, &sealer.seal(&[])).await {
        debug_log!(
            "[collab:{role_label}] sending the handshake ack failed ({err}), still listening"
        );
        return HostAcceptOutcome::Retry;
    }
    debug_log!("[collab:{role_label}] handshake complete with {peer_fingerprint}");
    HostAcceptOutcome::Established(Established {
        send_stream,
        recv_stream,
        sealer,
        opener,
        peer_fingerprint,
    })
}

enum HostAcceptOutcome {
    Established(Established),
    Retry,
    EndpointClosed,
}

/// One joiner-side connect-and-handshake attempt against `ticket`. Returns
/// `Err` with a human-readable reason on any failure — [`establish_initial`]
/// surfaces that reason directly as a fatal `CollabEvent::Error` (the first
/// connection isn't retried), while [`reconnect`] just logs it and tries
/// again after `RECONNECT_RETRY_INTERVAL`.
async fn connect_one_joiner(
    endpoint: &iroh::Endpoint,
    ticket: &CollabTicket,
    send_key: &SessionKey,
    recv_key: &SessionKey,
    role_label: &'static str,
) -> Result<Established, String> {
    // Fresh ciphers per attempt — see `accept_one_host`'s doc comment.
    let mut sealer = SealCipher::new(send_key);
    let mut opener = OpenCipher::new(recv_key);
    debug_log!(
        "[collab:{role_label}] connecting to host ({} addr(s), relay={})...",
        ticket.endpoint_addr.addrs.len(),
        ticket.endpoint_addr.addrs.iter().any(|a| a.is_relay())
    );
    let conn = endpoint
        .connect(ticket.endpoint_addr.clone(), ALPN)
        .await
        .map_err(|err| format!("failed to connect to peer: {err}"))?;
    let peer_fingerprint = conn.remote_id().fmt_short().to_string();
    debug_log!("[collab:{role_label}] connected to {peer_fingerprint}, opening bi-stream...");
    let (mut send_stream, mut recv_stream) = conn
        .open_bi()
        .await
        .map_err(|err| format!("failed to open stream to peer: {err}"))?;
    // The encrypted ping both proves this side holds the session key (the
    // host rejects the pairing otherwise) and unblocks the host's
    // `accept_bi` in the first place — that call only resolves once actual
    // data arrives on the stream (a documented quirk of iroh/QUIC
    // bidirectional streams, not optional).
    debug_log!("[collab:{role_label}] open_bi() succeeded, sending handshake ping...");
    write_frame(&mut send_stream, &sealer.seal(&[]))
        .await
        .map_err(|err| format!("failed to complete handshake with peer: {err}"))?;
    // ...and the host's ack is the mirror-image proof: nothing is reported
    // as connected here until the host has shown it can produce a frame
    // under the session key too.
    debug_log!("[collab:{role_label}] handshake ping sent, waiting for the host's ack...");
    let ack = tokio::time::timeout(HANDSHAKE_TIMEOUT, async {
        let frame = read_frame(&mut recv_stream).await?;
        opener.open(&frame)?;
        Ok::<(), FrameError>(())
    })
    .await;
    match ack {
        Ok(Ok(())) => {
            debug_log!("[collab:{role_label}] handshake complete with {peer_fingerprint}");
            Ok(Established {
                send_stream,
                recv_stream,
                sealer,
                opener,
                peer_fingerprint,
            })
        }
        Ok(Err(err)) => Err(format!(
            "handshake with the host failed: {err} — check the connection code"
        )),
        Err(_) => Err(
            "the host did not answer the handshake in time — check the connection code".to_string(),
        ),
    }
}

enum ReconnectOutcome {
    Established(Established),
    /// The endpoint itself is gone — nothing left to reconnect with. Only
    /// reachable on the host side (`accept_one_host`'s `EndpointClosed`);
    /// the joiner side has no equivalent failure mode, it just keeps
    /// retrying `connect_one_joiner`.
    EndpointClosed,
}

/// Retries connecting to the peer after an established connection dropped,
/// reusing `material`'s already-derived keys (and, for a joiner, its already
/// -pasted ticket) rather than generating anything new — see the module
/// doc. Loops until a connection succeeds or the endpoint closes; the
/// caller ([`run_session`]) bounds this with `tokio::time::timeout` so a
/// peer that's gone for good doesn't leave the session waiting forever.
async fn reconnect(
    role_label: &'static str,
    material: &RoleMaterial,
    endpoint: &iroh::Endpoint,
) -> ReconnectOutcome {
    loop {
        match material {
            RoleMaterial::Host { send_key, recv_key } => {
                match accept_one_host(endpoint, send_key, recv_key, role_label).await {
                    HostAcceptOutcome::Established(established) => {
                        return ReconnectOutcome::Established(established);
                    }
                    HostAcceptOutcome::Retry => continue,
                    HostAcceptOutcome::EndpointClosed => return ReconnectOutcome::EndpointClosed,
                }
            }
            RoleMaterial::Join {
                ticket,
                send_key,
                recv_key,
            } => match connect_one_joiner(endpoint, ticket, send_key, recv_key, role_label).await {
                Ok(established) => return ReconnectOutcome::Established(established),
                Err(reason) => {
                    debug_log!(
                        "[collab:{role_label}] reconnect attempt failed ({reason}), retrying in {RECONNECT_RETRY_INTERVAL:?}"
                    );
                    tokio::time::sleep(RECONNECT_RETRY_INTERVAL).await;
                }
            },
        }
    }
}

enum ConnectedOutcome {
    /// `EndSession` arrived, or the command channel closed (the
    /// `CollabSession` handle was dropped) — a deliberate, final end; the
    /// caller must not try to reconnect.
    Ended,
    /// The connection itself died (a read or write failed, or the reader
    /// task ended) — the caller should try to reconnect rather than treat
    /// this as final.
    ConnectionLost,
}

/// Shuttles bytes over one live connection until it ends, one way or the
/// other (see [`ConnectedOutcome`]). Consumes `established`: a reconnect
/// always produces a fresh stream pair and ciphers, never reuses these.
async fn run_connected(
    established: Established,
    cmd_rx: &mut UnboundedReceiver<CollabCommand>,
    event_tx: &EventSender<CollabEvent>,
    ctx: &egui::Context,
    role_label: &'static str,
) -> ConnectedOutcome {
    let Established {
        mut send_stream,
        recv_stream,
        mut sealer,
        opener,
        ..
    } = established;

    // The reader runs as its own task so it can be blocked on
    // `read_encrypted` (an inherently async wait for the peer's next
    // message) concurrently with this thread's loop waiting on local
    // commands — a single `tokio::select!` can't hold two separate `&mut`
    // borrows of the one stream pair across loop iterations the way two
    // tasks naturally can.
    let reader_event_tx = event_tx.clone();
    let reader_ctx = ctx.clone();
    let mut reader = tokio::spawn(async move {
        let mut recv_stream = recv_stream;
        let mut opener = opener;
        loop {
            match read_encrypted(&mut recv_stream, &mut opener).await {
                // The handshake ping/ack are consumed before this task
                // starts, but tolerate an empty frame here too — it carries
                // no update, so swallow it rather than surfacing a
                // meaningless empty `RemoteUpdate`.
                Ok(bytes) if bytes.is_empty() => {}
                Ok(bytes) => {
                    send_event(
                        &reader_event_tx,
                        &reader_ctx,
                        CollabEvent::RemoteUpdate(bytes),
                    );
                }
                Err(err) => {
                    debug_log!(
                        "[collab:{role_label}] read from peer failed, connection lost: {err}"
                    );
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
    let outcome = loop {
        tokio::select! {
            command = cmd_rx.recv() => {
                match command {
                    Some(CollabCommand::LocalEdit(bytes)) => {
                        if let Err(err) =
                            write_encrypted(&mut send_stream, &mut sealer, &bytes).await
                        {
                            debug_log!(
                                "[collab:{role_label}] write to peer failed, connection lost: {err}"
                            );
                            break ConnectedOutcome::ConnectionLost;
                        }
                    }
                    Some(CollabCommand::EndSession) => {
                        debug_log!("[collab:{role_label}] EndSession command received");
                        break ConnectedOutcome::Ended;
                    }
                    None => {
                        debug_log!(
                            "[collab:{role_label}] command channel closed (CollabSession dropped)"
                        );
                        break ConnectedOutcome::Ended;
                    }
                }
            }
            _ = &mut reader => {
                break ConnectedOutcome::ConnectionLost;
            }
        }
    };
    reader.abort();
    outcome
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

    #[test]
    #[ignore = "requires real internet access to iroh's public relay infrastructure"]
    fn a_dropped_connection_reconnects_with_the_same_ticket_and_keeps_working() {
        // Regression test for #81: dropping a joiner's connection (not a
        // graceful EndSession) must not end the host's session outright —
        // it should report `Reconnecting` and keep its endpoint listening,
        // so a fresh joiner pasting the *same* connection code can pair
        // again and the session carries on, rather than requiring a brand
        // new Host Session on the host's side.
        let ctx = egui::Context::default();
        let timeout = Duration::from_secs(30);

        let host = spawn_collab_session(SessionRole::Host, ctx.clone());
        let code = match recv_event(&host.event_rx, timeout) {
            CollabEvent::HostReady(code) => code,
            other => panic!("expected HostReady, got {other:?}"),
        };

        let joiner = spawn_collab_session(SessionRole::Join(code.clone()), ctx.clone());
        assert!(matches!(
            recv_event(&joiner.event_rx, timeout),
            CollabEvent::PeerConnected(_)
        ));
        assert!(matches!(
            recv_event(&host.event_rx, timeout),
            CollabEvent::PeerConnected(_)
        ));

        // Simulates the joiner's connection dying outright (network loss,
        // sleep/wake) rather than a graceful end: drop its handle without
        // sending `EndSession`.
        drop(joiner);

        assert!(matches!(
            recv_event(&host.event_rx, timeout),
            CollabEvent::Reconnecting
        ));

        // A fresh joiner, pasting the exact same code, reaches the same
        // still-listening host.
        let rejoined = spawn_collab_session(SessionRole::Join(code), ctx.clone());
        assert!(matches!(
            recv_event(&rejoined.event_rx, timeout),
            CollabEvent::PeerConnected(_)
        ));
        assert!(matches!(
            recv_event(&host.event_rx, timeout),
            CollabEvent::PeerConnected(_)
        ));

        // And bytes still flow normally after recovering.
        host.cmd_tx
            .send(CollabCommand::LocalEdit(b"still here".to_vec()))
            .unwrap();
        match recv_event(&rejoined.event_rx, timeout) {
            CollabEvent::RemoteUpdate(bytes) => assert_eq!(bytes, b"still here"),
            other => panic!("expected RemoteUpdate, got {other:?}"),
        }

        host.cmd_tx.send(CollabCommand::EndSession).unwrap();
        rejoined.cmd_tx.send(CollabCommand::EndSession).unwrap();
    }

    #[test]
    #[ignore = "requires real internet access to iroh's public relay infrastructure"]
    fn ending_the_session_while_reconnecting_stops_it_promptly() {
        // `EndSession` sent while the host is in its bounded reconnect
        // window must interrupt that wait immediately, not block until
        // `RECONNECT_TIMEOUT` elapses on its own.
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

        drop(joiner);
        assert!(matches!(
            recv_event(&host.event_rx, timeout),
            CollabEvent::Reconnecting
        ));

        host.cmd_tx.send(CollabCommand::EndSession).unwrap();

        // Observable as the event channel disconnecting once the background
        // thread exits — well before `RECONNECT_TIMEOUT` (60s) would have.
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            match host.event_rx.recv_timeout(Duration::from_millis(100)) {
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Ok(other) => panic!("expected no further events, got {other:?}"),
            }
            assert!(
                std::time::Instant::now() < deadline,
                "background thread did not exit promptly after EndSession while reconnecting"
            );
        }
    }
}
