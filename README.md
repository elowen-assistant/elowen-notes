# elowen-notes

## Purpose

Rust notes service for promoted knowledge. It stores versioned note documents, note revisions, authored-by metadata, and source references behind a service-level API rather than exposing ArangoDB details directly.

## Current Responsibilities

- search notes by text, source filters, and recency
- load the current revision for a note by id
- promote markdown into a new note or a new note revision
- normalize titles, summaries, tags, aliases, and source references
- bootstrap required ArangoDB collections and note types at startup

## Repository Layout

- `src/service/` - note search and promotion handlers plus promotion helpers
- `src/arangodb/` - ArangoDB bootstrap and low-level client helpers
- `src/routes.rs` - HTTP route wiring
- `src/models.rs` - API contracts and persisted note shapes
- `src/normalize.rs` - input normalization helpers
- `src/state.rs` - shared application state

## Runtime And Config Entrypoints

Run locally with:

```bash
cargo run
```

Important environment variables:

- `ARANGO_URL`
- `ARANGO_DATABASE`
- `ARANGO_USERNAME`
- `ARANGO_PASSWORD`
- `PORT`

## Local Verification

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --quiet
cargo doc --no-deps
```

## Related Docs

- `src/arangodb/`
- `../elowen-platform/db/`
