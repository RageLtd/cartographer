# Cartographer

Codebase structure mapping MCP server — because your agent shouldn't have to re-read every file.

## Overview

Cartographer is an MCP server that builds and maintains an import graph of your codebase using Tree-sitter AST parsing. It stores file metadata and import edges in SurrealDB, then answers graph queries with a bounded breadth-first walk over the edges. Storage is optional at build time: without it, Cartographer is a pure parser (`--parse-only`) and something else — such as mimir-acp — owns the graph.

Designed to complement [Goldfish](https://github.com/RageLtd/Goldfish) — Goldfish remembers what you *did*, Cartographer maps what *exists*.

```
File path → Tree-sitter AST parse → Store imports/symbols in SurrealDB
                                              │
Agent writes/edits → git status diff → Re-parse changed files
                                              │
Agent queries → Walk import graph (BFS, bounded) → Return relevant files
```

## Install

Add the marketplace and install:

```
/plugin marketplace add RageLtd/claude-plugins
/plugin install cartographer@rageltd
```

The plugin automatically downloads the binary on first session start.

## Languages

Built in (compiled into the binary, hand-written extractors):

- TypeScript / JavaScript / TSX / JSX (via `tree-sitter-typescript`, `tree-sitter-javascript`)
- Rust (via `tree-sitter-rust`)
- Ruby (via `tree-sitter-ruby`)
- Elixir (via `tree-sitter-elixir`)

### Host grammars

Any other tree-sitter grammar can be added without rebuilding Cartographer. A grammar is a directory under `~/.cartographer/grammars/<name>/` (set `CARTOGRAPHER_HOME` to move the data dir):

| File | Purpose |
|------|---------|
| `<name>.so` (or `.dylib`) | Shared library from `tree-sitter build`. Must target tree-sitter ABI 13–15; the current CLI emits 15. |
| `language.json` | Manifest: `extensions` (required), `resolvers`, and optional `name`, `library`, `symbol`, `tags`, `imports`, `default_visibility`. |
| `tags.scm` | Symbols, using tree-sitter's [code-navigation convention](https://tree-sitter.github.io/tree-sitter/4-code-navigation.html): `@definition.<kind>` on the defining node, `@name` inside it, optional `@doc` (with `#strip!` / `#select-adjacent!`). Most official grammars ship one under `queries/`. Cartographer adds an optional `@visibility` capture for the node that marks a definition public. |
| `imports.scm` | Imports, Cartographer's own convention: `@import.specifier` on the module/path node, `@import.symbol` on each imported name. |

Grammars are loaded **only** from this directory — never from the project being indexed — because loading a shared library runs arbitrary code. Run `cartographer grammars` to list what loaded and see why anything didn't.

Example, Python:

```bash
git clone https://github.com/tree-sitter/tree-sitter-python
mkdir -p ~/.cartographer/grammars/python
tree-sitter build -o ~/.cartographer/grammars/python/python.so tree-sitter-python
cp tree-sitter-python/queries/tags.scm ~/.cartographer/grammars/python/
```

`~/.cartographer/grammars/python/imports.scm`:

```scheme
(import_statement name: (dotted_name) @import.specifier)
(import_statement name: (aliased_import name: (dotted_name) @import.specifier))
(import_from_statement
  module_name: (dotted_name) @import.specifier
  name: (dotted_name) @import.symbol)
```

`~/.cartographer/grammars/python/language.json`:

```json
{
  "extensions": [".py"],
  "default_visibility": "public",
  "resolvers": [
    { "module-path": {
        "separator": ".",
        "extensions": [".py"],
        "index_names": ["__init__"],
        "root_marker": "pyproject.toml",
        "source_dirs": ["", "src"]
    } }
  ]
}
```

Two resolver strategies exist, tried in order. `relative-path` handles `./foo`-style specifiers against the importing file's directory (`extensions`, `index_names`). `module-path` splits on `separator`, walks from the nearest ancestor containing `root_marker` (or each ancestor of the file when unset) through `source_dirs`, and treats a trailing segment that isn't a module as an imported item (`extensions`, `index_names`, `strip_prefixes`). Relative dotted imports like Python's `from .sibling import x` are not resolved yet.

A host grammar may claim an extension a builtin owns; the manifest's registration wins and `cartographer grammars` shows the builtin as overridden.

Adding a *built-in* language still means a grammar crate dependency, a `BuiltinLanguage` variant in `src/parser/registry.rs`, and an extractor in `src/parser/`.

## MCP Tools

| Tool | Purpose |
|------|---------|
| `cartographer_index_project` | Full index of all supported files in a project |
| `cartographer_query` | Walk the import graph from entry points, return relevant files |
| `cartographer_search` | Full-text search across file paths and symbol names |
| `cartographer_get_file_info` | Detailed file info: symbols, imports, dependents |
| `cartographer_find_cycles` | Detect circular dependencies in the import graph |
| `cartographer_parse_file` | Parse a single file's AST and store in DB |
| `cartographer_detect_changes` | Diff git status, re-parse changed files |
| `cartographer_stats` | File count, import edges, languages breakdown |

### Resources

| Resource | Description |
|----------|-------------|
| `cartographer://project` | List all indexed projects and file counts |

## Slash Commands

| Command | Purpose |
|---------|---------|
| `/carto:index` | Index the current project |
| `/carto:deps <file>` | Show what a file depends on |
| `/carto:impact <file>` | Blast radius analysis — what breaks if you change a file |
| `/carto:search <query>` | Search files and symbols |
| `/carto:cycles` | Find circular dependencies |
| `/carto:stats` | Show index statistics |
| `/carto:info <file>` | Detailed file info (symbols, imports, dependents) |

## Hooks

Cartographer hooks into Claude Code sessions to provide structural context automatically:

| Event | What it does |
|-------|-------------|
| **SessionStart** | Downloads/updates the binary, injects graph-first guidance and index status |
| **UserPromptSubmit** | Extracts file mentions from prompts, injects their graph neighborhood |
| **PreToolUse (Read)** | Injects imports/dependents context when Claude reads a file |
| **PreToolUse (Edit/Write)** | Injects blast radius before Claude modifies a file |
| **PostToolUse (Edit/Write/Bash)** | Tracks changed files via `git diff`, reports lines changed + dependents |
| **PostCompact** | Re-injects structural summary of modified files after context compaction |

## CLI

The binary doubles as both an MCP server and a CLI for hooks:

```bash
cartographer                # Start MCP server (stdio transport, needs SurrealDB)
cartographer --parse-only    # Start MCP server without a database (parsing tools only)
cartographer grammars        # List built-in + host grammars; exit 1 if a host grammar fails to load
cartographer parse <file>    # Parse one file with every loaded grammar; print imports + symbols as JSON
cartographer hook:context    # SessionStart hook
cartographer hook:prompt     # UserPromptSubmit hook
cartographer hook:pre-read   # PreToolUse Read hook
cartographer hook:pre-edit   # PreToolUse Edit/Write hook
cartographer hook:post-edit  # PostToolUse Edit/Write/Bash hook
cartographer hook:post-compact # PostCompact hook
```

## What It Stores

A minimal import map in SurrealDB — no embeddings, no vectors. The connection comes from `SURREAL_URL`, `SURREAL_USER`, `SURREAL_PASS`, `SURREAL_NS`, `SURREAL_DB` (defaults: `localhost:8001`, `root`, `mimir`/`mimir`), and the schema is defined idempotently on connect:

- **cart_file**: `(project, file_path, language, symbols, symbol_names, content_hash, last_parsed_epoch, searchable)` — `symbols` is a JSON string of `Symbol[]`; `searchable` is `"<file_path> <symbol_names>"` under a BM25 full-text index
- **cart_import**: `(project, source_path, target_path, specifier, symbols, updated_at_epoch)`
- **cart_git_state**: `(project, last_status, updated_at_epoch)` — for change detection

Graph queries are a breadth-first walk over `cart_import`, one query per hop in each direction, bounded by depth and result count. Search is SurrealDB full-text (`@1@`, ranked by `search::score`).

### Building without storage

```bash
cargo build --no-default-features
```

drops the `surrealdb` dependency entirely. The binary then always runs in `--parse-only` mode: `cartographer_parse_file`, `cartographer_index_project` and `cartographer_detect_changes` parse and return results, while the graph, search and stats tools report that storage is unavailable.

Every release ships both variants for each platform: `cartographer-<platform>` (full) and `cartographer-parse-only-<platform>` (no storage). The Claude Code plugin downloads the full one; mimir consumes the parse-only one and owns the graph itself.

## Development

```bash
cargo build              # Build (debug)
cargo build --release    # Build (release)
cargo run                # Run MCP server (stdio transport)
cargo test               # Run all tests
```

## Requirements

- **Language:** Rust (2021 edition)
- **MCP SDK:** rmcp with `#[tool_router]` / `#[tool_handler]` macros
- **Database:** SurrealDB 3 over WebSocket (external server), optional via the default `surrealdb` Cargo feature
- **Tree-sitter:** 0.27 runtime. Six grammars compiled in; others loaded at runtime from `~/.cartographer/grammars` (ABI 13–15)
