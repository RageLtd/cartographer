//! Imports of a workspace package by its name, such as
//! `@scope/core/util` from another package of the same monorepo. The packages
//! are local code, so the import is an edge like a relative one.
//!
//! A workspace is the nearest directory above the importing file whose
//! `package.json` has `workspaces`. Nothing is cached between files: a
//! `Workspace` is read from disk each time, so an edited manifest is never
//! stale in a long-running server.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use sonic_rs::{JsonContainerTrait, JsonValueTrait, Value};

use super::resolver::{normalize_path, resolve_ts_js_path, typescript_source_of};

const MANIFEST_FILE: &str = "package.json";
const WILDCARD: char = '*';
/// How an `exports` key or target is written relative to its package.
const PACKAGE_RELATIVE: &str = "./";
/// Directories a workspace pattern never means, even when a wildcard matches.
const DEPENDENCY_DIR: &str = "node_modules";

#[derive(Deserialize)]
struct Manifest {
    name: Option<String>,
    workspaces: Option<Value>,
    exports: Option<Value>,
    module: Option<String>,
    main: Option<String>,
}

pub struct Workspace {
    packages: Vec<Package>,
}

struct Package {
    name: String,
    dir: PathBuf,
    manifest: Manifest,
}

impl Workspace {
    /// The workspace `file` belongs to, if it is inside one.
    pub fn containing(file: &Path) -> Option<Workspace> {
        file.parent()?.ancestors().find_map(|dir| {
            let patterns = workspace_patterns(&read_manifest(dir)?)?;
            let packages = patterns
                .iter()
                .flat_map(|pattern| matching_dirs(dir, pattern))
                .filter_map(package_at)
                .collect();
            Some(Workspace { packages })
        })
    }

    /// The file `specifier` names, when it starts with the name of a package
    /// in this workspace and that package exposes the rest of it.
    pub fn resolve(&self, specifier: &str) -> Option<String> {
        let (package, subpath) = self.packages.iter().find_map(|package| {
            let rest = specifier.strip_prefix(package.name.as_str())?;
            if rest.is_empty() {
                Some((package, ""))
            } else {
                rest.strip_prefix('/').map(|subpath| (package, subpath))
            }
        })?;
        package.resolve(subpath)
    }
}

impl Package {
    /// `subpath` is what follows the package name, empty for the bare name.
    fn resolve(&self, subpath: &str) -> Option<String> {
        match &self.manifest.exports {
            // With an exports map, only what it lists can be imported.
            Some(exports) => exported_targets(exports, subpath)
                .iter()
                .find_map(|target| self.exported_file(target)),
            None if subpath.is_empty() => [&self.manifest.module, &self.manifest.main]
                .into_iter()
                .flatten()
                .find_map(|entry| self.file_inside(entry))
                // Neither field: the package's index file.
                .or_else(|| self.file_inside("")),
            None => self.file_inside(subpath),
        }
    }

    /// The file an exports target names. Exports name files exactly, so
    /// nothing is probed beyond a `.js` target meaning its TypeScript source.
    fn exported_file(&self, target: &str) -> Option<String> {
        let candidate = self.inside(target.strip_prefix(PACKAGE_RELATIVE)?)?;
        let file = typescript_source_of(&candidate)
            .or_else(|| candidate.is_file().then_some(candidate))?;
        file.to_str().map(str::to_string)
    }

    /// A path inside the package, resolved the way a relative import is.
    fn file_inside(&self, relative: &str) -> Option<String> {
        let resolved = resolve_ts_js_path(&self.inside(relative)?)?;
        Path::new(&resolved)
            .starts_with(&self.dir)
            .then_some(resolved)
    }

    /// `relative` under the package directory, unless it climbs out of it.
    fn inside(&self, relative: &str) -> Option<PathBuf> {
        let path = normalize_path(&self.dir.join(relative));
        path.starts_with(&self.dir).then_some(path)
    }
}

// ============================================================================
// Finding the packages
// ============================================================================

/// The manifest in `dir`. A directory without one is ordinary; one that
/// cannot be read or parsed is reported and treated as absent.
fn read_manifest(dir: &Path) -> Option<Manifest> {
    let path = dir.join(MANIFEST_FILE);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == ErrorKind::NotFound => return None,
        Err(e) => {
            tracing::warn!("Could not read {}: {e}", path.display());
            return None;
        }
    };
    match sonic_rs::from_str(&text) {
        Ok(manifest) => Some(manifest),
        Err(e) => {
            tracing::warn!("Could not parse {}: {e}", path.display());
            None
        }
    }
}

