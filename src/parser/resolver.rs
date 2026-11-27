use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::rust_modules::{child_module_dir, child_module_file};

const TS_JS_EXTENSIONS: &[&str] = &[".ts", ".tsx", ".js", ".jsx", ".mjs"];

/// TypeScript is imported by the name of the JavaScript it compiles to, so a
/// specifier ending in the first extension may mean a source file ending in
/// one of the others. The order is the one the TypeScript compiler tries.
const TS_SOURCES_OF_JS_EXTENSION: &[(&str, &[&str])] =
    &[(".js", &[".ts", ".tsx"]), (".jsx", &[".tsx"])];

pub fn is_relative_import(specifier: &str) -> bool {
    specifier.starts_with("./") || specifier.starts_with("../")
}

pub fn resolve_ts_js_import(specifier: &str, from_file: &str) -> Option<String> {
    if !is_relative_import(specifier) {
        return None;
    }

    resolve_ts_js_path(&Path::new(from_file).parent()?.join(specifier))
}

/// The TypeScript or JavaScript file `path` names, as written, with an
/// extension added, or as a directory holding an index file.
pub(super) fn resolve_ts_js_path(path: &Path) -> Option<String> {
    let candidate = normalize_path(path);

    // The compiler looks for the TypeScript source before the file as written,
    // so compiled output sitting beside its source never wins.
    if let Some(source) = typescript_source_of(&candidate) {
        return source.to_str().map(str::to_string);
    }

    // Exact match with known extension
    for ext in TS_JS_EXTENSIONS {
        if candidate.to_str().is_some_and(|s| s.ends_with(ext)) && candidate.exists() {
            return candidate.to_str().map(|s| s.to_string());
        }
    }

    // Try appending extensions
    for ext in TS_JS_EXTENSIONS {
        let with_ext = append_ext(&candidate, ext);
        if with_ext.exists() {
            return with_ext.to_str().map(|s| s.to_string());
        }
    }

    // Try as directory with index file
    for ext in TS_JS_EXTENSIONS {
        let index = candidate.join(format!("index{ext}"));
        if index.exists() {
            return index.to_str().map(|s| s.to_string());
        }
    }

    None
}

pub(super) fn typescript_source_of(candidate: &Path) -> Option<PathBuf> {
    let written = candidate.to_str()?;
    TS_SOURCES_OF_JS_EXTENSION
        .iter()
        .find_map(|(js_ext, ts_exts)| {
            let stem = written.strip_suffix(js_ext)?;
            ts_exts
                .iter()
                .map(|ts_ext| PathBuf::from(format!("{stem}{ts_ext}")))
                .find(|source| source.is_file())
        })
}

pub fn resolve_rust_mod_decl(mod_name: &str, from_file: &str) -> Option<String> {
    let dir = child_module_dir(Path::new(from_file))?;
    child_module_file(&dir, mod_name)?
        .to_str()
        .map(str::to_string)
}

/// Resolve the `P` of a `#[path = "P"]` on a file-level `mod` item, which is
/// relative to the declaring file's directory whatever kind of file it is.
pub fn resolve_rust_path_attr(path: &str, from_file: &str) -> Option<String> {
    let candidate = normalize_path(&Path::new(from_file).parent()?.join(path));
    if !candidate.is_file() {
        return None;
    }
    candidate.to_str().map(str::to_string)
}

/// Resolve a Ruby require or require_relative to a file path.
pub fn resolve_ruby_require(specifier: &str, from_file: &str, is_relative: bool) -> Option<String> {
    if is_relative {
        let dir = Path::new(from_file).parent()?;
        let candidate = dir.join(specifier);
        let candidate = normalize_path(&candidate);

        // Try exact match
        if candidate.exists() {
            return candidate.to_str().map(|s| s.to_string());
        }
        // Try with .rb extension
        let with_rb = append_ext(&candidate, ".rb");
        if with_rb.exists() {
            return with_rb.to_str().map(|s| s.to_string());
        }
    }
    // For bare `require`, we don't resolve (external gems)
    None
}

/// Resolve an Elixir module name to a file path.
/// Converts `MyApp.Foo.Bar` to `lib/my_app/foo/bar.ex` relative to the project root.
pub fn resolve_elixir_module(module_name: &str, from_file: &str) -> Option<String> {
    // Find the project root (directory containing mix.exs)
    let mut dir = Path::new(from_file).parent()?;
    let project_root;
    loop {
        if dir.join("mix.exs").exists() {
            project_root = dir;
            break;
        }
        dir = dir.parent()?;
    }

    // Convert module name to path: MyApp.Foo.Bar -> my_app/foo/bar
    let path_part: String = module_name
        .split('.')
        .map(to_snake_case)
        .collect::<Vec<_>>()
        .join("/");

    // Try lib/ directory
    let as_file = project_root.join("lib").join(format!("{path_part}.ex"));
    if as_file.exists() {
        return as_file.to_str().map(|s| s.to_string());
    }

    // Try without the top-level app name prefix
    if let Some(rest) = path_part.split_once('/') {
        let as_file = project_root.join("lib").join(format!("{}.ex", rest.1));
        if as_file.exists() {
            return as_file.to_str().map(|s| s.to_string());
        }
    }

    None
}

