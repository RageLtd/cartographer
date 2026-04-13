use std::collections::{HashMap, HashSet, VecDeque};

use surrealdb_types::SurrealValue;

use super::client::Db;
use crate::constants::{DEFAULT_MAX_DEPTH, DEFAULT_MAX_RESULTS};
use crate::types::{RelevantFile, Symbol};

// ============================================================================
// Graph queries
// ============================================================================

pub async fn walk_import_graph(
    db: &Db,
    project: &str,
    entry_points: &[String],
    max_depth: Option<i64>,
    max_results: Option<i64>,
) -> Result<Vec<RelevantFile>, String> {
    if entry_points.is_empty() {
        return Ok(vec![]);
    }

    let max_depth = max_depth.unwrap_or(DEFAULT_MAX_DEPTH) as usize;
    let max_results = max_results.unwrap_or(DEFAULT_MAX_RESULTS) as usize;

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

        let deps = get_imports_from(db, project, &file_path).await?;
        for dep in &deps {
            if !visited.contains_key(dep) {
                visited.insert(dep.clone(), (depth + 1, "dependency".to_string()));
                queue.push_back((dep.clone(), depth + 1, "dependency".to_string()));
            }
        }

        let dependents = get_importers_of(db, project, &file_path).await?;
        for dep in &dependents {
            if !visited.contains_key(dep) {
                visited.insert(dep.clone(), (depth + 1, "dependent".to_string()));
                queue.push_back((dep.clone(), depth + 1, "dependent".to_string()));
            }
        }
    }

    let mut results: Vec<RelevantFile> = Vec::new();
    for (file_path, (depth, reason)) in &visited {
        let symbols = get_file_symbols(db, project, file_path).await?;
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

async fn get_imports_from(
    db: &Db,
    project: &str,
    source_path: &str,
) -> Result<Vec<String>, String> {
    #[derive(SurrealValue)]
    struct Row {
        target_path: String,
    }

    let mut result = db
        .query("SELECT target_path FROM cart_import WHERE project = $project AND source_path = $source")
        .bind(("project", project.to_string()))
        .bind(("source", source_path.to_string()))
        .await
        .map_err(|e| e.to_string())?;

    let rows: Vec<Row> = result.take(0).map_err(|e| e.to_string())?;
    Ok(rows.into_iter().map(|r| r.target_path).collect())
}

async fn get_importers_of(
    db: &Db,
    project: &str,
    target_path: &str,
) -> Result<Vec<String>, String> {
    #[derive(SurrealValue)]
    struct Row {
        source_path: String,
    }

    let mut result = db
        .query("SELECT source_path FROM cart_import WHERE project = $project AND target_path = $target")
        .bind(("project", project.to_string()))
        .bind(("target", target_path.to_string()))
        .await
        .map_err(|e| e.to_string())?;

    let rows: Vec<Row> = result.take(0).map_err(|e| e.to_string())?;
    Ok(rows.into_iter().map(|r| r.source_path).collect())
}

async fn get_file_symbols(db: &Db, project: &str, file_path: &str) -> Result<Vec<Symbol>, String> {
    #[derive(SurrealValue)]
    struct Row {
        symbols: String,
    }

    let mut result = db
        .query("SELECT symbols FROM cart_file WHERE project = $project AND file_path = $file_path LIMIT 1")
        .bind(("project", project.to_string()))
        .bind(("file_path", file_path.to_string()))
        .await
        .map_err(|e| e.to_string())?;

    let row: Option<Row> = result.take(0).map_err(|e| e.to_string())?;
    Ok(row
        .and_then(|r| sonic_rs::from_str(&r.symbols).ok())
        .unwrap_or_default())
}

// ============================================================================
// Full-text search
// ============================================================================

pub async fn search_files(
    db: &Db,
    project: &str,
    query: &str,
    limit: i64,
) -> Result<Vec<(String, Vec<Symbol>)>, String> {
    #[derive(SurrealValue)]
    struct Row {
        file_path: String,
        symbols: String,
    }

    let mut result = db
        .query(
            "SELECT file_path, symbols, search::score(1) AS score
             FROM cart_file
             WHERE project = $project AND searchable @1@ $query
             ORDER BY score DESC
             LIMIT $limit",
        )
        .bind(("project", project.to_string()))
        .bind(("query", query.to_string()))
        .bind(("limit", limit))
        .await
        .map_err(|e| e.to_string())?;

    let rows: Vec<Row> = result.take(0).map_err(|e| e.to_string())?;
    Ok(rows
        .into_iter()
        .map(|r| {
            let symbols: Vec<Symbol> = sonic_rs::from_str(&r.symbols).unwrap_or_default();
            (r.file_path, symbols)
        })
        .collect())
}

// ============================================================================
// Cycle detection — DFS-based
// ============================================================================

pub async fn find_cycles(db: &Db, project: &str) -> Result<Vec<Vec<String>>, String> {
    #[derive(SurrealValue)]
    struct Edge {
        source_path: String,
        target_path: String,
    }

    let mut result = db
        .query("SELECT source_path, target_path FROM cart_import WHERE project = $project")
        .bind(("project", project.to_string()))
        .await
        .map_err(|e| e.to_string())?;

    let edges: Vec<Edge> = result.take(0).map_err(|e| e.to_string())?;

    let mut graph: HashMap<String, Vec<String>> = HashMap::new();
    for edge in &edges {
        graph
            .entry(edge.source_path.clone())
            .or_default()
            .push(edge.target_path.clone());
    }

    let mut all_cycles: Vec<Vec<String>> = Vec::new();
    let mut seen_cycles: HashSet<String> = HashSet::new();

    for start in graph.keys() {
        let mut stack: Vec<(String, Vec<String>)> = vec![(start.clone(), vec![start.clone()])];
        let mut visited_in_path: HashSet<String> = HashSet::new();
        visited_in_path.insert(start.clone());

        while let Some((current, path)) = stack.pop() {
            if let Some(neighbors) = graph.get(&current) {
                for next in neighbors {
                    if next == start && path.len() > 1 {
                        let mut cycle = path.clone();
                        cycle.push(start.clone());

                        if let Some(min_pos) = cycle
                            .iter()
                            .take(cycle.len() - 1)
                            .enumerate()
                            .min_by_key(|(_, v)| v.as_str())
                            .map(|(i, _)| i)
                        {
                            let normalized: Vec<String> = cycle[min_pos..cycle.len() - 1]
                                .iter()
                                .chain(cycle[..min_pos].iter())
                                .cloned()
                                .collect();
                            let key = normalized.join("|");
                            if seen_cycles.insert(key) {
                                let mut final_cycle = normalized;
                                final_cycle.push(final_cycle[0].clone());
                                all_cycles.push(final_cycle);
                            }
                        }
                    } else if !visited_in_path.contains(next) && path.len() < 20 {
                        let mut new_path = path.clone();
                        new_path.push(next.clone());
                        visited_in_path.insert(next.clone());
                        stack.push((next.clone(), new_path));
                    }
                }
            }
        }
    }

    Ok(all_cycles)
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

pub async fn get_file_detail(
    db: &Db,
    project: &str,
    file_path: &str,
) -> Result<Option<FileDetail>, String> {
    #[derive(SurrealValue)]
    struct FileRow {
        language: String,
        symbols: String,
    }

    let mut result = db
        .query("SELECT language, symbols FROM cart_file WHERE project = $project AND file_path = $file_path LIMIT 1")
        .bind(("project", project.to_string()))
        .bind(("file_path", file_path.to_string()))
        .await
        .map_err(|e| e.to_string())?;

    let file_row: Option<FileRow> = result.take(0).map_err(|e| e.to_string())?;
    let file_row = match file_row {
        Some(r) => r,
        None => return Ok(None),
    };

    let symbols: Vec<Symbol> = sonic_rs::from_str(&file_row.symbols).unwrap_or_default();

    #[derive(SurrealValue)]
    struct ImportRow {
        target_path: String,
        symbols: String,
    }

    let mut imp_result = db
        .query("SELECT target_path, symbols FROM cart_import WHERE project = $project AND source_path = $file_path")
        .bind(("project", project.to_string()))
        .bind(("file_path", file_path.to_string()))
        .await
        .map_err(|e| e.to_string())?;

    let imp_rows: Vec<ImportRow> = imp_result.take(0).map_err(|e| e.to_string())?;
    let imports: Vec<(String, Vec<String>)> = imp_rows
        .into_iter()
        .map(|r| {
            let syms: Vec<String> = sonic_rs::from_str(&r.symbols).unwrap_or_default();
            (r.target_path, syms)
        })
        .collect();

    #[derive(SurrealValue)]
    struct DepRow {
        source_path: String,
        symbols: String,
    }

    let mut dep_result = db
        .query("SELECT source_path, symbols FROM cart_import WHERE project = $project AND target_path = $file_path")
        .bind(("project", project.to_string()))
        .bind(("file_path", file_path.to_string()))
        .await
        .map_err(|e| e.to_string())?;

    let dep_rows: Vec<DepRow> = dep_result.take(0).map_err(|e| e.to_string())?;
    let dependents: Vec<(String, Vec<String>)> = dep_rows
        .into_iter()
        .map(|r| {
            let syms: Vec<String> = sonic_rs::from_str(&r.symbols).unwrap_or_default();
            (r.source_path, syms)
        })
        .collect();

    Ok(Some(FileDetail {
        file_path: file_path.to_string(),
        language: file_row.language,
        symbols,
        imports,
        dependents,
    }))
}
