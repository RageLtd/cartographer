# Find Circular Dependencies

Detect circular dependency chains in the import graph.

## When to Use
- During architecture reviews to identify problematic coupling
- When investigating strange build issues or initialization order problems
- Before refactoring to understand existing dependency tangles
- As part of codebase health checks

## How to Use

Call `cartographer_find_cycles` with:
- `project`: The project root path

## Example

```
> cartographer_find_cycles({ "project": "/path/to/project" })
```

## Results

Each cycle includes:
- **cycle**: Ordered list of file paths forming the loop (last element repeats the first)
- **length**: Number of files in the cycle

If no cycles: "No circular dependencies found."

## Interpreting Results
- Short cycles (2-3 files) often indicate tightly coupled modules that should be merged or have a shared dependency extracted
- Long cycles may indicate architectural boundary violations
- Some cycles are intentional (e.g., mutual type references) — use judgment
