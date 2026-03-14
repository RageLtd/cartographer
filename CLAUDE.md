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
cargo clippy             # Lint (must pass with zero warnings)
cargo fmt                # Format code
```

## Architecture

### Data Flow

```
File path → parser/{ts_js,rust_ext,ruby,elixir}.rs (Tree-sitter AST) → db/queries.rs (SQLite upsert)
                                                                              ↓
Agent query → db/graph.rs (recursive CTE walk) → RelevantFile[]
```

### Module Layout

- **`src/main.rs`** — Entry point. Delegates CLI subcommands to `cli::run()` or starts MCP server on stdio. Creates `~/.cartographer/` data dir, opens DB, runs migrations.
- **`src/cli.rs`** — Thin CLI dispatcher. Routes hook subcommands (`hook:context`, `hook:prompt`, `hook:pre-read`, `hook:pre-edit`, `hook:post-edit`, `hook:post-compact`) to handlers in `hooks.rs`.
- **`src/hooks.rs`** — All 6 hook handlers. `run_hook()` combinator handles stdin parsing and output. `hook_context()` (SessionStart), `hook_prompt()` (UserPromptSubmit file mention lookup), `hook_pre_read()`/`hook_pre_edit()` (PreToolUse graph context injection), `hook_post_edit()` (PostToolUse git diff tracking), `hook_post_compact()` (PostCompact structural context re-injection).
- **`src/server.rs`** — `CartographerServer` with `#[tool_router]` (8 tools). Uses `Arc<Mutex<Connection>>` for thread-safe DB access.
- **`src/server_types.rs`** — Tool input schema structs (`ParseFileInput`, `QueryInput`, etc.).
- **`src/handler.rs`** — `ServerHandler` impl for `CartographerServer` (server info, resources).
- **`src/parser/mod.rs`** — `parse_file()` dispatch (extension → grammar → extract), `hash_file()`, `hash_content()`.
- **`src/parser/extractor.rs`** — Shared AST helpers: `strip_quotes()`, `get_doc_comment()`, `get_signature()`.
- **`src/parser/ts_js.rs`** — TypeScript/JavaScript/TSX/JSX extractor. Handles imports, exports, classes, functions, interfaces, enums, type aliases.
- **`src/parser/rust_ext.rs`** — Rust extractor. Handles `use`/`mod` declarations, structs, enums, traits, impls, macros, visibility modifiers.
- **`src/parser/ruby.rs`** — Ruby extractor. Handles `require`/`require_relative`, classes, modules, methods, singleton methods, constants, `attr_accessor`/`attr_reader`/`attr_writer`, `include`/`extend`/`prepend`.
- **`src/parser/elixir.rs`** — Elixir extractor. Handles `defmodule`, `def`/`defp`, `defmacro`/`defmacrop`, `defstruct`, `alias`/`import`/`use`/`require`. Supports `@doc`/`@moduledoc` heredoc comments.
- **`src/parser/resolver.rs`** — Import specifier resolution. `resolve_ts_js_import()` for relative JS/TS imports. `resolve_rust_module()` for `crate::`/`self::`/`super::` paths. `resolve_ruby_require()` for `require_relative`. `resolve_elixir_module()` for Elixir module-to-file mapping via `mix.exs` project root.
- **`src/indexer.rs`** — `full_index()` walks the file tree (skips unchanged files via content hash); `incremental_index()` re-parses modified files and removes deleted ones. `diff_git_status()` compares against last stored snapshot.
- **`src/db/queries.rs`** — File CRUD, import edge replacement, stats queries, git state persistence.
- **`src/db/graph.rs`** — Graph walking (`walk_import_graph` via recursive CTE), cycle detection (`find_cycles`), file detail (`get_file_detail`), FTS5 search (`search_files`).
- **`src/db/mod.rs`** — Re-exports `graph`, `migrations`, `queries`, and `setup` submodules.
- **`src/db/setup.rs`** — Database creation with WAL mode, performance PRAGMAs, and migration runner.
- **`src/db/migrations.rs`** — Versioned schema migrations. Three tables: `files`, `imports`, `git_state`. Plus indexes, FTS5 virtual table with sync triggers, and `symbol_names` column for clean text indexing.
- **`src/constants.rs`** — `LANGUAGE_CONFIG` maps file extensions to tree-sitter grammars. `SKIP_DIRS` lists directories to ignore during indexing. `data_dir()` and `default_db_path()` for `~/.cartographer/map.db`.
- **`src/types.rs`** — Core type definitions: `ImportEdge`, `Symbol`, `SymbolKind`, `Visibility`, `FileParseResult`, `RelevantFile`.

### Key Design Decisions

- **rusqlite with bundled SQLite** — no ORM, raw SQL. DB stored at `~/.cartographer/map.db`. Thread-safe via `Arc<Mutex<Connection>>`. Tool handlers are synchronous functions that acquire the lock directly.
- **Graph queries use recursive CTEs** — walks both dependencies (what a file imports) and dependents (what imports it) bidirectionally.
- **FTS5 for search** — content-synced virtual table with triggers. Indexes `file_path` and `symbol_names` (space-separated symbol names, not raw JSON).
- **Symbols store JSON** — the `symbols` column in `files` is a JSON string of `Symbol[]`, parsed at read time. A separate `symbol_names` column stores clean text for FTS.
- **Only relative/local imports are resolved** — bare specifiers (npm packages), external Rust crates, and Ruby gems are intentionally skipped.
- **Tree-sitter grammars compiled in** — TS/TSX, JS/JSX, Rust, Ruby, and Elixir grammars are statically linked. No runtime grammar loading.
- **Content hash skip optimization** — `full_index()` checks file content hashes before parsing; unchanged files are skipped entirely.
- **sonic-rs for JSON** — SIMD-accelerated JSON serialization/deserialization, API-compatible with serde_json.
- **Tests in separate files** — Large test modules use `#[path = "..._tests.rs"]` to keep source files under 500 lines.

## Runtime & Tooling

- **Language:** Rust (2021 edition)
- **MCP SDK:** rmcp 1.2 with `#[tool_router]` / `#[tool_handler]` macros
- **Async runtime:** Tokio
- **Testing:** `cargo test` — covers parser, resolver, DB queries, graph walks, FTS
