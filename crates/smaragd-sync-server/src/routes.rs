//! The HTTP API. Handlers are thin: authenticate, check the request belongs to the
//! caller's vault, and hand the blocking work to [`crate::db`]. The server never
//! interprets a blob — at most it checks that a pushed blob is shaped like a sealed
//! envelope, as a sanity check against garbage.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, FromRequestParts, Path, Query, Request, State};
use axum::handler::Handler;
use axum::http::header::AUTHORIZATION;
use axum::http::request::Parts;
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use serde::Deserialize;
use smaragd_sync_protocol::api::{
    ADMIN_TOKEN_HEADER, CreatePairingCodeResponse, CreateVaultRequest, CreateVaultResponse,
    HealthResponse, KDF_SALT_LEN, ListDevicesResponse, ListDocsResponse, MAX_BLOB_BYTES,
    PAIRING_CODE_TTL_SECS, PullUpdatesResponse, PushUpdateResponse, RedeemPairingRequest,
    RedeemPairingResponse, Snapshot, VaultInfo,
};
use smaragd_sync_protocol::envelope::Envelope;
use smaragd_sync_protocol::{DeviceId, DocId, VaultId};
use tokio::sync::Semaphore;

use crate::AppState;
use crate::auth::{constant_time_eq, hash_secret};
use crate::db::{self, Authed};
use crate::error::HttpError;

const MAX_DEVICE_NAME_CHARS: usize = 100;
/// Body limit for every route except the two that carry a sealed blob. Everything else
/// is a small JSON object (or no body at all), and two of those routes are open to
/// anyone, so they mustn't be able to make the server buffer megabytes.
const SMALL_BODY_BYTES: usize = 16 * 1024;
/// Body limit for pushing an update or a snapshot. JSON snapshots carry base64 blobs,
/// ~4/3 the raw size.
const BLOB_BODY_BYTES: usize = MAX_BLOB_BYTES * 2;
/// A whole request — waiting for a slot, reading the body, the database work — must
/// finish within this, so a client trickling its body can't hold a slot forever. Well
/// above what an 8 MiB upload over a slow link needs.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
/// Requests handled at once; more wait (within [`REQUEST_TIMEOUT`]) for a slot. Bounds
/// the memory buffered request bodies can take to about this many blob bodies.
pub const MAX_IN_FLIGHT_REQUESTS: usize = 32;

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// A request authenticated by its `Authorization: Bearer <device token>` header.
struct Auth(Authed);

impl Auth {
    /// The caller may only touch their own vault.
    fn require_vault(&self, vault: VaultId) -> Result<(), HttpError> {
        if self.0.vault_id == vault {
            Ok(())
        } else {
            Err(HttpError::Forbidden(
                "this device doesn't belong to that vault",
            ))
        }
    }
}

impl FromRequestParts<Arc<AppState>> for Auth {
    type Rejection = HttpError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, HttpError> {
        let token = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .map(str::trim)
            .filter(|token| !token.is_empty())
            .ok_or(HttpError::Unauthorized)?;
        let hash = hash_secret(token);
        let now = unix_now();
        state
            .db
            .run(move |conn| db::authenticate(conn, &hash, now))
            .await?
            .map(Auth)
            .ok_or(HttpError::Unauthorized)
    }
}

pub fn router(state: Arc<AppState>) -> Router {
    let blob_limit = DefaultBodyLimit::max(BLOB_BODY_BYTES);
    let api = Router::new()
        .route("/health", get(health))
        .route("/vaults", post(create_vault))
        .route("/vaults/{vault}", get(get_vault).delete(delete_vault))
        .route("/vaults/{vault}/pairing-codes", post(create_pairing_code))
        .route("/pairing/redeem", post(redeem_pairing_code))
        .route("/vaults/{vault}/devices", get(list_devices))
        .route("/vaults/{vault}/devices/{device}", delete(revoke_device))
        .route("/vaults/{vault}/docs", get(list_docs))
        .route(
            "/vaults/{vault}/docs/{doc}/updates",
            get(pull_updates).post(push_update.layer(blob_limit)),
        )
        .route(
            "/vaults/{vault}/docs/{doc}/snapshot",
            put(put_snapshot.layer(blob_limit)),
        )
        .layer(DefaultBodyLimit::max(SMALL_BODY_BYTES));
    let slots = Arc::new(Semaphore::new(MAX_IN_FLIGHT_REQUESTS));
    Router::new()
        .nest(smaragd_sync_protocol::api::API_PREFIX, api)
        .layer(middleware::from_fn(move |request: Request, next: Next| {
            let slots = Arc::clone(&slots);
            async move { limit_request(&slots, request, next).await }
        }))
        .with_state(state)
}

