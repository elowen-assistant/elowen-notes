//! Elowen notes service.
//!
//! This crate owns note promotion, retrieval, and ArangoDB bootstrap for the
//! notes subsystem. The binary entrypoint is intentionally thin; most behavior
//! lives in focused modules so it is easier to test and maintain.

mod arangodb;
mod config;
mod error;
mod models;
mod normalize;
mod routes;
mod service;
mod state;
mod tracing;

use anyhow::Context;
use axum::Router;
use reqwest::Client;
use std::{env, net::SocketAddr};

pub use error::AppError;

/// Starts the notes HTTP service.
pub async fn run() -> anyhow::Result<()> {
    tracing::init_tracing("elowen-notes");

    let arango = config::ArangoConfig::from_env()?;
    let client = Client::builder().build()?;

    arangodb::bootstrap::bootstrap_arangodb(&client, &arango).await?;

    let port = env::var("PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(8080);
    let address = SocketAddr::from(([0, 0, 0, 0], port));

    let app = Router::new().merge(routes::router(state::AppState { client, arango }));

    ::tracing::info!(%address, "starting elowen-notes");

    let listener = tokio::net::TcpListener::bind(address)
        .await
        .context("failed to bind notes listener")?;
    axum::serve(listener, app).await?;
    Ok(())
}
