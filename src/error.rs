//! Error type used by HTTP handlers and service orchestration.

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;

/// Application error that maps internal failures to HTTP responses.
#[derive(Debug)]
pub struct AppError {
    status: StatusCode,
    error: anyhow::Error,
}

impl AppError {
    /// Builds a `400 Bad Request` error.
    pub(crate) fn bad_request(message: impl Into<anyhow::Error>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            error: message.into(),
        }
    }

    /// Builds a `404 Not Found` error.
    pub(crate) fn not_found(message: impl Into<anyhow::Error>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            error: message.into(),
        }
    }
}

impl<E> From<E> for AppError
where
    E: Into<anyhow::Error>,
{
    fn from(error: E) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            error: error.into(),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({
                "error": self.error.to_string(),
            })),
        )
            .into_response()
    }
}
