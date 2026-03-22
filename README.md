# elowen-notes

Rust service for versioned notes and promoted knowledge. It is the canonical store for note documents that graduate from transient thread/job context into reusable memory.

The service is ArangoDB-backed so notes can be modeled as documents plus explicit graph edges:

- note documents hold Markdown content and frontmatter-like metadata
- link relationships are stored explicitly for backlinks and traversal
- ArangoSearch can power keyword retrieval over note titles, summaries, and bodies
- attachments can remain separate documents with references from notes

Migration guardrail:

- domain IDs and service contracts must stay database-agnostic so the internals can be migrated to MongoDB later if needed

## Initial Scope

- note creation and versioning
- tags and note types
- source references
- keyword and filtered retrieval
