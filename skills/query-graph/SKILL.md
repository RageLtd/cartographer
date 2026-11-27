---
name: query-graph
description: Walk Cartographer's import graph from one or more files to find what they depend on and what depends on them. Use for dependency and structure questions before reaching for Grep or Glob, and when planning to move or rename files.
---

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
   - `max_depth`: How many levels to traverse (default: 2)
   - `max_results`: Limit results (default: 20)

Entry points can be absolute paths OR search terms. An entry point that is not an absolute path is searched for in the index, and the walk starts from the files that match.

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
- **reason**: Why this file was included: `entry` (a file the walk started from), `dependency` (reached by following an import) or `dependent` (reached because it imports a file already found)
- **depth**: How many import hops from the query file
- **symbols**: Functions, types, and exports defined in the file

Use dependency results to understand what a file needs. Use dependent results to assess the blast radius of changes.
