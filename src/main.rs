use anyhow::{Context, anyhow, bail};
use axum::{Router, routing::get};
use reqwest::{Client, Method, StatusCode};
use serde_json::{Value, json};
use std::{env, net::SocketAddr, time::Duration};
use tracing::{info, warn};

#[derive(Clone, Debug)]
struct ArangoConfig {
    base_url: String,
    database: String,
    username: String,
    password: String,
}

impl ArangoConfig {
    fn from_env() -> anyhow::Result<Self> {
        Ok(Self {
            base_url: env::var("ELOWEN_ARANGODB_URL")
                .context("missing ELOWEN_ARANGODB_URL")?
                .trim_end_matches('/')
                .to_owned(),
            database: env::var("ELOWEN_ARANGODB_DATABASE")
                .context("missing ELOWEN_ARANGODB_DATABASE")?,
            username: env::var("ELOWEN_ARANGODB_USERNAME")
                .context("missing ELOWEN_ARANGODB_USERNAME")?,
            password: env::var("ELOWEN_ARANGODB_PASSWORD")
                .context("missing ELOWEN_ARANGODB_PASSWORD")?,
        })
    }
}

#[derive(Clone, Copy)]
struct CollectionSpec {
    name: &'static str,
    collection_type: u8,
}

const COLLECTIONS: &[CollectionSpec] = &[
    CollectionSpec {
        name: "notes",
        collection_type: 2,
    },
    CollectionSpec {
        name: "note_revisions",
        collection_type: 2,
    },
    CollectionSpec {
        name: "note_types",
        collection_type: 2,
    },
    CollectionSpec {
        name: "attachments",
        collection_type: 2,
    },
    CollectionSpec {
        name: "note_links",
        collection_type: 3,
    },
    CollectionSpec {
        name: "note_sources",
        collection_type: 3,
    },
];

fn persistent_index(fields: &[&str], name: &'static str, unique: bool, sparse: bool) -> Value {
    json!({
        "type": "persistent",
        "fields": fields,
        "name": name,
        "unique": unique,
        "sparse": sparse
    })
}

fn notes_search_properties() -> Value {
    json!({
        "links": {
            "notes": {
                "includeAllFields": false,
                "fields": {
                    "title": {},
                    "slug": {},
                    "tags": {},
                    "aliases": {}
                }
            },
            "note_revisions": {
                "includeAllFields": false,
                "fields": {
                    "body_markdown": { "analyzers": ["text_en"] },
                    "summary": { "analyzers": ["text_en"] },
                    "frontmatter": {}
                }
            }
        }
    })
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let arango = ArangoConfig::from_env()?;
    let client = Client::builder().build()?;

    bootstrap_arangodb(&client, &arango).await?;

    let port = env::var("PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(8080);
    let address = SocketAddr::from(([0, 0, 0, 0], port));

    let app = Router::new().route("/health", get(|| async { "ok" }));

    info!(%address, "starting elowen-notes");

    let listener = tokio::net::TcpListener::bind(address).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn bootstrap_arangodb(client: &Client, config: &ArangoConfig) -> anyhow::Result<()> {
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
            StatusCode::CREATED,
            StatusCode::ACCEPTED,
            StatusCode::CONFLICT,
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
            StatusCode::OK,
            StatusCode::CREATED,
            StatusCode::ACCEPTED,
            StatusCode::CONFLICT,
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
            StatusCode::OK,
            StatusCode::CREATED,
            StatusCode::ACCEPTED,
            StatusCode::CONFLICT,
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
            StatusCode::OK,
            StatusCode::CREATED,
            StatusCode::ACCEPTED,
            StatusCode::CONFLICT,
        ],
    )
    .await
    .context("failed to ensure notes_search view")?;

    send_json(
        client,
        Method::PUT,
        &properties_url,
        config,
        Some(notes_search_properties()),
        &[StatusCode::OK, StatusCode::ACCEPTED],
    )
    .await
    .context("failed to configure notes_search view")?;

    Ok(())
}

async fn send_json(
    client: &Client,
    method: Method,
    url: &str,
    config: &ArangoConfig,
    body: Option<Value>,
    accepted_statuses: &[StatusCode],
) -> anyhow::Result<()> {
    let mut request = client
        .request(method, url)
        .basic_auth(&config.username, Some(&config.password));

    if let Some(body) = body {
        request = request.json(&body);
    }

    let response = request.send().await?;
    let status = response.status();

    if accepted_statuses.contains(&status) {
        return Ok(());
    }

    let payload = response.text().await.unwrap_or_default();
    Err(anyhow!(
        "unexpected status {} from {}: {}",
        status,
        url,
        payload
    ))
}
