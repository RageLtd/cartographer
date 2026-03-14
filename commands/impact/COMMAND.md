---
name: carto:impact
description: Analyze the blast radius of changing a file
arguments:
  - name: file
    description: File path or search term to analyze impact for
    required: true
---

Analyze the blast radius of changing the specified file.

1. If the argument looks like a relative path, resolve it against the current working directory
2. Call `cartographer_get_file_info` to get direct dependents and imports
3. Call `cartographer_query` with the file as an entry point and `max_depth: 5` for transitive dependents
4. Check for dependency cycles with `cartographer_find_cycles`
5. Present a risk assessment:
   - **Direct dependents**: Files that directly import this file
   - **Transitive impact**: Files indirectly affected
   - **Cycles**: Any circular dependencies involving this file
   - **Risk level**: Low (0-2 dependents), Medium (3-5), High (6+), Critical (10+ transitive)
6. List the public symbols defined in the file that dependents rely on
