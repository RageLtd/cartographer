# Query Import Graph

Query the import graph to find dependencies and dependents of files.

## When to Use
- To understand what a file depends on (its imports, transitive dependencies)
- To find what files would be affected by changing a given file (dependents)
- To trace import chains and understand module boundaries
- When planning refactors that involve moving or renaming files

## How to Use

1. Ensure the project is indexed first (use `cartographer_index_project` if needed)
2. Call `cartographer_query` with:
   - `file_path`: The file to query from
   - `direction`: `"dependencies"` (what it imports) or `"dependents"` (what imports it)
   - `depth`: How many levels to traverse (default: 3)

## Example

```
Find what src/server.rs depends on:
→ cartographer_query({ "file_path": "src/server.rs", "direction": "dependencies", "depth": 3 })

Find what would be affected by changing src/types.rs:
→ cartographer_query({ "file_path": "src/types.rs", "direction": "dependents", "depth": 3 })
```

## Other Useful Tools

- `cartographer_search`: Full-text search across indexed file paths and symbol names
- `cartographer_stats`: Show index statistics (file counts, edge counts)
- `cartographer_get_file_info`: Get detailed info about a specific file (symbols, imports)

## Interpreting Results

Each result includes:
- **file_path**: Relative path from project root
- **depth**: How many import hops from the query file
- **symbols**: Functions, types, and exports defined in the file

Use dependency results to understand what a file needs. Use dependent results to assess the blast radius of changes.
