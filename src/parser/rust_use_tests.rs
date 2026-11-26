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
