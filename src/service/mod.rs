//! Core note retrieval and promotion behavior.

mod promotion;

use crate::{
    AppError,
    arangodb::{
        bootstrap::ensure_note_type,
        client::{insert_document, run_aql},
    },
    models::{NoteDetail, NoteSummary, PromoteNoteRequest, SearchNotesQuery},
    normalize::{
        derive_summary, derive_title, normalize_note_author, normalize_optional_list,
        normalize_source_references, sanitize_optional_string, slugify,
    },
    state::AppState,
};
use anyhow::anyhow;
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use chrono::Utc;
use serde_json::json;
use ulid::Ulid;

use self::promotion::{
    RevisionDocument, default_author, load_note_head, revision_document, update_note_head,
};

/// Searches notes using the current query filters.
pub(crate) async fn search_notes(
    State(state): State<AppState>,
    Query(query): Query<SearchNotesQuery>,
) -> Result<Json<Vec<NoteSummary>>, AppError> {
    let search_text = query.q.unwrap_or_default().trim().to_string();
    let context_text = query.context.unwrap_or_default().trim().to_string();
    let source_kind = sanitize_optional_string(query.source_kind);
    let source_id = sanitize_optional_string(query.source_id);
    let prefer_note_ids = parse_csv_list(query.prefer_note_ids);
    let prefer_source_kind = sanitize_optional_string(query.prefer_source_kind);
    let prefer_source_id = sanitize_optional_string(query.prefer_source_id);
    let limit = query.limit.unwrap_or(8).clamp(1, 20) as i64;

    let notes = run_aql::<NoteSummary>(
        &state.client,
        &state.arango,
        r#"
        FOR note IN notes
            LET revision = note.current_revision_key == null
                ? null
                : DOCUMENT(CONCAT("note_revisions/", note.current_revision_key))
            LET title_text = LOWER(note.title)
            LET slug_text = LOWER(note.slug)
            LET summary_text = revision != null && revision.summary != null ? LOWER(revision.summary) : ""
            LET body_text = revision != null && revision.body_markdown != null ? LOWER(revision.body_markdown) : ""
            LET query_text = LOWER(@query)
            LET context_query = LOWER(@context)
            LET query_title_match = @query != "" && CONTAINS(title_text, query_text)
            LET query_slug_match = @query != "" && CONTAINS(slug_text, query_text)
            LET query_summary_match = @query != "" && CONTAINS(summary_text, query_text)
            LET query_body_match = @query != "" && CONTAINS(body_text, query_text)
            LET context_title_match = @context != "" && CONTAINS(title_text, context_query)
            LET context_summary_match = @context != "" && CONTAINS(summary_text, context_query)
            LET context_body_match = @context != "" && CONTAINS(body_text, context_query)
            LET preferred_note_match = LENGTH(@prefer_note_ids) > 0 && POSITION(@prefer_note_ids, note.note_id)
            LET preferred_source_match = @prefer_source_kind != null
                && note.source_kind == @prefer_source_kind
                && (@prefer_source_id == null OR note.source_id == @prefer_source_id)
            LET query_score =
                (query_title_match ? 140 : 0)
                + (query_slug_match ? 120 : 0)
                + (query_summary_match ? 80 : 0)
                + (query_body_match ? 35 : 0)
            LET context_score =
                (context_title_match ? 45 : 0)
                + (context_summary_match ? 30 : 0)
                + (context_body_match ? 15 : 0)
            LET preference_score =
                (preferred_note_match ? 220 : 0)
                + (preferred_source_match ? 180 : 0)
            LET relevance_score = query_score + context_score + preference_score
            FILTER (@source_kind == null OR note.source_kind == @source_kind)
                AND (@source_id == null OR note.source_id == @source_id)
                AND (
                    (@query == "" AND @context == "" AND LENGTH(@prefer_note_ids) == 0 AND @prefer_source_kind == null)
                    OR relevance_score > 0
                )
            SORT relevance_score DESC, note.updated_at DESC
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
                updated_at: note.updated_at,
                relevance_score: relevance_score,
                match_reasons: APPEND(
                    preferred_source_match ? ["preferred_source"] : [],
                    APPEND(
                        preferred_note_match ? ["preferred_note"] : [],
                        APPEND(
                            query_title_match ? ["query_title"] : [],
                            APPEND(
                                query_slug_match ? ["query_slug"] : [],
                                APPEND(
                                    query_summary_match ? ["query_summary"] : [],
                                    APPEND(
                                        query_body_match ? ["query_body"] : [],
                                        APPEND(
                                            context_title_match ? ["context_title"] : [],
                                            APPEND(
                                                context_summary_match ? ["context_summary"] : [],
                                                context_body_match ? ["context_body"] : []
                                            )
                                        )
                                    )
                                )
                            )
                        )
                    )
                )
            }
        "#,
        json!({
            "query": search_text,
            "context": context_text,
            "limit": limit,
            "source_kind": source_kind,
            "source_id": source_id,
            "prefer_note_ids": prefer_note_ids,
            "prefer_source_kind": prefer_source_kind,
            "prefer_source_id": prefer_source_id,
        }),
    )
    .await?;

    Ok(Json(notes))
}

