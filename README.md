# Cartographer

Codebase structure mapping MCP server — because your agent shouldn't have to re-read every file.

## Overview

Cartographer is an MCP server that builds and maintains an import graph of your codebase using Tree-sitter AST parsing. When the agent encounters a file, Cartographer parses its AST to understand what it is and what it imports. When the agent modifies files, Cartographer diffs git status to detect changes and re-parses them.

Designed to complement [Goldfish](https://github.com/RageLtd/Goldfish) — Goldfish remembers what you *did*, Cartographer maps what *exists*.

```
User mentions file ──► Tree-sitter AST parse ──► Store imports/exports in SQLite
                                                         │
Agent writes/edits ──► git status diff ──► Re-parse changed files
                                                         │
Agent queries ──► Walk import graph (recursive CTE) ──► Return relevant files
```

## Languages

- TypeScript / JavaScript / TSX / JSX (via `tree-sitter-typescript`, `tree-sitter-javascript`)
- Rust (via `tree-sitter-rust`)

Adding a new language: install its tree-sitter grammar package and add an entry to `LANGUAGE_CONFIG` in `src/constants.ts`.

## MCP Interface

### Tools

| Tool | Purpose |
|------|---------|
| `cartographer_parse_file` | Parse a file's AST, extract imports/exports, store in DB |
| `cartographer_detect_changes` | Diff git status since last snapshot, re-parse changed files |
| `cartographer_query` | Walk the import graph from entry points, return relevant files |
| `cartographer_index_project` | Full index of all supported files in a project |
| `cartographer_stats` | File count, import edges, languages breakdown |

### Resources

| Resource | Description |
|----------|-------------|
| `cartographer://project` | List all indexed projects and file counts |

## What It Stores

A minimal import map in SQLite — no embeddings, no vectors:

- **files** table: `(project, file_path, language, exports[], content_hash)`
- **imports** table: `(project, source_path, target_path, specifier, symbols[])`
- **git_state** table: `(project, last_status)` — for change detection

Graph queries use recursive CTEs. File search uses FTS5.

## Installation

```bash
bun install
```

### MCP Configuration (stdio)

```json
{
  "mcpServers": {
    "cartographer": {
      "command": "bun",
      "args": ["run", "/path/to/cartographer/src/index.ts"]
    }
  }
}
```

### HTTP Transport

```bash
TRANSPORT=http PORT=3457 bun src/index.ts
```

## Agent Workflow

1. **First session**: Agent calls `cartographer_index_project` to do a full scan
2. **User mentions a file**: Agent calls `cartographer_parse_file` to understand it
3. **Agent writes/edits/runs bash**: Agent calls `cartographer_detect_changes` to update the index
4. **Agent needs context**: Agent calls `cartographer_query` with file paths or search terms to get the dependency neighborhood

## Goldfish Integration (planned)

When both are running, the agent can cross-reference:
- Cartographer: "auth.ts imports from session.ts and types.ts"
- Goldfish: "you refactored auth.ts and session.ts together 3 times this week"
- Combined: session.ts gets boosted relevance even for queries that only mention auth

## Requirements

- **Runtime:** Bun
- **Database:** SQLite with FTS5 (built-in with Bun)
- **Tree-sitter:** Native bindings (works with Bun's runtime, no compile step needed)
