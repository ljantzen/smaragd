//! The native HTTP client for the sync server: a blocking `ureq` implementation of
//! [`SyncTransport`] (what the engine needs) plus the control-plane calls the UI
//! needs to create a vault, pair devices and manage them.
//!
//! Native-only: `ureq` is a blocking socket client with no browser story. A future
//! browser build would implement [`SyncTransport`] over `fetch` instead; nothing in
//! the engine changes.
//!
//! Every failure to *reach* the server (DNS, refused, timeout, TLS) is reported as
//! [`TransportError::Offline`] — retryable, and local edits keep queuing meanwhile —
//! while an HTTP answer is mapped by status: 401 means the device's token is no
//! longer valid, other 4xx are refusals with the server's message, 5xx are server
//! faults.

use std::io::Read;
use std::time::Duration;

use serde::Serialize;
use serde::de::DeserializeOwned;
use smaragd_sync_protocol::api::{
    ADMIN_TOKEN_HEADER, API_PREFIX, ApiError, CreatePairingCodeResponse, CreateVaultRequest,
    CreateVaultResponse, DeviceInfo, HealthResponse, ListDevicesResponse, ListDocsResponse,
    MAX_BLOB_BYTES, PullUpdatesResponse, PushUpdateResponse, RedeemPairingRequest,
    RedeemPairingResponse, Snapshot, VaultInfo,
};
use smaragd_sync_protocol::ticket::ServerAddr;
use smaragd_sync_protocol::{DeviceId, DocId, VaultId};

use super::transport::{SyncTransport, TransportError};

/// Largest response body accepted: a snapshot is base64 (4/3 of the raw blob) inside
/// JSON, so give it headroom over `MAX_BLOB_BYTES`.
const MAX_RESPONSE_BYTES: u64 = (MAX_BLOB_BYTES as u64) * 2;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// A connection to one sync server, optionally authenticated as one device.
#[derive(Debug, Clone)]
pub struct HttpClient {
    agent: ureq::Agent,
    /// `scheme://host:port[/path]/v1`
    api_base: String,
    token: Option<String>,
}

enum Body<'a> {
    None,
    Json(Vec<u8>),
    Bytes(&'a [u8]),
}

impl HttpClient {
    pub fn new(server: &ServerAddr) -> Self {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            // The API never redirects, and following one would be a leak: ureq drops only
            // `Authorization` on a redirect, so the admin token header (and every request
            // body) would go wherever the `Location` says, even from https to http.
            .max_redirects(0)
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_global(Some(REQUEST_TIMEOUT))
            .build()
            .into();
        Self {
            agent,
            api_base: format!("{}{API_PREFIX}", server.base_url()),
            token: None,
        }
    }

    /// The same connection, authenticated as the device owning `token`.
    pub fn with_token(mut self, token: impl Into<String>) -> Self {
        self.token = Some(token.into());
        self
    }

    fn request(
        &self,
        method: &str,
        path: &str,
        extra_header: Option<(&str, &str)>,
        body: Body<'_>,
    ) -> Result<Vec<u8>, TransportError> {
        let url = format!("{}{path}", self.api_base);
        macro_rules! decorate {
            ($request:expr) => {{
                let mut request = $request;
                if let Some(token) = &self.token {
                    request = request.header("Authorization", format!("Bearer {token}"));
                }
                if let Some((name, value)) = extra_header {
                    request = request.header(name, value);
                }
                request
            }};
        }
        let result = match (method, body) {
            ("GET", Body::None) => decorate!(self.agent.get(&url)).call(),
            ("DELETE", Body::None) => decorate!(self.agent.delete(&url)).call(),
            ("POST", Body::None) => decorate!(self.agent.post(&url)).send_empty(),
            ("POST", Body::Json(bytes)) => decorate!(self.agent.post(&url))
                .header("Content-Type", "application/json")
                .send(&bytes[..]),
            ("PUT", Body::Json(bytes)) => decorate!(self.agent.put(&url))
                .header("Content-Type", "application/json")
                .send(&bytes[..]),
            ("POST", Body::Bytes(bytes)) => decorate!(self.agent.post(&url))
                .header("Content-Type", "application/octet-stream")
                .send(bytes),
            (method, _) => unreachable!("unsupported request shape: {method}"),
        };
        let mut response = result.map_err(|err| TransportError::Offline(err.to_string()))?;

        let status = response.status().as_u16();
        let mut bytes = Vec::new();
        response
            .body_mut()
            .as_reader()
            .take(MAX_RESPONSE_BYTES)
            .read_to_end(&mut bytes)
            .map_err(|err| TransportError::Offline(format!("reading the response: {err}")))?;

        match status {
            200..=299 => Ok(bytes),
            401 => Err(TransportError::Unauthorized),
            400..=499 | 507 => Err(TransportError::Rejected(server_message(&bytes, status))),
            _ => Err(TransportError::Server(server_message(&bytes, status))),
        }
    }

