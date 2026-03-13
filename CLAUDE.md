# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What This Is

Cartographer is an MCP server that builds an import graph of codebases using Tree-sitter AST parsing. It stores file metadata and import edges in SQLite, then answers graph queries via recursive CTEs. Designed as a companion to [Goldfish](https://github.com/RageLtd/Goldfish) — Goldfish remembers what you *did*, Cartographer maps what *exists*.

## Commands

```bash
cargo build              # Build (debug)
cargo build --release    # Build (release)
cargo run                # Run MCP server (stdio transport)
cargo test               # Run all tests
cargo check              # Type-check without building
```

## Architecture

### Data Flow

```
File path → parser/extractor.rs (Tree-sitter AST) → db/queries.rs (SQLite upsert)
                                                          ↓
Agent query → db/queries.rs (recursive CTE walk) → RelevantFile[]
```

### Module Layout

- **`src/main.rs`** — Entry point. Creates `~/.cartographer/` data dir, opens DB, runs migrations, starts MCP server on stdio transport.
- **`src/server.rs`** — `CartographerServer` with `#[tool_router]` (5 tools) and `ServerHandler` impl (1 resource). Uses `Arc<Mutex<Connection>>` for thread-safe DB access.
- **`src/parser/extractor.rs`** — Core AST logic. `extract_ts_js()` and `extract_rust()` walk tree-sitter parse trees to extract import edges and symbol info (signatures, doc comments, visibility). Grammars are compiled into the binary.
- **`src/parser/resolver.rs`** — Import specifier resolution. `resolve_ts_js_import()` handles relative imports with extension/index probing. `resolve_rust_module()` handles `crate::`/`self::`/`super::` paths. Bare/external specifiers are intentionally skipped.
- **`src/parser/mod.rs`** — `parse_file()` dispatch (extension → grammar → extract), `hash_file()`, `hash_content()`.
- **`src/indexer.rs`** — `full_index()` walks the file tree; `incremental_index()` re-parses modified files and removes deleted ones. `diff_git_status()` compares current `git status --porcelain` against the last stored snapshot.
- **`src/db/queries.rs`** — All SQLite operations: file CRUD, import edge replacement, graph walking (`walk_import_graph` via recursive CTE), FTS5 search (`search_files`), git state persistence.
- **`src/db/mod.rs`** — Re-exports `migrations`, `queries`, and `setup` submodules.
- **`src/db/setup.rs`** — Database creation with WAL mode, performance PRAGMAs, and migration runner.
- **`src/db/migrations.rs`** — Versioned schema migrations. Three tables: `files`, `imports`, `git_state`. Plus indexes, FTS5 virtual table with sync triggers, and `symbol_names` column for clean text indexing.
- **`src/constants.rs`** — `LANGUAGE_CONFIG` maps file extensions to tree-sitter grammars. `SKIP_DIRS` lists directories to ignore during indexing. `data_dir()` and `default_db_path()` for `~/.cartographer/map.db`.
- **`src/types.rs`** — Core type definitions: `ImportEdge`, `Symbol`, `SymbolKind`, `Visibility`, `FileParseResult`, `RelevantFile`.

### Key Design Decisions

- **rusqlite with bundled SQLite** — no ORM, raw SQL. DB stored at `~/.cartographer/map.db`. Thread-safe via `Arc<Mutex<Connection>>`. Tool handlers are synchronous functions that acquire the lock directly.
- **Graph queries use recursive CTEs** — walks both dependencies (what a file imports) and dependents (what imports it) bidirectionally.
- **FTS5 for search** — content-synced virtual table with triggers. Indexes `file_path` and `symbol_names` (space-separated symbol names, not raw JSON).
- **Symbols store JSON** — the `symbols` column in `files` is a JSON string of `Symbol[]`, parsed at read time. A separate `symbol_names` column stores clean text for FTS.
- **Only relative/local imports are resolved** — bare specifiers (npm packages) and external Rust crates are intentionally skipped.
- **Tree-sitter grammars compiled in** — TS/TSX, JS/JSX, and Rust grammars are statically linked. No runtime grammar loading.
- **sonic-rs for JSON** — SIMD-accelerated JSON serialization/deserialization, API-compatible with serde_json.

## Runtime & Tooling

- **Language:** Rust (2021 edition)
- **MCP SDK:** rmcp 1.2 with `#[tool_router]` / `#[tool_handler]` macros
- **Async runtime:** Tokio
- **Testing:** `cargo test` — covers parser, resolver, DB queries, graph walks, FTS
