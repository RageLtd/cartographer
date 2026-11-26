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

// Cargo auto-discovered targets are crate roots: their children live beside
// them, not under a directory named after the file.

#[test]
fn mod_in_integration_test_resolves_to_sibling_directory_module() {
    let fixture = write_files(&[
        CARGO_TOML,
        ("tests/it.rs", "mod common;\n"),
        ("tests/common/mod.rs", "pub fn setup() {}\n"),
    ]);

    assert_single_edge_to(&imports_of(&fixture, "tests/it.rs"), "tests/common/mod.rs");
}

#[test]
fn mod_in_integration_test_resolves_to_sibling_file() {
    let fixture = write_files(&[
        CARGO_TOML,
        ("tests/it.rs", "mod common;\n"),
        ("tests/common.rs", "pub fn setup() {}\n"),
    ]);

    assert_single_edge_to(&imports_of(&fixture, "tests/it.rs"), "tests/common.rs");
}

#[test]
fn mod_in_bin_target_resolves_to_sibling_file() {
    let fixture = write_files(&[
        CARGO_TOML,
        ("src/bin/tool.rs", "mod helper;\n"),
        ("src/bin/helper.rs", "pub fn help() {}\n"),
    ]);

    assert_single_edge_to(
        &imports_of(&fixture, "src/bin/tool.rs"),
        "src/bin/helper.rs",
    );
}

#[test]
fn mod_in_example_resolves_to_sibling_file() {
    let fixture = write_files(&[
        CARGO_TOML,
        ("examples/demo.rs", "mod shared;\n"),
        ("examples/shared.rs", "pub fn show() {}\n"),
    ]);

    assert_single_edge_to(
        &imports_of(&fixture, "examples/demo.rs"),
        "examples/shared.rs",
    );
}

#[test]
fn mod_in_bench_resolves_to_sibling_file() {
    let fixture = write_files(&[
        CARGO_TOML,
        ("benches/speed.rs", "mod shared;\n"),
        ("benches/shared.rs", "pub fn measure() {}\n"),
    ]);

    assert_single_edge_to(
        &imports_of(&fixture, "benches/speed.rs"),
        "benches/shared.rs",
    );
}

#[test]
fn mod_in_build_script_resolves_to_sibling_file() {
    let fixture = write_files(&[
        CARGO_TOML,
        ("build.rs", "mod gen;\n"),
        ("gen.rs", "pub fn generate() {}\n"),
    ]);

    // The full path, since a bare `gen.rs` suffix would also match `build/gen.rs`.
    let expected = fixture.root.join("gen.rs");
    assert_single_edge_to(
        &imports_of(&fixture, "build.rs"),
        expected.to_str().expect("utf-8 path"),
    );
}

// A child is looked for in exactly one directory; a same-named file anywhere
// else never produces an edge.

#[test]
fn crate_root_child_beside_it_wins_over_stem_directory() {
    let fixture = write_files(&[
        CARGO_TOML,
        ("tests/it.rs", "mod common;\n"),
        ("tests/common.rs", "pub fn setup() {}\n"),
        ("tests/it/common.rs", "pub fn other() {}\n"),
    ]);

    assert_single_edge_to(&imports_of(&fixture, "tests/it.rs"), "tests/common.rs");
}

#[test]
fn mod_in_module_file_ignores_sibling_of_the_file() {
    let fixture = write_files(&[
        CARGO_TOML,
        ("src/lib.rs", "mod server;\nmod x;\n"),
        ("src/server.rs", "mod x;\n"),
        ("src/x.rs", "pub fn x() {}\n"),
    ]);

    let edges = imports_of(&fixture, "src/server.rs");
    assert!(edges.is_empty(), "expected no edges, got {edges:?}");
}

#[test]
fn mod_in_module_file_stem_directory_wins_over_sibling() {
    let fixture = write_files(&[
        CARGO_TOML,
        ("src/lib.rs", "mod server;\nmod x;\n"),
        ("src/server.rs", "mod x;\n"),
        ("src/server/x.rs", "pub fn inner() {}\n"),
        ("src/x.rs", "pub fn outer() {}\n"),
    ]);

    assert_single_edge_to(&imports_of(&fixture, "src/server.rs"), "src/server/x.rs");
}

#[test]
fn file_nested_below_tests_is_a_module_file_using_its_stem_directory() {
    let fixture = write_files(&[
        CARGO_TOML,
        ("tests/sub/helper.rs", "mod deep;\n"),
        ("tests/sub/helper/deep.rs", "pub fn deep() {}\n"),
    ]);

    assert_single_edge_to(
        &imports_of(&fixture, "tests/sub/helper.rs"),
        "tests/sub/helper/deep.rs",
    );
}

#[test]
fn file_nested_below_tests_ignores_sibling_of_the_file() {
    let fixture = write_files(&[
        CARGO_TOML,
        ("tests/sub/helper.rs", "mod deep;\n"),
        ("tests/sub/deep.rs", "pub fn deep() {}\n"),
    ]);

    let edges = imports_of(&fixture, "tests/sub/helper.rs");
    assert!(edges.is_empty(), "expected no edges, got {edges:?}");
}

