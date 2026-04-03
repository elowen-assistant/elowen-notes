//! Configuration and static schema metadata for the notes service.

use anyhow::Context;
use serde_json::{Value, json};
use std::env;

/// Connection details for the backing ArangoDB database.
#[derive(Clone, Debug)]
pub(crate) struct ArangoConfig {
    pub(crate) base_url: String,
    pub(crate) database: String,
    pub(crate) username: String,
    pub(crate) password: String,
}

/// Static description of a collection that must exist during bootstrap.
#[derive(Clone, Copy)]
pub(crate) struct CollectionSpec {
    pub(crate) name: &'static str,
    pub(crate) collection_type: u8,
}

pub(crate) const COLLECTIONS: &[CollectionSpec] = &[
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

impl ArangoConfig {
    /// Loads the ArangoDB connection configuration from environment variables.
    pub(crate) fn from_env() -> anyhow::Result<Self> {
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

/// Builds a persistent index specification for the ArangoDB HTTP API.
pub(crate) fn persistent_index(
    fields: &[&str],
    name: &'static str,
    unique: bool,
    sparse: bool,
) -> Value {
    json!({
        "type": "persistent",
        "fields": fields,
        "name": name,
        "unique": unique,
        "sparse": sparse
    })
}

/// Builds the configured `notes_search` view properties.
pub(crate) fn notes_search_properties() -> Value {
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
