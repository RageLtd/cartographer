use std::cmp::Ordering;

use rusqlite::{Connection, TransactionBehavior};

/// Bump whenever `SCHEMA` or what is stored in it changes, such as how
/// `search_words` cuts the search text. The store is a cache of the index, so
/// a file written under an older version is emptied and refilled by the next
/// index rather than migrated. A file written under a newer version is
/// refused: see `is_current`.
const SCHEMA_VERSION: i64 = 4;

const USER_VERSION: &str = "user_version";

// Imports name their files by path, with no foreign key: an edge may be stored
// before its target is indexed, and re-upserting a file must keep the edges
// into and out of it.
//
// `file_search` rows share their rowid with `files.id`. Its text is already cut
// into words by `search_words`, joined with spaces; the unicode61 tokenizer
// only lowercases them.
const SCHEMA: &str = "
    CREATE TABLE files (
        id INTEGER PRIMARY KEY,
        project TEXT NOT NULL,
        file_path TEXT NOT NULL,
        language TEXT NOT NULL,
        symbols TEXT NOT NULL,
        content_hash TEXT NOT NULL,
        last_parsed_epoch INTEGER NOT NULL,
        UNIQUE (project, file_path)
    );

    CREATE TABLE imports (
        project TEXT NOT NULL,
        source_path TEXT NOT NULL,
        target_path TEXT NOT NULL,
        specifier TEXT NOT NULL,
        symbols TEXT NOT NULL,
        is_dependency INTEGER NOT NULL,
        updated_at_epoch INTEGER NOT NULL,
        PRIMARY KEY (project, source_path, target_path, specifier)
    );
    CREATE INDEX imports_target ON imports (project, target_path);

    CREATE VIRTUAL TABLE file_search USING fts5(searchable, tokenize = 'unicode61');
";

/// Brings the store to `SCHEMA_VERSION`, discarding whatever an older version
/// left in it. Fails, changing nothing, on a store from a newer version.
pub fn init_schema(conn: &mut Connection) -> Result<(), String> {
    let failed = |e: rusqlite::Error| format!("Failed to initialize the Cartographer schema: {e}");

    if is_current(user_version(conn).map_err(failed)?)? {
        return Ok(());
    }

    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(failed)?;
    // Another process may have initialized the store while this one waited
    // for the write lock.
    if !is_current(user_version(&tx).map_err(failed)?)? {
        drop_all_tables(&tx).map_err(failed)?;
        tx.execute_batch(SCHEMA).map_err(failed)?;
        tx.pragma_update(None, USER_VERSION, SCHEMA_VERSION)
            .map_err(failed)?;
        tracing::info!("Cartographer schema initialized (version {SCHEMA_VERSION})");
    }
    tx.commit().map_err(failed)
}

/// Whether a store at `version` can be used as it is. `false` means it is
/// older and is to be rebuilt.
///
/// A newer store is an error and is left alone. The server and the hooks are
/// separate processes sharing the file, and after an update the two can be
/// different versions for a while. If each emptied whatever it did not
/// recognise, they would keep erasing each other's index.
fn is_current(version: i64) -> Result<bool, String> {
    match version.cmp(&SCHEMA_VERSION) {
        Ordering::Equal => Ok(true),
        Ordering::Less => Ok(false),
        Ordering::Greater => Err(format!(
            "The store was written by a newer Cartographer (schema version {version}; this \
             one reads up to {SCHEMA_VERSION}). Update Cartographer, or delete the store file \
             to rebuild it."
        )),
    }
}

fn user_version(conn: &Connection) -> rusqlite::Result<i64> {
    conn.pragma_query_value(None, USER_VERSION, |row| row.get(0))
}

fn drop_all_tables(conn: &Connection) -> rusqlite::Result<()> {
    // Virtual tables first: dropping one drops its shadow tables, which the
    // `IF EXISTS` then skips.
    let tables: Vec<String> = conn
        .prepare(
            "SELECT name FROM sqlite_master
             WHERE type = 'table' AND name NOT LIKE 'sqlite_%'
             ORDER BY sql LIKE 'CREATE VIRTUAL TABLE%' DESC",
        )?
        .query_map([], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;

    for table in tables {
        let quoted = table.replace('"', "\"\"");
        conn.execute_batch(&format!("DROP TABLE IF EXISTS \"{quoted}\""))?;
    }
    Ok(())
}
