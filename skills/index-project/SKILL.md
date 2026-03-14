# Index Project

Index the current codebase to build its import graph.

## When to Use
- At the start of a session when you need to understand codebase structure
- After significant file changes (new files, moved files, changed imports)
- When asked about project dependencies or architecture
- When `cartographer_query` returns empty results (project may not be indexed)

## How to Use

1. Call `cartographer_index_project` with the current project root path
2. Optionally call `cartographer_stats` to show indexing results

## Example

```
Step 1: Index the project
> cartographer_index_project({ "project": "/path/to/project" })

Step 2: Show stats (optional)
> cartographer_stats({ "project": "/path/to/project" })
```

## Notes
- Indexing is incremental — only changed files are re-parsed (content hash check)
- Supported languages: TypeScript, JavaScript, TSX, JSX, Rust, Ruby, Elixir
- External/bare imports (npm packages, crates, gems) are tracked but not resolved
- The index persists in `~/.cartographer/map.db` across sessions
- For updating after small changes, prefer `cartographer_detect_changes` instead
