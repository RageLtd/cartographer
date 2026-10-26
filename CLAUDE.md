# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What This Is

Cartographer is an MCP server that builds an import graph of codebases using Tree-sitter AST parsing. It stores file metadata and import edges in SurrealDB (behind the default `surrealdb` feature), then answers graph queries with a breadth-first walk over the import edges. Built without the feature, it is a pure parser that runs in `--parse-only` mode. Designed as a companion to [Goldfish](https://github.com/RageLtd/Goldfish) — Goldfish remembers what you *did*, Cartographer maps what *exists*.

## Commands

```bash
cargo build              # Build (debug)
cargo build --release    # Build (release)
cargo build --no-default-features   # Parse-only binary: no SurrealDB dependency, storage tools report unavailable
cargo test --no-default-features    # Must pass too — CI checks both configurations
cargo run                # Run MCP server (stdio transport)
cargo run -- grammars    # List built-in + host grammars; validate ~/.cartographer/grammars (exit 1 on a broken grammar)
cargo run -- parse <file>   # Parse one file through the registry and print the JSON result (smoke test for a new grammar)
cargo test               # Run all tests
cargo check              # Type-check without building
cargo clippy             # Lint (must pass with zero warnings)
cargo fmt                # Format code
```

## Architecture

### Data Flow

```
File path → parser/registry.rs (extension → grammar + extractor)
          → parser/{ts_js,rust_ext,ruby,elixir}.rs (built-in, hand-written)
            or parser/generic.rs (host grammar, tags.scm / imports.scm queries) → db/queries.rs (upsert)
                                                                                        ↓
Agent query → db/graph.rs (graph walk) → RelevantFile[]
```

### Module Layout

- **`src/main.rs`** — Entry point. Delegates CLI subcommands to `cli::run()` or starts the MCP server on stdio. Builds the `LanguageRegistry` (built-in + host grammars) via `grammars.rs`, connects to the DB unless `--parse-only`.
- **`src/cli.rs`** — Thin CLI dispatcher over `args[1..]`. Routes `grammars` to `grammars::run_cli()`, `parse <file>` to `parse_cli::run_cli()`, and hook subcommands (`hook:context`, `hook:prompt`, `hook:pre-read`, `hook:pre-edit`, `hook:post-edit`, `hook:post-compact`) to handlers in `hooks.rs`.
- **`src/grammars.rs`** — Assembles the registry (`load_registry()` → `RegistryReport` with failures), logs it for the server (`log_report`), and implements the `grammars` CLI listing/validation (`run_cli`).
- **`src/parse_cli.rs`** — `cartographer parse <file>`: parses one file through the assembled registry and prints `ParseReport` JSON (camelCase). Exit 1 with the parser's error on failure, 2 on usage.
- **`src/hooks.rs`** — All 6 hook handlers. `run_hook()` combinator handles stdin parsing and output. `hook_context()` (SessionStart), `hook_prompt()` (UserPromptSubmit file mention lookup), `hook_pre_read()`/`hook_pre_edit()` (PreToolUse graph context injection), `hook_post_edit()` (PostToolUse git diff tracking), `hook_post_compact()` (PostCompact structural context re-injection).
- **`src/server.rs`** — `CartographerServer` state (`db`, `registry: Arc<LanguageRegistry>`) and the combined `tool_router()`. Tools live in per-concern submodules, each with its own `#[tool_router(router = ..., vis = "pub(crate)")]` block summed with `+`:
  - **`src/server/parse_tools.rs`** — `cartographer_parse_file`, `cartographer_detect_changes`, `cartographer_index_project` (the registry-dependent tools).
  - **`src/server/graph_tools.rs`** — `cartographer_query`, `cartographer_stats` (includes loaded grammars), `cartographer_search`, `cartographer_get_file_info`, `cartographer_find_cycles`.
  - **`src/server/project_tools.rs`** — `cartographer_rules`, `cartographer_project_path`.
- **`src/server_types.rs`** — Tool input schema structs (`ParseFileInput`, `QueryInput`, etc.).
- **`src/handler.rs`** — `ServerHandler` impl for `CartographerServer` (server info, resources).
- **`src/parser/mod.rs`** — `parse_file(registry, path, crate_root)`: looks the file up in the registry, parses, dispatches on `Extractor` (exhaustive match). `hash_file()`, `hash_content()`.
- **`src/parser/registry.rs`** — `BuiltinLanguage` (the six compiled-in grammars: name, extensions, grammar, extractor), `Extractor` enum (`TsJs | Rust | Ruby | Elixir | Generic(Arc<GenericLanguage>)`), `LanguageDef`, and `LanguageRegistry` (extension → language; later registrations override earlier ones so a host grammar can claim a builtin's extension; `owned_extensions()` reports the effective mapping).
- **`src/parser/dynamic.rs`** — `load_grammar(path, symbol)`: `dlopen`s a `tree-sitter build` shared library via `libloading`, reads the ABI version before constructing a `Language`, rejects anything outside `MIN_COMPATIBLE_LANGUAGE_VERSION..=LANGUAGE_VERSION`, and deliberately leaks the library handle (the `Language` points into its mapped pages). `language_symbol(name)` → `tree_sitter_<name>`.
- **`src/parser/generic.rs`** — Query-driven extractor for host grammars. Symbols from a `tags.scm` (`@definition.<kind>`, `@name`, optional `@doc` with `#strip!` / `#select-adjacent!`, plus Cartographer's `@visibility`); imports from an `imports.scm` (`@import.specifier`, `@import.symbol`), resolved through `ResolverStrategy` list. Merges the one-match-per-symbol output tree-sitter produces for unquantified captures.
- **`src/parser/manifest.rs`** — Discovery under `~/.cartographer/grammars/<name>/`: `language.json` (`GrammarManifest`), library lookup (must be a bare file name inside the grammar dir), query files, `register_host_grammars()` collecting per-directory `GrammarLoadError`s instead of aborting.
- **`src/parser/fixtures.rs`** — Test-only: compiles `tests/fixtures/grammars/tiny` (a CLI-generated fixture grammar) into a `.so` with `cc`, skipping with a message when no compiler is present.
- **`src/parser/extractor.rs`** — Shared AST helpers: `strip_quotes()`, `get_doc_comment()`, `get_signature()`.
- **`src/parser/ts_js.rs`** — TypeScript/JavaScript/TSX/JSX extractor. Handles imports, exports, classes, functions, interfaces, enums, type aliases.
- **`src/parser/rust_ext.rs`** — Rust extractor. Handles `use`/`mod` declarations, structs, enums, traits, impls, macros, visibility modifiers.
- **`src/parser/ruby.rs`** — Ruby extractor. Handles `require`/`require_relative`, classes, modules, methods, singleton methods, constants, `attr_accessor`/`attr_reader`/`attr_writer`, `include`/`extend`/`prepend`.
- **`src/parser/elixir.rs`** — Elixir extractor. Handles `defmodule`, `def`/`defp`, `defmacro`/`defmacrop`, `defstruct`, `alias`/`import`/`use`/`require`. Supports `@doc`/`@moduledoc` heredoc comments.
- **`src/parser/resolver.rs`** — Import specifier resolution. `resolve_ts_js_import()` for relative JS/TS imports. `resolve_rust_module()` for `crate::`/`self::`/`super::` paths. `resolve_ruby_require()` for `require_relative`. `resolve_elixir_module()` for Elixir module-to-file mapping via `mix.exs` project root. For host grammars: `ResolverStrategy` (`relative-path` and `module-path`, deserialized from the manifest) and `resolve_with_strategies()`.
- **`src/indexer.rs`** — `full_index()` walks the file tree (skips unchanged files via content hash); `incremental_index()` re-parses modified files and removes deleted ones. `diff_git_status()` compares against last stored snapshot.
- **`src/db/mod.rs`** — The feature gate. With `surrealdb` it exposes `client`, `queries`, `graph`, `schema`; without it, `disabled` is re-exported under the same module names.
- **`src/db/client.rs`** — `Db = Surreal<Client>` and `connect()`: WebSocket connection from `SURREAL_URL` / `SURREAL_USER` / `SURREAL_PASS` / `SURREAL_NS` / `SURREAL_DB` (defaults `localhost:8001`, `root`, `mimir/mimir`), then `init_schema()`.
- **`src/db/schema.rs`** — Idempotent `DEFINE … IF NOT EXISTS` for three SCHEMAFULL tables: `cart_file`, `cart_import`, `cart_git_state`, their unique indexes, and a BM25 full-text index over `cart_file.searchable`.
- **`src/db/queries.rs`** — File upsert/remove, content-hash lookup, import edge replacement, suffix lookup for file mentions, stats, git state persistence. Rows deserialize via `#[derive(SurrealValue)]`.
- **`src/db/graph.rs`** — `walk_import_graph` (BFS over `cart_import`, one query per hop in each direction, bounded by depth and result count), `find_cycles`, `get_file_detail`, and `search_files` (`searchable @1@ $query` ranked by `search::score`).
- **`src/db/disabled.rs`** — Compiled only without the feature: the same API over an uninhabited `enum Db {}`. `connect()` returns a "built without `surrealdb`" error; every `fn(&Db, …)` body is `match *db {}`, so the compiler proves they can never run. This is why no `#[cfg]` appears outside `src/db/` and `main.rs`.
- **`src/constants.rs`** — `SKIP_DIRS` lists directories to ignore during indexing. `data_dir()` (`$CARTOGRAPHER_HOME`, else `~/.cartographer`) and `grammars_dir()` for host grammar discovery. Extension → language mapping lives in `parser/registry.rs`, not here.
- **`src/types.rs`** — Core type definitions: `ImportEdge`, `Symbol`, `SymbolKind`, `Visibility`, `FileParseResult`, `RelevantFile`.

### Key Design Decisions

- **SurrealDB over WebSocket, raw SurrealQL** — no ORM. The DB is an external server shared with Mimir (`mimir/mimir` namespace by default); Cartographer owns the `cart_*` tables. Tool handlers are synchronous and bridge to the async client through `run_async()` (`block_in_place` + `block_on`).
- **Storage is a Cargo feature** — `surrealdb` is on by default. `--no-default-features` removes the dependency entirely; the binary then forces `--parse-only` and the `db` module's API is satisfied by uninhabited stubs rather than `#[cfg]` at every call site. Keep it that way: new storage functions get a stub in `db/disabled.rs`, not a `cfg` in the caller. The release workflow builds both variants per platform: `cartographer-<platform>` (full; what `scripts/ensure-binary.sh` downloads) and `cartographer-parse-only-<platform>` (for mimir).
- **Graph queries are a bounded BFS** — `walk_import_graph` expands dependencies (what a file imports) and dependents (what imports it) one hop per query until `max_depth` / `max_results`.
- **Full-text search is BM25 in SurrealDB** — `cart_file.searchable` holds `"<file_path> <symbol_names>"` and is indexed with the `cart_analyzer` analyzer; `search_files` queries it with `@1@` and ranks by `search::score(1)`.
- **Symbols store JSON** — `cart_file.symbols` is a JSON string of `Symbol[]`, parsed at read time; `symbol_names` is the space-separated clean text that feeds `searchable`.
- **Only relative/local imports are resolved** — bare specifiers (npm packages), external Rust crates, and Ruby gems are intentionally skipped.
- **Built-in grammars are statically linked** — TS/TSX, JS/JSX, Rust, Ruby, and Elixir ship in the binary with hand-written extractors; they stay authoritative for their extensions unless a host grammar's manifest explicitly claims one.
- **Host grammars load at runtime, from the user's data dir only** — `~/.cartographer/grammars/<name>/` with a `tree-sitter build` `.so`, `language.json`, and `tags.scm` / `imports.scm`. Loaded via `libloading` after an ABI check, extracted with queries rather than code. Never load a shared library from a path inside the project being indexed: `dlopen` runs arbitrary code.
- **Tree-sitter 0.27 runtime** — accepts grammar ABI 13 through 15; the grammar crates bind through `tree-sitter-language`, so bumping the runtime does not require bumping them.
- **Content hash skip optimization** — `full_index()` checks file content hashes before parsing; unchanged files are skipped entirely.
- **sonic-rs for JSON** — SIMD-accelerated JSON serialization/deserialization, API-compatible with serde_json.
- **Tests in separate files** — Large test modules use `#[path = "..._tests.rs"]` to keep source files under 500 lines.

## Runtime & Tooling

- **Language:** Rust (2021 edition)
- **MCP SDK:** rmcp 1.2 with `#[tool_router]` / `#[tool_handler]` macros
- **Async runtime:** Tokio
- **Database:** SurrealDB 3 (`surrealdb` + `surrealdb-types` crates), optional via the `surrealdb` feature
- **Testing:** `cargo test` — covers parser, resolver, registry, dynamic loading, query extraction, manifest discovery, DB queries, graph walks. The dynamic-grammar tests compile `tests/fixtures/grammars/tiny` with `cc` and skip (loudly) when no C compiler is installed.
