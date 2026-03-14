# File Detail

Get comprehensive information about a specific file: its symbols, what it imports, and what imports it.

## When to Use
- To understand a file's API surface (exported symbols, signatures, doc comments)
- To see a file's direct dependencies and dependents at a glance
- Before modifying a file to understand its connections
- When reviewing code to check documentation coverage

## How to Use

Call `cartographer_get_file_info` with:
- `project`: The project root path
- `file_path`: Absolute path to the file

## Example

```
> cartographer_get_file_info({ "project": "/path/to/project", "file_path": "/path/to/project/src/server.rs" })
```

## Results

- **symbols**: All defined symbols with kind (function, struct, class, etc.), visibility (public/private), signatures, and doc comments
- **imports**: What this file imports, with which symbols from each target
- **dependents**: What other files import this file, with which symbols they use

## Complementary Tools
- Use `cartographer_query` for transitive (multi-hop) dependency analysis
- Use `cartographer_search` if you don't know the exact file path
