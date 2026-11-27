use std::collections::{HashMap, VecDeque};

use rusqlite::{params, OptionalExtension};

use super::client::Db;
use super::search_words::search_words;
use crate::constants::{DEFAULT_MAX_DEPTH, DEFAULT_MAX_RESULTS};
use crate::types::{RelevantFile, Symbol};

// ============================================================================
// Graph queries
// ============================================================================

pub fn walk_import_graph(
    db: &Db,
    project: &str,
    entry_points: &[String],
    max_depth: Option<i64>,
    max_results: Option<i64>,
) -> Result<Vec<RelevantFile>, String> {
    if entry_points.is_empty() {
        return Ok(vec![]);
    }

    // The limits arrive from a tool call as signed numbers. Cast as it is, a
    // negative one would become enormous and unbound the walk.
    let max_depth = max_depth.unwrap_or(DEFAULT_MAX_DEPTH).max(0) as usize;
    let max_results = max_results.unwrap_or(DEFAULT_MAX_RESULTS).max(1) as usize;

    let mut visited: HashMap<String, (usize, String)> = HashMap::new();
    let mut queue: VecDeque<(String, usize, String)> = VecDeque::new();

    for ep in entry_points {
        if !visited.contains_key(ep) {
            visited.insert(ep.clone(), (0, "entry".to_string()));
            queue.push_back((ep.clone(), 0, "entry".to_string()));
        }
    }

    while let Some((file_path, depth, _reason)) = queue.pop_front() {
        if depth >= max_depth || visited.len() >= max_results {
            break;
        }

        let deps = get_imports_from(db, project, &file_path)?;
        for (dep, _) in &deps {
            if !visited.contains_key(dep) {
                visited.insert(dep.clone(), (depth + 1, "dependency".to_string()));
                queue.push_back((dep.clone(), depth + 1, "dependency".to_string()));
            }
        }

        let dependents = get_importers_of(db, project, &file_path)?;
        for (dep, _) in &dependents {
            if !visited.contains_key(dep) {
                visited.insert(dep.clone(), (depth + 1, "dependent".to_string()));
                queue.push_back((dep.clone(), depth + 1, "dependent".to_string()));
            }
        }
    }

    let mut results: Vec<RelevantFile> = Vec::new();
    for (file_path, (depth, reason)) in &visited {
        let symbols = get_file_symbols(db, project, file_path)?;
        let relative_path = file_path
            .strip_prefix(project)
            .unwrap_or(file_path)
            .trim_start_matches('/')
            .to_string();

        results.push(RelevantFile {
            file_path: file_path.clone(),
            relative_path,
            reason: reason.clone(),
            depth: *depth as i64,
            symbols,
        });
    }

    results.sort_by_key(|f| f.depth);
    results.truncate(max_results);
    Ok(results)
}

/// The files `source_path` imports, each with the symbols it imports from them.
fn get_imports_from(
    db: &Db,
    project: &str,
    source_path: &str,
) -> Result<Vec<(String, Vec<String>)>, String> {
    db.read(
        &format!("Failed to read the imports of {source_path}"),
        |conn| {
            conn.prepare(
                "SELECT target_path, symbols FROM imports WHERE project = ?1 AND source_path = ?2",
            )?
            .query_map([project, source_path], edge_row)?
            .collect()
        },
    )
}

/// The files importing `target_path`, each with the symbols it imports.
fn get_importers_of(
    db: &Db,
    project: &str,
    target_path: &str,
) -> Result<Vec<(String, Vec<String>)>, String> {
    db.read(
        &format!("Failed to read the importers of {target_path}"),
        |conn| {
            conn.prepare(
                "SELECT source_path, symbols FROM imports WHERE project = ?1 AND target_path = ?2",
            )?
            .query_map([project, target_path], edge_row)?
            .collect()
        },
    )
}

fn get_file_symbols(db: &Db, project: &str, file_path: &str) -> Result<Vec<Symbol>, String> {
    let symbols: Option<String> = db.read(
        &format!("Failed to read the symbols of {file_path}"),
        |conn| {
            conn.query_row(
                "SELECT symbols FROM files WHERE project = ?1 AND file_path = ?2",
                [project, file_path],
                |row| row.get(0),
            )
            .optional()
        },
    )?;
    Ok(symbols
        .and_then(|s| sonic_rs::from_str(&s).ok())
        .unwrap_or_default())
}

// ============================================================================
// Full-text search
// ============================================================================

pub fn search_files(
    db: &Db,
    project: &str,
    query: &str,
    limit: i64,
) -> Result<Vec<(String, Vec<Symbol>)>, String> {
    let Some(expression) = match_expression(query) else {
        return Ok(vec![]);
    };

    let rows: Vec<(String, String)> =
        db.read(&format!("Failed to search for {query:?}"), |conn| {
            conn.prepare(
                "SELECT files.file_path, files.symbols
             FROM file_search JOIN files ON files.id = file_search.rowid
             WHERE file_search MATCH ?1 AND files.project = ?2
             ORDER BY bm25(file_search)
             LIMIT ?3",
            )?
            // To SQLite a negative LIMIT means no limit.
            .query_map(params![expression, project, limit.max(0)], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })?
            .collect()
        })?;
    Ok(rows
        .into_iter()
        .map(|(file_path, symbols)| {
            let symbols: Vec<Symbol> = sonic_rs::from_str(&symbols).unwrap_or_default();
            (file_path, symbols)
        })
        .collect())
}

/// The FTS5 expression requiring every word of `query`, or `None` when it has
/// none. Each word is quoted, so nothing in a query is ever read as FTS5
/// syntax.
fn match_expression(query: &str) -> Option<String> {
    let terms: Vec<String> = search_words(query)
        .into_iter()
        .map(|word| format!("\"{word}\""))
        .collect();
    (!terms.is_empty()).then(|| terms.join(" "))
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
    db: &Db,
    project: &str,
    file_path: &str,
) -> Result<Option<FileDetail>, String> {
    let file_row: Option<(String, String)> =
        db.read(&format!("Failed to read file {file_path}"), |conn| {
            conn.query_row(
                "SELECT language, symbols FROM files WHERE project = ?1 AND file_path = ?2",
                [project, file_path],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
        })?;
    let (language, symbols) = match file_row {
        Some(r) => r,
        None => return Ok(None),
    };

    let symbols: Vec<Symbol> = sonic_rs::from_str(&symbols).unwrap_or_default();

    Ok(Some(FileDetail {
        file_path: file_path.to_string(),
        language,
        symbols,
        imports: get_imports_from(db, project, file_path)?,
        dependents: get_importers_of(db, project, file_path)?,
    }))
}

/// A (path, imported symbols) pair from a row of path and symbols JSON.
fn edge_row(row: &rusqlite::Row) -> rusqlite::Result<(String, Vec<String>)> {
    let symbols: String = row.get(1)?;
    Ok((
        row.get(0)?,
        sonic_rs::from_str(&symbols).unwrap_or_default(),
    ))
}

#[cfg(test)]
#[path = "graph_tests.rs"]
mod tests;
