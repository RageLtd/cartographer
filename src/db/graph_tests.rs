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

// The limits arrive from a tool call as signed numbers. A negative one must
// not turn into a walk of the whole graph.
#[test]
fn a_negative_depth_or_result_limit_walks_no_further_than_the_entry() {
    let (db, _dir) = store();
    let (a, _b, _c, _d) = chain(&db);

    let negative_depth =
        walk_import_graph(&db, PROJECT, std::slice::from_ref(&a), Some(-1), Some(50)).unwrap();
    let negative_results =
        walk_import_graph(&db, PROJECT, std::slice::from_ref(&a), Some(5), Some(-1)).unwrap();

    assert_eq!(walked(&negative_depth), vec![row(&a, 0, "entry")]);
    assert_eq!(walked(&negative_results), vec![row(&a, 0, "entry")]);
}

#[test]
fn a_negative_search_limit_finds_nothing() {
    let (db, _dir) = store();
    search_fixture(&db);

    let found = search_files(&db, PROJECT, "server", -1).unwrap();

    assert_eq!(paths(&found), Vec::<String>::new());
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
