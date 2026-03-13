use std::collections::HashSet;

use rusqlite::Connection;

use crate::constants::{DEFAULT_MAX_DEPTH, DEFAULT_MAX_RESULTS};
use crate::types::{RelevantFile, Symbol};

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

    let placeholders: Vec<String> = entry_points
        .iter()
        .enumerate()
        .map(|(i, _)| format!("?{}", i + 2))
        .collect();
    let placeholders_str = placeholders.join(", ");

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
        let relative_path = if let Some(stripped) = file_path.strip_prefix(project) {
            stripped.trim_start_matches('/').to_string()
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
// Cycle detection
// ============================================================================

pub fn find_cycles(db: &Connection, project: &str) -> rusqlite::Result<Vec<Vec<String>>> {
    let mut stmt = db.prepare(
        "WITH RECURSIVE
        chain(file_path, origin, path, depth) AS (
            SELECT DISTINCT source_path, source_path, source_path, 0
            FROM imports WHERE project = ?1

            UNION ALL

            SELECT i.target_path, chain.origin,
                   chain.path || '|' || i.target_path,
                   chain.depth + 1
            FROM chain
            JOIN imports i ON i.source_path = chain.file_path AND i.project = ?1
            WHERE chain.depth < 20
              AND chain.path NOT LIKE '%|' || i.target_path || '|%'
              AND i.target_path != chain.origin
        )
        SELECT chain.path || '|' || i.target_path
        FROM chain
        JOIN imports i ON i.source_path = chain.file_path AND i.project = ?1
        WHERE i.target_path = chain.origin",
    )?;

    let rows = stmt.query_map([project], |row| row.get::<_, String>(0))?;

    let mut seen = HashSet::new();
    let mut cycles = Vec::new();

    for row in rows {
        let path_str = row?;
        let parts: Vec<String> = path_str.split('|').map(|s| s.to_string()).collect();

        // Normalize: rotate so the lexicographically smallest element is first
        if let Some(min_pos) = parts
            .iter()
            .enumerate()
            .take(parts.len() - 1)
            .min_by_key(|(_, v)| v.as_str())
            .map(|(i, _)| i)
        {
            let normalized: Vec<String> = parts[min_pos..parts.len() - 1]
                .iter()
                .chain(parts[..min_pos].iter())
                .cloned()
                .collect();
            let key = normalized.join("|");
            if seen.insert(key) {
                let mut cycle = normalized;
                cycle.push(cycle[0].clone());
                cycles.push(cycle);
            }
        }
    }

    Ok(cycles)
}

// ============================================================================
// File detail
// ============================================================================

pub struct FileDetail {
    pub file_path: String,
    pub language: String,
    pub symbols: Vec<Symbol>,
    pub imports: Vec<(String, Vec<String>)>,
    pub dependents: Vec<(String, Vec<String>)>,
}

pub fn get_file_detail(
    db: &Connection,
    project: &str,
    file_path: &str,
) -> rusqlite::Result<Option<FileDetail>> {
    let mut stmt = db.prepare_cached(
        "SELECT language, symbols FROM files WHERE project = ?1 AND file_path = ?2",
    )?;
    let file_row = stmt.query_row(rusqlite::params![project, file_path], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    });

    let (language, symbols_str) = match file_row {
        Ok(r) => r,
        Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
        Err(e) => return Err(e),
    };

    let symbols: Vec<Symbol> = sonic_rs::from_str(&symbols_str).unwrap_or_default();

    let mut imp_stmt = db.prepare_cached(
        "SELECT target_path, symbols FROM imports WHERE project = ?1 AND source_path = ?2",
    )?;
    let imports: Vec<(String, Vec<String>)> = imp_stmt
        .query_map(rusqlite::params![project, file_path], |row| {
            let target: String = row.get(0)?;
            let syms_str: String = row.get(1)?;
            Ok((target, syms_str))
        })?
        .filter_map(|r| r.ok())
        .map(|(target, syms_str)| {
            let syms: Vec<String> = sonic_rs::from_str(&syms_str).unwrap_or_default();
            (target, syms)
        })
        .collect();

    let mut dep_stmt = db.prepare_cached(
        "SELECT source_path, symbols FROM imports WHERE project = ?1 AND target_path = ?2",
    )?;
    let dependents: Vec<(String, Vec<String>)> = dep_stmt
        .query_map(rusqlite::params![project, file_path], |row| {
            let source: String = row.get(0)?;
            let syms_str: String = row.get(1)?;
            Ok((source, syms_str))
        })?
        .filter_map(|r| r.ok())
        .map(|(source, syms_str)| {
            let syms: Vec<String> = sonic_rs::from_str(&syms_str).unwrap_or_default();
            (source, syms)
        })
        .collect();

    Ok(Some(FileDetail {
        file_path: file_path.to_string(),
        language,
        symbols,
        imports,
        dependents,
    }))
}

#[cfg(test)]
#[path = "graph_tests.rs"]
mod tests;
