//! `use crate::…`, `use self::…` and `use super::…` declarations: a path is
//! walked one module at a time from its starting module, and the first segment
//! that is not a module is an item of the file reached so far.

use super::{imports_of, write_files, Fixture, CARGO_TOML};
use crate::types::ImportEdge;

/// The two places Rust keeps the `net` module's file; its children are in
/// `src/net/` either way.
const NET_LAYOUTS: [&str; 2] = ["src/net/mod.rs", "src/net.rs"];
/// Stands for the `net` module's file in expected targets.
const NET: &str = "{net}";
const FILE_UNDER_TEST: &str = "src/a.rs";

/// `(target relative to the fixture root, symbols)`, sorted.
type Edges = Vec<(String, Vec<String>)>;

/// The edges `use` declarations produce; `mod` items also produce edges and
/// carry a `mod <name>` specifier.
fn use_edges(fixture: &Fixture, rel: &str) -> Vec<ImportEdge> {
    imports_of(fixture, rel)
        .into_iter()
        .filter(|e| !e.specifier.starts_with("mod "))
        .collect()
}

fn relative(fixture: &Fixture, edges: &[ImportEdge]) -> Edges {
    let mut got: Edges = edges
        .iter()
        .map(|e| {
            let target = std::path::Path::new(&e.target)
                .strip_prefix(&fixture.root)
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|_| e.target.clone());
            (target, e.symbols.clone())
        })
        .collect();
    got.sort();
    got
}

/// A crate with `types`, `net` (file at `net_file`), `net::tcp`, `net::udp` and
/// `a`, where `source` is prepended to the file `under_test`.
fn net_crate(net_file: &str, under_test: &str, source: &str) -> Fixture {
    let mut files: Vec<(&str, String)> = vec![
        CARGO_TOML,
        ("src/lib.rs", "mod types;\nmod net;\nmod a;\n"),
        (
            "src/types.rs",
            "pub struct Thing;\npub struct Other;\npub enum Kind { Variant }\n",
        ),
        (net_file, "mod tcp;\nmod udp;\npub struct Shared;\n"),
        ("src/net/tcp.rs", "pub struct Conn;\n"),
        ("src/net/udp.rs", "pub struct Socket;\n"),
        (FILE_UNDER_TEST, "pub fn f() {}\n"),
    ]
    .into_iter()
    .map(|(path, contents)| (path, contents.to_string()))
    .collect();
    let entry = files
        .iter_mut()
        .find(|(path, _)| *path == under_test)
        .expect("file under test is part of the crate");
    entry.1.insert_str(0, source);
    let borrowed: Vec<(&str, &str)> = files.iter().map(|(p, c)| (*p, c.as_str())).collect();
    write_files(&borrowed)
}

