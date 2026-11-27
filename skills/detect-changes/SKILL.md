# Detect Changes

Bring the import graph in line with the files on disk, and see what changed.

## When to Use
- After editing, creating, renaming, or deleting source files during a session
- After a pull, a checkout, or discarding edits
- When the SessionStart hook reports "Run `cartographer_detect_changes` if files have changed"
- Before querying the graph if you suspect the index is stale

## How to Use

Call `cartographer_detect_changes` with:
- `project`: The project root path

The tool walks the project, re-parses every file that is new or whose content differs from the index, and removes files that are no longer on disk.

## Example

```
> cartographer_detect_changes({ "project": "/path/to/project" })
```

## Results

- **indexed**: Number of files re-parsed
- **removed**: Number of files removed from the index
- **modified**: Paths of the files re-parsed
- **deleted**: Paths of the files removed
- If no changes: "No changes detected."

## Notes
- Does the same work as `cartographer_index_project`; the difference is that it lists what changed
- Compares file content, not git status, so the project does not have to be a git repository
- Unavailable when the server runs with `--parse-only`, because there is no index to compare with
