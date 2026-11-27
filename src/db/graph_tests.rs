use std::collections::HashSet;

use super::*;
use crate::db::client::Db;
use crate::db::fixtures_tests::*;
use crate::db::queries::{replace_imports, upsert_file};
use crate::types::{RelevantFile, Symbol};

/// The graph `d -> a -> b -> c` in `PROJECT`, every file carrying a symbol
/// named after it in upper case.
fn chain(db: &Db) -> (String, String, String, String) {
    let (a, b, c, d) = (p("src/a.rs"), p("src/b.rs"), p("src/c.rs"), p("src/d.rs"));
    add_file(db, PROJECT, &a, &["A"]);
    add_file(db, PROJECT, &b, &["B"]);
    add_file(db, PROJECT, &c, &["C"]);
    add_file(db, PROJECT, &d, &["D"]);
    replace_imports(db, PROJECT, &a, &[edge(&a, &b, "./b", &["B"])]).unwrap();
    replace_imports(db, PROJECT, &b, &[edge(&b, &c, "./c", &["C"])]).unwrap();
    replace_imports(db, PROJECT, &d, &[edge(&d, &a, "./a", &["A"])]).unwrap();
    (a, b, c, d)
}

/// (file, depth, reason) for each result, sorted.
fn walked(results: &[RelevantFile]) -> Vec<(String, i64, String)> {
    let mut rows: Vec<_> = results
        .iter()
        .map(|r| (r.file_path.clone(), r.depth, r.reason.clone()))
        .collect();
    rows.sort();
    rows
}

fn row(file: &str, depth: i64, reason: &str) -> (String, i64, String) {
    (file.to_string(), depth, reason.to_string())
}

fn paths(found: &[(String, Vec<Symbol>)]) -> Vec<String> {
    found.iter().map(|(f, _)| f.clone()).collect()
}

/// A cycle as reported, with its closing repeat checked and dropped, as a set.
fn cycle_members(cycle: &[String]) -> HashSet<String> {
    assert_eq!(
        cycle.first(),
        cycle.last(),
        "a cycle ends where it starts: {cycle:?}"
    );
    cycle[..cycle.len() - 1].iter().cloned().collect()
}

// ============================================================================
// walk_import_graph
// ============================================================================

#[test]
fn walk_returns_entry_dependencies_and_dependents() {
    let (db, _dir) = store();
    let (a, b, c, d) = chain(&db);

    let results =
        walk_import_graph(&db, PROJECT, std::slice::from_ref(&a), Some(5), Some(50)).unwrap();

    assert_eq!(
        walked(&results),
        vec![
            row(&a, 0, "entry"),
            row(&b, 1, "dependency"),
            row(&c, 2, "dependency"),
            row(&d, 1, "dependent"),
        ]
    );
    assert_eq!(results[0].file_path, a, "results are ordered by depth");
    for result in &results {
        let expected = result
            .file_path
            .trim_start_matches(&format!("{PROJECT}/"))
            .to_string();
        assert_eq!(result.relative_path, expected);
        let name = expected
            .trim_start_matches("src/")
            .trim_end_matches(".rs")
            .to_uppercase();
        assert_eq!(names(&result.symbols), vec![name]);
    }
}

#[test]
fn walk_honours_max_depth() {
    let (db, _dir) = store();
    let (a, b, _c, d) = chain(&db);

    let one_hop =
        walk_import_graph(&db, PROJECT, std::slice::from_ref(&a), Some(1), Some(50)).unwrap();
    assert_eq!(
        walked(&one_hop),
        vec![
            row(&a, 0, "entry"),
            row(&b, 1, "dependency"),
            row(&d, 1, "dependent"),
        ]
    );

    let no_hops =
        walk_import_graph(&db, PROJECT, std::slice::from_ref(&a), Some(0), Some(50)).unwrap();
    assert_eq!(walked(&no_hops), vec![row(&a, 0, "entry")]);
}

