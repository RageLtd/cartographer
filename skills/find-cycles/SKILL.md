# Find Circular Dependencies

Find the groups of files that depend on one another in a cycle.

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

Each entry is one group:
- **files**: The files in the group, sorted by path. Every file in a group can reach every other by following imports, so any two of them are on a cycle together.
- **size**: Number of files in the group

If there are none: "No circular dependencies found."

A file that imports into a group without being imported back is not in the group, and neither is a file the group imports one way.

## Interpreting Results
- The tool reports groups, not individual cycles: one tangled group can contain millions of distinct cycles
- To see how two files in a group are connected, call `cartographer_get_file_info` on one and follow its imports within the group
- A group of two is a pair of files that import each other; they often belong in one file, or share something worth extracting
- A large group marks a part of the codebase where no file can be understood or changed alone
- A Rust `mod child;` declaration is not counted as a dependency, so a child module that uses its parent is not reported for that alone. A parent that also uses the child, while the child uses the parent, is a real two-way dependency and is reported
- Some groups are intentional (e.g., mutual type references) — use judgment
