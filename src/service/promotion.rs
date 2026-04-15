//! Helpers for promoting markdown into canonical notes and revisions.

use crate::{
    AppError,
    arangodb::client::run_aql,
    models::{ExistingNoteHead, NoteAuthor, NoteSourceReference},
    state::AppState,
};
use anyhow::anyhow;
use chrono::Utc;
use serde_json::{Value, json};

pub(super) struct RevisionDocument<'a> {
    pub(super) revision_id: &'a str,
    pub(super) note_id: &'a str,
    pub(super) version: i32,
    pub(super) summary: &'a str,
    pub(super) body_markdown: &'a str,
    pub(super) frontmatter: Value,
    pub(super) created_at: chrono::DateTime<Utc>,
    pub(super) previous_revision_id: Option<String>,
    pub(super) authored_by: Option<NoteAuthor>,
    pub(super) source_references: Vec<NoteSourceReference>,
}

pub(super) fn revision_document(value: RevisionDocument<'_>) -> Value {
    json!({
        "_key": value.revision_id,
        "revision_id": value.revision_id,
        "note_id": value.note_id,
        "version": value.version,
        "summary": value.summary,
        "body_markdown": value.body_markdown,
        "frontmatter": value.frontmatter,
        "created_at": value.created_at,
        "previous_revision_id": value.previous_revision_id,
        "authored_by": value.authored_by,
        "source_references": value.source_references,
    })
}

pub(super) fn default_author() -> Option<NoteAuthor> {
    Some(NoteAuthor {
        actor_type: "system".to_string(),
        actor_id: "elowen-notes".to_string(),
        display_name: Some("Elowen Notes".to_string()),
    })
}

pub(super) async fn load_note_head(
    state: &AppState,
    note_id: &str,
) -> Result<ExistingNoteHead, AppError> {
    let mut results = run_aql::<ExistingNoteHead>(
        &state.client,
        &state.arango,
        r#"
        LET note = DOCUMENT(CONCAT("notes/", @note_id))
        FILTER note != null
        LET revision = DOCUMENT(CONCAT("note_revisions/", note.current_revision_key))
        RETURN {
            note_id: note.note_id,
            title: note.title,
            slug: note.slug,
            tags: note.tags,
            aliases: note.aliases,
            note_type: note.note_type,
            source_kind: note.source_kind,
            source_id: note.source_id,
            current_revision_id: note.current_revision_id,
            current_version: revision != null ? revision.version : 0
        }
        "#,
        json!({ "note_id": note_id }),
    )
    .await?;

    results
        .pop()
        .ok_or_else(|| AppError::not_found(anyhow!("note not found")))
}

pub(super) async fn update_note_head(
    state: &AppState,
    note_id: &str,
    patch: Value,
) -> Result<(), AppError> {
    run_aql::<Value>(
        &state.client,
        &state.arango,
        r#"
        UPDATE { _key: @note_id } WITH @patch IN notes
        RETURN NEW
        "#,
        json!({
            "note_id": note_id,
            "patch": patch,
        }),
    )
    .await?;
    Ok(())
}
