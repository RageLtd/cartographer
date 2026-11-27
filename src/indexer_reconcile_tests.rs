//! Reconciling the index with the files on disk, after a full index and after
//! a change-detection run. No git repository is involved: only content counts.

use std::fs::{self, OpenOptions};

use super::*;
use crate::indexer::{detect_changes, Changes};
use crate::parser::registry::LanguageRegistry;

/// A file nothing in the fixture imports and that imports nothing.
const LONER: &str = "src/loner.ts";
const LONER_SOURCE: &str = "export function alone() {\n  return 1;\n}\n";

#[derive(Clone, Copy, Debug)]
enum Run {
    Full,
    Detect,
}

const RUNS: [Run; 2] = [Run::Full, Run::Detect];

fn detect(db: &Db, project: &TestProject) -> Changes {
    detect_changes(
        db,
        &LanguageRegistry::builtin(),
        &project.root,
        &project.root,
    )
    .unwrap()
}

fn run(db: &Db, project: &TestProject, kind: Run) {
    match kind {
        Run::Full => {
            index(db, project);
        }
        Run::Detect => {
            detect(db, project);
        }
    }
}

/// (re-parsed, removed), each relative to the root and sorted.
fn reported(project: &TestProject, changes: Changes) -> (Vec<String>, Vec<String>) {
    let sorted = |paths: Vec<String>| {
        let mut paths: Vec<String> = paths.iter().map(|p| relative(project, p)).collect();
        paths.sort();
        paths
    };
    (sorted(changes.reparsed), sorted(changes.removed))
}

fn indexed(kind: Run) -> (TestProject, Db) {
    let project = test_project();
    let db = open(&project.store_path).unwrap();
    run(&db, &project, kind);
    (project, db)
}

fn detail(db: &Db, project: &TestProject, file: &str) -> Option<Detail> {
    get_file_detail(db, &project.root, &project.path(file))
        .unwrap()
        .map(|d| Detail {
            language: d.language,
            symbols: d.symbols.into_iter().map(|s| s.name).collect(),
            imports: relative_edges(project, d.imports),
            dependents: relative_edges(project, d.dependents),
        })
}

fn imports_of(db: &Db, project: &TestProject, file: &str) -> Vec<Edge> {
    detail(db, project, file)
        .unwrap_or_else(|| panic!("{file} should be indexed"))
        .imports
}

fn write(project: &TestProject, file: &str, source: &str) {
    let path = project.path(file);
    fs::create_dir_all(std::path::Path::new(&path).parent().unwrap()).unwrap();
    fs::write(path, source).unwrap();
}

/// Replaces the content and puts the old modification time back, so that
/// nothing but the content says the file changed.
fn rewrite_keeping_mtime(project: &TestProject, file: &str, source: &str) {
    let path = project.path(file);
    let mtime = fs::metadata(&path).unwrap().modified().unwrap();
    fs::write(&path, source).unwrap();
    let handle = OpenOptions::new().write(true).open(&path).unwrap();
    handle.set_modified(mtime).unwrap();
}

fn search(db: &Db, project: &TestProject, query: &str) -> Vec<String> {
    search_files(db, &project.root, query, 10)
        .unwrap()
        .into_iter()
        .map(|(path, _)| relative(project, &path))
        .collect()
}

#[test]
fn a_deleted_file_and_its_edges_leave_the_index() {
    for kind in RUNS {
        let (project, db) = indexed(kind);
        fs::remove_file(project.path(ROUTER)).unwrap();

        run(&db, &project, kind);

        assert_eq!(detail(&db, &project, ROUTER), None, "{kind:?}");
        let key = project.root.as_str();
        assert_eq!(
            get_file_count(&db, key).unwrap(),
            FILE_COUNT as i64 - 1,
            "{kind:?}"
        );
        assert_eq!(get_import_count(&db, key).unwrap(), 2, "{kind:?}");
        assert_eq!(imports_of(&db, &project, SERVER), vec![], "{kind:?}");
        let util = detail(&db, &project, UTIL).unwrap();
        assert_eq!(
            util.dependents,
            vec![edge(MAIN, &["formatPath"])],
            "{kind:?}"
        );
    }
}

#[test]
fn a_change_detection_run_reparses_a_file_whose_content_changed() {
    let (project, db) = indexed(Run::Detect);
    rewrite_keeping_mtime(
        &project,
        SERVER,
        "import { formatPath } from \"./util\";\n\
         \n\
         export function stopServer() {\n  formatPath(\"/\");\n}\n",
    );

    let changes = detect(&db, &project);

    let (reparsed, removed) = reported(&project, changes);
    assert!(reparsed.contains(&SERVER.to_string()), "{reparsed:?}");
    assert_eq!(removed, Vec::<String>::new());
    assert_eq!(search(&db, &project, "stopServer"), strings(&[SERVER]));
    let server = detail(&db, &project, SERVER).unwrap();
    assert_eq!(server.symbols, strings(&["stopServer"]));
    assert_eq!(server.imports, vec![edge(UTIL, &["formatPath"])]);
}

#[test]
fn a_change_detection_run_on_an_unchanged_project_changes_nothing() {
    let (project, db) = indexed(Run::Full);

    let changes = detect(&db, &project);

    assert_eq!(reported(&project, changes), (vec![], vec![]));
    assert_eq!(answers(&db, &project), expected_answers());
}

