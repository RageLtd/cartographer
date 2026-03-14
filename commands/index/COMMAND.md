---
name: carto:index
description: Index the current project to build its import graph
---

Index this project's codebase using Cartographer.

1. Call `cartographer_index_project` with the current working directory as the project path
2. Call `cartographer_stats` with the same project path
3. Report the results: files indexed, import edges found, languages detected