/// Convert a PascalCase string to snake_case.
fn to_snake_case(s: &str) -> String {
    s.chars()
        .enumerate()
        .flat_map(|(i, ch)| {
            let prefix = if ch.is_uppercase() && i > 0 {
                Some('_')
            } else {
                None
            };
            prefix.into_iter().chain(ch.to_lowercase())
        })
        .collect()
}

fn append_ext(path: &Path, ext: &str) -> PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(ext);
    PathBuf::from(s)
}

/// Normalize a path by resolving `.` and `..` components without requiring the path to exist.
pub(super) fn normalize_path(path: &Path) -> PathBuf {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                components.pop();
            }
            std::path::Component::CurDir => {}
            other => components.push(other),
        }
    }
    components.iter().collect()
}

// ============================================================================
// Config-driven resolution for host-provided grammars
// ============================================================================

/// How many directories above the importing file a `module-path` strategy
/// without a `root_marker` will search for a module root.
const MODULE_ROOT_SEARCH_DEPTH: usize = 10;

/// A declarative description of how a language's import specifiers map to files.
/// Grammar manifests list one or more; they are tried in order.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResolverStrategy {
    /// `./foo` / `../bar` specifiers resolved against the importing file's
    /// directory, trying each extension and then `<index_name><ext>` inside a
    /// directory (the JavaScript / Ruby shape).
    RelativePath {
        extensions: Vec<String>,
        #[serde(default)]
        index_names: Vec<String>,
    },
    /// `a.b.c` / `A::B` specifiers split on `separator` and walked from a
    /// module root (the Rust / Python / Elixir shape). The root is the nearest
    /// ancestor containing `root_marker` when given, otherwise each ancestor of
    /// the importing file is tried. `source_dirs` (e.g. `["src"]`) sit between
    /// the root and the module path; an empty list means the root itself.
    ModulePath {
        separator: String,
        extensions: Vec<String>,
        #[serde(default)]
        root_marker: Option<String>,
        #[serde(default)]
        source_dirs: Vec<String>,
        #[serde(default)]
        index_names: Vec<String>,
        #[serde(default)]
        strip_prefixes: Vec<String>,
    },
}

/// A resolved import. `trailing_item` is set when the final path segment was
/// not a module but an item inside the resolved file (`crate::auth::check`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub path: String,
    pub trailing_item: Option<String>,
}

pub fn resolve_with_strategies(
    strategies: &[ResolverStrategy],
    specifier: &str,
    from_file: &str,
) -> Option<Resolved> {
    strategies
        .iter()
        .find_map(|strategy| resolve_with_strategy(strategy, specifier, from_file))
}

fn resolve_with_strategy(
    strategy: &ResolverStrategy,
    specifier: &str,
    from_file: &str,
) -> Option<Resolved> {
    match strategy {
        ResolverStrategy::RelativePath {
            extensions,
            index_names,
        } => {
            if !is_relative_import(specifier) {
                return None;
            }
            let dir = Path::new(from_file).parent()?;
            let base = normalize_path(&dir.join(specifier));
            first_existing(&base, extensions, index_names).map(|path| Resolved {
                path,
                trailing_item: None,
            })
        }
        ResolverStrategy::ModulePath {
            separator,
            extensions,
            root_marker,
            source_dirs,
            index_names,
            strip_prefixes,
        } => {
            let stripped = strip_prefixes
                .iter()
                .find_map(|prefix| specifier.strip_prefix(prefix.as_str()))
                .unwrap_or(specifier);
            let segments: Vec<&str> = stripped
                .split(separator.as_str())
                .filter(|s| !s.is_empty())
                .collect();
            if segments.is_empty() {
                return None;
            }

            let from_dir = Path::new(from_file).parent()?;
            let roots: Vec<PathBuf> = match root_marker {
                Some(marker) => vec![find_ancestor_containing(from_dir, marker)?],
                None => from_dir
                    .ancestors()
                    .take(MODULE_ROOT_SEARCH_DEPTH)
                    .map(Path::to_path_buf)
                    .collect(),
            };
            let source_dirs: Vec<&str> = if source_dirs.is_empty() {
                vec![""]
            } else {
                source_dirs.iter().map(String::as_str).collect()
            };

            for root in &roots {
                for source_dir in &source_dirs {
                    let base = root.join(source_dir);
                    let full = segments.iter().fold(base.clone(), |p, seg| p.join(seg));
                    if let Some(path) = first_existing(&full, extensions, index_names) {
                        return Some(Resolved {
                            path,
                            trailing_item: None,
                        });
                    }
                    // `a::b::item` — the last segment may be an item inside module `a::b`.
                    if let Some((item, module_segments)) = segments.split_last() {
                        if module_segments.is_empty() {
                            continue;
                        }
                        let module = module_segments.iter().fold(base, |p, seg| p.join(seg));
                        if let Some(path) = first_existing(&module, extensions, index_names) {
                            return Some(Resolved {
                                path,
                                trailing_item: Some(item.to_string()),
                            });
                        }
                    }
                }
            }
            None
        }
    }
}

