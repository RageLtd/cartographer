---
name: carto:stats
description: Show index statistics for the current project
---

Show Cartographer index statistics for this project.

1. Call `cartographer_stats` with the current working directory as the project
2. Present the results:
   - Total files indexed
   - Total import edges
   - Language breakdown (file counts per language)
3. If no files are indexed, suggest running `/carto:index`
