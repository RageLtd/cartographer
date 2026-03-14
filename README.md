# Cartographer

Codebase structure mapping MCP server — because your agent shouldn't have to re-read every file.

## Overview

Cartographer is an MCP server that builds and maintains an import graph of your codebase using Tree-sitter AST parsing. It stores file metadata and import edges in SQLite, then answers graph queries via recursive CTEs.

Designed to complement [Goldfish](https://github.com/RageLtd/Goldfish) — Goldfish remembers what you *did*, Cartographer maps what *exists*.

```
File path → Tree-sitter AST parse → Store imports/symbols in SQLite
                                              │
Agent writes/edits → git status diff → Re-parse changed files
                                              │
Agent queries → Walk import graph (recursive CTE) → Return relevant files
```

## Install

Add the marketplace and install:

```
/plugin marketplace add RageLtd/claude-plugins
/plugin install cartographer@rageltd
```

The plugin automatically downloads the binary on first session start.

## Languages

- TypeScript / JavaScript / TSX / JSX (via `tree-sitter-typescript`, `tree-sitter-javascript`)
- Rust (via `tree-sitter-rust`)
- Ruby (via `tree-sitter-ruby`)
- Elixir (via `tree-sitter-elixir`)

Adding a new language: add a tree-sitter grammar dependency, an entry to `LANGUAGE_CONFIG` in `src/constants.rs`, and an extractor in `src/parser/`.

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
cartographer                # Start MCP server (stdio transport)
cartographer hook:context    # SessionStart hook
cartographer hook:prompt     # UserPromptSubmit hook
cartographer hook:pre-read   # PreToolUse Read hook
cartographer hook:pre-edit   # PreToolUse Edit/Write hook
cartographer hook:post-edit  # PostToolUse Edit/Write/Bash hook
cartographer hook:post-compact # PostCompact hook
```

## What It Stores

A minimal import map in SQLite (`~/.cartographer/map.db`) — no embeddings, no vectors:

- **files** table: `(project, file_path, language, symbols, symbol_names, content_hash)`
- **imports** table: `(project, source_path, target_path, specifier, symbols)`
- **git_state** table: `(project, last_status)` — for change detection

Graph queries use recursive CTEs. File search uses FTS5.

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
- **Database:** SQLite via rusqlite (bundled, no system deps)
- **Tree-sitter:** Grammars compiled into the binary (no runtime loading)