fn find_ancestor_containing(start: &Path, marker: &str) -> Option<PathBuf> {
    start
        .ancestors()
        .find(|dir| dir.join(marker).exists())
        .map(Path::to_path_buf)
}

/// First existing file for `base`: as-is when it already carries one of the
/// extensions, then `base<ext>`, then `base/<index><ext>`.
fn first_existing(base: &Path, extensions: &[String], index_names: &[String]) -> Option<String> {
    let base_str = base.to_str()?;
    if extensions
        .iter()
        .any(|ext| base_str.ends_with(ext.as_str()))
        && base.is_file()
    {
        return Some(base_str.to_string());
    }
    for ext in extensions {
        let candidate = append_ext(base, ext);
        if candidate.is_file() {
            return candidate.to_str().map(str::to_string);
        }
    }
    for index in index_names {
        for ext in extensions {
            let candidate = base.join(format!("{index}{ext}"));
            if candidate.is_file() {
                return candidate.to_str().map(str::to_string);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_resolve_ts_import_with_extension() {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("utils.ts"), "export const x = 1;").unwrap();
        let from_file = src.join("index.ts");
        fs::write(&from_file, "").unwrap();

        let result = resolve_ts_js_import("./utils.ts", from_file.to_str().unwrap());
        assert!(result.is_some());
        assert!(result.unwrap().ends_with("utils.ts"));
    }

    #[test]
    fn test_resolve_ts_import_without_extension() {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("utils.ts"), "export const x = 1;").unwrap();
        let from_file = src.join("index.ts");
        fs::write(&from_file, "").unwrap();

        let result = resolve_ts_js_import("./utils", from_file.to_str().unwrap());
        assert!(result.is_some());
        assert!(result.unwrap().ends_with("utils.ts"));
    }

    #[test]
    fn test_resolve_ts_import_index_file() {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("src");
        let components = src.join("components");
        fs::create_dir_all(&components).unwrap();
        fs::write(components.join("index.ts"), "export {};").unwrap();
        let from_file = src.join("index.ts");
        fs::write(&from_file, "").unwrap();

        let result = resolve_ts_js_import("./components", from_file.to_str().unwrap());
        assert!(result.is_some());
        assert!(result.unwrap().ends_with("components/index.ts"));
    }

    #[test]
    fn test_bare_import_returns_none() {
        let result = resolve_ts_js_import("lodash", "/some/file.ts");
        assert!(result.is_none());
    }

    /// Resolves `specifier` from `src/index.ts` in a directory holding `files`,
    /// returning the target relative to `src/`.
    fn resolve_among(files: &[&str], specifier: &str) -> Option<String> {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("src");
        fs::create_dir_all(&src).unwrap();
        for file in files {
            fs::write(src.join(file), "").unwrap();
        }
        let from_file = src.join("index.ts");
        fs::write(&from_file, "").unwrap();

        let resolved = resolve_ts_js_import(specifier, from_file.to_str().unwrap())?;
        let relative = Path::new(&resolved).strip_prefix(&src).unwrap();
        Some(relative.to_str().unwrap().to_string())
    }

    #[test]
    fn test_js_specifier_resolves_to_typescript_source() {
        assert_eq!(
            resolve_among(&["utils.ts"], "./utils.js").as_deref(),
            Some("utils.ts")
        );
        assert_eq!(
            resolve_among(&["view.tsx"], "./view.js").as_deref(),
            Some("view.tsx")
        );
        assert_eq!(
            resolve_among(&["Button.tsx"], "./Button.jsx").as_deref(),
            Some("Button.tsx")
        );
    }

    #[test]
    fn test_js_specifier_prefers_typescript_source_over_compiled_output() {
        assert_eq!(
            resolve_among(&["utils.ts", "utils.js"], "./utils.js").as_deref(),
            Some("utils.ts")
        );
    }

    #[test]
    fn test_js_specifier_without_typescript_source_resolves_as_written() {
        assert_eq!(
            resolve_among(&["utils.js"], "./utils.js").as_deref(),
            Some("utils.js")
        );
        assert_eq!(resolve_among(&[], "./utils.js"), None);
        // `.mjs` has no TypeScript source this resolver knows of.
        assert_eq!(resolve_among(&["utils.ts"], "./utils.mjs"), None);
    }

    #[test]
    fn test_resolve_rust_mod_decl() {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("db.rs"), "pub fn query() {}").unwrap();

        let result = resolve_rust_mod_decl("db", src.join("main.rs").to_str().unwrap());
        assert!(result.is_some());
        assert!(result.unwrap().ends_with("db.rs"));
    }
}
