//! Request and response models for the notes service.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Summary view returned by note searches.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NoteSummary {
    pub note_id: String,
    pub title: String,
    pub slug: String,
    pub summary: String,
    pub tags: Vec<String>,
    pub aliases: Vec<String>,
    pub note_type: String,
    pub source_kind: Option<String>,
    pub source_id: Option<String>,
    pub current_revision_id: String,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub relevance_score: f64,
    #[serde(default)]
    pub match_reasons: Vec<String>,
}

/// Stored note revision payload.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NoteRevision {
    pub revision_id: String,
    pub note_id: String,
    pub version: i32,
    pub summary: String,
    pub body_markdown: String,
    pub frontmatter: Value,
    pub created_at: DateTime<Utc>,
    pub previous_revision_id: Option<String>,
    pub authored_by: Option<NoteAuthor>,
    pub source_references: Vec<NoteSourceReference>,
}

/// Fully resolved note payload returned by `GET /api/v1/notes/{note_id}`.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NoteDetail {
    pub note: NoteSummary,
    pub revision: NoteRevision,
}

/// Query parameters for note search.
#[derive(Debug, Deserialize)]
pub struct SearchNotesQuery {
    pub q: Option<String>,
    /// Additional free-text context used for ranking without replacing `q`.
    pub context: Option<String>,
    pub limit: Option<usize>,
    pub source_kind: Option<String>,
    pub source_id: Option<String>,
    /// Comma-separated list of note ids to boost in ranking.
    pub prefer_note_ids: Option<String>,
    /// Preferred source kind to boost in ranking without filtering.
    pub prefer_source_kind: Option<String>,
    /// Preferred source id to boost in ranking without filtering.
    pub prefer_source_id: Option<String>,
}

/// Promotion request used to create a note or append a revision.
#[derive(Debug, Deserialize)]
pub struct PromoteNoteRequest {
    pub note_id: Option<String>,
    pub source_kind: Option<String>,
    pub source_id: Option<String>,
    pub title: Option<String>,
    pub slug: Option<String>,
    pub summary: Option<String>,
    pub body_markdown: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub note_type: Option<String>,
    pub frontmatter: Option<Value>,
    pub authored_by: Option<NoteAuthor>,
    #[serde(default)]
    pub source_references: Vec<NoteSourceReference>,
}

/// Author metadata attached to a stored revision.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NoteAuthor {
    pub actor_type: String,
    pub actor_id: String,
    pub display_name: Option<String>,
}

/// Explicit source lineage attached to a note revision.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NoteSourceReference {
    pub source_kind: String,
    pub source_id: String,
    pub label: Option<String>,
}

/// Lightweight note head used when appending a new revision.
#[derive(Debug, Deserialize)]
pub(crate) struct ExistingNoteHead {
    pub(crate) note_id: String,
    pub(crate) title: String,
    pub(crate) slug: String,
    pub(crate) tags: Vec<String>,
    pub(crate) aliases: Vec<String>,
    pub(crate) note_type: String,
    pub(crate) source_kind: Option<String>,
    pub(crate) source_id: Option<String>,
    pub(crate) current_revision_id: String,
    pub(crate) current_version: i32,
}
