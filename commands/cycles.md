---
description: Find circular dependencies in the import graph
---

Detect circular dependencies in this project.

1. Call `cartographer_find_cycles` with the current working directory as the project
2. If groups are found, present each one showing:
   - The files in the group and its size
   - A brief assessment of severity
3. Each group is a set of files that can all reach one another by following imports. The tool does not list the individual cycles inside a group; if asked how two of the files are connected, call `cartographer_get_file_info` on one and follow its imports within the group
4. If no groups are found, report that the dependency graph is acyclic
