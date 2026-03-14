use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension};

use crate::types::{ImportEdge, Symbol};

fn epoch_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

// ============================================================================
// File operations
// ============================================================================

pub fn upsert_file(
    db: &Connection,
    project: &str,
    file_path: &str,
    language: &str,
    symbols: &[Symbol],
    content_hash: &str,
) -> rusqlite::Result<()> {
    let symbols_json = sonic_rs::to_string(symbols).unwrap_or_else(|_| "[]".to_string());
    let symbol_names: String = symbols
        .iter()
        .map(|s| s.name.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    db.execute(
        "INSERT INTO files (project, file_path, language, symbols, symbol_names, last_parsed_epoch, content_hash)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(project, file_path) DO UPDATE SET
            language = excluded.language,
            symbols = excluded.symbols,
            symbol_names = excluded.symbol_names,
            last_parsed_epoch = excluded.last_parsed_epoch,
            content_hash = excluded.content_hash",
        rusqlite::params![project, file_path, language, symbols_json, symbol_names, epoch_ms(), content_hash],
    )?;
    Ok(())
}

pub fn get_file_hash(
    db: &Connection,
    project: &str,
    file_path: &str,
) -> rusqlite::Result<Option<String>> {
    let mut stmt =
        db.prepare_cached("SELECT content_hash FROM files WHERE project = ?1 AND file_path = ?2")?;
    stmt.query_row(rusqlite::params![project, file_path], |row| {
        row.get::<_, String>(0)
    })
    .optional()
}

pub fn remove_file(db: &Connection, project: &str, file_path: &str) -> rusqlite::Result<()> {
    db.execute(
        "DELETE FROM files WHERE project = ?1 AND file_path = ?2",
        rusqlite::params![project, file_path],
    )?;
    db.execute(
        "DELETE FROM imports WHERE project = ?1 AND (source_path = ?2 OR target_path = ?2)",
        rusqlite::params![project, file_path],
    )?;
    Ok(())
}

pub fn replace_imports(
    db: &Connection,
    project: &str,
    source_path: &str,
    edges: &[ImportEdge],
) -> rusqlite::Result<()> {
    let now = epoch_ms();
    db.execute(
        "DELETE FROM imports WHERE project = ?1 AND source_path = ?2",
        rusqlite::params![project, source_path],
    )?;

    let mut stmt = db.prepare_cached(
        "INSERT OR REPLACE INTO imports (project, source_path, target_path, specifier, symbols, updated_at_epoch)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )?;

    for edge in edges {
        let symbols_json = sonic_rs::to_string(&edge.symbols).unwrap_or_else(|_| "[]".to_string());
        stmt.execute(rusqlite::params![
            project,
            edge.source,
            edge.target,
            edge.specifier,
            symbols_json,
            now
        ])?;
    }

    Ok(())
}

// ============================================================================
// Stats
// ============================================================================

pub fn get_file_count(db: &Connection, project: &str) -> rusqlite::Result<i64> {
    db.query_row(
        "SELECT COUNT(*) FROM files WHERE project = ?1",
        [project],
        |row| row.get(0),
    )
}

pub fn get_language_counts(
    db: &Connection,
    project: &str,
) -> rusqlite::Result<HashMap<String, usize>> {
    let mut stmt = db.prepare_cached(
        "SELECT language, COUNT(*) FROM files WHERE project = ?1 GROUP BY language",
    )?;
    let rows = stmt.query_map([project], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize))
    })?;
    rows.collect::<rusqlite::Result<HashMap<_, _>>>()
}

pub fn get_import_count(db: &Connection, project: &str) -> rusqlite::Result<i64> {
    db.query_row(
        "SELECT COUNT(*) FROM imports WHERE project = ?1",
        [project],
        |row| row.get(0),
    )
}

pub fn get_project_stats(db: &Connection) -> rusqlite::Result<Vec<(String, i64)>> {
    let mut stmt = db.prepare("SELECT project, COUNT(*) as count FROM files GROUP BY project")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    rows.collect()
}

// ============================================================================
// Git state tracking
// ============================================================================

pub fn get_last_git_status(
    db: &Connection,
    project: &str,
) -> rusqlite::Result<HashMap<String, String>> {
    let mut stmt = db.prepare_cached("SELECT last_status FROM git_state WHERE project = ?1")?;
    Ok(stmt
        .query_row([project], |row| row.get::<_, String>(0))
        .optional()?
        .and_then(|json| sonic_rs::from_str(&json).ok())
        .unwrap_or_default())
}

