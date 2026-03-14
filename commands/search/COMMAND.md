---
name: carto:search
description: Search indexed files by path or symbol name
arguments:
  - name: query
    description: Search term (file path fragment or symbol name)
    required: true
---

Search the Cartographer index for files matching the query.

1. Call `cartographer_search` with the current working directory as the project and the provided query
2. Present results as a list showing:
   - File path (relative to project root)
   - Key symbols defined in each file (name, kind, visibility)
3. If no results found, suggest the user run `/carto:index` first
