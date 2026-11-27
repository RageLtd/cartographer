# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What This Is

Cartographer is an MCP server that builds an import graph of codebases using Tree-sitter AST parsing. It stores file metadata and import edges in an embedded SQLite file under the data directory, then answers graph queries with a breadth-first walk over the import edges. Started with `--parse-only`, it is a pure parser that never opens the store. Designed as a companion to [Goldfish](https://github.com/RageLtd/Goldfish) — Goldfish remembers what you *did*, Cartographer maps what *exists*.

## Commands

```bash
cargo build              # Build (debug)
cargo build --release    # Build (release)
cargo run                # Run MCP server (stdio transport)
cargo run -- --parse-only   # Run MCP server without opening the store (parsing tools only)
cargo run -- grammars    # List built-in + host grammars; validate ~/.cartographer/grammars (exit 1 on a broken grammar)
cargo run -- parse <file>   # Parse one file through the registry and print the JSON result (smoke test for a new grammar)
cargo test               # Run all tests
cargo check              # Type-check without building
cargo clippy --all-targets -- -D warnings   # Lint, as CI runs it (must pass with zero warnings)
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

- **`src/main.rs`** — Entry point. Delegates CLI subcommands to `cli::run()` or starts the MCP server on stdio. Builds the `LanguageRegistry` (built-in + host grammars) via `grammars.rs`, opens the store unless `--parse-only`.
- **`src/cli.rs`** — Thin CLI dispatcher over `args[1..]`. Routes `grammars` to `grammars::run_cli()`, `parse <file>` to `parse_cli::run_cli()`, and hook subcommands (`hook:context`, `hook:prompt`, `hook:pre-read`, `hook:pre-edit`, `hook:post-edit`, `hook:post-compact`) to handlers in `hooks.rs`.
- **`src/grammars.rs`** — Assembles the registry (`load_registry()` → `RegistryReport` with failures), logs it for the server (`log_report`), and implements the `grammars` CLI listing/validation (`run_cli`).
- **`src/parse_cli.rs`** — `cartographer parse <file>`: parses one file through the assembled registry and prints `ParseReport` JSON (camelCase). Exit 1 with the parser's error on failure, 2 on usage.
- **`src/hooks.rs`** — All 6 hook handlers. `run_hook()` combinator handles stdin parsing and output. `hook_context()` (SessionStart), `hook_prompt()` (UserPromptSubmit file mention lookup), `hook_pre_read()`/`hook_pre_edit()` (PreToolUse graph context injection), `hook_post_edit()` (PostToolUse git diff tracking), `hook_post_compact()` (PostCompact structural context re-injection). Each public `hook_*` function passes `store_path()` to a private function holding the hook's body (`session_start`, `prompt_submit`, `file_tool`, `post_edit`, `post_compact`), which opens the store at that path; tests call those with a store in a temporary directory.
- **`src/server.rs`** — `CartographerServer` state (`db`, `registry: Arc<LanguageRegistry>`) and the combined `tool_router()`. Tools live in per-concern submodules, each with its own `#[tool_router(router = ..., vis = "pub(crate)")]` block summed with `+`:
  - **`src/server/parse_tools.rs`** — `cartographer_parse_file`, `cartographer_detect_changes`, `cartographer_index_project`, `cartographer_uses_in_file` (the registry-dependent tools).
  - **`src/server/graph_tools.rs`** — `cartographer_query`, `cartographer_stats` (includes loaded grammars), `cartographer_search`, `cartographer_get_file_info`, `cartographer_find_cycles`, `cartographer_find_uses`.
- **`src/server_types.rs`** — Tool input schema structs (`ParseFileInput`, `QueryInput`, etc.).
- **`src/handler.rs`** — `ServerHandler` impl for `CartographerServer` (server info, resources).
- **`src/parser/mod.rs`** — `parse_file(registry, path)`: looks the file up in the registry, parses, dispatches on `Extractor` (exhaustive match). `hash_file()`, `hash_content()`.
- **`src/parser/registry.rs`** — `BuiltinLanguage` (the six compiled-in grammars: name, extensions, grammar, extractor), `Extractor` enum (`TsJs | Rust | Ruby | Elixir | Generic(Arc<GenericLanguage>)`), `LanguageDef`, and `LanguageRegistry` (extension → language; later registrations override earlier ones so a host grammar can claim a builtin's extension; `owned_extensions()` reports the effective mapping).
- **`src/parser/dynamic.rs`** — `load_grammar(path, symbol)`: `dlopen`s a `tree-sitter build` shared library via `libloading`, reads the ABI version before constructing a `Language`, rejects anything outside `MIN_COMPATIBLE_LANGUAGE_VERSION..=LANGUAGE_VERSION`, and deliberately leaks the library handle (the `Language` points into its mapped pages). `language_symbol(name)` → `tree_sitter_<name>`.
- **`src/parser/generic.rs`** — Query-driven extractor for host grammars. Symbols from a `tags.scm` (`@definition.<kind>`, `@name`, optional `@doc` with `#strip!` / `#select-adjacent!`, plus Cartographer's `@visibility`); imports from an `imports.scm` (`@import.specifier`, `@import.symbol`), resolved through `ResolverStrategy` list. Merges the one-match-per-symbol output tree-sitter produces for unquantified captures.
- **`src/parser/manifest.rs`** — Discovery under `~/.cartographer/grammars/<name>/`: `language.json` (`GrammarManifest`), library lookup (must be a bare file name inside the grammar dir), query files, `register_host_grammars()` collecting per-directory `GrammarLoadError`s instead of aborting.
- **`src/parser/fixtures.rs`** — Test-only: compiles `tests/fixtures/grammars/tiny` (a CLI-generated fixture grammar) into a `.so` with `cc`, skipping with a message when no compiler is present.
- **`src/parser/extractor.rs`** — Shared AST helpers: `strip_quotes()`, `get_doc_comment()`, `get_signature()`.
- **`src/parser/ts_js.rs`** — TypeScript/JavaScript/TSX/JSX extractor. Handles imports, exports, classes, functions, interfaces, enums, type aliases.
- **`src/parser/rust_ext.rs`** — Rust extractor. Handles `use`/`mod` declarations, structs, enums, traits, impls, macros, visibility modifiers.
- **`src/parser/rust_modules.rs`** — Where a Rust file's child modules live. `package_root()` finds the nearest ancestor holding a `Cargo.toml`. `child_module_dir()` returns the one directory to search: the file's own directory for `mod.rs`/`lib.rs`/`main.rs` and Cargo auto-discovered targets (`build.rs`, files directly in `src/bin/`, `tests/`, `examples/`, `benches/`), otherwise `a/b/` for `a/b.rs`. `child_module_file()` finds `x.rs` / `x/mod.rs` in such a directory. `parent_module()` goes the other way and returns a `Module` (file + children directory). A top-level module's parent is whichever of `src/lib.rs` / `src/main.rs` has a top-level `mod <name>;` (parsed with tree-sitter, at most once per file being parsed, through a `RootFileModules` that `extract_rust` creates and passes down); when both or neither do, the file is unknown but the children directory is still `src/`. Elsewhere the parent must be exactly one of `dir/mod.rs`, a directory target's `dir/main.rs`, or a non-target `dir.rs`. `crate_root_module()` follows parents up to a crate root: `src/lib.rs`, `src/main.rs`, `build.rs`, flat targets and `<target dir>/<name>/main.rs`. Targets given a `path` in `Cargo.toml` are not recognised.
- **`src/parser/rust_use.rs`** — `use_edges()` for `use crate::…` / `self::…` / `super::…`: flattens the use tree, walks each path one child module at a time from its starting module, and takes the first non-module segment as the imported item. One edge per target file per declaration (never to the file itself), specifier as written; no edge when a path ends in a module whose file cannot be determined.
- **`src/parser/ruby.rs`** — Ruby extractor. Handles `require`/`require_relative`, classes, modules, methods, singleton methods, constants, `attr_accessor`/`attr_reader`/`attr_writer`, `include`/`extend`/`prepend`.
- **`src/parser/elixir.rs`** — Elixir extractor. Handles `defmodule`, `def`/`defp`, `defmacro`/`defmacrop`, `defstruct`, `alias`/`import`/`use`/`require`. Supports `@doc`/`@moduledoc` heredoc comments.
- **`src/uses.rs`** — `find_uses(db, registry, project, file, symbol)`: every use of a symbol across a project. Checks the file defines the symbol, then walks outward from it: each dependent from the stored graph is parsed with `ts_uses::uses_in_file`, and a file that re-exports the symbol is queued with the name it exports it under. Returns `SymbolUses` (`FileNotIndexed`, `SymbolNotDefined`, or `Found` files with lines).
- **`src/parser/ts_uses.rs`** — `uses_in_file(registry, file, target, symbol)`: where one TS/JS file uses a symbol `target` exports. Reads the file's top-level imports and exports for the local names bound to the symbol (renames, `import * as ns`, the default export, re-exports), then collects the lines of matching identifiers. Parsing only, no index. Matching is by name, with no scope analysis.
- **`src/parser/workspace.rs`** — JS/TS imports of a workspace package by name. `Workspace::containing(file)` finds the nearest ancestor `package.json` with `workspaces` and reads the packages its patterns name; `resolve(specifier)` matches a package name and resolves the rest through the package's `exports` (exact subpaths, one-`*` patterns, conditions in written order) or, without `exports`, through `module` / `main` / an index file and plain paths. Read from disk per file, never cached. `ts_js.rs` builds one lazily per file through its `Importer`.
- **`src/parser/resolver.rs`** — Import specifier resolution. `resolve_ts_js_import()` for relative JS/TS imports, over `resolve_ts_js_path()`, which probes one path (as written, with an extension, as a directory with an index). `resolve_rust_mod_decl()` for `mod x;`, looking for `x.rs` then `x/mod.rs` (`rust_modules::child_module_file()`) in the directory `rust_modules::child_module_dir()` picks. `resolve_ruby_require()` for `require_relative`. `resolve_elixir_module()` for Elixir module-to-file mapping via `mix.exs` project root. For host grammars: `ResolverStrategy` (`relative-path` and `module-path`, deserialized from the manifest) and `resolve_with_strategies()`.
- **`src/indexer.rs`** — `reconcile()` brings the index in line with the files on disk: it walks the file tree, removes indexed files that are gone, and re-parses files whose content hash differs. `full_index()` and `detect_changes()` both call it and differ only in what they report (counts, or the `Changes` lists). Git is never consulted.
- **`src/db/mod.rs`** — The storage layer: `client`, `queries`, `graph`, `cycles`, and the private `schema` and `search_words`.
- **`src/db/cycles.rs`** — `find_cycles`: reads the dependency edges and returns the strongly connected components of more than one file, each sorted, by Kosaraju's two passes with explicit stacks (an import chain can be deeper than the call stack).
- **`src/db/client.rs`** — `Db`, a cloneable handle on one SQLite connection (`Arc<Mutex<Connection>>`). `open(path)` creates the file and its directory, sets a busy timeout and WAL mode, and runs `init_schema()`. `store_path()` is `data_dir()/cartographer.db`; `connect()` opens it. `Db::read` and `Db::write` are the only way queries reach the connection: `write` runs its closure in one `BEGIN IMMEDIATE` transaction, and both return `Err("{what}: {cause}")`.
- **`src/db/schema.rs`** — Tables `files`, `imports` (primary key `project, source_path, target_path, specifier`; no foreign key to `files`), and the FTS5 table `file_search`, whose rowid is `files.id`. `SCHEMA_VERSION` is kept in `PRAGMA user_version`; a file at any other version has every table dropped and recreated.
- **`src/db/search_words.rs`** — `search_words(text)`: the runs of letters and of digits in a string. Used for both the stored search text and the query, so the two are always cut the same way.
- **`src/db/queries.rs`** — File upsert/remove, content-hash lookup, the list of indexed paths, import edge replacement (edges with the same source, target and specifier are merged), suffix lookup for file mentions (`GLOB`, so `_` and `%` are literal and case matters), stats.
- **`src/db/graph.rs`** — `walk_import_graph` (BFS over `imports`, one query per hop in each direction, bounded by depth and result count), `get_file_detail`, and `search_files` (FTS5 `MATCH` requiring every word of the query, ordered by `bm25`).
- **`src/constants.rs`** — `SKIP_DIRS` lists directories to ignore during indexing. `data_dir()` (`$CARTOGRAPHER_HOME`, else `~/.cartographer`) and `grammars_dir()` for host grammar discovery. Extension → language mapping lives in `parser/registry.rs`, not here.
- **`src/types.rs`** — Core type definitions: `ImportEdge`, `EdgeKind` (`Import` or `ModuleDeclaration`), `Symbol`, `SymbolKind`, `Visibility`, `FileParseResult`, `RelevantFile`.

### The Claude Code plugin

The repository root is also the plugin. `claude plugin validate .` checks it, and `claude --plugin-dir .` loads it into a session without publishing.

- **`.claude-plugin/plugin.json`** — the manifest. It registers the MCP server (`mcpServers`); nothing else does, and the marketplace entry in `RageLtd/claude-plugins` cannot while this file exists. `commands/` and `skills/` are found by default and need no field.
- **`scripts/run-server.sh`** — the command the manifest registers. It downloads the binary if it is missing, then `exec`s it. It is a launcher because a stdio server that fails its first connection is not started again, and on a first session the server can start before the hook below has finished. Only the binary may write to standard output there: it is the MCP channel.
- **`scripts/ensure-binary.sh`** (and `.ps1`) — run by the SessionStart hook: downloads the latest release's binary for the platform into `~/.cartographer/bin` when the recorded version differs. A failed download leaves the installed binary and exits non-zero.
- **`hooks/hooks.json`** — the hooks, each a `cartographer hook:*` subcommand of the installed binary.
- **`commands/<name>.md`** — slash commands, named `/cartographer:<name>` after the plugin and the file. **`skills/<name>/SKILL.md`** — skills, each with `name` and `description` frontmatter.
- **`tests/ensure_binary.rs`** — runs both scripts for real with a stand-in `curl` and a temporary `HOME`.

### Key Design Decisions

- **Embedded SQLite, raw SQL** — `rusqlite` with the `bundled` feature, no ORM and no server. One file, `cartographer.db` under the data directory, shared by the MCP server and the short-lived hook processes; WAL mode and a busy timeout let a hook read while the server writes. Storage functions are synchronous, and so are the tool handlers that call them.
- **The store is a cache, not a record** — everything in it can be rebuilt by indexing. A schema change bumps `SCHEMA_VERSION` and old files are emptied on open; there are no migrations. Bump it too when what is written changes without the tables changing, such as how `search_words` cuts text.
- **One binary** — there is no storage Cargo feature. `--parse-only` is a run-time flag: the server starts without a `Db` and the storage tools report that storage is unavailable. Mimir runs the same release asset, `cartographer-<platform>`, that way.
- **Graph queries are a bounded BFS** — `walk_import_graph` expands dependencies (what a file imports) and dependents (what imports it) one hop per query until `max_depth` / `max_results`.
- **Cycles are reported as groups, not paths** — `find_cycles` returns each set of files that can all reach one another. Enumerating the cycles themselves cannot be both complete and bounded (fifteen files that all import each other hold millions), and the search it replaced was bounded by silently missing some. The MCP tool reports `files` and `size` per group.
- **Symbol uses are found at query time, in two layers** — nothing about uses is stored. The graph says which files import a file; `ts_uses::uses_in_file` parses each one when asked. The split exists because Mimir runs Cartographer in parse-only mode and owns its own graph: `cartographer_uses_in_file` is the parsing layer alone and needs no index, and `cartographer_find_uses` adds the graph walk for stand-alone use. Keep graph knowledge out of `ts_uses.rs`.
- **Full-text search is FTS5 with BM25** — `file_search.searchable` holds the words of the file path and of each symbol name, already cut by `search_words` (at punctuation, `_` included, and where letters meet digits; camelCase is not split). The `unicode61` tokenizer only lowercases them. A query is cut the same way and every word must match.
- **Symbols store JSON** — `files.symbols` is a JSON string of `Symbol[]`, parsed at read time.
- **Import edges are unique per source, target and specifier** — the parser can emit several (`import type { A } from "./a"` beside `import { b } from "./a"`); `replace_imports` merges them into one row with the symbols of each. Edges name files by path with no foreign key, because an edge is stored before its target is indexed and must survive the target being re-upserted.
- **An edge records whether it is a dependency** — every extractor sets `ImportEdge::kind`. Only the Rust extractor produces `EdgeKind::ModuleDeclaration`, for `mod x;`: the parent has to write it for the child to exist, so it says where the child lives, not that the parent uses it. The store keeps `EdgeKind::is_dependency()` as `imports.is_dependency`. `find_cycles` reads only dependencies, because a `mod child;` plus the child's `use super::…` is not a cycle; the graph walk, dependents and file detail read every edge, because removing or renaming the child still breaks the parent. A new edge kind must decide `is_dependency()` in its `match`.
- **Only imports of local code are resolved** — installed npm packages, external Rust crates, and Ruby gems are intentionally skipped. Rust `use` paths resolve only when they start with `crate`, `self` or `super`. A JS/TS bare specifier resolves only when it names a package of the importing file's own workspace (`package.json` `workspaces`; not `pnpm-workspace.yaml`, not `tsconfig` paths).
- **Workspace exports are read for the source file, not as Node would** — Node picks the conditions one runtime has active. The graph wants the file a person would edit, so every condition of an `exports` entry is a candidate, in the order written, and the first whose file exists wins. A subpath the map does not list has no edge, as in Node.
- **Built-in grammars are statically linked** — TS/TSX, JS/JSX, Rust, Ruby, and Elixir ship in the binary with hand-written extractors; they stay authoritative for their extensions unless a host grammar's manifest explicitly claims one.
- **Host grammars load at runtime, from the user's data dir only** — `~/.cartographer/grammars/<name>/` with a `tree-sitter build` `.so`, `language.json`, and `tags.scm` / `imports.scm`. Loaded via `libloading` after an ABI check, extracted with queries rather than code. Never load a shared library from a path inside the project being indexed: `dlopen` runs arbitrary code.
- **Tree-sitter 0.27 runtime** — accepts grammar ABI 13 through 15; the grammar crates bind through `tree-sitter-language`, so bumping the runtime does not require bumping them.
- **Change is decided by content hash** — `reconcile()` hashes every file and parses only those whose hash differs from the index. `git status` was dropped because it only sees uncommitted differences. One exception to the skip: an import resolves against the files that exist, so when any file is added or removed, the unchanged files are parsed again to recompute their edges (which importers are affected is not recorded, since an unresolved import stores no edge). A change to one file's content that alters how another file's imports resolve, such as moving a Rust `mod` declaration between `lib.rs` and `main.rs`, is not caught until that other file changes. Editing a `package.json` (its `workspaces`, `exports`, `module` or `main`) is the same case for the JS/TS files that import through it.
- **sonic-rs for JSON** — SIMD-accelerated JSON serialization/deserialization, API-compatible with serde_json.
- **Tests in separate files** — Large test modules use `#[path = "..._tests.rs"]` to keep source files under 500 lines.

## Runtime & Tooling

- **Language:** Rust (2021 edition)
- **MCP SDK:** rmcp 1.2 with `#[tool_router]` / `#[tool_handler]` macros
- **Async runtime:** Tokio
- **Database:** SQLite through `rusqlite` 0.43 (`bundled`, which compiles SQLite in with FTS5)
- **Testing:** `cargo test` — covers parser, resolver, registry, dynamic loading, query extraction, manifest discovery, the store (`src/db/*_tests.rs`, each test on its own file in a temporary directory), indexing a project on disk into a store (`src/indexer_tests.rs`), and the hooks answering from one (`src/hooks_tests.rs`). No test needs an external service. The dynamic-grammar tests compile `tests/fixtures/grammars/tiny` with `cc` and skip (loudly) when no C compiler is installed.