/// Runs a request in one of the [`MAX_IN_FLIGHT_REQUESTS`] slots, and gives up on it
/// after [`REQUEST_TIMEOUT`] (queueing included). Slow *headers* are the reverse
/// proxy's job, as is limiting connections per client — see the README.
async fn limit_request(slots: &Semaphore, request: Request, next: Next) -> Response {
    let run = async {
        let Ok(_slot) = slots.acquire().await else {
            return HttpError::Internal("request semaphore closed".into()).into_response();
        };
        next.run(request).await
    };
    match tokio::time::timeout(REQUEST_TIMEOUT, run).await {
        Ok(response) => response,
        Err(_) => HttpError::Timeout.into_response(),
    }
}

/// Whether the request carries this server's admin token (never, if it has none).
fn has_admin_token(headers: &HeaderMap, config: &crate::Config) -> bool {
    match (headers.get(ADMIN_TOKEN_HEADER), &config.admin_token) {
        (Some(given), Some(expected)) => {
            constant_time_eq(given.to_str().unwrap_or_default(), expected)
        }
        _ => false,
    }
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".into(),
        version: env!("CARGO_PKG_VERSION").into(),
    })
}

fn checked_device_name(name: &str) -> Result<String, HttpError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > MAX_DEVICE_NAME_CHARS {
        return Err(HttpError::BadRequest(format!(
            "device name must be 1-{MAX_DEVICE_NAME_CHARS} characters"
        )));
    }
    Ok(name.to_string())
}

/// Anything stored must at least look like a sealed envelope.
fn require_envelope(bytes: &[u8]) -> Result<(), HttpError> {
    Envelope::parse(bytes)
        .map(|_| ())
        .map_err(|err| HttpError::BadRequest(err.to_string()))
}

/// Takes the body as bytes and parses it only once the caller is allowed in, so an
/// anonymous request to a closed server costs no JSON parsing.
async fn create_vault(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<(StatusCode, Json<CreateVaultResponse>), HttpError> {
    if !state.config.allow_open_registration && !has_admin_token(&headers, &state.config) {
        return Err(HttpError::Forbidden(
            "this server only lets its administrator create vaults (send the admin token)",
        ));
    }
    let request: CreateVaultRequest = serde_json::from_slice(&body)
        .map_err(|err| HttpError::BadRequest(format!("invalid request body: {err}")))?;
    if request.kdf_salt.len() != KDF_SALT_LEN {
        return Err(HttpError::BadRequest(format!(
            "kdf_salt must be exactly {KDF_SALT_LEN} bytes"
        )));
    }
    let name = checked_device_name(&request.device_name)?;
    let now = unix_now();
    let created = state
        .db
        .run(move |conn| db::create_vault(conn, &request.kdf_salt, &name, now))
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(CreateVaultResponse {
            vault: created.vault,
            device_id: created.device_id,
            device_token: created.token,
        }),
    ))
}

async fn get_vault(
    State(state): State<Arc<AppState>>,
    auth: Auth,
    Path(vault): Path<VaultId>,
) -> Result<Json<VaultInfo>, HttpError> {
    auth.require_vault(vault)?;
    state
        .db
        .run(move |conn| db::get_vault(conn, vault))
        .await?
        .map(Json)
        .ok_or(HttpError::NotFound("no such vault"))
}

/// Operator-only: a device token isn't enough, so one leaked token can't wipe a vault.
/// A device that wants out revokes itself instead, and maintenance removes a vault
/// once its last device is gone.
async fn delete_vault(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(vault): Path<VaultId>,
) -> Result<StatusCode, HttpError> {
    if !has_admin_token(&headers, &state.config) {
        return Err(HttpError::Forbidden(
            "only the server's administrator can delete a vault (send the admin token)",
        ));
    }
    let removed = state
        .db
        .run(move |conn| db::delete_vault(conn, vault))
        .await?;
    if removed {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(HttpError::NotFound("no such vault"))
    }
}

