use super::*;
use crate::db::fixtures_tests::*;
use crate::db::graph::search_files;
use crate::db::queries::{
    get_file_count, get_file_hash, get_import_count, get_project_stats, remove_file,
    replace_imports, upsert_file,
};

#[test]
fn data_survives_closing_and_reopening_the_store_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(STORE_FILE);
    let (a, b) = (p("src/a.rs"), p("src/b.rs"));
    {
        let db = open(&path).unwrap();
        upsert_file(&db, PROJECT, &a, "rust", &symbols(&["persisted_fn"]), "h1").unwrap();
        add_file(&db, PROJECT, &b, &[]);
        replace_imports(&db, PROJECT, &a, &[edge(&a, &b, "./b", &["B"])]).unwrap();
    }

    let db = open(&path).unwrap();

    assert_eq!(
        get_file_hash(&db, PROJECT, &a).unwrap(),
        Some("h1".to_string())
    );
    assert_eq!(get_file_count(&db, PROJECT).unwrap(), 2);
    assert_eq!(get_import_count(&db, PROJECT).unwrap(), 1);
    let found = search_files(&db, PROJECT, "persisted_fn", 10).unwrap();
    assert_eq!(
        found.iter().map(|(f, _)| f.clone()).collect::<Vec<_>>(),
        vec![a]
    );
}

const OLDER_SCHEMA_VERSION: i64 = 1;
const NEWER_SCHEMA_VERSION: i64 = 31337;

/// Writes a store file as some other version of Cartographer left it: tables
/// of another shape, one row each, at schema version `version`.
fn store_left_by_version(path: &std::path::Path, version: i64) {
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.execute_batch(&format!(
        "CREATE TABLE files (legacy TEXT NOT NULL);
         INSERT INTO files VALUES ('left over');
         CREATE TABLE imports (legacy TEXT NOT NULL);
         INSERT INTO imports VALUES ('left over');
         CREATE TABLE git_state (legacy TEXT NOT NULL);
         INSERT INTO git_state VALUES ('left over');
         PRAGMA user_version = {version};"
    ))
    .unwrap();
}

// The store is a cache of the index: a file from an older schema version is
// discarded rather than migrated, and a re-index fills it again.
#[test]
fn a_store_file_from_an_older_schema_version_opens_empty() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(STORE_FILE);
    store_left_by_version(&path, OLDER_SCHEMA_VERSION);

    let db = open(&path).unwrap();

    assert_eq!(get_file_count(&db, PROJECT).unwrap(), 0);
    assert!(get_project_stats(&db).unwrap().is_empty());

    let a = p("src/a.rs");
    upsert_file(&db, PROJECT, &a, "rust", &symbols(&["fresh"]), "h1").unwrap();
    assert_eq!(
        get_file_hash(&db, PROJECT, &a).unwrap(),
        Some("h1".to_string())
    );
    assert_eq!(get_file_count(&db, PROJECT).unwrap(), 1);
}

// After an update, a hook from the new version and a server still running the
// old one share the file. If the old one emptied a newer store, each would
// keep erasing the other's index.
#[test]
fn a_store_file_from_a_newer_schema_version_is_refused_and_left_as_it_was() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(STORE_FILE);
    store_left_by_version(&path, NEWER_SCHEMA_VERSION);

    let refused = open(&path);

    let message = refused.err().expect("a newer store is not opened");
    assert!(message.contains("newer"), "{message}");
    let conn = rusqlite::Connection::open(&path).unwrap();
    let rows: i64 = conn
        .query_row(
            "SELECT count(*) FROM files WHERE legacy = 'left over'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(rows, 1);
    let version: i64 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(version, NEWER_SCHEMA_VERSION);
}

// A hook process opens the store while the server holds it open.
#[test]
fn a_second_handle_reads_while_the_first_is_open() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(STORE_FILE);
    let server = open(&path).unwrap();
    add_file(&server, PROJECT, &p("a.rs"), &[]);

    let hook = open(&path).unwrap();
    assert_eq!(get_file_count(&hook, PROJECT).unwrap(), 1);

    add_file(&server, PROJECT, &p("b.rs"), &[]);
    assert_eq!(get_file_count(&hook, PROJECT).unwrap(), 2);
    assert_eq!(get_file_count(&server, PROJECT).unwrap(), 2);
}

#[cfg(unix)]
#[test]
fn writes_to_a_store_file_that_cannot_be_written_are_errors() {
    use std::fs::{self, OpenOptions, Permissions};
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(STORE_FILE);
    let (a, b) = (p("a.rs"), p("b.rs"));
    {
        let db = open(&path).unwrap();
        add_file(&db, PROJECT, &a, &[]);
    }
    fs::set_permissions(&path, Permissions::from_mode(0o444)).unwrap();
    if OpenOptions::new().write(true).open(&path).is_ok() {
        eprintln!("skipping: a read-only file is still writable here (running as root?)");
        return;
    }

    // Refusing to open a store it cannot write is also an honest error; what
    // must never happen is a write that reports Ok.
    let db = match open(&path) {
        Ok(db) => db,
        Err(e) => {
            assert!(!e.is_empty());
            return;
        }
    };

    assert!(
        upsert_file(&db, PROJECT, &b, "rust", &symbols(&["b"]), "h").is_err(),
        "upsert_file reported Ok on a read-only store"
    );
    assert!(
        replace_imports(&db, PROJECT, &a, &[edge(&a, &b, "./b", &["B"])]).is_err(),
        "replace_imports reported Ok on a read-only store"
    );
    assert!(
        remove_file(&db, PROJECT, &a).is_err(),
        "remove_file reported Ok on a read-only store"
    );
}

#[test]
fn opening_a_store_inside_a_regular_file_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let not_a_dir = dir.path().join("plain-file");
    std::fs::write(&not_a_dir, "not a directory").unwrap();

    assert!(open(&not_a_dir.join(STORE_FILE)).is_err());
}
