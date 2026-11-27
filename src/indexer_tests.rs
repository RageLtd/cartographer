//! Indexing a project on disk into a store file, then answering from it.

use std::collections::{BTreeSet, HashMap};

use crate::db::client::{open, Db};
use crate::db::graph::{find_cycles, get_file_detail, search_files, walk_import_graph};
use crate::db::queries::{get_file_count, get_import_count, get_language_counts};
use crate::project_fixture_tests::*;

/// An edge as (path relative to the root, sorted symbols).
type Edge = (String, Vec<String>);

#[derive(Debug, PartialEq)]
struct Detail {
    language: String,
    symbols: Vec<String>,
    imports: Vec<Edge>,
    dependents: Vec<Edge>,
}

/// What the store answers about the test project, with every path relative
/// to its root and every list in a fixed order.
#[derive(Debug, PartialEq)]
struct Answers {
    /// (file, depth, reason) of the walk from `SERVER`.
    walk: Vec<(String, i64, String)>,
    /// The files a search for `formatPath` finds, best first.
    search: Vec<String>,
    server_detail: Option<Detail>,
    /// Each cycle's members.
    cycles: Vec<BTreeSet<String>>,
    file_count: i64,
    import_count: i64,
    languages: HashMap<String, usize>,
}

fn relative(project: &TestProject, path: &str) -> String {
    path.strip_prefix(&format!("{}/", project.root))
        .unwrap_or_else(|| panic!("{path} is outside the project"))
        .to_string()
}

fn relative_edges(project: &TestProject, edges: Vec<(String, Vec<String>)>) -> Vec<Edge> {
    let mut edges: Vec<Edge> = edges
        .into_iter()
        .map(|(path, mut symbols)| {
            symbols.sort();
            (relative(project, &path), symbols)
        })
        .collect();
    edges.sort();
    edges
}

fn answers(db: &Db, project: &TestProject) -> Answers {
    let key = project.root.as_str();

    let mut walk: Vec<(String, i64, String)> = walk_import_graph(
        db,
        key,
        std::slice::from_ref(&project.path(SERVER)),
        Some(5),
        Some(50),
    )
    .unwrap()
    .into_iter()
    .map(|f| (f.relative_path, f.depth, f.reason))
    .collect();
    walk.sort();

    let search = search_files(db, key, "formatPath", 10)
        .unwrap()
        .into_iter()
        .map(|(path, _)| relative(project, &path))
        .collect();

    let server_detail = get_file_detail(db, key, &project.path(SERVER))
        .unwrap()
        .map(|d| Detail {
            language: d.language,
            symbols: d.symbols.into_iter().map(|s| s.name).collect(),
            imports: relative_edges(project, d.imports),
            dependents: relative_edges(project, d.dependents),
        });

    let mut cycles: Vec<BTreeSet<String>> = find_cycles(db, key)
        .unwrap()
        .into_iter()
        .map(|cycle| {
            assert_eq!(cycle.first(), cycle.last(), "a cycle ends where it starts");
            cycle[..cycle.len() - 1]
                .iter()
                .map(|path| relative(project, path))
                .collect()
        })
        .collect();
    cycles.sort();

    Answers {
        walk,
        search,
        server_detail,
        cycles,
        file_count: get_file_count(db, key).unwrap(),
        import_count: get_import_count(db, key).unwrap(),
        languages: get_language_counts(db, key).unwrap(),
    }
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

fn edge(file: &str, symbols: &[&str]) -> Edge {
    (file.to_string(), strings(symbols))
}

fn step(file: &str, depth: i64, reason: &str) -> (String, i64, String) {
    (file.to_string(), depth, reason.to_string())
}

fn expected_answers() -> Answers {
    Answers {
        walk: vec![
            step(MAIN, 1, "dependent"),
            step(ROUTER, 1, "dependency"),
            step(SERVER, 0, "entry"),
            step(UTIL, 2, "dependency"),
        ],
        search: strings(&[UTIL]),
        server_detail: Some(Detail {
            language: "typescript".to_string(),
            symbols: strings(&["startServer"]),
            imports: vec![edge(ROUTER, &["route"])],
            dependents: vec![edge(MAIN, &["startServer"]), edge(ROUTER, &["startServer"])],
        }),
        cycles: vec![BTreeSet::from([ROUTER.to_string(), SERVER.to_string()])],
        file_count: FILE_COUNT as i64,
        import_count: IMPORT_COUNT,
        languages: HashMap::from([("typescript".to_string(), 3), ("javascript".to_string(), 1)]),
    }
}

#[test]
fn an_indexed_project_answers_from_the_store_file() {
    let project = test_project();
    let db = open(&project.store_path).unwrap();

    assert_eq!(index(&db, &project), (FILE_COUNT, 0));

    assert_eq!(answers(&db, &project), expected_answers());
}

#[test]
fn indexing_an_unchanged_project_again_leaves_the_same_answers() {
    let project = test_project();
    let db = open(&project.store_path).unwrap();
    index(&db, &project);

    index(&db, &project);

    assert_eq!(answers(&db, &project), expected_answers());
}

#[test]
fn the_index_survives_closing_and_reopening_the_store_file() {
    let project = test_project();
    {
        let db = open(&project.store_path).unwrap();
        index(&db, &project);
    }

    let db = open(&project.store_path).unwrap();

    assert_eq!(answers(&db, &project), expected_answers());
}

#[path = "indexer_reconcile_tests.rs"]
mod reconcile;