async fn create_pairing_code(
    State(state): State<Arc<AppState>>,
    auth: Auth,
    Path(vault): Path<VaultId>,
) -> Result<Json<CreatePairingCodeResponse>, HttpError> {
    auth.require_vault(vault)?;
    let (device, now) = (auth.0.device_id, unix_now());
    let code = state
        .db
        .run(move |conn| db::create_pairing_code(conn, vault, device, now, PAIRING_CODE_TTL_SECS))
        .await?;
    Ok(Json(CreatePairingCodeResponse {
        code,
        expires_in_secs: PAIRING_CODE_TTL_SECS,
    }))
}

async fn redeem_pairing_code(
    State(state): State<Arc<AppState>>,
    Json(request): Json<RedeemPairingRequest>,
) -> Result<Json<RedeemPairingResponse>, HttpError> {
    let name = checked_device_name(&request.device_name)?;
    let now = unix_now();
    let redeemed = state
        .db
        .run(move |conn| db::redeem_pairing_code(conn, &request.code, &name, now))
        .await?
        // One message for wrong, expired and already-used codes alike.
        .ok_or(HttpError::NotFound("invalid or expired pairing code"))?;
    Ok(Json(RedeemPairingResponse {
        vault: redeemed.vault,
        device_id: redeemed.device_id,
        device_token: redeemed.token,
    }))
}

async fn list_devices(
    State(state): State<Arc<AppState>>,
    auth: Auth,
    Path(vault): Path<VaultId>,
) -> Result<Json<ListDevicesResponse>, HttpError> {
    auth.require_vault(vault)?;
    let devices = state
        .db
        .run(move |conn| db::list_devices(conn, vault))
        .await?;
    Ok(Json(ListDevicesResponse { devices }))
}

async fn revoke_device(
    State(state): State<Arc<AppState>>,
    auth: Auth,
    Path((vault, device)): Path<(VaultId, DeviceId)>,
) -> Result<StatusCode, HttpError> {
    auth.require_vault(vault)?;
    let now = unix_now();
    let removed = state
        .db
        .run(move |conn| db::revoke_device(conn, vault, device, now))
        .await?;
    if removed {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(HttpError::NotFound("no such device"))
    }
}

async fn list_docs(
    State(state): State<Arc<AppState>>,
    auth: Auth,
    Path(vault): Path<VaultId>,
) -> Result<Json<ListDocsResponse>, HttpError> {
    auth.require_vault(vault)?;
    let docs = state.db.run(move |conn| db::list_docs(conn, vault)).await?;
    Ok(Json(ListDocsResponse {
        docs,
        max_file_bytes: state.config.max_file_bytes,
    }))
}

async fn push_update(
    State(state): State<Arc<AppState>>,
    auth: Auth,
    Path((vault, doc)): Path<(VaultId, DocId)>,
    body: Bytes,
) -> Result<Json<PushUpdateResponse>, HttpError> {
    auth.require_vault(vault)?;
    if body.len() > MAX_BLOB_BYTES {
        return Err(HttpError::PayloadTooLarge);
    }
    require_envelope(&body)?;
    let (device, quota, now) = (auth.0.device_id, state.config.vault_quota_bytes, unix_now());
    let seq = state
        .db
        .run(move |conn| db::push_update(conn, vault, doc, device, &body, quota, now))
        .await?;
    Ok(Json(PushUpdateResponse { seq }))
}

#[derive(Deserialize)]
struct SinceQuery {
    since: Option<u64>,
}

async fn pull_updates(
    State(state): State<Arc<AppState>>,
    auth: Auth,
    Path((vault, doc)): Path<(VaultId, DocId)>,
    Query(query): Query<SinceQuery>,
) -> Result<Json<PullUpdatesResponse>, HttpError> {
    auth.require_vault(vault)?;
    let since = query.since.unwrap_or(0);
    let pulled = state
        .db
        .run(move |conn| db::pull_updates(conn, vault, doc, since))
        .await?;
    Ok(Json(pulled))
}

async fn put_snapshot(
    State(state): State<Arc<AppState>>,
    auth: Auth,
    Path((vault, doc)): Path<(VaultId, DocId)>,
    Json(snapshot): Json<Snapshot>,
) -> Result<StatusCode, HttpError> {
    auth.require_vault(vault)?;
    if snapshot.blob.len() > MAX_BLOB_BYTES {
        return Err(HttpError::PayloadTooLarge);
    }
    require_envelope(&snapshot.blob)?;
    let quota = state.config.vault_quota_bytes;
    state
        .db
        .run(move |conn| {
            db::put_snapshot(conn, vault, doc, snapshot.upto_seq, &snapshot.blob, quota)
        })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
