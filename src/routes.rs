//! HTTP route assembly for the notes service.

use axum::{
    Router,
    routing::{get, post},
};

use crate::{service, state::AppState};

/// Builds the notes service router.
pub(crate) fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/api/v1/notes/search", get(service::search_notes))
        .route("/api/v1/notes/promotions", post(service::promote_note))
        .route("/api/v1/notes/{note_id}", get(service::get_note))
        .with_state(state)
}
