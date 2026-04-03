//! Bootstrap logic that ensures the ArangoDB database and collections exist.

use crate::config::{
    ArangoConfig, COLLECTIONS, CollectionSpec, notes_search_properties, persistent_index,
};
use anyhow::{Context, bail};
use chrono::Utc;
use reqwest::{Client, Method, StatusCode as ReqwestStatusCode};
use serde_json::{Value, json};
use std::time::Duration;
use tracing::{info, warn};

use super::client::send_json;
use crate::normalize::slugify;

/// Ensures the configured ArangoDB database and indexes exist.
pub(crate) async fn bootstrap_arangodb(
    client: &Client,
    config: &ArangoConfig,
) -> anyhow::Result<()> {
    wait_for_arangodb(client, config).await?;
    ensure_database(client, config).await?;

    for collection in COLLECTIONS {
        ensure_collection(client, config, *collection).await?;
    }

    ensure_index(
        client,
        config,
        "notes",
        persistent_index(&["note_id"], "idx_notes_note_id", true, false),
    )
    .await?;
    ensure_index(
        client,
        config,
        "notes",
        persistent_index(&["slug"], "idx_notes_slug", true, true),
    )
    .await?;
    ensure_index(
        client,
        config,
        "notes",
        persistent_index(
            &["source_kind", "source_id"],
            "idx_notes_source_lookup",
            false,
            true,
        ),
    )
    .await?;
    ensure_index(
        client,
        config,
        "note_revisions",
        persistent_index(&["revision_id"], "idx_revisions_revision_id", true, false),
    )
    .await?;
    ensure_index(
        client,
        config,
        "note_revisions",
        persistent_index(
            &["note_id", "created_at"],
            "idx_revisions_note_created",
            false,
            false,
        ),
    )
    .await?;
    ensure_index(
        client,
        config,
        "note_revisions",
        persistent_index(
            &["note_id", "version"],
            "idx_revisions_note_version",
            true,
            false,
        ),
    )
    .await?;
    ensure_index(
        client,
        config,
        "note_revisions",
        persistent_index(
            &["previous_revision_id"],
            "idx_revisions_previous_revision",
            false,
            true,
        ),
    )
    .await?;
    ensure_index(
        client,
        config,
        "note_types",
        persistent_index(&["type_key"], "idx_note_types_type_key", true, false),
    )
    .await?;
    ensure_index(
        client,
        config,
        "attachments",
        persistent_index(
            &["attachment_id"],
            "idx_attachments_attachment_id",
            true,
            false,
        ),
    )
    .await?;

    ensure_search_view(client, config).await?;

    info!(database = %config.database, "ArangoDB bootstrap complete");
    Ok(())
}

/// Ensures a note type row exists for the provided note type.
pub(crate) async fn ensure_note_type(
    client: &Client,
    config: &ArangoConfig,
    note_type: &str,
) -> anyhow::Result<()> {
    let key = slugify(note_type);
    let url = format!(
        "{}/_db/{}/_api/document/{}",
        config.base_url, config.database, "note_types"
    );
    send_json(
        client,
        Method::POST,
        &url,
        config,
        Some(json!({
            "_key": key,
            "type_key": note_type,
            "created_at": Utc::now(),
        })),
        &[
            ReqwestStatusCode::CREATED,
            ReqwestStatusCode::ACCEPTED,
            ReqwestStatusCode::CONFLICT,
        ],
    )
    .await
    .context("failed to ensure note type")?;
    Ok(())
}

async fn wait_for_arangodb(client: &Client, config: &ArangoConfig) -> anyhow::Result<()> {
    let url = format!("{}/_api/version", config.base_url);

    for attempt in 1..=30 {
        match client
            .get(&url)
            .basic_auth(&config.username, Some(&config.password))
            .send()
            .await
        {
            Ok(response) if response.status().is_success() => {
                info!(attempt, "ArangoDB is reachable");
                return Ok(());
            }
            Ok(response) => {
                warn!(attempt, status = %response.status(), "ArangoDB not ready yet");
            }
            Err(error) => {
                warn!(attempt, error = %error, "waiting for ArangoDB");
            }
        }

        tokio::time::sleep(Duration::from_secs(2)).await;
    }

    bail!("ArangoDB did not become ready in time")
}

async fn ensure_database(client: &Client, config: &ArangoConfig) -> anyhow::Result<()> {
    let url = format!("{}/_db/_system/_api/database", config.base_url);
    let body = json!({ "name": config.database });

    send_json(
        client,
        Method::POST,
        &url,
        config,
        Some(body),
        &[
            ReqwestStatusCode::CREATED,
            ReqwestStatusCode::ACCEPTED,
            ReqwestStatusCode::CONFLICT,
        ],
    )
    .await
    .context("failed to ensure ArangoDB database")?;

    Ok(())
}

async fn ensure_collection(
    client: &Client,
    config: &ArangoConfig,
    collection: CollectionSpec,
) -> anyhow::Result<()> {
    let url = format!(
        "{}/_db/{}/_api/collection",
        config.base_url, config.database
    );
    let body = json!({
        "name": collection.name,
        "type": collection.collection_type
    });

    send_json(
        client,
        Method::POST,
        &url,
        config,
        Some(body),
        &[
            ReqwestStatusCode::OK,
            ReqwestStatusCode::CREATED,
            ReqwestStatusCode::ACCEPTED,
            ReqwestStatusCode::CONFLICT,
        ],
    )
    .await
    .with_context(|| format!("failed to ensure collection {}", collection.name))?;

    Ok(())
}

async fn ensure_index(
    client: &Client,
    config: &ArangoConfig,
    collection: &str,
    body: Value,
) -> anyhow::Result<()> {
    let url = format!(
        "{}/_db/{}/_api/index?collection={}",
        config.base_url, config.database, collection
    );

    send_json(
        client,
        Method::POST,
        &url,
        config,
        Some(body),
        &[
            ReqwestStatusCode::OK,
            ReqwestStatusCode::CREATED,
            ReqwestStatusCode::ACCEPTED,
            ReqwestStatusCode::CONFLICT,
        ],
    )
    .await
    .with_context(|| format!("failed to ensure index for collection {}", collection))?;

    Ok(())
}

async fn ensure_search_view(client: &Client, config: &ArangoConfig) -> anyhow::Result<()> {
    let create_url = format!("{}/_db/{}/_api/view", config.base_url, config.database);
    let properties_url = format!(
        "{}/_db/{}/_api/view/{}/properties",
        config.base_url, config.database, "notes_search"
    );

    send_json(
        client,
        Method::POST,
        &create_url,
        config,
        Some(json!({
            "name": "notes_search",
            "type": "arangosearch"
        })),
        &[
            ReqwestStatusCode::OK,
            ReqwestStatusCode::CREATED,
            ReqwestStatusCode::ACCEPTED,
            ReqwestStatusCode::CONFLICT,
        ],
    )
    .await
    .context("failed to ensure notes_search view")?;

    send_json(
        client,
        Method::PATCH,
        &properties_url,
        config,
        Some(notes_search_properties()),
        &[
            ReqwestStatusCode::OK,
            ReqwestStatusCode::CREATED,
            ReqwestStatusCode::ACCEPTED,
        ],
    )
    .await
    .context("failed to configure notes_search view")?;

    Ok(())
}
