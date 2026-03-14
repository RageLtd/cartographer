# Detect Changes

Incrementally update the import graph after file modifications.

## When to Use
- After editing, creating, or deleting source files during a session
- When the SessionStart hook reports "Run `cartographer_detect_changes` if files have changed"
- As a lightweight alternative to full re-indexing
- Before querying the graph if you suspect the index is stale

## How to Use

Call `cartographer_detect_changes` with:
- `project`: The project root path

The tool compares current git status against the last stored snapshot, re-parses modified files, and removes deleted ones.

## Example

```
> cartographer_detect_changes({ "project": "/path/to/project" })
```

## Results

- **indexed**: Number of files re-parsed
- **removed**: Number of files removed from the index
- **modified**: List of modified file paths
- **deleted**: List of deleted file paths
- If no changes: "No changes detected."

## Notes
- Much faster than `cartographer_index_project` for small changes
- Uses git status diff — only works in git repositories
- Automatically saves the new git snapshot after processing
