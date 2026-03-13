# Index Project

Index the current codebase to build its import graph.

## When to Use
- At the start of a session when you need to understand codebase structure
- After significant file changes (new files, moved files, changed imports)
- When asked about project dependencies or architecture

## How to Use

1. Call the `cartographer_index_project` MCP tool with the current project root path
2. Optionally call `cartographer_stats` to show indexing results (file count, import edges, languages detected)

## Example

```
Step 1: Index the project
→ cartographer_index_project({ "path": "/path/to/project" })

Step 2: Show stats (optional)
→ cartographer_stats()
```

## Notes
- Indexing is incremental — only changed files are re-parsed
- Supported languages: TypeScript, JavaScript, Rust (TSX/JSX included)
- External/bare imports (npm packages, external crates) are tracked but not resolved
- The index persists in `~/.cartographer/map.db` across sessions
