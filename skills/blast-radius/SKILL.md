# Blast Radius Analysis

Assess the impact of changing a file by combining dependency analysis with file detail.

## When to Use
- Before making changes to a widely-imported file
- When planning refactors to understand downstream effects
- To answer "what would break if I change X?"
- During code review to evaluate risk

## How to Use

This is a composite workflow using multiple Cartographer tools:

1. **Get file detail** to see direct imports and dependents:
   ```
   > cartographer_get_file_info({ "project": "...", "file_path": "..." })
   ```

2. **Query dependents** for transitive impact (files that indirectly depend on this file):
   ```
   > cartographer_query({ "project": "...", "entry_points": ["<file>"], "max_depth": 5 })
   ```

3. **Assess risk** based on:
   - Number of direct dependents (high fan-in = high risk)
   - Depth of transitive dependency chain
   - Whether public symbols are being changed
   - Whether the file is part of a dependency cycle

## Example

To assess blast radius of changing `src/types.rs`:

```
Step 1: Direct connections
> cartographer_get_file_info({ "project": "/path/to/project", "file_path": "/path/to/project/src/types.rs" })

Step 2: Transitive dependents
> cartographer_query({ "project": "/path/to/project", "entry_points": ["src/types.rs"], "max_depth": 5 })

Step 3: Check for cycles involving this file
> cartographer_find_cycles({ "project": "/path/to/project" })
```

## Risk Levels
- **Low**: 0-2 dependents, no public API changes
- **Medium**: 3-5 dependents, or public signature changes
- **High**: 6+ dependents, or part of a dependency cycle
- **Critical**: Core type file with 10+ transitive dependents
