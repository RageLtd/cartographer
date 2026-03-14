use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process::Command;

use rusqlite::Connection;

use crate::constants::{SKIP_DIRS, SUPPORTED_EXTENSIONS};
use crate::db::queries::{
    get_file_hash, remove_file, replace_imports, save_git_status, upsert_file,
};
use crate::parser::{hash_file, parse_file};

// ============================================================================
// Git status
// ============================================================================

pub fn get_current_git_status(project_root: &str) -> HashMap<String, String> {
    let mut status = HashMap::new();

    let output = Command::new("git")
        .args(["status", "--porcelain=v1"])
        .current_dir(project_root)
        .output();

    let output = match output {
        Ok(o) if o.status.success() => o,
        _ => return status,
    };

    let raw = String::from_utf8_lossy(&output.stdout);
    for line in raw.lines().filter(|l| !l.is_empty()) {
        if line.len() < 4 {
            continue;
        }
        let status_code = line[..2].trim().to_string();
        let file_rel = &line[3..];
        let file_path = Path::new(project_root).join(file_rel);
        let file_path_str = file_path.to_string_lossy().to_string();

        let ext = Path::new(&file_path_str)
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| format!(".{e}"))
            .unwrap_or_default();

        if !SUPPORTED_EXTENSIONS.contains(ext.as_str()) {
            continue;
        }

        status.insert(file_path_str, status_code);
    }

    status
}

pub fn diff_git_status(
    last_status: &HashMap<String, String>,
    current_status: &HashMap<String, String>,
) -> (Vec<String>, Vec<String>) {
    let mut modified = Vec::new();
    let mut deleted = Vec::new();

    for (file_path, status) in current_status {
        if status == "D" || status == "D " {
            deleted.push(file_path.clone());
        } else if last_status.get(file_path) != Some(status) {
            modified.push(file_path.clone());
        }
    }

    // Files in last but not in current — check if they still exist
    for file_path in last_status.keys() {
        if !current_status.contains_key(file_path) && !Path::new(file_path).exists() {
            deleted.push(file_path.clone());
        }
    }

    (modified, deleted)
}

// ============================================================================
// Full index
// ============================================================================

pub fn full_index(
    db: &Connection,
    project: &str,
    project_root: &str,
) -> Result<(usize, usize), String> {
    let files = walk_project_files(project_root);
    let (indexed, skipped) = files.iter().fold((0, 0), |(ok, err), file_path| {
        match index_single_file(db, project, file_path) {
            Ok(()) => (ok + 1, err),
            Err(_) => (ok, err + 1),
        }
    });

    let status = get_current_git_status(project_root);
    save_git_status(db, project, &status).map_err(|e| format!("Failed to save git status: {e}"))?;

    Ok((indexed, skipped))
}

// ============================================================================
// Incremental index
// ============================================================================

pub fn incremental_index(
    db: &Connection,
    project: &str,
    modified: &[String],
    deleted: &[String],
) -> Result<(usize, usize), String> {
    let mut indexed = 0;
    let mut removed = 0;

    for file_path in deleted {
        remove_file(db, project, file_path)
            .map_err(|e| format!("Failed to remove {file_path}: {e}"))?;
        removed += 1;
    }

    for file_path in modified {
        if !Path::new(file_path).exists() {
            remove_file(db, project, file_path)
                .map_err(|e| format!("Failed to remove {file_path}: {e}"))?;
            removed += 1;
            continue;
        }

        match index_single_file(db, project, file_path) {
            Ok(()) => indexed += 1,
            Err(e) => {
                tracing::warn!("Failed to index {file_path}: {e}");
            }
        }
    }

    Ok((indexed, removed))
}

// ============================================================================
// Single file index
// ============================================================================

pub fn index_single_file(db: &Connection, project: &str, file_path: &str) -> Result<(), String> {
    let hash = hash_file(file_path)?;

    // Skip re-parsing if the file content hasn't changed
    if let Ok(Some(existing_hash)) = get_file_hash(db, project, file_path) {
        if existing_hash == hash {
            return Ok(());
        }
    }

    let result = parse_file(file_path, None)?;
    upsert_file(
        db,
        project,
        file_path,
        &result.language,
        &result.symbols,
        &hash,
    )
    .map_err(|e| format!("Failed to upsert {file_path}: {e}"))?;
    replace_imports(db, project, file_path, &result.imports)
        .map_err(|e| format!("Failed to replace imports for {file_path}: {e}"))?;
    Ok(())
}

// ============================================================================
// File walking
// ============================================================================

pub fn walk_project_files(dir: &str) -> Vec<String> {
    let mut files = Vec::new();
    walk_recursive(Path::new(dir), &mut files);
    files
}

fn walk_recursive(dir: &Path, files: &mut Vec<String>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if path.is_dir() {
            if SKIP_DIRS.contains(name_str.as_ref()) || name_str.starts_with('.') {
                continue;
            }
            walk_recursive(&path, files);
        } else if path.is_file() {
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| format!(".{e}"))
                .unwrap_or_default();
            if SUPPORTED_EXTENSIONS.contains(ext.as_str()) {
                if let Some(s) = path.to_str() {
                    files.push(s.to_string());
                }
            }
        }
    }
}
