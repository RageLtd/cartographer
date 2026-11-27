use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, OptionalExtension};

use super::client::Db;
use super::search_words::search_words;
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
    db: &Db,
    project: &str,
    file_path: &str,
    language: &str,
    symbols: &[Symbol],
    content_hash: &str,
) -> Result<(), String> {
    let symbols_json = sonic_rs::to_string(symbols)
        .map_err(|e| format!("Failed to serialize the symbols of {file_path}: {e}"))?;
    let mut words = search_words(file_path);
    for symbol in symbols {
        words.extend(search_words(&symbol.name));
    }
    let searchable = words.join(" ");
    let now = epoch_ms();

    db.write(&format!("Failed to upsert file {file_path}"), |tx| {
        let id: i64 = tx.query_row(
            "INSERT INTO files
                (project, file_path, language, symbols, content_hash, last_parsed_epoch)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (project, file_path) DO UPDATE SET
                language = excluded.language,
                symbols = excluded.symbols,
                content_hash = excluded.content_hash,
                last_parsed_epoch = excluded.last_parsed_epoch
             RETURNING id",
            params![
                project,
                file_path,
                language,
                symbols_json,
                content_hash,
                now
            ],
            |row| row.get(0),
        )?;
        tx.execute("DELETE FROM file_search WHERE rowid = ?1", [id])?;
        tx.execute(
            "INSERT INTO file_search (rowid, searchable) VALUES (?1, ?2)",
            params![id, searchable],
        )?;
        Ok(())
    })
}

pub fn get_file_hash(db: &Db, project: &str, file_path: &str) -> Result<Option<String>, String> {
    db.read(&format!("Failed to read the hash of {file_path}"), |conn| {
        conn.query_row(
            "SELECT content_hash FROM files WHERE project = ?1 AND file_path = ?2",
            [project, file_path],
            |row| row.get(0),
        )
        .optional()
    })
}

/// Every file indexed for `project`.
pub fn get_file_paths(db: &Db, project: &str) -> Result<Vec<String>, String> {
    db.read("Failed to list the indexed files", |conn| {
        conn.prepare("SELECT file_path FROM files WHERE project = ?1")?
            .query_map([project], |row| row.get(0))?
            .collect()
    })
}

pub fn remove_file(db: &Db, project: &str, file_path: &str) -> Result<(), String> {
    db.write(&format!("Failed to remove file {file_path}"), |tx| {
        tx.execute(
            "DELETE FROM file_search WHERE rowid IN
                (SELECT id FROM files WHERE project = ?1 AND file_path = ?2)",
            [project, file_path],
        )?;
        tx.execute(
            "DELETE FROM files WHERE project = ?1 AND file_path = ?2",
            [project, file_path],
        )?;
        tx.execute(
            "DELETE FROM imports
             WHERE project = ?1 AND (source_path = ?2 OR target_path = ?2)",
            [project, file_path],
        )?;
        Ok(())
    })
}

pub fn replace_imports(
    db: &Db,
    project: &str,
    source_path: &str,
    edges: &[ImportEdge],
) -> Result<(), String> {
    let now = epoch_ms();
    let edges: Vec<(ImportEdge, String)> = merge_duplicate_edges(edges)
        .into_iter()
        .map(|edge| sonic_rs::to_string(&edge.symbols).map(|symbols_json| (edge, symbols_json)))
        .collect::<Result<_, _>>()
        .map_err(|e| format!("Failed to serialize the imports of {source_path}: {e}"))?;

    db.write(
        &format!("Failed to replace the imports of {source_path}"),
        |tx| {
            tx.execute(
                "DELETE FROM imports WHERE project = ?1 AND source_path = ?2",
                [project, source_path],
            )?;
            let mut insert = tx.prepare(
                "INSERT INTO imports
                    (project, source_path, target_path, specifier, symbols, is_dependency,
                     updated_at_epoch)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )?;
            for (edge, symbols_json) in &edges {
                insert.execute(params![
                    project,
                    edge.source,
                    edge.target,
                    edge.specifier,
                    symbols_json,
                    edge.kind.is_dependency(),
                    now
                ])?;
            }
            Ok(())
        },
    )
}

/// One edge per source, target and specifier, carrying the symbols of every
/// declaration that produced it: `import type { A } from "./b"` and
/// `import { b } from "./b"` are one edge.
fn merge_duplicate_edges(edges: &[ImportEdge]) -> Vec<ImportEdge> {
    let mut merged: Vec<ImportEdge> = Vec::new();
    for edge in edges {
        let existing = merged.iter_mut().find(|m| {
            m.source == edge.source && m.target == edge.target && m.specifier == edge.specifier
        });
        match existing {
            Some(m) => {
                for symbol in &edge.symbols {
                    if !m.symbols.contains(symbol) {
                        m.symbols.push(symbol.clone());
                    }
                }
            }
            None => merged.push(edge.clone()),
        }
    }
    merged
}

// ============================================================================
// Stats
// ============================================================================

/// Indexed files in `project` whose path ends with the path `suffix` — resolves
/// bare file mentions such as `auth.rs` to full paths. The suffix matches whole
/// path segments, so `auth.rs` does not find `oauth.rs`.
pub fn find_files_by_suffix(
    db: &Db,
    project: &str,
    suffix: &str,
    limit: i64,
) -> Result<Vec<String>, String> {
    // GLOB rather than LIKE: it is case-sensitive, like paths.
    // The `/` keeps the match on a segment boundary. A suffix that brings its
    // own is either a whole path or one written from the project's root.
    let separator = if suffix.starts_with('/') { "" } else { "/" };
    let pattern = format!("*{separator}{}", glob_literal(suffix));
    db.read(
        &format!("Failed to search files by suffix {suffix:?}"),
        |conn| {
            conn.prepare(
                "SELECT file_path FROM files WHERE project = ?1 AND file_path GLOB ?2 LIMIT ?3",
            )?
            .query_map(params![project, pattern, limit], |row| row.get(0))?
            .collect()
        },
    )
}

/// `text` as a GLOB pattern that matches only itself.
fn glob_literal(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '*' | '?' | '[' => format!("[{c}]"),
            _ => c.to_string(),
        })
        .collect()
}

pub fn get_file_count(db: &Db, project: &str) -> Result<i64, String> {
    db.read("Failed to count files", |conn| {
        conn.query_row(
            "SELECT count(*) FROM files WHERE project = ?1",
            [project],
            |row| row.get(0),
        )
    })
}

pub fn get_language_counts(db: &Db, project: &str) -> Result<HashMap<String, usize>, String> {
    db.read("Failed to count files by language", |conn| {
        conn.prepare("SELECT language, count(*) FROM files WHERE project = ?1 GROUP BY language")?
            .query_map([project], |row| {
                Ok((row.get(0)?, row.get::<_, i64>(1)? as usize))
            })?
            .collect()
    })
}

pub fn get_import_count(db: &Db, project: &str) -> Result<i64, String> {
    db.read("Failed to count imports", |conn| {
        conn.query_row(
            "SELECT count(*) FROM imports WHERE project = ?1",
            [project],
            |row| row.get(0),
        )
    })
}

pub fn get_project_stats(db: &Db) -> Result<Vec<(String, i64)>, String> {
    db.read("Failed to count files by project", |conn| {
        conn.prepare("SELECT project, count(*) FROM files GROUP BY project")?
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect()
    })
}

#[cfg(test)]
#[path = "queries_tests.rs"]
mod tests;
