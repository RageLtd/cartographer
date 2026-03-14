---
name: carto:deps
description: Show what a file depends on (its imports)
arguments:
  - name: file
    description: File path or search term to look up dependencies for
    required: true
---

Show the dependencies of the specified file.

1. If the argument looks like a relative path, resolve it against the current working directory
2. Call `cartographer_get_file_info` to get direct imports
3. Call `cartographer_query` with the file as an entry point and `max_depth: 3` for transitive dependencies
4. Present results organized by depth level:
   - Direct imports (depth 1)
   - Transitive dependencies (depth 2+)
5. Include symbol counts for each dependency
