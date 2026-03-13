use std::path::{Path, PathBuf};

const TS_JS_EXTENSIONS: &[&str] = &[".ts", ".tsx", ".js", ".jsx", ".mjs"];

pub fn is_relative_import(specifier: &str) -> bool {
    specifier.starts_with("./") || specifier.starts_with("../")
}

pub fn resolve_ts_js_import(specifier: &str, from_file: &str) -> Option<String> {
    if !is_relative_import(specifier) {
        return None;
    }

    let dir = Path::new(from_file).parent()?;
    let candidate = dir.join(specifier);
    let candidate = normalize_path(&candidate);

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

pub fn resolve_rust_module(module_path: &str, from_file: &str, crate_root: &str) -> Option<String> {
    let (segments_str, base_dir);

    if let Some(rest) = module_path.strip_prefix("crate::") {
        segments_str = rest;
        base_dir = PathBuf::from(crate_root);
    } else if let Some(rest) = module_path.strip_prefix("self::") {
        segments_str = rest;
        base_dir = Path::new(from_file).parent()?.to_path_buf();
    } else if let Some(rest) = module_path.strip_prefix("super::") {
        segments_str = rest;
        base_dir = Path::new(from_file).parent()?.parent()?.to_path_buf();
    } else {
        // External crate — skip
        return None;
    }

    let segments: Vec<&str> = segments_str.split("::").collect();
    if segments.is_empty() {
        return None;
    }

    let mut rel_path = base_dir;
    for seg in &segments {
        rel_path = rel_path.join(seg);
    }

    // Try as file: foo/bar.rs
    let as_file = rel_path.with_extension("rs");
    if as_file.exists() {
        return as_file.to_str().map(|s| s.to_string());
    }

    // Try as directory: foo/bar/mod.rs
    let as_mod = rel_path.join("mod.rs");
    if as_mod.exists() {
        return as_mod.to_str().map(|s| s.to_string());
    }

    None
}

pub fn resolve_rust_mod_decl(mod_name: &str, from_file: &str) -> Option<String> {
    let dir = Path::new(from_file).parent()?;

    let as_file = dir.join(format!("{mod_name}.rs"));
    if as_file.exists() {
        return as_file.to_str().map(|s| s.to_string());
    }

    let as_mod = dir.join(mod_name).join("mod.rs");
    if as_mod.exists() {
        return as_mod.to_str().map(|s| s.to_string());
    }

    None
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
    let mut result = String::new();
    for (i, ch) in s.chars().enumerate() {
        if ch.is_uppercase() && i > 0 {
            result.push('_');
        }
        result.push(ch.to_lowercase().next().unwrap_or(ch));
    }
    result
}

fn append_ext(path: &Path, ext: &str) -> PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(ext);
    PathBuf::from(s)
}

/// Normalize a path by resolving `.` and `..` components without requiring the path to exist.
fn normalize_path(path: &Path) -> PathBuf {
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

    #[test]
    fn test_resolve_rust_crate_module() {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("auth.rs"), "pub fn check() {}").unwrap();

        let result = resolve_rust_module(
            "crate::auth",
            src.join("main.rs").to_str().unwrap(),
            src.to_str().unwrap(),
        );
        assert!(result.is_some());
        assert!(result.unwrap().ends_with("auth.rs"));
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

    #[test]
    fn test_external_crate_returns_none() {
        let result = resolve_rust_module("tokio::runtime", "/src/main.rs", "/src");
        assert!(result.is_none());
    }
}
