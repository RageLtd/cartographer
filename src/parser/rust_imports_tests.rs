//! End-to-end tests for Rust import resolution: each builds a small crate on
//! disk and checks the import edges `parse_file` returns for one of its files.

use std::fs;
use std::path::PathBuf;

use tempfile::TempDir;

use super::parse_file;
use super::registry::LanguageRegistry;
use crate::types::ImportEdge;

const CARGO_TOML: (&str, &str) = ("Cargo.toml", "[package]\nname = \"x\"\n");

/// A crate written to a temp dir. `root` is canonicalised so paths built from
/// it match what the resolver reports on macOS, where the temp dir is a symlink.
struct Fixture {
    _dir: TempDir,
    root: PathBuf,
}

/// Writes each `(relative path, contents)` pair under a fresh temp dir,
/// creating parent directories as needed.
fn write_files(files: &[(&str, &str)]) -> Fixture {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().canonicalize().expect("canonical temp dir");
    for (rel, contents) in files {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent dirs");
        }
        fs::write(&path, contents).expect("write fixture file");
    }
    Fixture { _dir: dir, root }
}

/// Parses `rel` inside the fixture and returns its import edges.
fn imports_of(fixture: &Fixture, rel: &str) -> Vec<ImportEdge> {
    let path = fixture.root.join(rel);
    parse_file(
        &LanguageRegistry::builtin(),
        path.to_str().expect("utf-8 path"),
        None,
    )
    .expect("rust file parses")
    .imports
}

fn assert_single_edge_to(edges: &[ImportEdge], target_suffix: &str) {
    assert_eq!(edges.len(), 1, "expected one edge, got {edges:?}");
    assert!(
        edges[0].target.ends_with(target_suffix),
        "expected target ending in {target_suffix}, got {}",
        edges[0].target
    );
}

#[test]
fn mod_in_lib_rs_resolves_to_sibling_file() {
    let fixture = write_files(&[
        CARGO_TOML,
        ("src/lib.rs", "mod util;\n"),
        ("src/util.rs", "pub fn helper() {}\n"),
    ]);

    assert_single_edge_to(&imports_of(&fixture, "src/lib.rs"), "src/util.rs");
}

#[test]
fn mod_in_lib_rs_resolves_to_sibling_directory_module() {
    let fixture = write_files(&[
        CARGO_TOML,
        ("src/lib.rs", "mod util;\n"),
        ("src/util/mod.rs", "pub fn helper() {}\n"),
    ]);

    assert_single_edge_to(&imports_of(&fixture, "src/lib.rs"), "src/util/mod.rs");
}

#[test]
fn mod_in_mod_rs_resolves_to_sibling_file() {
    let fixture = write_files(&[
        CARGO_TOML,
        ("src/lib.rs", "mod net;\n"),
        ("src/net/mod.rs", "mod tcp;\n"),
        ("src/net/tcp.rs", "pub fn connect() {}\n"),
    ]);

    assert_single_edge_to(&imports_of(&fixture, "src/net/mod.rs"), "src/net/tcp.rs");
}

#[test]
fn mod_in_non_mod_rs_file_resolves_to_file_under_stem_directory() {
    let fixture = write_files(&[
        CARGO_TOML,
        ("src/lib.rs", "mod server;\n"),
        ("src/server.rs", "mod tools;\n"),
        ("src/server/tools.rs", "pub fn tool() {}\n"),
    ]);

    assert_single_edge_to(
        &imports_of(&fixture, "src/server.rs"),
        "src/server/tools.rs",
    );
}

#[test]
fn mod_in_non_mod_rs_file_resolves_to_directory_module_under_stem_directory() {
    let fixture = write_files(&[
        CARGO_TOML,
        ("src/lib.rs", "mod server;\n"),
        ("src/server.rs", "mod tools;\n"),
        ("src/server/tools/mod.rs", "pub fn tool() {}\n"),
    ]);

    assert_single_edge_to(
        &imports_of(&fixture, "src/server.rs"),
        "src/server/tools/mod.rs",
    );
}

#[test]
fn inline_mod_has_no_edge() {
    let fixture = write_files(&[CARGO_TOML, ("src/lib.rs", "mod inner { fn f() {} }\n")]);

    let edges = imports_of(&fixture, "src/lib.rs");
    assert!(edges.is_empty(), "expected no edges, got {edges:?}");
}

#[test]
fn mod_with_no_file_on_disk_has_no_edge() {
    let fixture = write_files(&[CARGO_TOML, ("src/lib.rs", "mod missing;\n")]);

    let edges = imports_of(&fixture, "src/lib.rs");
    assert!(edges.is_empty(), "expected no edges, got {edges:?}");
}