pub fn save_git_status(
    db: &Connection,
    project: &str,
    status: &HashMap<String, String>,
) -> rusqlite::Result<()> {
    let json = sonic_rs::to_string(status).unwrap_or_else(|_| "{}".to_string());
    db.execute(
        "INSERT INTO git_state (project, last_status, updated_at_epoch)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(project) DO UPDATE SET
            last_status = excluded.last_status,
            updated_at_epoch = excluded.updated_at_epoch",
        rusqlite::params![project, json, epoch_ms()],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::setup::create_database;
    use crate::types::{SymbolKind, Visibility};

    fn test_db() -> Connection {
        create_database(":memory:").unwrap()
    }

    fn make_symbol(name: &str, kind: SymbolKind) -> Symbol {
        Symbol {
            name: name.to_string(),
            kind,
            signature: format!("fn {name}()"),
            doc_comment: None,
            visibility: Visibility::Exported,
            line: 1,
        }
    }

    #[test]
    fn test_upsert_and_retrieve_file() {
        let db = test_db();
        let syms = vec![make_symbol("foo", SymbolKind::Function)];
        upsert_file(
            &db,
            "/project",
            "/project/src/a.ts",
            "typescript",
            &syms,
            "abc123",
        )
        .unwrap();

        assert_eq!(get_file_count(&db, "/project").unwrap(), 1);
        assert_eq!(
            get_file_hash(&db, "/project", "/project/src/a.ts").unwrap(),
            Some("abc123".to_string())
        );
        let langs = get_language_counts(&db, "/project").unwrap();
        assert_eq!(langs.get("typescript"), Some(&1));
    }

    #[test]
    fn test_upsert_updates_existing() {
        let db = test_db();
        let syms1 = vec![make_symbol("foo", SymbolKind::Function)];
        upsert_file(&db, "/p", "/p/a.ts", "typescript", &syms1, "hash1").unwrap();

        let syms2 = vec![
            make_symbol("foo", SymbolKind::Function),
            make_symbol("bar", SymbolKind::Const),
        ];
        upsert_file(&db, "/p", "/p/a.ts", "typescript", &syms2, "hash2").unwrap();

        assert_eq!(get_file_count(&db, "/p").unwrap(), 1);
        assert_eq!(
            get_file_hash(&db, "/p", "/p/a.ts").unwrap(),
            Some("hash2".to_string())
        );
    }

    #[test]
    fn test_remove_file_cleans_imports() {
        let db = test_db();
        upsert_file(&db, "/p", "/p/a.ts", "typescript", &[], "h1").unwrap();
        upsert_file(&db, "/p", "/p/b.ts", "typescript", &[], "h2").unwrap();

        let edge = ImportEdge {
            source: "/p/a.ts".into(),
            target: "/p/b.ts".into(),
            specifier: "./b".into(),
            symbols: vec!["default".into()],
        };
        replace_imports(&db, "/p", "/p/a.ts", &[edge]).unwrap();
        assert_eq!(get_import_count(&db, "/p").unwrap(), 1);

        remove_file(&db, "/p", "/p/a.ts").unwrap();
        assert_eq!(get_file_count(&db, "/p").unwrap(), 1);
        assert_eq!(get_import_count(&db, "/p").unwrap(), 0);
    }

    #[test]
    fn test_replace_imports_overwrites() {
        let db = test_db();
        upsert_file(&db, "/p", "/p/a.ts", "typescript", &[], "h").unwrap();
        upsert_file(&db, "/p", "/p/b.ts", "typescript", &[], "h").unwrap();
        upsert_file(&db, "/p", "/p/c.ts", "typescript", &[], "h").unwrap();

        let edge1 = ImportEdge {
            source: "/p/a.ts".into(),
            target: "/p/b.ts".into(),
            specifier: "./b".into(),
            symbols: vec![],
        };
        replace_imports(&db, "/p", "/p/a.ts", &[edge1]).unwrap();
        assert_eq!(get_import_count(&db, "/p").unwrap(), 1);

        let edge2 = ImportEdge {
            source: "/p/a.ts".into(),
            target: "/p/c.ts".into(),
            specifier: "./c".into(),
            symbols: vec![],
        };
        replace_imports(&db, "/p", "/p/a.ts", &[edge2]).unwrap();
        assert_eq!(get_import_count(&db, "/p").unwrap(), 1);
    }

    #[test]
    fn test_git_status_round_trip() {
        let db = test_db();
        let mut status = HashMap::new();
        status.insert("/p/a.ts".to_string(), "M".to_string());
        status.insert("/p/b.ts".to_string(), "??".to_string());

        save_git_status(&db, "/p", &status).unwrap();
        let loaded = get_last_git_status(&db, "/p").unwrap();

        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded.get("/p/a.ts").unwrap(), "M");
        assert_eq!(loaded.get("/p/b.ts").unwrap(), "??");
    }

    #[test]
    fn test_git_status_empty_project() {
        let db = test_db();
        let loaded = get_last_git_status(&db, "/nonexistent").unwrap();
        assert!(loaded.is_empty());
    }
}
