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