/// The patterns naming a workspace's package directories: `workspaces` as a
/// list (npm, Yarn) or as an object with a `packages` list (Bun, Yarn).
fn workspace_patterns(manifest: &Manifest) -> Option<Vec<String>> {
    let workspaces = manifest.workspaces.as_ref()?;
    let list = match workspaces.as_array() {
        Some(list) => list,
        None => workspaces.get("packages")?.as_array()?,
    };
    Some(
        list.iter()
            .filter_map(|pattern| pattern.as_str().map(str::to_string))
            .collect(),
    )
}

/// The directories under `root` that `pattern` names. A segment may hold one
/// `*`; anything else, such as `**` or a `!` exclusion, matches nothing.
fn matching_dirs(root: &Path, pattern: &str) -> Vec<PathBuf> {
    pattern
        .split('/')
        .filter(|segment| !segment.is_empty() && *segment != ".")
        .fold(vec![root.to_path_buf()], |dirs, segment| {
            dirs.iter()
                .flat_map(|dir| child_dirs_matching(dir, segment))
                .collect()
        })
}

fn child_dirs_matching(dir: &Path, segment: &str) -> Vec<PathBuf> {
    if !segment.contains(WILDCARD) {
        let child = dir.join(segment);
        return if child.is_dir() {
            vec![child]
        } else {
            Vec::new()
        };
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name != DEPENDENCY_DIR
                && !name.starts_with('.')
                && wildcard_capture(segment, &name).is_some()
        })
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect()
}

fn package_at(dir: PathBuf) -> Option<Package> {
    let manifest = read_manifest(&dir)?;
    let name = manifest.name.clone()?;
    Some(Package {
        name,
        dir,
        manifest,
    })
}

/// What the `*` of `pattern` stands for in `text`, when `pattern` has exactly
/// one and the rest of it matches.
fn wildcard_capture<'a>(pattern: &str, text: &'a str) -> Option<&'a str> {
    let (prefix, suffix) = pattern.split_once(WILDCARD)?;
    if suffix.contains(WILDCARD) {
        return None;
    }
    text.strip_prefix(prefix)?.strip_suffix(suffix)
}

// ============================================================================
// Reading an exports map
// ============================================================================

/// The targets `exports` gives for `subpath`, in the order to try them.
///
/// This is simpler than Node's rules on purpose. Node picks the conditions
/// one runtime has active; the graph wants the source file whatever would
/// load it, so every condition is a candidate and the caller takes the first
/// whose file exists.
fn exported_targets(exports: &Value, subpath: &str) -> Vec<String> {
    let subpaths = exports
        .as_object()
        .filter(|map| map.iter().any(|(key, _)| key.starts_with('.')));
    let Some(subpaths) = subpaths else {
        // A target or conditions for the package itself: nothing else is exposed.
        return if subpath.is_empty() {
            targets(exports)
        } else {
            Vec::new()
        };
    };

    let key = if subpath.is_empty() {
        ".".to_string()
    } else {
        format!("{PACKAGE_RELATIVE}{subpath}")
    };
    if let Some(exact) = subpaths.get(&key) {
        return targets(exact);
    }
    subpaths
        .iter()
        .filter_map(|(pattern, value)| Some((pattern, wildcard_capture(pattern, &key)?, value)))
        // As Node does, the most specific pattern wins.
        .max_by_key(|(pattern, _, _)| pattern.len())
        .map(|(_, captured, value)| {
            targets(value)
                .iter()
                .map(|target| target.replace(WILDCARD, captured))
                .collect()
        })
        .unwrap_or_default()
}

/// Every file an exports value names: a string, conditions (an object), or
/// alternatives (an array), nested to any depth, in the order written.
fn targets(value: &Value) -> Vec<String> {
    if let Some(target) = value.as_str() {
        return vec![target.to_string()];
    }
    if let Some(conditions) = value.as_object() {
        return conditions
            .iter()
            .flat_map(|(_, value)| targets(value))
            .collect();
    }
    if let Some(alternatives) = value.as_array() {
        return alternatives.iter().flat_map(targets).collect();
    }
    Vec::new()
}

#[cfg(test)]
#[path = "workspace_tests.rs"]
mod tests;
