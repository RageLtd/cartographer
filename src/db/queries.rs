use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use surrealdb_types::SurrealValue;

use super::client::Db;
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

pub async fn upsert_file(
    db: &Db,
    project: &str,
    file_path: &str,
    language: &str,
    symbols: &[Symbol],
    content_hash: &str,
) -> Result<(), String> {
    let symbols_json = sonic_rs::to_string(symbols).unwrap_or_else(|_| "[]".to_string());
    let symbol_names: String = symbols.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join(" ");
    let searchable = format!("{file_path} {symbol_names}");
    let now = epoch_ms();

    db.query(
        "DELETE cart_file WHERE project = $project AND file_path = $file_path;
         CREATE cart_file SET
            project = $project,
            file_path = $file_path,
            language = $language,
            symbols = $symbols,
            symbol_names = $symbol_names,
            searchable = $searchable,
            content_hash = $content_hash,
            last_parsed_epoch = $now",
    )
    .bind(("project", project.to_string()))
    .bind(("file_path", file_path.to_string()))
    .bind(("language", language.to_string()))
    .bind(("symbols", symbols_json))
    .bind(("symbol_names", symbol_names))
    .bind(("searchable", searchable))
    .bind(("content_hash", content_hash.to_string()))
    .bind(("now", now))
    .await
    .map_err(|e| format!("Failed to upsert file {file_path}: {e}"))?;

    Ok(())
}

pub async fn get_file_hash(db: &Db, project: &str, file_path: &str) -> Result<Option<String>, String> {
    #[derive(SurrealValue)]
    struct Row {
        content_hash: String,
    }

    let mut result = db
        .query("SELECT content_hash FROM cart_file WHERE project = $project AND file_path = $file_path LIMIT 1")
        .bind(("project", project.to_string()))
        .bind(("file_path", file_path.to_string()))
        .await
        .map_err(|e| e.to_string())?;

    let row: Option<Row> = result.take(0).map_err(|e| e.to_string())?;
    Ok(row.map(|r| r.content_hash))
}

pub async fn remove_file(db: &Db, project: &str, file_path: &str) -> Result<(), String> {
    db.query(
        "DELETE cart_file WHERE project = $project AND file_path = $file_path;
         DELETE cart_import WHERE project = $project AND (source_path = $file_path OR target_path = $file_path)",
    )
    .bind(("project", project.to_string()))
    .bind(("file_path", file_path.to_string()))
    .await
    .map_err(|e| format!("Failed to remove file {file_path}: {e}"))?;

    Ok(())
}

pub async fn replace_imports(
    db: &Db,
    project: &str,
    source_path: &str,
    edges: &[ImportEdge],
) -> Result<(), String> {
    let now = epoch_ms();

    db.query("DELETE cart_import WHERE project = $project AND source_path = $source_path")
        .bind(("project", project.to_string()))
        .bind(("source_path", source_path.to_string()))
        .await
        .map_err(|e| format!("Failed to delete old imports: {e}"))?;

    for edge in edges {
        let symbols_json = sonic_rs::to_string(&edge.symbols).unwrap_or_else(|_| "[]".to_string());
        db.query(
            "CREATE cart_import SET
                project = $project,
                source_path = $source_path,
                target_path = $target_path,
                specifier = $specifier,
                symbols = $symbols,
                updated_at_epoch = $now",
        )
        .bind(("project", project.to_string()))
        .bind(("source_path", edge.source.clone()))
        .bind(("target_path", edge.target.clone()))
        .bind(("specifier", edge.specifier.clone()))
        .bind(("symbols", symbols_json))
        .bind(("now", now))
        .await
        .map_err(|e| format!("Failed to insert import edge: {e}"))?;
    }

    Ok(())
}

// ============================================================================
// Stats
// ============================================================================

pub async fn get_file_count(db: &Db, project: &str) -> Result<i64, String> {
    #[derive(SurrealValue)]
    struct Row {
        count: i64,
    }

    let mut result = db
        .query("SELECT count() as count FROM cart_file WHERE project = $project GROUP ALL")
        .bind(("project", project.to_string()))
        .await
        .map_err(|e| e.to_string())?;

    let row: Option<Row> = result.take(0).map_err(|e| e.to_string())?;
    Ok(row.map(|r| r.count).unwrap_or(0))
}

pub async fn get_language_counts(db: &Db, project: &str) -> Result<HashMap<String, usize>, String> {
    #[derive(SurrealValue)]
    struct Row {
        language: String,
        count: i64,
    }

    let mut result = db
        .query("SELECT language, count() as count FROM cart_file WHERE project = $project GROUP BY language")
        .bind(("project", project.to_string()))
        .await
        .map_err(|e| e.to_string())?;

    let rows: Vec<Row> = result.take(0).map_err(|e| e.to_string())?;
    Ok(rows.into_iter().map(|r| (r.language, r.count as usize)).collect())
}

pub async fn get_import_count(db: &Db, project: &str) -> Result<i64, String> {
    #[derive(SurrealValue)]
    struct Row {
        count: i64,
    }

    let mut result = db
        .query("SELECT count() as count FROM cart_import WHERE project = $project GROUP ALL")
        .bind(("project", project.to_string()))
        .await
        .map_err(|e| e.to_string())?;

    let row: Option<Row> = result.take(0).map_err(|e| e.to_string())?;
    Ok(row.map(|r| r.count).unwrap_or(0))
}

pub async fn get_project_stats(db: &Db) -> Result<Vec<(String, i64)>, String> {
    #[derive(SurrealValue)]
    struct Row {
        project: String,
        count: i64,
    }

    let mut result = db
        .query("SELECT project, count() as count FROM cart_file GROUP BY project")
        .await
        .map_err(|e| e.to_string())?;

    let rows: Vec<Row> = result.take(0).map_err(|e| e.to_string())?;
    Ok(rows.into_iter().map(|r| (r.project, r.count)).collect())
}

// ============================================================================
// Git state tracking
// ============================================================================

pub async fn get_last_git_status(db: &Db, project: &str) -> Result<HashMap<String, String>, String> {
    #[derive(SurrealValue)]
    struct Row {
        last_status: String,
    }

    let mut result = db
        .query("SELECT last_status FROM cart_git_state WHERE project = $project LIMIT 1")
        .bind(("project", project.to_string()))
        .await
        .map_err(|e| e.to_string())?;

    let row: Option<Row> = result.take(0).map_err(|e| e.to_string())?;
    Ok(row
        .and_then(|r| sonic_rs::from_str(&r.last_status).ok())
        .unwrap_or_default())
}

pub async fn save_git_status(
    db: &Db,
    project: &str,
    status: &HashMap<String, String>,
) -> Result<(), String> {
    let json = sonic_rs::to_string(status).unwrap_or_else(|_| "{}".to_string());
    let now = epoch_ms();

    db.query(
        "DELETE cart_git_state WHERE project = $project;
         CREATE cart_git_state SET
            project = $project,
            last_status = $json,
            updated_at_epoch = $now",
    )
    .bind(("project", project.to_string()))
    .bind(("json", json))
    .bind(("now", now))
    .await
    .map_err(|e| format!("Failed to save git status: {e}"))?;

    Ok(())
}