#[test]
fn a_renamed_file_is_indexed_under_its_new_path_only() {
    const RENAMED: &str = "src/format.js";
    for kind in RUNS {
        let (project, db) = indexed(kind);
        fs::rename(project.path(UTIL), project.path(RENAMED)).unwrap();

        run(&db, &project, kind);

        assert_eq!(detail(&db, &project, UTIL), None, "{kind:?}");
        let renamed = detail(&db, &project, RENAMED).expect("indexed under the new path");
        assert_eq!(renamed.symbols, strings(&["formatPath"]), "{kind:?}");
        assert_eq!(
            search(&db, &project, "formatPath"),
            strings(&[RENAMED]),
            "{kind:?}"
        );
    }
}

#[test]
fn a_new_file_in_a_new_directory_is_indexed() {
    const HELPER: &str = "src/lib/deep/helper.ts";
    for kind in RUNS {
        let (project, db) = indexed(kind);
        write(
            &project,
            HELPER,
            "import { formatPath } from \"../../util\";\n\
             \n\
             export function helper() {\n  return formatPath(\"/\");\n}\n",
        );

        run(&db, &project, kind);

        let helper = detail(&db, &project, HELPER).expect("the new file is indexed");
        assert_eq!(helper.symbols, strings(&["helper"]), "{kind:?}");
        assert_eq!(
            helper.imports,
            vec![edge(UTIL, &["formatPath"])],
            "{kind:?}"
        );
        assert_eq!(
            get_file_count(&db, &project.root).unwrap(),
            FILE_COUNT as i64 + 1,
            "{kind:?}"
        );
    }
}

#[test]
fn a_reverted_edit_returns_the_index_to_what_it_was() {
    for kind in RUNS {
        let (project, db) = indexed(kind);
        let original = fs::read_to_string(project.path(SERVER)).unwrap();
        rewrite_keeping_mtime(&project, SERVER, "export function elsewhere() {}\n");
        run(&db, &project, kind);
        assert_ne!(answers(&db, &project), expected_answers(), "{kind:?}");

        rewrite_keeping_mtime(&project, SERVER, &original);
        run(&db, &project, kind);

        assert_eq!(answers(&db, &project), expected_answers(), "{kind:?}");
    }
}

#[test]
fn an_unchanged_importer_follows_the_file_it_imports_leaving_and_returning() {
    for kind in RUNS {
        let (project, db) = indexed(kind);
        let util_source = fs::read_to_string(project.path(UTIL)).unwrap();
        let with_util = vec![edge(SERVER, &["startServer"]), edge(UTIL, &["formatPath"])];

        fs::remove_file(project.path(UTIL)).unwrap();
        run(&db, &project, kind);
        assert_eq!(
            imports_of(&db, &project, MAIN),
            vec![edge(SERVER, &["startServer"])],
            "{kind:?}: the edge to a deleted file goes"
        );

        write(&project, UTIL, &util_source);
        run(&db, &project, kind);
        assert_eq!(
            imports_of(&db, &project, MAIN),
            with_util,
            "{kind:?}: the edge returns with the file"
        );
    }
}

#[test]
fn an_import_of_a_file_not_yet_created_appears_once_it_is() {
    const EARLY: &str = "src/early.ts";
    const LATER: &str = "src/later.ts";
    for kind in RUNS {
        let (project, db) = indexed(kind);
        write(
            &project,
            EARLY,
            "import { later } from \"./later\";\n\
             \n\
             export function early() {\n  later();\n}\n",
        );
        run(&db, &project, kind);
        assert_eq!(imports_of(&db, &project, EARLY), vec![], "{kind:?}");

        write(&project, LATER, "export function later() {}\n");
        run(&db, &project, kind);

        assert_eq!(
            imports_of(&db, &project, EARLY),
            vec![edge(LATER, &["later"])],
            "{kind:?}"
        );
    }
}

#[test]
fn a_change_detection_run_reports_what_it_reparsed_and_removed() {
    const ADDED: &str = "src/added.ts";
    let project = test_project();
    write(&project, LONER, LONER_SOURCE);
    let db = open(&project.store_path).unwrap();
    index(&db, &project);

    write(&project, ADDED, "export function added() {}\n");
    rewrite_keeping_mtime(
        &project,
        MAIN,
        "import { startServer } from \"./server\";\n\
         import { formatPath } from \"./util\";\n\
         \n\
         export function main() {\n  startServer(formatPath(\"/\"));\n}\n\
         export function alsoMain() {}\n",
    );
    fs::remove_file(project.path(LONER)).unwrap();

    let changes = detect(&db, &project);

    assert_eq!(
        reported(&project, changes),
        (strings(&[ADDED, MAIN]), strings(&[LONER]))
    );
}

#[test]
fn a_file_that_cannot_be_read_does_not_stop_the_rest_being_reconciled() {
    const BROKEN: &str = "src/broken.ts";
    for kind in RUNS {
        let (project, db) = indexed(kind);
        // Not UTF-8, so neither hashing nor parsing can read it.
        fs::write(project.path(BROKEN), [0xff, 0xfe, 0x00, 0xc3]).unwrap();
        write(&project, LONER, LONER_SOURCE);
        fs::remove_file(project.path(ROUTER)).unwrap();

        match kind {
            Run::Full => {
                // main, server, util and the loner parse; the broken file does not.
                assert_eq!(index(&db, &project), (4, 1));
            }
            Run::Detect => {
                let (reparsed, removed) = reported(&project, detect(&db, &project));
                assert!(reparsed.contains(&LONER.to_string()), "{reparsed:?}");
                assert_eq!(removed, strings(&[ROUTER]));
            }
        }

        assert_eq!(detail(&db, &project, BROKEN), None, "{kind:?}");
        assert_eq!(detail(&db, &project, ROUTER), None, "{kind:?}");
        assert_eq!(
            search(&db, &project, "alone"),
            strings(&[LONER]),
            "{kind:?}"
        );
    }
}