/// `(file under test, source, expected edges)`; `NET` in a file or target
/// stands for the `net` module's file.
type Case<'a> = (&'a str, &'a str, &'a [(&'a str, &'a [&'a str])]);

/// Runs every case against both `net` layouts and reports all mismatches at once.
fn check_cases(cases: &[Case]) {
    let mut failures = Vec::new();
    for net_file in NET_LAYOUTS {
        for (file, source, expected) in cases {
            let file = file.replace(NET, net_file);
            let fixture = net_crate(net_file, &file, source);
            let got = relative(&fixture, &use_edges(&fixture, &file));
            let mut want: Edges = expected
                .iter()
                .map(|(target, symbols)| {
                    let symbols = symbols.iter().map(|s| s.to_string()).collect();
                    (target.replace(NET, net_file), symbols)
                })
                .collect();
            want.sort();
            if got != want {
                failures.push(format!(
                    "[{net_file}] {file}: {source:?}\n  want {want:?}\n  got  {got:?}"
                ));
            }
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

#[test]
fn use_path_walks_modules_to_the_file_holding_the_item() {
    check_cases(&[
        (
            FILE_UNDER_TEST,
            "use crate::types;\n",
            &[("src/types.rs", &["types"])],
        ),
        (
            FILE_UNDER_TEST,
            "use crate::types::Thing;\n",
            &[("src/types.rs", &["Thing"])],
        ),
        (FILE_UNDER_TEST, "use crate::net;\n", &[(NET, &["net"])]),
        (
            FILE_UNDER_TEST,
            "use crate::net::tcp::Conn;\n",
            &[("src/net/tcp.rs", &["Conn"])],
        ),
        (
            FILE_UNDER_TEST,
            "use crate::types::Kind::Variant;\n",
            &[("src/types.rs", &["Kind"])],
        ),
        (
            FILE_UNDER_TEST,
            "use crate::Thing;\n",
            &[("src/lib.rs", &["Thing"])],
        ),
    ]);
}

#[test]
fn first_segment_that_is_not_a_module_is_an_item_of_the_crate_root() {
    check_cases(&[(
        FILE_UNDER_TEST,
        "use crate::missing::Thing;\n",
        &[("src/lib.rs", &["missing"])],
    )]);
}

#[test]
fn self_and_super_start_at_the_current_and_parent_module() {
    const TCP: &str = "src/net/tcp.rs";
    check_cases(&[
        (NET, "use self::tcp::Conn;\n", &[(TCP, &["Conn"])]),
        (
            TCP,
            "use super::udp::Socket;\n",
            &[("src/net/udp.rs", &["Socket"])],
        ),
        (TCP, "use super::Shared;\n", &[(NET, &["Shared"])]),
        (
            TCP,
            "use super::super::types::Thing;\n",
            &[("src/types.rs", &["Thing"])],
        ),
    ]);
}

#[test]
fn use_trees_record_each_imported_name() {
    check_cases(&[
        (
            FILE_UNDER_TEST,
            "use crate::types::{Thing, Other};\n",
            &[("src/types.rs", &["Thing", "Other"])],
        ),
        (
            FILE_UNDER_TEST,
            "use crate::types::{Thing as T, Other};\n",
            &[("src/types.rs", &["Thing", "Other"])],
        ),
        (
            FILE_UNDER_TEST,
            "use crate::types::*;\n",
            &[("src/types.rs", &["*"])],
        ),
        (
            FILE_UNDER_TEST,
            "use crate::net::{self, tcp::Conn};\n",
            &[(NET, &["net"]), ("src/net/tcp.rs", &["Conn"])],
        ),
        (
            FILE_UNDER_TEST,
            "use crate::{types::Thing, net::tcp::Conn};\n",
            &[("src/types.rs", &["Thing"]), ("src/net/tcp.rs", &["Conn"])],
        ),
        (
            FILE_UNDER_TEST,
            "pub use crate::types::Thing;\n",
            &[("src/types.rs", &["Thing"])],
        ),
        (
            FILE_UNDER_TEST,
            "pub(crate) use crate::types::Thing;\n",
            &[("src/types.rs", &["Thing"])],
        ),
    ]);
}

#[test]
fn external_crate_paths_have_no_edge() {
    check_cases(&[
        (FILE_UNDER_TEST, "use std::fmt;\n", &[]),
        (FILE_UNDER_TEST, "use tokio::spawn;\n", &[]),
        (FILE_UNDER_TEST, "use ::serde::Serialize;\n", &[]),
    ]);
}

fn specifiers(fixture: &Fixture, rel: &str) -> Vec<String> {
    let mut got: Vec<String> = use_edges(fixture, rel)
        .into_iter()
        .map(|e| e.specifier)
        .collect();
    got.sort();
    got
}

#[test]
fn every_edge_of_one_declaration_carries_its_path_as_written() {
    let source = "pub(crate) use crate::{types::Thing, net::tcp::Conn};\n";
    let fixture = net_crate(NET_LAYOUTS[0], FILE_UNDER_TEST, source);

    let spec = "crate::{types::Thing, net::tcp::Conn}";
    assert_eq!(specifiers(&fixture, FILE_UNDER_TEST), [spec, spec]);
}

#[test]
fn separate_declarations_into_one_file_give_separate_edges() {
    let source = "use crate::types::Thing;\nuse crate::types::Other;\n";
    let fixture = net_crate(NET_LAYOUTS[0], FILE_UNDER_TEST, source);

    assert_eq!(
        specifiers(&fixture, FILE_UNDER_TEST),
        ["crate::types::Other", "crate::types::Thing"]
    );
}

#[test]
fn crate_root_is_main_rs_without_lib_rs() {
    let fixture = write_files(&[
        CARGO_TOML,
        ("src/main.rs", "mod a;\nfn main() {}\n"),
        ("src/a.rs", "use crate::Thing;\n"),
    ]);

    let edges = use_edges(&fixture, "src/a.rs");
    assert_eq!(
        relative(&fixture, &edges),
        [("src/main.rs".to_string(), vec!["Thing".to_string()])]
    );
}

#[test]
fn use_of_the_crate_root_from_the_crate_root_has_no_edge() {
    let fixture = write_files(&[
        CARGO_TOML,
        ("src/lib.rs", "use crate::Thing;\npub struct Thing;\n"),
    ]);

    let edges = use_edges(&fixture, "src/lib.rs");
    assert!(edges.is_empty(), "expected no edges, got {edges:?}");
}

#[test]
fn crate_in_an_auto_discovered_target_starts_at_that_target() {
    for (root, child) in [
        ("tests/it.rs", "tests/common/mod.rs"),
        ("src/bin/tool.rs", "src/bin/common.rs"),
        ("examples/demo.rs", "examples/common.rs"),
    ] {
        // A `common` module in the library too, which the target must not reach.
        let fixture = write_files(&[
            CARGO_TOML,
            ("src/lib.rs", "mod common;\n"),
            ("src/common.rs", "pub fn setup() {}\n"),
            (root, "mod common;\nuse crate::common::setup;\n"),
            (child, "pub fn setup() {}\n"),
        ]);

        let edges = use_edges(&fixture, root);
        assert_eq!(
            relative(&fixture, &edges),
            [(child.to_string(), vec!["setup".to_string()])],
            "from {root}"
        );
    }
}

#[test]
fn use_without_a_cargo_toml_has_no_edge() {
    let fixture = write_files(&[
        ("src/lib.rs", "mod x;\n"),
        ("src/a.rs", "use crate::x::Y;\n"),
        ("src/x.rs", "pub struct Y;\n"),
    ]);

    let edges = use_edges(&fixture, "src/a.rs");
    assert!(edges.is_empty(), "expected no edges, got {edges:?}");
}

const LIB: &str = "src/lib.rs";
const MAIN: &str = "src/main.rs";
const TYPES: &str = "src/types.rs";
const CLI: &str = "src/cli.rs";
const CRATE_RUN: &str = "use crate::run;\n";
const SUPER_RUN: &str = "use super::run;\n";
const THING: &str = "use crate::types::Thing;\n";

/// The single edge `(target, symbol)` a case expects, or none.
type Expected<'a> = Option<(&'a str, &'a str)>;

fn single_edge(expected: Expected) -> Edges {
    expected
        .map(|(target, symbol)| (target.to_string(), vec![symbol.to_string()]))
        .into_iter()
        .collect()
}

/// `(src/lib.rs, src/main.rs, file under test, its source, expected edge)`.
type RootCase<'a> = (&'a str, &'a str, &'a str, &'a str, Expected<'a>);

/// Runs each case in a package with both `src/lib.rs` and `src/main.rs`, the
/// file under test written last, and reports all mismatches at once.
fn check_root_cases(cases: &[RootCase]) {
    let mut failures = Vec::new();
    for (lib, main, file, source, expected) in cases {
        let fixture = write_files(&[
            CARGO_TOML,
            (LIB, lib),
            (MAIN, main),
            (TYPES, "pub struct Thing;\n"),
            ("src/net/mod.rs", "mod tcp;\n"),
            (file, source),
        ]);
        let got = relative(&fixture, &use_edges(&fixture, file));
        let want = single_edge(*expected);
        if got != want {
            failures.push(format!(
                "lib.rs {lib:?}, main.rs {main:?}, {file}: {source:?}\n  want {want:?}\n  got  {got:?}"
            ));
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

#[test]
fn crate_root_is_the_root_file_that_declares_the_top_level_module() {
    const IN_MAIN: &str = "mod types;\nmod cli;\nmod net;\n";
    const IN_LIB: &str = "mod types;\nmod cli;\n";
    const TYPES_ONLY: &str = "mod types;\n";
    const PUB: &str = "pub mod cli;\n";
    const CFG: &str = "#[cfg(feature = \"x\")]\nmod cli;\n";
    const TCP: &str = "src/net/tcp.rs";
    check_root_cases(&[
        (TYPES_ONLY, IN_MAIN, CLI, CRATE_RUN, Some((MAIN, "run"))),
        (TYPES_ONLY, IN_MAIN, CLI, SUPER_RUN, Some((MAIN, "run"))),
        (IN_LIB, TYPES_ONLY, CLI, CRATE_RUN, Some((LIB, "run"))),
        (IN_LIB, TYPES_ONLY, CLI, SUPER_RUN, Some((LIB, "run"))),
        (TYPES_ONLY, IN_MAIN, TCP, CRATE_RUN, Some((MAIN, "run"))),
        (PUB, "", CLI, CRATE_RUN, Some((LIB, "run"))),
        ("", CFG, CLI, CRATE_RUN, Some((MAIN, "run"))),
    ]);
}

#[test]
fn crate_root_declared_by_both_or_neither_root_file_is_unknown() {
    const BOTH: &str = "mod types;\nmod cli;\n";
    const NEITHER: &str = "mod types;\n";
    check_root_cases(&[
        (BOTH, BOTH, CLI, CRATE_RUN, None),
        (BOTH, BOTH, CLI, SUPER_RUN, None),
        (NEITHER, NEITHER, CLI, CRATE_RUN, None),
        (NEITHER, NEITHER, CLI, SUPER_RUN, None),
        // A path through a module does not depend on which root it starts at.
        (BOTH, BOTH, CLI, THING, Some((TYPES, "Thing"))),
        (NEITHER, NEITHER, CLI, THING, Some((TYPES, "Thing"))),
    ]);
}

#[test]
fn each_root_file_is_its_own_crate_root_beside_the_other() {
    const USES: &str = "mod types;\nuse crate::types::Thing;\n";
    const TYPES_ONLY: &str = "mod types;\n";
    check_root_cases(&[
        (TYPES_ONLY, USES, MAIN, USES, Some((TYPES, "Thing"))),
        (USES, TYPES_ONLY, LIB, USES, Some((TYPES, "Thing"))),
    ]);
}

#[test]
fn directory_target_main_rs_is_the_crate_root_of_the_modules_beside_it() {
    const TARGET_ROOT: &str = "mod helper;\npub struct Top;\n";
    const HELPER: &str = "pub fn run() {}\n";
    const USE_RUN: &str = "use crate::helper::run;\n";
    let mut failures = Vec::new();
    for dir in ["src/bin/tool", "tests/suite", "examples/demo", "benches/b"] {
        let main = format!("{dir}/main.rs");
        let helper = format!("{dir}/helper.rs");
        for (file, prefix, source, expected) in [
            (&main, TARGET_ROOT, USE_RUN, (&helper, "run")),
            (&helper, HELPER, "use crate::Top;\n", (&main, "Top")),
            (&helper, HELPER, "use super::Top;\n", (&main, "Top")),
        ] {
            // The library's own `helper` and root, which the target must not reach.
            let fixture = write_files(&[
                CARGO_TOML,
                (LIB, TARGET_ROOT),
                ("src/helper.rs", HELPER),
                (&main, TARGET_ROOT),
                (&helper, HELPER),
                (file, &format!("{prefix}{source}")),
            ]);
            let got = relative(&fixture, &use_edges(&fixture, file));
            let want = single_edge(Some((expected.0.as_str(), expected.1)));
            if got != want {
                failures.push(format!(
                    "{file}: {source:?}\n  want {want:?}\n  got  {got:?}"
                ));
            }
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

#[test]
fn module_shared_by_several_flat_targets_has_no_crate_root() {
    for source in ["use crate::other::Thing;\n", "use super::Thing;\n"] {
        let fixture = write_files(&[
            CARGO_TOML,
            (LIB, "mod other;\npub struct Thing;\n"),
            ("src/other.rs", "pub struct Thing;\n"),
            ("tests/a.rs", "mod common;\nmod other;\npub struct Thing;\n"),
            ("tests/b.rs", "mod common;\n"),
            ("tests/other.rs", "pub struct Thing;\n"),
            ("tests/common/mod.rs", source),
        ]);

        let edges = use_edges(&fixture, "tests/common/mod.rs");
        assert!(edges.is_empty(), "{source:?}: want no edge, got {edges:?}");
    }
}
