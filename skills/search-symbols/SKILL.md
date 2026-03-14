# Search Files and Symbols

Full-text search across indexed file paths and symbol names.

## When to Use
- To find where a function, type, struct, or class is defined
- To locate files by partial path name
- When you know a symbol name but not which file it's in
- As a faster alternative to Grep for structural lookups (searches the index, not file contents)

## How to Use

Call `cartographer_search` with:
- `project`: The project root path
- `query`: Search term (file path fragment or symbol name)
- `limit`: Max results (default: 10)

## Examples

```
Find files containing "Server" in path or symbols:
> cartographer_search({ "project": "/path/to/project", "query": "Server" })

Find where "ImportEdge" is defined:
> cartographer_search({ "project": "/path/to/project", "query": "ImportEdge" })

Find parser-related files:
> cartographer_search({ "project": "/path/to/project", "query": "parser" })
```

## Results

Each result includes:
- **file_path**: Absolute path
- **relative_path**: Path relative to project root
- **symbols**: All symbols defined in the file (with kind, visibility, doc comments)

## When to Use Grep Instead
- Searching for string literals, error messages, or comments in file *contents*
- Pattern matching with regex
- Search needs to include non-indexed files (e.g., config files, markdown)
