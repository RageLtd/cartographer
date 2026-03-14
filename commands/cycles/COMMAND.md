---
name: carto:cycles
description: Find circular dependencies in the import graph
---

Detect circular dependencies in this project.

1. Call `cartographer_find_cycles` with the current working directory as the project
2. If cycles are found, present each one showing:
   - The cycle chain (A → B → C → A)
   - The cycle length
   - A brief assessment of severity
3. If no cycles found, report that the dependency graph is acyclic
