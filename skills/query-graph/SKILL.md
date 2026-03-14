# Query Import Graph

Walk the import graph from entry point files to find dependencies and dependents.

## When to Use
- To understand what a file depends on (its imports, transitive dependencies)
- To find what files would be affected by changing a given file (dependents)
- To trace import chains and understand module boundaries
- When planning refactors that involve moving or renaming files
- **Before using Grep or Glob** for structural/dependency questions

## How to Use

1. Ensure the project is indexed (use `cartographer_index_project` if needed)
2. Call `cartographer_query` with:
   - `entry_points`: Array of file paths or search terms
   - `max_depth`: How many levels to traverse (default: 3)
   - `max_results`: Limit results (default: 50)

Entry points can be absolute paths OR search terms — the tool will resolve search terms via FTS.

## Examples

```
Find what src/server.rs depends on:
> cartographer_query({ "project": "/path/to/project", "entry_points": ["src/server.rs"] })

Find what would be affected by changing types.rs:
> cartographer_query({ "project": "/path/to/project", "entry_points": ["types.rs"] })

Search by symbol name:
> cartographer_query({ "project": "/path/to/project", "entry_points": ["CartographerServer"] })
```

## Interpreting Results

Each result includes:
- **path**: Relative path from project root
- **reason**: Why this file was included (e.g., "imports src/types.rs")
- **depth**: How many import hops from the query file
- **symbols**: Functions, types, and exports defined in the file

Use dependency results to understand what a file needs. Use dependent results to assess the blast radius of changes.