#[test]
fn missing_child_of_module_file_has_no_edge() {
    let fixture = write_files(&[
        CARGO_TOML,
        ("src/lib.rs", "mod server;\n"),
        ("src/server.rs", "mod missing;\n"),
    ]);

    let edges = imports_of(&fixture, "src/server.rs");
    assert!(edges.is_empty(), "expected no edges, got {edges:?}");
}

// `#[path = "P"]` on a `mod` item names its file outright: `P` is resolved
// against the declaring file's own directory, whatever kind of file that is.

/// One edge to exactly `fixture.root/rel`, naming `module` as plain `mod`
/// edges do.
fn assert_single_path_edge(fixture: &Fixture, edges: &[ImportEdge], rel: &str, module: &str) {
    let expected = fixture.root.join(rel);
    assert_single_edge_to(edges, expected.to_str().expect("utf-8 path"));
    assert_eq!(edges[0].symbols, vec![module.to_string()]);
}

#[test]
fn path_attribute_in_module_file_resolves_beside_the_file() {
    let fixture = write_files(&[
        CARGO_TOML,
        ("src/lib.rs", "mod a;\n"),
        ("src/a.rs", "#[path = \"a_tests.rs\"]\nmod tests;\n"),
        ("src/a_tests.rs", "fn t() {}\n"),
    ]);

    let edges = imports_of(&fixture, "src/a.rs");
    assert_single_path_edge(&fixture, &edges, "src/a_tests.rs", "tests");
}

#[test]
fn path_attribute_resolves_with_other_attributes_around_it() {
    for source in [
        "#[cfg(test)]\n#[path = \"a_tests.rs\"]\nmod tests;\n",
        "#[path = \"a_tests.rs\"]\n#[cfg(test)]\nmod tests;\n",
    ] {
        let fixture = write_files(&[
            CARGO_TOML,
            ("src/lib.rs", "mod a;\n"),
            ("src/a.rs", source),
            ("src/a_tests.rs", "fn t() {}\n"),
        ]);

        let edges = imports_of(&fixture, "src/a.rs");
        assert_single_path_edge(&fixture, &edges, "src/a_tests.rs", "tests");
    }
}

#[test]
fn path_attribute_with_subdirectory_resolves_from_lib_rs() {
    let fixture = write_files(&[
        CARGO_TOML,
        (
            "src/lib.rs",
            "#[path = \"support/helpers.rs\"]\nmod helpers;\n",
        ),
        ("src/support/helpers.rs", "pub fn help() {}\n"),
    ]);

    let edges = imports_of(&fixture, "src/lib.rs");
    assert_single_path_edge(&fixture, &edges, "src/support/helpers.rs", "helpers");
}

#[test]
fn path_attribute_climbing_out_of_mod_rs_reports_normalised_target() {
    let fixture = write_files(&[
        CARGO_TOML,
        ("src/lib.rs", "mod net;\n"),
        (
            "src/net/mod.rs",
            "#[path = \"../shared/util.rs\"]\nmod util;\n",
        ),
        ("src/shared/util.rs", "pub fn util() {}\n"),
    ]);

    let edges = imports_of(&fixture, "src/net/mod.rs");
    assert_single_path_edge(&fixture, &edges, "src/shared/util.rs", "util");
    assert!(!edges[0].target.contains(".."), "{}", edges[0].target);
}

#[test]
fn path_attribute_to_missing_file_has_no_edge_even_if_name_lookup_would_hit() {
    let fixture = write_files(&[
        CARGO_TOML,
        ("src/lib.rs", "mod a;\n"),
        ("src/a.rs", "#[path = \"nope.rs\"]\nmod tests;\n"),
        ("src/a/tests.rs", "fn t() {}\n"),
    ]);

    let edges = imports_of(&fixture, "src/a.rs");
    assert!(edges.is_empty(), "expected no edges, got {edges:?}");
}

#[test]
fn path_attribute_applies_only_to_its_own_mod_item() {
    let fixture = write_files(&[
        CARGO_TOML,
        (
            "src/lib.rs",
            "#[path = \"first_impl.rs\"]\nmod first;\nmod second;\n",
        ),
        ("src/first_impl.rs", "pub fn one() {}\n"),
        ("src/second.rs", "pub fn two() {}\n"),
    ]);

    let mut edges = imports_of(&fixture, "src/lib.rs");
    edges.sort_by(|a, b| a.target.cmp(&b.target));
    let first = fixture.root.join("src/first_impl.rs");
    let second = fixture.root.join("src/second.rs");
    let got: Vec<(&str, &[String])> = edges
        .iter()
        .map(|e| (e.target.as_str(), e.symbols.as_slice()))
        .collect();
    assert_eq!(
        got,
        [
            (first.to_str().expect("utf-8"), &["first".to_string()][..]),
            (second.to_str().expect("utf-8"), &["second".to_string()][..]),
        ]
    );
}

#[path = "rust_use_tests.rs"]
mod use_decls;
