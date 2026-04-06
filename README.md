# elowen-notes

Rust service for versioned notes and promoted knowledge. It is the canonical store for note documents that graduate from transient thread/job context into reusable memory.

The service is ArangoDB-backed so notes can be modeled as documents plus explicit graph edges:

- note documents hold Markdown content and frontmatter-like metadata
- link relationships are stored explicitly for backlinks and traversal
- ArangoSearch can power keyword retrieval over note titles, summaries, and bodies
- attachments can remain separate documents with references from notes

Migration guardrail:

- domain IDs and service contracts must stay database-agnostic so the internals can be migrated to MongoDB later if needed

## Current Responsibilities

- note creation and versioning
- tags and note types
- source references
- keyword and filtered retrieval
- explicit note revision ancestry
- authored-by metadata for promoted knowledge
- thread and job note lookup through `elowen-api`
- job-note revision updates instead of parallel duplicate note creation

## Runtime Notes

`elowen-notes` keeps ArangoDB-specific behavior inside the service boundary. API and UI callers should continue to treat note identifiers and payloads as service contracts rather than database records.

The VPS deployment runs this service from a prebuilt GHCR image rather than compiling on the server.