#[test]
fn walk_honours_max_results() {
    let (db, _dir) = store();
    let (a, _b, _c, _d) = chain(&db);

    let results =
        walk_import_graph(&db, PROJECT, std::slice::from_ref(&a), Some(5), Some(2)).unwrap();

    assert_eq!(results.len(), 2);
    assert_eq!(results[0].file_path, a);
    assert_eq!(results[0].reason, "entry");
}

#[test]
fn walk_from_no_entry_points_is_empty() {
    let (db, _dir) = store();
    chain(&db);

    let results = walk_import_graph(&db, PROJECT, &[], None, None).unwrap();

    assert!(results.is_empty());
}

#[test]
fn walk_stays_inside_the_project() {
    let (db, _dir) = store();
    let (a, b, _c, d) = chain(&db);
    let stranger = p("src/stranger.rs");
    add_file(&db, OTHER_PROJECT, &a, &[]);
    add_file(&db, OTHER_PROJECT, &stranger, &[]);
    replace_imports(
        &db,
        OTHER_PROJECT,
        &stranger,
        &[edge(&stranger, &a, "./a", &["A"])],
    )
    .unwrap();

    let results =
        walk_import_graph(&db, PROJECT, std::slice::from_ref(&a), Some(1), Some(50)).unwrap();

    assert_eq!(
        walked(&results),
        vec![
            row(&a, 0, "entry"),
            row(&b, 1, "dependency"),
            row(&d, 1, "dependent"),
        ]
    );
}

// ============================================================================
// get_file_detail
// ============================================================================

#[test]
fn file_detail_of_an_unknown_file_is_none() {
    let (db, _dir) = store();
    let a = p("src/a.rs");
    add_file(&db, OTHER_PROJECT, &a, &["A"]);

    assert!(get_file_detail(&db, PROJECT, &a).unwrap().is_none());
}

#[test]
fn file_detail_lists_language_symbols_imports_and_dependents() {
    let (db, _dir) = store();
    let (a, b, _c, d) = chain(&db);
    upsert_file(
        &db,
        PROJECT,
        &a,
        "typescript",
        &symbols(&["A", "helper"]),
        "h",
    )
    .unwrap();

    let detail = get_file_detail(&db, PROJECT, &a).unwrap().unwrap();

    assert_eq!(detail.file_path, a);
    assert_eq!(detail.language, "typescript");
    assert_eq!(names(&detail.symbols), strings(&["A", "helper"]));
    assert_eq!(detail.imports, vec![(b, strings(&["B"]))]);
    assert_eq!(detail.dependents, vec![(d, strings(&["A"]))]);
}

// ============================================================================
// search_files
// ============================================================================

fn search_fixture(db: &Db) {
    add_file(
        db,
        PROJECT,
        &p("src/db/graph.rs"),
        &["walk_import_graph", "find_cycles"],
    );
    add_file(
        db,
        PROJECT,
        &p("src/parser/mod.rs"),
        &["parse_file", "hash_file"],
    );
    add_file(db, PROJECT, &p("src/server.rs"), &["CartographerServer"]);
    add_file(
        db,
        OTHER_PROJECT,
        &p("src/db/graph.rs"),
        &["walk_import_graph"],
    );
}

#[test]
fn search_matches_file_paths() {
    let (db, _dir) = store();
    search_fixture(&db);

    let found = search_files(&db, PROJECT, "server", 10).unwrap();

    assert_eq!(paths(&found), vec![p("src/server.rs")]);
    assert_eq!(names(&found[0].1), strings(&["CartographerServer"]));
}

#[test]
fn search_matches_symbol_names_case_insensitively() {
    let (db, _dir) = store();
    search_fixture(&db);

    for query in [
        "cartographerserver",
        "CARTOGRAPHERSERVER",
        "CartographerServer",
    ] {
        let found = search_files(&db, PROJECT, query, 10).unwrap();
        assert_eq!(paths(&found), vec![p("src/server.rs")], "query {query:?}");
    }
    let found = search_files(&db, PROJECT, "Cycles", 10).unwrap();
    assert_eq!(paths(&found), vec![p("src/db/graph.rs")]);
}

