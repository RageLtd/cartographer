---
name: find-uses
description: Find every place a symbol is used across the project, by file and line, with Cartographer. Use before renaming, changing or removing an exported function, constant, class or type, or when asked where something is used. TypeScript and JavaScript only.
---

# Find Uses of a Symbol

Find every place a symbol is used across the project, by file and line.

## When to Use
- Before renaming, changing or removing an exported function, constant, class or type
- When asked "where is this used?" or "what calls this?"
- To check whether an export is still used anywhere
- Instead of a text search for a name, when comments, strings and unrelated symbols of the same name would get in the way

## How to Use

Call `cartographer_find_uses` with:
- `project`: The project root path
- `file_path`: The file that defines the symbol
- `symbol`: The symbol's name as that file exports it. Use `default` for a default export

If you know the name but not the file, find the file first with `cartographer_search`.

## Example

```
> cartographer_find_uses({
    "project": "/path/to/project",
    "file_path": "/path/to/project/src/skills/shared.ts",
    "symbol": "SHARED_SKILLS"
  })
```

## Results

- **symbol**, **definedIn**: What was asked
- **totalFiles**, **totalLines**: How many files use it, and on how many lines
- **files**: Each file with the **lines** where it names the symbol. The defining file is included

Two other answers are possible, and they mean different things:
- "No symbol named … is defined in …" — the file has no such symbol. The answer lists the symbols it does define
- A result whose only file is the defining file — the symbol exists and nothing else uses it

## What It Follows
- Renamed imports: `import { a as b }` counts uses of `b`
- Namespace imports: `import * as ns` counts `ns.a`
- Imports by workspace package name, such as `@scope/core/util`
- Re-exports: a file that re-exports the symbol passes it on, under the same name or a new one, and the files importing from that file are searched too

## Limits
- TypeScript and JavaScript only. A file in another language is refused
- A use is matched by name inside a file that imports the symbol. If that file also declares a local variable of the same name, its uses are counted too
- A symbol reached through a dynamic `import()` is not followed
- Which files import which comes from the index, so run `cartographer_detect_changes` first if files have changed. The lines come from the files as they are on disk
- Needs the index. In parse-only mode use `cartographer_uses_in_file`, which answers for one file at a time
