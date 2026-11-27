use std::collections::HashMap;

use super::*;
use crate::db::fixtures_tests::*;
use crate::db::graph::search_files;
use crate::db::queries::{
    get_file_count, get_file_hash, get_import_count, get_last_git_status, get_project_stats,
    remove_file, replace_imports, save_git_status, upsert_file,
};

#[test]
fn data_survives_closing_and_reopening_the_store_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(STORE_FILE);
    let (a, b) = (p("src/a.rs"), p("src/b.rs"));
    let status = HashMap::from([("src/a.rs".to_string(), "M".to_string())]);
    {
        let db = open(&path).unwrap();
        upsert_file(&db, PROJECT, &a, "rust", &symbols(&["persisted_fn"]), "h1").unwrap();
        add_file(&db, PROJECT, &b, &[]);
        replace_imports(&db, PROJECT, &a, &[edge(&a, &b, "./b", &["B"])]).unwrap();
        save_git_status(&db, PROJECT, &status).unwrap();
    }

    let db = open(&path).unwrap();

    assert_eq!(
        get_file_hash(&db, PROJECT, &a).unwrap(),
        Some("h1".to_string())
    );
    assert_eq!(get_file_count(&db, PROJECT).unwrap(), 2);
    assert_eq!(get_import_count(&db, PROJECT).unwrap(), 1);
    assert_eq!(get_last_git_status(&db, PROJECT).unwrap(), status);
    let found = search_files(&db, PROJECT, "persisted_fn", 10).unwrap();
    assert_eq!(
        found.iter().map(|(f, _)| f.clone()).collect::<Vec<_>>(),
        vec![a]
    );
}

// The store is a cache of the index: a file from another schema version is
// discarded rather than migrated, and a re-index fills it again.
#[test]
fn a_store_file_from_another_schema_version_opens_empty() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(STORE_FILE);
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        let mut sql = String::new();
        for table in [
            "cart_file",
            "cart_import",
            "cart_git_state",
            "files",
            "imports",
            "git_state",
        ] {
            sql.push_str(&format!(
                "CREATE TABLE {table} (legacy TEXT NOT NULL);
                 INSERT INTO {table} VALUES ('left over');"
            ));
        }
        sql.push_str("PRAGMA user_version = 31337;");
        conn.execute_batch(&sql).unwrap();
    }

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
    assert!(
        save_git_status(
            &db,
            PROJECT,
            &HashMap::from([("a".to_string(), "M".to_string())])
        )
        .is_err(),
        "save_git_status reported Ok on a read-only store"
    );
}

#[test]
fn opening_a_store_inside_a_regular_file_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let not_a_dir = dir.path().join("plain-file");
    std::fs::write(&not_a_dir, "not a directory").unwrap();

    assert!(open(&not_a_dir.join(STORE_FILE)).is_err());
}
