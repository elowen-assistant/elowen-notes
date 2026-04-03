//! Shared process state for request handlers and service functions.

use crate::config::ArangoConfig;
use reqwest::Client;

/// Shared state injected into Axum handlers.
#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) client: Client,
    pub(crate) arango: ArangoConfig,
}
