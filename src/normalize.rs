//! Pure normalization helpers for note payloads.

use crate::models::{NoteAuthor, NoteSourceReference};

/// Trims an optional string and removes empty values.
pub(crate) fn sanitize_optional_string(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// Normalizes a list of user-provided strings and removes empties.
pub(crate) fn normalize_optional_list(values: Vec<String>) -> Option<Vec<String>> {
    let normalized = values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    (!normalized.is_empty()).then_some(normalized)
}

/// Normalizes a note author and rejects incomplete authors.
pub(crate) fn normalize_note_author(author: Option<NoteAuthor>) -> Option<NoteAuthor> {
    let mut author = author?;
    author.actor_type = author.actor_type.trim().to_string();
    author.actor_id = author.actor_id.trim().to_string();
    author.display_name = sanitize_optional_string(author.display_name);

    if author.actor_type.is_empty() || author.actor_id.is_empty() {
        return None;
    }

    Some(author)
}

/// Normalizes note source references and ensures the primary source is present.
pub(crate) fn normalize_source_references(
    references: Vec<NoteSourceReference>,
    source_kind: Option<&str>,
    source_id: Option<&str>,
) -> Vec<NoteSourceReference> {
    let mut normalized = references
        .into_iter()
        .filter_map(|reference| {
            let source_kind = reference.source_kind.trim().to_string();
            let source_id = reference.source_id.trim().to_string();
            if source_kind.is_empty() || source_id.is_empty() {
                return None;
            }

            Some(NoteSourceReference {
                source_kind,
                source_id,
                label: sanitize_optional_string(reference.label),
            })
        })
        .collect::<Vec<_>>();

    let has_primary_reference = normalized.iter().any(|reference| {
        Some(reference.source_kind.as_str()) == source_kind
            && Some(reference.source_id.as_str()) == source_id
    });

    if !has_primary_reference && let (Some(source_kind), Some(source_id)) = (source_kind, source_id)
    {
        normalized.insert(
            0,
            NoteSourceReference {
                source_kind: source_kind.to_string(),
                source_id: source_id.to_string(),
                label: None,
            },
        );
    }

    normalized
}

/// Derives a title from the first non-empty line of markdown.
pub(crate) fn derive_title(body_markdown: &str) -> Option<String> {
    body_markdown
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| line.trim_start_matches('#').trim().to_string())
}

/// Derives a compact summary from markdown content.
pub(crate) fn derive_summary(body_markdown: &str) -> String {
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

/// Converts a string into a slug-compatible identifier.
pub(crate) fn slugify(value: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_source_references_inserts_primary_reference() {
        let references = normalize_source_references(
            vec![NoteSourceReference {
                source_kind: " job ".into(),
                source_id: " 123 ".into(),
                label: Some(" Result ".into()),
            }],
            Some("thread"),
            Some("abc"),
        );

        assert_eq!(references[0].source_kind, "thread");
        assert_eq!(references[0].source_id, "abc");
        assert_eq!(references[1].source_kind, "job");
        assert_eq!(references[1].label.as_deref(), Some("Result"));
    }

    #[test]
    fn derive_summary_truncates_long_text() {
        let body = "a".repeat(300);
        let summary = derive_summary(&body);
        assert_eq!(summary.len(), 280);
        assert!(summary.ends_with("..."));
    }

    #[test]
    fn slugify_collapses_non_alphanumeric_runs() {
        assert_eq!(slugify("Hello, Rust World!"), "hello-rust-world");
    }
}