#[test]
fn search_ranks_the_best_match_first() {
    let (db, _dir) = store();
    add_file(
        &db,
        PROJECT,
        &p("src/user.rs"),
        &["check_auth", "load_user"],
    );
    add_file(
        &db,
        PROJECT,
        &p("src/auth.rs"),
        &["auth_login", "auth_logout"],
    );

    let found = search_files(&db, PROJECT, "auth", 10).unwrap();

    assert_eq!(paths(&found), vec![p("src/auth.rs"), p("src/user.rs")]);
}

#[test]
fn search_is_scoped_to_the_project() {
    let (db, _dir) = store();
    search_fixture(&db);

    let found = search_files(&db, OTHER_PROJECT, "parse_file", 10).unwrap();
    assert!(found.is_empty(), "{found:?}");

    let found = search_files(&db, PROJECT, "walk_import_graph", 10).unwrap();
    assert_eq!(paths(&found), vec![p("src/db/graph.rs")]);
}

#[test]
fn search_honours_its_limit() {
    let (db, _dir) = store();
    for file in ["src/util/a.rs", "src/util/b.rs", "src/util/c.rs"] {
        add_file(&db, PROJECT, &p(file), &[]);
    }

    let found = search_files(&db, PROJECT, "util", 2).unwrap();

    assert_eq!(found.len(), 2);
}

#[test]
fn search_finds_path_and_identifier_queries_with_punctuation() {
    let (db, _dir) = store();
    search_fixture(&db);

    for (query, expected) in [
        ("src/db", "src/db/graph.rs"),
        ("parse_file", "src/parser/mod.rs"),
        ("graph.rs", "src/db/graph.rs"),
    ] {
        let found = search_files(&db, PROJECT, query, 10).unwrap();
        assert!(
            paths(&found).contains(&p(expected)),
            "query {query:?} found {found:?}"
        );
    }
}

#[test]
fn search_splits_words_where_letters_meet_digits() {
    let (db, _dir) = store();
    add_file(&db, PROJECT, &p("src/hash/sha256.rs"), &["digest"]);
    add_file(&db, PROJECT, &p("src/math.rs"), &["Vec3", "utf8_decode"]);
    add_file(&db, PROJECT, &p("src/plain.rs"), &["read_line"]);

    for (query, expected) in [
        ("sha", "src/hash/sha256.rs"),
        ("256", "src/hash/sha256.rs"),
        ("sha256", "src/hash/sha256.rs"),
        ("vec", "src/math.rs"),
        ("utf", "src/math.rs"),
    ] {
        let found = search_files(&db, PROJECT, query, 10).unwrap();
        assert_eq!(paths(&found), vec![p(expected)], "query {query:?}");
    }
}

#[test]
fn search_never_errors_on_punctuation() {
    let (db, _dir) = store();
    search_fixture(&db);

    for query in [
        "foo.rs", "\"", "'", "*", "foo\"bar", "(", ")", "-", "^", ":", "a:b", "NEAR", "AND",
        "OR x", "NOT", "{}", "[x]", "%", "_", "\\", ";", "", "   ",
    ] {
        let result = search_files(&db, PROJECT, query, 10);
        assert!(result.is_ok(), "query {query:?} failed: {result:?}");
    }
}

// ============================================================================
// find_cycles
// ============================================================================

#[test]
fn a_two_file_cycle_is_reported_once() {
    let (db, _dir) = store();
    let (a, b) = (p("a.rs"), p("b.rs"));
    replace_imports(&db, PROJECT, &a, &[edge(&a, &b, "./b", &[])]).unwrap();
    replace_imports(&db, PROJECT, &b, &[edge(&b, &a, "./a", &[])]).unwrap();

    let cycles = find_cycles(&db, PROJECT).unwrap();

    assert_eq!(cycles.len(), 1, "{cycles:?}");
    assert_eq!(cycle_members(&cycles[0]), HashSet::from([a, b]));
}

