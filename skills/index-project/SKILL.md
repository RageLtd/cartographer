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
- Indexing is incremental — only changed files are re-parsed (content hash check), and files no longer on disk are removed from the index
- Supported languages: TypeScript, JavaScript, TSX, JSX, Rust, Ruby, Elixir
- Imports of installed packages (npm packages, crates, gems) produce no edge. Imports of another package in the same JavaScript or TypeScript workspace, by package name, do
- The index persists in `~/.cartographer/cartographer.db` across sessions
- `cartographer_detect_changes` does the same work and lists which files changed
