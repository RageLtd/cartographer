use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;

use crate::constants::{DEFAULT_MAX_DEPTH, DEFAULT_MAX_RESULTS};
use crate::types::{ImportEdge, RelevantFile, Symbol};

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

#[allow(dead_code)]
pub fn get_file_hash(db: &Connection, project: &str, file_path: &str) -> rusqlite::Result<Option<String>> {
    let mut stmt = db.prepare_cached(
        "SELECT content_hash FROM files WHERE project = ?1 AND file_path = ?2",
    )?;
    let result = stmt.query_row(rusqlite::params![project, file_path], |row| {
        row.get::<_, String>(0)
    });
    match result {
        Ok(hash) => Ok(Some(hash)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e),
    }
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

pub struct TrackedFile {
    pub file_path: String,
    pub content_hash: String,
    pub language: String,
}

pub fn get_tracked_files(db: &Connection, project: &str) -> rusqlite::Result<Vec<TrackedFile>> {
    let mut stmt = db.prepare_cached(
        "SELECT file_path, content_hash, language FROM files WHERE project = ?1",
    )?;
    let rows = stmt.query_map([project], |row| {
        Ok(TrackedFile {
            file_path: row.get(0)?,
            content_hash: row.get(1)?,
            language: row.get(2)?,
        })
    })?;
    rows.collect()
}

// ============================================================================
// Graph queries
// ============================================================================

pub fn walk_import_graph(
    db: &Connection,
    project: &str,
    entry_points: &[String],
    max_depth: Option<i64>,
    max_results: Option<i64>,
) -> rusqlite::Result<Vec<RelevantFile>> {
    if entry_points.is_empty() {
        return Ok(vec![]);
    }

    let max_depth = max_depth.unwrap_or(DEFAULT_MAX_DEPTH);
    let max_results = max_results.unwrap_or(DEFAULT_MAX_RESULTS);

    let placeholders: Vec<String> = entry_points.iter().enumerate().map(|(i, _)| format!("?{}", i + 2)).collect();
    let placeholders_str = placeholders.join(", ");

    // Build the query with the right number of placeholders
    let sql = format!(
        "WITH RECURSIVE
        reachable(file_path, depth, reason) AS (
            SELECT file_path, 0, 'entry'
            FROM files
            WHERE project = ?1 AND file_path IN ({placeholders})

            UNION

            SELECT i.target_path, r.depth + 1, 'dependency'
            FROM reachable r
            JOIN imports i ON i.source_path = r.file_path AND i.project = ?1
            WHERE r.depth < ?{depth_param}

            UNION

            SELECT i.source_path, r.depth + 1, 'dependent'
            FROM reachable r
            JOIN imports i ON i.target_path = r.file_path AND i.project = ?1
            WHERE r.depth < ?{depth_param}
        )
        SELECT DISTINCT
            r.file_path,
            MIN(r.depth) as depth,
            CASE MIN(CASE r.reason WHEN 'entry' THEN 0 WHEN 'dependency' THEN 1 WHEN 'dependent' THEN 2 END)
                WHEN 0 THEN 'entry'
                WHEN 1 THEN 'dependency'
                WHEN 2 THEN 'dependent'
            END as reason,
            COALESCE(f.symbols, '[]') as symbols
        FROM reachable r
        LEFT JOIN files f ON f.file_path = r.file_path AND f.project = ?1
        GROUP BY r.file_path
        ORDER BY depth ASC
        LIMIT ?{limit_param}",
        placeholders = placeholders_str,
        depth_param = entry_points.len() + 2,
        limit_param = entry_points.len() + 3,
    );

    let mut stmt = db.prepare(&sql)?;

    // Build params: project, ...entry_points, max_depth, max_results
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    params.push(Box::new(project.to_string()));
    for ep in entry_points {
        params.push(Box::new(ep.clone()));
    }
    params.push(Box::new(max_depth));
    params.push(Box::new(max_results));

    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    let rows = stmt.query_map(param_refs.as_slice(), |row| {
        let file_path: String = row.get(0)?;
        let depth: i64 = row.get(1)?;
        let reason: String = row.get(2)?;
        let symbols_str: String = row.get(3)?;
        Ok((file_path, depth, reason, symbols_str))
    })?;

    let mut results = Vec::new();
    for row in rows {
        let (file_path, depth, reason, symbols_str) = row?;
        let symbols: Vec<Symbol> = sonic_rs::from_str(&symbols_str).unwrap_or_default();
        let relative_path = if file_path.starts_with(project) {
            file_path[project.len()..].trim_start_matches('/').to_string()
        } else {
            file_path.clone()
        };
        results.push(RelevantFile {
            file_path,
            relative_path,
            reason,
            depth,
            symbols,
        });
    }

    Ok(results)
}

pub fn search_files(
    db: &Connection,
    project: &str,
    query: &str,
    limit: i64,
) -> rusqlite::Result<Vec<(String, Vec<Symbol>)>> {
    let mut stmt = db.prepare_cached(
        "SELECT f.file_path, f.symbols
         FROM files_fts fts
         JOIN files f ON f.id = fts.rowid
         WHERE files_fts MATCH ?1 AND f.project = ?2
         ORDER BY rank
         LIMIT ?3",
    )?;

    // Wrap query in double quotes to treat as phrase literal, preventing FTS5 operator injection
    let safe_query = format!("\"{}\"", query.replace('"', "\"\""));

    let rows = stmt.query_map(rusqlite::params![safe_query, project, limit], |row| {
        let file_path: String = row.get(0)?;
        let symbols_str: String = row.get(1)?;
        Ok((file_path, symbols_str))
    })?;

    let mut results = Vec::new();
    for row in rows {
        let (file_path, symbols_str) = row?;
        let symbols: Vec<Symbol> = sonic_rs::from_str(&symbols_str).unwrap_or_default();
        results.push((file_path, symbols));
    }

    Ok(results)
}

// ============================================================================
// Git state tracking
// ============================================================================

pub fn get_last_git_status(db: &Connection, project: &str) -> rusqlite::Result<HashMap<String, String>> {
    let mut stmt = db.prepare_cached("SELECT last_status FROM git_state WHERE project = ?1")?;
    let result = stmt.query_row([project], |row| row.get::<_, String>(0));
    match result {
        Ok(json) => Ok(sonic_rs::from_str(&json).unwrap_or_default()),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(HashMap::new()),
        Err(e) => Err(e),
    }
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

// ============================================================================
// Stats
// ============================================================================

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

    // ========================================================================
    // CRUD tests
    // ========================================================================

    #[test]
    fn test_upsert_and_retrieve_file() {
        let db = test_db();
        let syms = vec![make_symbol("foo", SymbolKind::Function)];
        upsert_file(&db, "/project", "/project/src/a.ts", "typescript", &syms, "abc123").unwrap();

        let tracked = get_tracked_files(&db, "/project").unwrap();
        assert_eq!(tracked.len(), 1);
        assert_eq!(tracked[0].file_path, "/project/src/a.ts");
        assert_eq!(tracked[0].language, "typescript");
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

        let tracked = get_tracked_files(&db, "/p").unwrap();
        assert_eq!(tracked.len(), 1);
        assert_eq!(tracked[0].content_hash, "hash2");
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
        assert_eq!(get_tracked_files(&db, "/p").unwrap().len(), 1);
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

        // Replace with different import — old one should be gone
        let edge2 = ImportEdge {
            source: "/p/a.ts".into(),
            target: "/p/c.ts".into(),
            specifier: "./c".into(),
            symbols: vec![],
        };
        replace_imports(&db, "/p", "/p/a.ts", &[edge2]).unwrap();
        assert_eq!(get_import_count(&db, "/p").unwrap(), 1);
    }

    // ========================================================================
    // Graph walk tests
    // ========================================================================

    /// Seed a diamond-shaped graph:
    ///   A → B → D
    ///   A → C → D
    fn seed_diamond(db: &Connection) {
        let proj = "/proj";
        for f in &["/proj/a.ts", "/proj/b.ts", "/proj/c.ts", "/proj/d.ts"] {
            upsert_file(db, proj, f, "typescript", &[], "h").unwrap();
        }
        replace_imports(db, proj, "/proj/a.ts", &[
            ImportEdge { source: "/proj/a.ts".into(), target: "/proj/b.ts".into(), specifier: "./b".into(), symbols: vec![] },
            ImportEdge { source: "/proj/a.ts".into(), target: "/proj/c.ts".into(), specifier: "./c".into(), symbols: vec![] },
        ]).unwrap();
        replace_imports(db, proj, "/proj/b.ts", &[
            ImportEdge { source: "/proj/b.ts".into(), target: "/proj/d.ts".into(), specifier: "./d".into(), symbols: vec![] },
        ]).unwrap();
        replace_imports(db, proj, "/proj/c.ts", &[
            ImportEdge { source: "/proj/c.ts".into(), target: "/proj/d.ts".into(), specifier: "./d".into(), symbols: vec![] },
        ]).unwrap();
    }

    #[test]
    fn test_graph_walk_entry_only() {
        let db = test_db();
        seed_diamond(&db);

        let results = walk_import_graph(
            &db, "/proj",
            &["/proj/a.ts".into()],
            Some(0), Some(20),
        ).unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].file_path, "/proj/a.ts");
        assert_eq!(results[0].reason, "entry");
        assert_eq!(results[0].depth, 0);
    }

    #[test]
    fn test_graph_walk_depth_1() {
        let db = test_db();
        seed_diamond(&db);

        let results = walk_import_graph(
            &db, "/proj",
            &["/proj/a.ts".into()],
            Some(1), Some(20),
        ).unwrap();

        // A (entry) + B, C (dependencies at depth 1)
        assert_eq!(results.len(), 3);

        let entry = results.iter().find(|r| r.reason == "entry").unwrap();
        assert_eq!(entry.file_path, "/proj/a.ts");

        let deps: Vec<&str> = results.iter()
            .filter(|r| r.reason == "dependency")
            .map(|r| r.file_path.as_str())
            .collect();
        assert!(deps.contains(&"/proj/b.ts"));
        assert!(deps.contains(&"/proj/c.ts"));
    }

    #[test]
    fn test_graph_walk_depth_2_reaches_d() {
        let db = test_db();
        seed_diamond(&db);

        let results = walk_import_graph(
            &db, "/proj",
            &["/proj/a.ts".into()],
            Some(2), Some(20),
        ).unwrap();

        // A + B + C + D
        assert_eq!(results.len(), 4);
        let d = results.iter().find(|r| r.file_path == "/proj/d.ts").unwrap();
        assert_eq!(d.reason, "dependency");
        assert_eq!(d.depth, 2);
    }

    #[test]
    fn test_graph_walk_dependents() {
        let db = test_db();
        seed_diamond(&db);

        // Start from D — should find B and C as dependents at depth 1
        let results = walk_import_graph(
            &db, "/proj",
            &["/proj/d.ts".into()],
            Some(1), Some(20),
        ).unwrap();

        assert_eq!(results.len(), 3); // D + B + C
        let dependents: Vec<&str> = results.iter()
            .filter(|r| r.reason == "dependent")
            .map(|r| r.file_path.as_str())
            .collect();
        assert!(dependents.contains(&"/proj/b.ts"));
        assert!(dependents.contains(&"/proj/c.ts"));
    }

    #[test]
    fn test_graph_walk_bidirectional() {
        let db = test_db();
        seed_diamond(&db);

        // Start from B — should find A (dependent) and D (dependency) at depth 1
        let results = walk_import_graph(
            &db, "/proj",
            &["/proj/b.ts".into()],
            Some(1), Some(20),
        ).unwrap();

        let paths: Vec<&str> = results.iter().map(|r| r.file_path.as_str()).collect();
        assert!(paths.contains(&"/proj/b.ts")); // entry
        assert!(paths.contains(&"/proj/a.ts")); // dependent
        assert!(paths.contains(&"/proj/d.ts")); // dependency
    }

    #[test]
    fn test_graph_walk_max_results_limit() {
        let db = test_db();
        seed_diamond(&db);

        let results = walk_import_graph(
            &db, "/proj",
            &["/proj/a.ts".into()],
            Some(5), Some(2),
        ).unwrap();

        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_graph_walk_empty_entry_points() {
        let db = test_db();
        let results = walk_import_graph(&db, "/proj", &[], Some(2), Some(20)).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_graph_walk_relative_path() {
        let db = test_db();
        upsert_file(&db, "/proj", "/proj/src/a.ts", "typescript", &[], "h").unwrap();

        let results = walk_import_graph(
            &db, "/proj",
            &["/proj/src/a.ts".into()],
            Some(0), Some(20),
        ).unwrap();

        assert_eq!(results[0].relative_path, "src/a.ts");
    }

    #[test]
    fn test_graph_walk_preserves_symbols() {
        let db = test_db();
        let syms = vec![
            make_symbol("validate", SymbolKind::Function),
            make_symbol("Config", SymbolKind::Interface),
        ];
        upsert_file(&db, "/p", "/p/a.ts", "typescript", &syms, "h").unwrap();

        let results = walk_import_graph(
            &db, "/p",
            &["/p/a.ts".into()],
            Some(0), Some(20),
        ).unwrap();

        assert_eq!(results[0].symbols.len(), 2);
        assert!(results[0].symbols.iter().any(|s| s.name == "validate"));
        assert!(results[0].symbols.iter().any(|s| s.name == "Config"));
    }

    // ========================================================================
    // FTS search tests
    // ========================================================================

    #[test]
    fn test_fts_search_by_file_path() {
        let db = test_db();
        upsert_file(&db, "/p", "/p/src/auth/middleware.ts", "typescript", &[], "h").unwrap();
        upsert_file(&db, "/p", "/p/src/db/queries.ts", "typescript", &[], "h").unwrap();

        let results = search_files(&db, "/p", "middleware", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "/p/src/auth/middleware.ts");
    }

    #[test]
    fn test_fts_search_by_symbol_name() {
        let db = test_db();
        let syms = vec![
            make_symbol("validateToken", SymbolKind::Function),
            make_symbol("refreshToken", SymbolKind::Function),
        ];
        upsert_file(&db, "/p", "/p/src/auth.ts", "typescript", &syms, "h").unwrap();

        let other_syms = vec![make_symbol("query", SymbolKind::Function)];
        upsert_file(&db, "/p", "/p/src/db.ts", "typescript", &other_syms, "h").unwrap();

        let results = search_files(&db, "/p", "validateToken", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "/p/src/auth.ts");
    }

    #[test]
    fn test_fts_search_no_json_noise() {
        let db = test_db();
        // "name" appears in JSON keys — should NOT match
        let syms = vec![make_symbol("doStuff", SymbolKind::Function)];
        upsert_file(&db, "/p", "/p/a.ts", "typescript", &syms, "h").unwrap();

        // Searching for "kind" or "signature" (JSON keys) should return nothing
        let results = search_files(&db, "/p", "signature", 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_fts_respects_project_filter() {
        let db = test_db();
        upsert_file(&db, "/p1", "/p1/a.ts", "typescript", &[], "h").unwrap();
        upsert_file(&db, "/p2", "/p2/a.ts", "typescript", &[], "h").unwrap();

        let results = search_files(&db, "/p1", "a", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "/p1/a.ts");
    }

    // ========================================================================
    // Git state tests
    // ========================================================================

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