    fn json<T: DeserializeOwned>(
        &self,
        method: &str,
        path: &str,
        extra_header: Option<(&str, &str)>,
        body: Body<'_>,
    ) -> Result<T, TransportError> {
        let bytes = self.request(method, path, extra_header, body)?;
        serde_json::from_slice(&bytes)
            .map_err(|err| TransportError::Server(format!("unreadable server reply: {err}")))
    }

    fn json_body(value: &impl Serialize) -> Body<'static> {
        Body::Json(serde_json::to_vec(value).expect("protocol types always serialize"))
    }

    // --- control plane -----------------------------------------------------

    /// `GET /health` — also what the Settings "Test connection" button calls.
    pub fn health(&self) -> Result<HealthResponse, TransportError> {
        self.json("GET", "/health", None, Body::None)
    }

    /// Creates a vault and this client's first device. `admin_token` is only needed
    /// when the server has open registration turned off. `kdf_salt` is the fresh
    /// random salt the caller derived the vault key with.
    pub fn create_vault(
        &self,
        admin_token: Option<&str>,
        device_name: &str,
        kdf_salt: &[u8],
    ) -> Result<CreateVaultResponse, TransportError> {
        let header = admin_token.map(|token| (ADMIN_TOKEN_HEADER, token));
        self.json(
            "POST",
            "/vaults",
            header,
            Self::json_body(&CreateVaultRequest {
                device_name: device_name.to_string(),
                kdf_salt: kdf_salt.to_vec(),
            }),
        )
    }

    pub fn vault_info(&self, vault: VaultId) -> Result<VaultInfo, TransportError> {
        self.json("GET", &format!("/vaults/{vault}"), None, Body::None)
    }

    /// Deletes a vault and everything in it. Operator-only: the server wants its admin
    /// token for this, not a device token (a device leaves by revoking itself).
    pub fn delete_vault(&self, admin_token: &str, vault: VaultId) -> Result<(), TransportError> {
        self.request(
            "DELETE",
            &format!("/vaults/{vault}"),
            Some((ADMIN_TOKEN_HEADER, admin_token)),
            Body::None,
        )
        .map(|_| ())
    }

    /// Mints a single-use, short-lived code another device can redeem to join.
    pub fn create_pairing_code(
        &self,
        vault: VaultId,
    ) -> Result<CreatePairingCodeResponse, TransportError> {
        self.json(
            "POST",
            &format!("/vaults/{vault}/pairing-codes"),
            None,
            Body::None,
        )
    }

    /// Exchanges a pairing code for this device's own token (needs no token itself).
    pub fn redeem_pairing_code(
        &self,
        code: &str,
        device_name: &str,
    ) -> Result<RedeemPairingResponse, TransportError> {
        self.json(
            "POST",
            "/pairing/redeem",
            None,
            Self::json_body(&RedeemPairingRequest {
                code: code.to_string(),
                device_name: device_name.to_string(),
            }),
        )
    }

    pub fn list_devices(&self, vault: VaultId) -> Result<Vec<DeviceInfo>, TransportError> {
        let listing: ListDevicesResponse =
            self.json("GET", &format!("/vaults/{vault}/devices"), None, Body::None)?;
        Ok(listing.devices)
    }

    pub fn revoke_device(&self, vault: VaultId, device: DeviceId) -> Result<(), TransportError> {
        self.request(
            "DELETE",
            &format!("/vaults/{vault}/devices/{device}"),
            None,
            Body::None,
        )
        .map(|_| ())
    }

    /// The data-plane transport for one vault, authenticated as this client's device.
    pub fn transport(&self, vault: VaultId) -> HttpTransport {
        HttpTransport {
            client: self.clone(),
            vault,
        }
    }
}

