use std::collections::HashSet;
use std::fs;
use std::path::Path;

use crate::constants::SKIP_DIRS;
use crate::db::client::Db;
use crate::db::queries::{
    get_file_hash, get_file_paths, remove_file, replace_imports, upsert_file,
};
use crate::parser::registry::LanguageRegistry;
use crate::parser::{hash_file, parse_file};

// ============================================================================
// Reconciling the index with the files on disk
// ============================================================================

/// What a reconcile changed in the index, by absolute path.
pub struct Changes {
    /// Files that were new or whose content had changed.
    pub reparsed: Vec<String>,
    /// Indexed files that are no longer on disk.
    pub removed: Vec<String>,
}

struct Reconciled {
    changes: Changes,
    unchanged: usize,
    /// Files on disk that could not be read or parsed.
    failed: usize,
}

/// Brings the index in line with the project's files. Returns how many files
/// are indexed and how many could not be read or parsed.
pub fn full_index(
    db: &Db,
    registry: &LanguageRegistry,
    project: &str,
    project_root: &str,
) -> Result<(usize, usize), String> {
    let reconciled = reconcile(db, registry, project, project_root)?;
    Ok((
        reconciled.changes.reparsed.len() + reconciled.unchanged,
        reconciled.failed,
    ))
}

/// Brings the index in line with the project's files and says what changed.
pub fn detect_changes(
    db: &Db,
    registry: &LanguageRegistry,
    project: &str,
    project_root: &str,
) -> Result<Changes, String> {
    Ok(reconcile(db, registry, project, project_root)?.changes)
}

/// Compares content hashes, never git: a pull, a checkout or a revert changes
/// files without leaving anything for `git status` to report.
///
/// A file that cannot be read or parsed is counted and left as it was; a store
/// failure stops the reconcile.
fn reconcile(
    db: &Db,
    registry: &LanguageRegistry,
    project: &str,
    project_root: &str,
) -> Result<Reconciled, String> {
    let on_disk = walk_project_files(registry, project_root);
    let on_disk_set: HashSet<&str> = on_disk.iter().map(String::as_str).collect();

    let mut removed = Vec::new();
    for file_path in get_file_paths(db, project)? {
        if !on_disk_set.contains(file_path.as_str()) {
            remove_file(db, project, &file_path)?;
            removed.push(file_path);
        }
    }

    let mut reparsed = Vec::new();
    let mut unchanged = Vec::new();
    let mut added = false;
    let mut failed = 0;
    for file_path in &on_disk {
        let hash = match hash_file(file_path) {
            Ok(hash) => hash,
            Err(e) => {
                tracing::warn!("Skipping {file_path}: {e}");
                failed += 1;
                continue;
            }
        };
        let stored = get_file_hash(db, project, file_path)?;
        if stored.as_deref() == Some(hash.as_str()) {
            unchanged.push(file_path);
            continue;
        }
        let parsed = match parse_file(registry, file_path) {
            Ok(parsed) => parsed,
            Err(e) => {
                tracing::warn!("Skipping {file_path}: {e}");
                failed += 1;
                continue;
            }
        };
        upsert_file(
            db,
            project,
            file_path,
            &parsed.language,
            &parsed.symbols,
            &hash,
        )?;
        replace_imports(db, project, file_path, &parsed.imports)?;
        added |= stored.is_none();
        reparsed.push(file_path.clone());
    }

    // An import resolves against the files that exist, so a file appearing or
    // disappearing changes the edges of importers whose own content did not
    // change. Which importers is not recorded (an unresolved import stores no
    // edge), so all of them are resolved again.
    if added || !removed.is_empty() {
        for file_path in &unchanged {
            match parse_file(registry, file_path) {
                Ok(parsed) => replace_imports(db, project, file_path, &parsed.imports)?,
                Err(e) => tracing::warn!("Could not resolve the imports of {file_path}: {e}"),
            }
        }
    }

    Ok(Reconciled {
        changes: Changes { reparsed, removed },
        unchanged: unchanged.len(),
        failed,
    })
}

// ============================================================================
// File walking
// ============================================================================

pub fn walk_project_files(registry: &LanguageRegistry, dir: &str) -> Vec<String> {
    let mut files = Vec::new();
    walk_recursive(registry, Path::new(dir), &mut files);
    files
}

fn walk_recursive(registry: &LanguageRegistry, dir: &Path, files: &mut Vec<String>) {
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
            walk_recursive(registry, &path, files);
        } else if path.is_file() {
            if let Some(s) = path.to_str() {
                if registry.supports_path(s) {
                    files.push(s.to_string());
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "indexer_tests.rs"]
mod tests;
