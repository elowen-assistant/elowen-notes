use anyhow::{Context, anyhow, bail};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use chrono::{DateTime, Utc};
use reqwest::{Client, Method, StatusCode as ReqwestStatusCode};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{env, net::SocketAddr, time::Duration};
use tracing::{info, warn};
use ulid::Ulid;

#[derive(Clone, Debug)]
struct ArangoConfig {
    base_url: String,
    database: String,
    username: String,
    password: String,
}

#[derive(Clone)]
struct AppState {
    client: Client,
    arango: ArangoConfig,
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

#[derive(Debug, Serialize, Deserialize, Clone)]
struct NoteSummary {
    note_id: String,
    title: String,
    slug: String,
    summary: String,
    tags: Vec<String>,
    aliases: Vec<String>,
    note_type: String,
    source_kind: Option<String>,
    source_id: Option<String>,
    current_revision_id: String,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct NoteRevision {
    revision_id: String,
    note_id: String,
    version: i32,
    summary: String,
    body_markdown: String,
    frontmatter: Value,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct NoteDetail {
    note: NoteSummary,
    revision: NoteRevision,
}

#[derive(Debug, Deserialize)]
struct SearchNotesQuery {
    q: Option<String>,
    limit: Option<usize>,
    source_kind: Option<String>,
    source_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PromoteNoteRequest {
    source_kind: Option<String>,
    source_id: Option<String>,
    title: Option<String>,
    slug: Option<String>,
    summary: Option<String>,
    body_markdown: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    aliases: Vec<String>,
    note_type: Option<String>,
    frontmatter: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct CursorResponse<T> {
    result: Vec<T>,
}

#[derive(Debug)]
struct AppError {
    status: StatusCode,
    error: anyhow::Error,
}

impl AppError {
    fn bad_request(message: impl Into<anyhow::Error>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            error: message.into(),
        }
    }

    fn not_found(message: impl Into<anyhow::Error>) -> Self {
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

    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/api/v1/notes/search", get(search_notes))
        .route("/api/v1/notes/promotions", post(promote_note))
        .route("/api/v1/notes/{note_id}", get(get_note))
        .with_state(AppState { client, arango });

    info!(%address, "starting elowen-notes");

    let listener = tokio::net::TcpListener::bind(address).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn search_notes(
    State(state): State<AppState>,
    Query(query): Query<SearchNotesQuery>,
) -> Result<Json<Vec<NoteSummary>>, AppError> {
    let search_text = query.q.unwrap_or_default().trim().to_string();
    let source_kind = sanitize_optional_string(query.source_kind);
    let source_id = sanitize_optional_string(query.source_id);
    let limit = query.limit.unwrap_or(8).clamp(1, 20) as i64;

    let notes = run_aql::<NoteSummary>(
        &state.client,
        &state.arango,
        r#"
        FOR note IN notes
            LET revision = note.current_revision_key == null
                ? null
                : DOCUMENT(CONCAT("note_revisions/", note.current_revision_key))
            FILTER (@source_kind == null OR note.source_kind == @source_kind)
                AND (@source_id == null OR note.source_id == @source_id)
                AND (
                    @query == ""
                    OR CONTAINS(LOWER(note.title), LOWER(@query))
                    OR CONTAINS(LOWER(note.slug), LOWER(@query))
                    OR (revision != null AND revision.summary != null AND CONTAINS(LOWER(revision.summary), LOWER(@query)))
                    OR (revision != null AND revision.body_markdown != null AND CONTAINS(LOWER(revision.body_markdown), LOWER(@query)))
                )
            SORT note.updated_at DESC
            LIMIT @limit
            RETURN {
                note_id: note.note_id,
                title: note.title,
                slug: note.slug,
                summary: revision != null && revision.summary != null ? revision.summary : "",
                tags: note.tags,
                aliases: note.aliases,
                note_type: note.note_type,
                source_kind: note.source_kind,
                source_id: note.source_id,
                current_revision_id: note.current_revision_id,
                updated_at: note.updated_at
            }
        "#,
        json!({
            "query": search_text,
            "limit": limit,
            "source_kind": source_kind,
            "source_id": source_id,
        }),
    )
    .await?;

    Ok(Json(notes))
}

async fn get_note(
    State(state): State<AppState>,
    Path(note_id): Path<String>,
) -> Result<Json<NoteDetail>, AppError> {
    let mut results = run_aql::<NoteDetail>(
        &state.client,
        &state.arango,
        r#"
        LET note = DOCUMENT(CONCAT("notes/", @note_id))
        FILTER note != null
        LET revision = DOCUMENT(CONCAT("note_revisions/", note.current_revision_key))
        RETURN {
            note: {
                note_id: note.note_id,
                title: note.title,
                slug: note.slug,
                summary: revision != null && revision.summary != null ? revision.summary : "",
                tags: note.tags,
                aliases: note.aliases,
                note_type: note.note_type,
                source_kind: note.source_kind,
                source_id: note.source_id,
                current_revision_id: note.current_revision_id,
                updated_at: note.updated_at
            },
            revision: {
                revision_id: revision.revision_id,
                note_id: revision.note_id,
                version: revision.version,
                summary: revision.summary,
                body_markdown: revision.body_markdown,
                frontmatter: revision.frontmatter,
                created_at: revision.created_at
            }
        }
        "#,
        json!({ "note_id": note_id }),
    )
    .await?;

    results
        .pop()
        .map(Json)
        .ok_or_else(|| AppError::not_found(anyhow!("note not found")))
}

async fn promote_note(
    State(state): State<AppState>,
    Json(request): Json<PromoteNoteRequest>,
) -> Result<(StatusCode, Json<NoteDetail>), AppError> {
    let body_markdown = request.body_markdown.trim().to_string();
    if body_markdown.is_empty() {
        return Err(AppError::bad_request(anyhow!(
            "body_markdown is required for note promotion"
        )));
    }

    let note_id = Ulid::new().to_string();
    let revision_id = Ulid::new().to_string();
    let created_at = Utc::now();
    let source_kind = sanitize_optional_string(request.source_kind);
    let source_id = sanitize_optional_string(request.source_id);
    let title = sanitize_optional_string(request.title)
        .or_else(|| derive_title(&body_markdown))
        .unwrap_or_else(|| format!("Promoted Note {}", &note_id[..8].to_ascii_lowercase()));
    let slug_base = sanitize_optional_string(request.slug)
        .unwrap_or_else(|| slugify(&title))
        .trim_matches('-')
        .to_string();
    let slug = if slug_base.is_empty() {
        note_id.to_ascii_lowercase()
    } else {
        format!("{slug_base}-{}", &note_id[..8].to_ascii_lowercase())
    };
    let summary =
        sanitize_optional_string(request.summary).unwrap_or_else(|| derive_summary(&body_markdown));
    let note_type =
        sanitize_optional_string(request.note_type).unwrap_or_else(|| "general".to_string());
    let frontmatter = request.frontmatter.unwrap_or_else(|| json!({}));
    let tags = request
        .tags
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    let aliases = request
        .aliases
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();

    ensure_note_type(&state.client, &state.arango, &note_type).await?;

    insert_document(
        &state.client,
        &state.arango,
        "note_revisions",
        json!({
            "_key": revision_id,
            "revision_id": revision_id,
            "note_id": note_id,
            "version": 1,
            "summary": summary,
            "body_markdown": body_markdown,
            "frontmatter": frontmatter,
            "created_at": created_at,
        }),
    )
    .await?;

    insert_document(
        &state.client,
        &state.arango,
        "notes",
        json!({
            "_key": note_id,
            "note_id": note_id,
            "title": title,
            "slug": slug,
            "tags": tags,
            "aliases": aliases,
            "note_type": note_type,
            "source_kind": source_kind,
            "source_id": source_id,
            "current_revision_id": revision_id,
            "current_revision_key": revision_id,
            "created_at": created_at,
            "updated_at": created_at,
        }),
    )
    .await?;

    let detail = get_note(State(state), Path(note_id)).await?.0;
    Ok((StatusCode::CREATED, Json(detail)))
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

async fn ensure_note_type(
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

async fn insert_document(
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

async fn run_aql<T>(
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

async fn send_json(
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

fn sanitize_optional_string(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn derive_title(body_markdown: &str) -> Option<String> {
    body_markdown
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| line.trim_start_matches('#').trim().to_string())
}

fn derive_summary(body_markdown: &str) -> String {
    let normalized = body_markdown
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if normalized.len() > 280 {
        format!("{}...", &normalized[..277])
    } else {
        normalized
    }
}

fn slugify(value: &str) -> String {
    let mut slug = String::new();
    let mut last_was_dash = false;

    for ch in value.chars() {
        let lowered = ch.to_ascii_lowercase();
        if lowered.is_ascii_alphanumeric() {
            slug.push(lowered);
            last_was_dash = false;
        } else if !last_was_dash {
            slug.push('-');
            last_was_dash = true;
        }
    }

    slug.trim_matches('-').to_string()
}