#[test]
fn a_three_file_cycle_is_reported_once() {
    let (db, _dir) = store();
    let (a, b, c) = (p("a.rs"), p("b.rs"), p("c.rs"));
    replace_imports(&db, PROJECT, &a, &[edge(&a, &b, "./b", &[])]).unwrap();
    replace_imports(&db, PROJECT, &b, &[edge(&b, &c, "./c", &[])]).unwrap();
    replace_imports(&db, PROJECT, &c, &[edge(&c, &a, "./a", &[])]).unwrap();

    let cycles = find_cycles(&db, PROJECT).unwrap();

    assert_eq!(cycles.len(), 1, "{cycles:?}");
    assert_eq!(cycles[0].len(), 4, "{cycles:?}");
    assert_eq!(cycle_members(&cycles[0]), HashSet::from([a, b, c]));
}

// A Rust parent must declare `mod child;` for the child to exist. That says
// where the child lives, not that the parent depends on it.
#[test]
fn a_module_declaration_and_the_childs_use_of_its_parent_are_not_a_cycle() {
    let (db, _dir) = store();
    let (parent, child) = (p("src/parser/mod.rs"), p("src/parser/tests.rs"));
    add_file(&db, PROJECT, &parent, &["parse_file"]);
    add_file(&db, PROJECT, &child, &[]);
    replace_imports(
        &db,
        PROJECT,
        &parent,
        &[module_edge(&parent, &child, "tests")],
    )
    .unwrap();
    replace_imports(
        &db,
        PROJECT,
        &child,
        &[edge(&child, &parent, "super::parse_file", &["parse_file"])],
    )
    .unwrap();

    let cycles = find_cycles(&db, PROJECT).unwrap();

    assert!(cycles.is_empty(), "{cycles:?}");
    // The declaration is still an edge everywhere else: removing or renaming
    // the child breaks the parent.
    let detail = get_file_detail(&db, PROJECT, &child).unwrap().unwrap();
    assert_eq!(
        detail.dependents,
        vec![(parent.clone(), strings(&["tests"]))]
    );
    let walk = walk_import_graph(
        &db,
        PROJECT,
        std::slice::from_ref(&parent),
        Some(1),
        Some(50),
    )
    .unwrap();
    assert_eq!(
        walked(&walk),
        vec![row(&parent, 0, "entry"), row(&child, 1, "dependency")]
    );
}

#[test]
fn a_parent_that_uses_its_child_which_uses_it_back_is_a_cycle() {
    let (db, _dir) = store();
    let (parent, child) = (p("src/parser/mod.rs"), p("src/parser/registry.rs"));
    replace_imports(
        &db,
        PROJECT,
        &parent,
        &[
            module_edge(&parent, &child, "registry"),
            edge(&parent, &child, "self::registry::Registry", &["Registry"]),
        ],
    )
    .unwrap();
    replace_imports(
        &db,
        PROJECT,
        &child,
        &[edge(&child, &parent, "super::parse_file", &["parse_file"])],
    )
    .unwrap();

    let cycles = find_cycles(&db, PROJECT).unwrap();

    assert_eq!(cycles.len(), 1, "{cycles:?}");
    assert_eq!(cycle_members(&cycles[0]), HashSet::from([parent, child]));
}

#[test]
fn an_acyclic_graph_reports_no_cycles() {
    let (db, _dir) = store();
    let (a, b, c) = (p("a.rs"), p("b.rs"), p("c.rs"));
    replace_imports(
        &db,
        PROJECT,
        &a,
        &[edge(&a, &b, "./b", &[]), edge(&a, &c, "./c", &[])],
    )
    .unwrap();
    replace_imports(&db, PROJECT, &b, &[edge(&b, &c, "./c", &[])]).unwrap();
    // The same files form a cycle in another project.
    replace_imports(&db, OTHER_PROJECT, &c, &[edge(&c, &a, "./a", &[])]).unwrap();
    replace_imports(&db, OTHER_PROJECT, &a, &[edge(&a, &c, "./c", &[])]).unwrap();

    let cycles = find_cycles(&db, PROJECT).unwrap();

    assert!(cycles.is_empty(), "{cycles:?}");
}