/// Resolves a note and its current revision by note id.
pub(crate) async fn get_note(
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
                created_at: revision.created_at,
                previous_revision_id: revision.previous_revision_id,
                authored_by: revision.authored_by,
                source_references: revision.source_references != null ? revision.source_references : []
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

/// Promotes markdown into a note or creates a new note revision.
pub(crate) async fn promote_note(
    State(state): State<AppState>,
    Json(request): Json<PromoteNoteRequest>,
) -> Result<(StatusCode, Json<NoteDetail>), AppError> {
    let body_markdown = request.body_markdown.trim().to_string();
    if body_markdown.is_empty() {
        return Err(AppError::bad_request(anyhow!(
            "body_markdown is required for note promotion"
        )));
    }

    let existing_note = match sanitize_optional_string(request.note_id.clone()) {
        Some(note_id) => Some(load_note_head(&state, &note_id).await?),
        None => None,
    };
    let note_id = existing_note
        .as_ref()
        .map(|note| note.note_id.clone())
        .unwrap_or_else(|| Ulid::new().to_string());
    let revision_id = Ulid::new().to_string();
    let created_at = Utc::now();
    let source_kind = sanitize_optional_string(request.source_kind).or_else(|| {
        existing_note
            .as_ref()
            .and_then(|note| note.source_kind.clone())
    });
    let source_id = sanitize_optional_string(request.source_id).or_else(|| {
        existing_note
            .as_ref()
            .and_then(|note| note.source_id.clone())
    });
    let title = sanitize_optional_string(request.title)
        .or_else(|| existing_note.as_ref().map(|note| note.title.clone()))
        .or_else(|| derive_title(&body_markdown))
        .unwrap_or_else(|| format!("Promoted Note {}", &note_id[..8].to_ascii_lowercase()));
    let slug = if let Some(existing_note) = existing_note.as_ref() {
        sanitize_optional_string(request.slug).unwrap_or_else(|| existing_note.slug.clone())
    } else {
        let slug_base = sanitize_optional_string(request.slug)
            .unwrap_or_else(|| slugify(&title))
            .trim_matches('-')
            .to_string();
        if slug_base.is_empty() {
            note_id.to_ascii_lowercase()
        } else {
            format!("{slug_base}-{}", &note_id[..8].to_ascii_lowercase())
        }
    };
    let summary =
        sanitize_optional_string(request.summary).unwrap_or_else(|| derive_summary(&body_markdown));
    let note_type = sanitize_optional_string(request.note_type)
        .or_else(|| existing_note.as_ref().map(|note| note.note_type.clone()))
        .unwrap_or_else(|| "general".to_string());
    let frontmatter = request.frontmatter.unwrap_or_else(|| json!({}));
    let tags = normalize_optional_list(request.tags)
        .or_else(|| existing_note.as_ref().map(|note| note.tags.clone()))
        .unwrap_or_default();
    let aliases = normalize_optional_list(request.aliases)
        .or_else(|| existing_note.as_ref().map(|note| note.aliases.clone()))
        .unwrap_or_default();
    let previous_revision_id = existing_note
        .as_ref()
        .map(|note| note.current_revision_id.clone());
    let version = existing_note
        .as_ref()
        .map(|note| note.current_version + 1)
        .unwrap_or(1);
    let authored_by = normalize_note_author(request.authored_by).or_else(default_author);
    let source_references = normalize_source_references(
        request.source_references,
        source_kind.as_deref(),
        source_id.as_deref(),
    );

    ensure_note_type(&state.client, &state.arango, &note_type).await?;

    insert_document(
        &state.client,
        &state.arango,
        "note_revisions",
        revision_document(RevisionDocument {
            revision_id: &revision_id,
            note_id: &note_id,
            version,
            summary: &summary,
            body_markdown: &body_markdown,
            frontmatter,
            created_at,
            previous_revision_id,
            authored_by,
            source_references,
        }),
    )
    .await?;

    if existing_note.is_some() {
        update_note_head(
            &state,
            &note_id,
            json!({
                "title": title,
                "slug": slug,
                "tags": tags,
                "aliases": aliases,
                "note_type": note_type,
                "source_kind": source_kind,
                "source_id": source_id,
                "current_revision_id": revision_id,
                "current_revision_key": revision_id,
                "updated_at": created_at,
            }),
        )
        .await?;
    } else {
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
    }

    let detail = get_note(State(state), Path(note_id)).await?.0;
    Ok((StatusCode::CREATED, Json(detail)))
}

fn parse_csv_list(value: Option<String>) -> Vec<String> {
    value
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revision_document_preserves_revision_ids() {
        let value = revision_document(RevisionDocument {
            revision_id: "rev-1",
            note_id: "note-1",
            version: 2,
            summary: "summary",
            body_markdown: "body",
            frontmatter: json!({}),
            created_at: Utc::now(),
            previous_revision_id: Some("rev-0".into()),
            authored_by: default_author(),
            source_references: vec![],
        });

        assert_eq!(value["revision_id"], "rev-1");
        assert_eq!(value["previous_revision_id"], "rev-0");
        assert_eq!(value["version"], 2);
    }

    #[test]
    fn parse_csv_list_ignores_empty_entries() {
        assert_eq!(
            parse_csv_list(Some(" note-1, ,note-2 ,, note-3 ".to_string())),
            vec!["note-1", "note-2", "note-3"]
        );
    }
}
