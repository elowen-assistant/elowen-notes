//! Low-level ArangoDB HTTP helpers.

use crate::{config::ArangoConfig, error::AppError};
use anyhow::{Context, bail};
use reqwest::{Client, Method, StatusCode as ReqwestStatusCode};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::time::Duration;

#[derive(Debug, serde::Deserialize)]
struct CursorResponse<T> {
    result: Vec<T>,
}

/// Executes an AQL query and deserializes the returned result documents.
pub(crate) async fn run_aql<T>(
    client: &Client,
    config: &ArangoConfig,
    query: &str,
    bind_vars: Value,
) -> Result<Vec<T>, AppError>
where
    T: DeserializeOwned,
{
    let url = format!("{}/_db/{}/_api/cursor", config.base_url, config.database);
    let response = send_json(
        client,
        Method::POST,
        &url,
        config,
        Some(json!({
            "query": query,
            "bindVars": bind_vars,
        })),
        &[
            ReqwestStatusCode::OK,
            ReqwestStatusCode::CREATED,
            ReqwestStatusCode::ACCEPTED,
        ],
    )
    .await?;

    let cursor = response
        .json::<CursorResponse<T>>()
        .await
        .context("failed to decode Arango cursor result")?;

    Ok(cursor.result)
}

/// Inserts a document into an Arango collection.
pub(crate) async fn insert_document(
    client: &Client,
    config: &ArangoConfig,
    collection: &str,
    body: Value,
) -> anyhow::Result<()> {
    let url = format!(
        "{}/_db/{}/_api/document/{}",
        config.base_url, config.database, collection
    );
    send_json(
        client,
        Method::POST,
        &url,
        config,
        Some(body),
        &[ReqwestStatusCode::CREATED, ReqwestStatusCode::ACCEPTED],
    )
    .await
    .with_context(|| format!("failed to insert document into {}", collection))?;
    Ok(())
}

/// Sends a JSON request to ArangoDB and validates the response status.
pub(crate) async fn send_json(
    client: &Client,
    method: Method,
    url: &str,
    config: &ArangoConfig,
    body: Option<Value>,
    accepted_statuses: &[ReqwestStatusCode],
) -> anyhow::Result<reqwest::Response> {
    let mut request = client
        .request(method, url)
        .basic_auth(&config.username, Some(&config.password))
        .timeout(Duration::from_secs(15));

    if let Some(body) = body {
        request = request.json(&body);
    }

    let response = request
        .send()
        .await
        .with_context(|| format!("request to {} failed", url))?;
    let status = response.status();

    if accepted_statuses.contains(&status) {
        return Ok(response);
    }

    let body = response.text().await.unwrap_or_default();
    bail!("request to {} returned {}: {}", url, status, body)
}
