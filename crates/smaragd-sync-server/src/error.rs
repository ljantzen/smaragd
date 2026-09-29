//! HTTP-facing errors: each maps to a status code and the protocol's JSON
//! [`ApiError`] body. Internal failures are logged in full but reported to the
//! client only as an opaque 500, so database details never leak.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use smaragd_sync_protocol::api::ApiError;

#[derive(Debug, thiserror::Error)]
pub enum HttpError {
    #[error("missing or invalid credentials")]
    Unauthorized,
    #[error("{0}")]
    Forbidden(&'static str),
    #[error("{0}")]
    NotFound(&'static str),
    #[error("{0}")]
    BadRequest(String),
    #[error("request body is too large")]
    PayloadTooLarge,
    #[error("this vault has reached its storage quota")]
    QuotaExceeded,
    #[error("the request took too long")]
    Timeout,
    #[error("internal error")]
    Internal(String),
}

impl From<rusqlite::Error> for HttpError {
    fn from(err: rusqlite::Error) -> Self {
        HttpError::Internal(err.to_string())
    }
}

impl IntoResponse for HttpError {
    fn into_response(self) -> Response {
        let status = match &self {
            HttpError::Unauthorized => StatusCode::UNAUTHORIZED,
            HttpError::Forbidden(_) => StatusCode::FORBIDDEN,
            HttpError::NotFound(_) => StatusCode::NOT_FOUND,
            HttpError::BadRequest(_) => StatusCode::BAD_REQUEST,
            HttpError::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            HttpError::QuotaExceeded => StatusCode::INSUFFICIENT_STORAGE,
            HttpError::Timeout => StatusCode::REQUEST_TIMEOUT,
            HttpError::Internal(detail) => {
                tracing::error!("internal error: {detail}");
                StatusCode::INTERNAL_SERVER_ERROR
            }
        };
        let body = ApiError {
            error: self.to_string(),
        };
        (status, Json(body)).into_response()
    }
}