/// The server's own explanation if it sent one, else just the status.
fn server_message(body: &[u8], status: u16) -> String {
    serde_json::from_slice::<ApiError>(body)
        .map(|err| err.error)
        .unwrap_or_else(|_| format!("HTTP {status}"))
}

/// [`SyncTransport`] over HTTP for one vault.
#[derive(Debug, Clone)]
pub struct HttpTransport {
    client: HttpClient,
    vault: VaultId,
}

impl SyncTransport for HttpTransport {
    fn list_docs(&self) -> Result<ListDocsResponse, TransportError> {
        self.client.json(
            "GET",
            &format!("/vaults/{}/docs", self.vault),
            None,
            Body::None,
        )
    }

    fn pull(&self, doc: DocId, since: u64) -> Result<PullUpdatesResponse, TransportError> {
        self.client.json(
            "GET",
            &format!("/vaults/{}/docs/{doc}/updates?since={since}", self.vault),
            None,
            Body::None,
        )
    }

    fn push(&self, doc: DocId, sealed: &[u8]) -> Result<u64, TransportError> {
        let pushed: PushUpdateResponse = self.client.json(
            "POST",
            &format!("/vaults/{}/docs/{doc}/updates", self.vault),
            None,
            Body::Bytes(sealed),
        )?;
        Ok(pushed.seq)
    }

    fn put_snapshot(&self, doc: DocId, snapshot: &Snapshot) -> Result<(), TransportError> {
        self.client
            .request(
                "PUT",
                &format!("/vaults/{}/docs/{doc}/snapshot", self.vault),
                None,
                HttpClient::json_body(snapshot),
            )
            .map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn closed_port_server() -> ServerAddr {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        ServerAddr {
            host: "127.0.0.1".into(),
            port,
            use_tls: false,
            path: String::new(),
        }
    }

    #[test]
    fn an_unreachable_server_is_reported_as_offline_not_as_a_rejection() {
        let client = HttpClient::new(&closed_port_server());
        assert!(matches!(client.health(), Err(TransportError::Offline(_))));

        let transport = client
            .with_token("t")
            .transport(VaultId(Uuid::from_u128(1)));
        assert!(matches!(
            transport.list_docs(),
            Err(TransportError::Offline(_))
        ));
        assert!(matches!(
            transport.push(DocId(Uuid::from_u128(2)), b"x"),
            Err(TransportError::Offline(_))
        ));
    }

    #[test]
    fn the_api_base_includes_scheme_port_path_and_version() {
        let server = ServerAddr {
            host: "sync.example.com".into(),
            port: 8443,
            use_tls: true,
            path: "/smaragd/".into(),
        };
        assert_eq!(
            HttpClient::new(&server).api_base,
            "https://sync.example.com:8443/smaragd/v1"
        );
    }

    #[test]
    fn a_redirect_is_never_followed_so_the_admin_token_stays_put() {
        use std::io::{Read as _, Write as _};
        let elsewhere = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        elsewhere.set_nonblocking(true).unwrap();
        let target = elsewhere.local_addr().unwrap();
        let redirector = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = redirector.local_addr().unwrap().port();
        let serve = std::thread::spawn(move || {
            let (mut conn, _) = redirector.accept().unwrap();
            let mut buf = [0u8; 4096];
            let _ = conn.read(&mut buf);
            let reply = format!(
                "HTTP/1.1 302 Found\r\nLocation: http://{target}/v1/vaults\r\n\
                 Content-Length: 0\r\nConnection: close\r\n\r\n"
            );
            conn.write_all(reply.as_bytes()).unwrap();
        });

        let server = ServerAddr {
            host: "127.0.0.1".into(),
            port,
            use_tls: false,
            path: String::new(),
        };
        let result =
            HttpClient::new(&server).delete_vault("admin-secret", VaultId(Uuid::from_u128(1)));
        serve.join().unwrap();
        assert!(result.is_err(), "a redirect is not a deleted vault");
        assert!(
            elsewhere.accept().is_err(),
            "the redirect target was contacted"
        );
    }

    #[test]
    fn server_messages_are_surfaced_when_present() {
        assert_eq!(
            server_message(br#"{"error":"vault is full"}"#, 507),
            "vault is full"
        );
        assert_eq!(server_message(b"<html>", 502), "HTTP 502");
    }
}
