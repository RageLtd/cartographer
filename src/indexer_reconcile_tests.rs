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

// ============================================================================
// A file is removed only when it is known to be gone
// ============================================================================

fn try_run(db: &Db, project: &TestProject, kind: Run) -> Result<(), String> {
    let registry = LanguageRegistry::builtin();
    match kind {
        Run::Full => {
            crate::indexer::full_index(db, &registry, &project.root, &project.root).map(|_| ())
        }
        Run::Detect => detect_changes(db, &registry, &project.root, &project.root).map(|_| ()),
    }
}

// An unmounted volume or a renamed directory looks, to a walk, like a project
// with no files in it.
#[test]
fn a_project_directory_that_is_not_there_is_an_error_and_removes_nothing() {
    for kind in RUNS {
        let (project, db) = indexed(kind);
        let elsewhere = format!("{}-moved", project.root);
        fs::rename(&project.root, &elsewhere).unwrap();

        let outcome = try_run(&db, &project, kind);

        fs::rename(&elsewhere, &project.root).unwrap();
        let message = outcome.expect_err("the project directory is missing");
        assert!(message.contains(&project.root), "{kind:?}: {message}");
        assert_eq!(answers(&db, &project), expected_answers(), "{kind:?}");
    }
}

#[cfg(unix)]
#[test]
fn files_under_a_directory_that_cannot_be_read_stay_indexed() {
    use std::os::unix::fs::PermissionsExt;

    const LOCKED_DIR: &str = "src/locked";
    const LOCKED_FILE: &str = "src/locked/inner.ts";
    for kind in RUNS {
        let project = test_project();
        write(&project, LOCKED_FILE, "export function inner() {}\n");
        let db = open(&project.store_path).unwrap();
        run(&db, &project, kind);
        let locked = project.path(LOCKED_DIR);
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        if fs::read_dir(&locked).is_ok() {
            fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
            eprintln!("skipping: a directory with no permissions is still readable here (running as root?)");
            return;
        }

        let outcome = try_run(&db, &project, kind);

        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(outcome, Ok(()), "{kind:?}");
        assert_eq!(
            detail(&db, &project, LOCKED_FILE).map(|d| d.symbols),
            Some(strings(&["inner"])),
            "{kind:?}"
        );
    }
}

// `cartographer_parse_file` can store a file the walk does not visit, such as
// one in a dot-directory. Not being walked is not the same as being deleted.
#[test]
fn a_stored_file_the_walk_skips_is_kept_until_it_is_deleted() {
    const SKIPPED: &str = ".storybook/main.ts";
    for kind in RUNS {
        let (project, db) = indexed(kind);
        write(&project, SKIPPED, "export const story = 1;\n");
        crate::db::queries::upsert_file(
            &db,
            &project.root,
            &project.path(SKIPPED),
            "typescript",
            &[],
            "hash",
        )
        .unwrap();

        run(&db, &project, kind);
        assert!(detail(&db, &project, SKIPPED).is_some(), "{kind:?}");

        fs::remove_file(project.path(SKIPPED)).unwrap();
        run(&db, &project, kind);
        assert_eq!(detail(&db, &project, SKIPPED), None, "{kind:?}");
    }
}

// A file recorded with its new hash is not looked at again until it changes,
// so one recorded without its imports would keep the wrong edges.
#[test]
fn a_file_whose_imports_cannot_be_stored_is_not_recorded_as_indexed() {
    for kind in RUNS {
        let project = test_project();
        let db = open(&project.store_path).unwrap();
        rusqlite::Connection::open(&project.store_path)
            .unwrap()
            .execute_batch("DROP TABLE imports")
            .unwrap();

        let outcome = try_run(&db, &project, kind);

        assert!(outcome.is_err(), "{kind:?}");
        assert_eq!(
            crate::db::queries::get_file_paths(&db, &project.root).unwrap(),
            Vec::<String>::new(),
            "{kind:?}"
        );
    }
}

// ============================================================================
// A change in one file that alters how another file's imports resolve
// ============================================================================

// Which crate root `src/a.rs` belongs to is decided by which of `lib.rs` and
// `main.rs` declares `mod a;`. With neither declaring it, `crate::top` in
// `a.rs` names an item in an unknown file; once `lib.rs` declares it, the
// item is in `lib.rs`, though `a.rs` itself has not changed.
#[test]
fn an_unchanged_file_gains_its_edge_when_another_files_change_lets_its_import_resolve() {
    const LIB_WITHOUT_THE_MODULE: &str = "pub fn top() {}\n";
    const LIB_DECLARING_THE_MODULE: &str = "mod a;\n\npub fn top() {}\n";
    for kind in RUNS {
        let dir = tempfile::tempdir().unwrap();
        // Canonical, because the Rust resolver reports canonical paths and
        // the temporary directory is a symlink on macOS.
        let root = dir.path().canonicalize().unwrap().join("crate");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
        fs::write(root.join("src/lib.rs"), LIB_WITHOUT_THE_MODULE).unwrap();
        fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
        fs::write(root.join("src/a.rs"), "use crate::top;\n").unwrap();
        let project = TestProject::at(root.to_str().unwrap(), dir);
        let db = open(&project.store_path).unwrap();

        run(&db, &project, kind);
        assert_eq!(imports_of(&db, &project, "src/a.rs"), vec![], "{kind:?}");

        fs::write(project.path("src/lib.rs"), LIB_DECLARING_THE_MODULE).unwrap();
        run(&db, &project, kind);

        assert_eq!(
            imports_of(&db, &project, "src/a.rs"),
            vec![edge("src/lib.rs", &["top"])],
            "{kind:?}"
        );
    }
}
