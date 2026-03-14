---
name: carto:info
description: Get detailed information about a specific file
arguments:
  - name: file
    description: File path or search term
    required: true
---

Show detailed information about a file from the Cartographer index.

1. If the argument looks like a relative path, resolve it against the current working directory
2. If the file is not found directly, use `cartographer_search` to find matches
3. Call `cartographer_get_file_info` for the resolved file path
4. Present:
   - **Symbols**: All defined symbols with kind, visibility, signature, and doc comment status
   - **Imports**: What this file depends on (with imported symbols)
   - **Dependents**: What files import this one (with which symbols they use)
   - **Documentation coverage**: Count of documented vs undocumented symbols
