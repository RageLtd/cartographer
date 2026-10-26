//! Discovery of host-provided grammars.
//!
//! A grammar lives in its own directory under the grammars root (by default
//! `~/.cartographer/grammars/<name>/`) and consists of:
//!
//! - `language.json` — the manifest (see [`GrammarManifest`]);
//! - `<name>.so` — the shared library produced by `tree-sitter build`;
//! - `tags.scm` and/or `imports.scm` — the extraction queries.
//!
//! Grammars are only ever loaded from this root, which the user owns. Nothing
//! inside a project being indexed can cause a library to be opened: loading a
//! shared object runs arbitrary code, so that boundary is deliberate.

use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use serde::Deserialize;

use crate::types::Visibility;

use super::dynamic::{language_symbol, load_grammar};
use super::generic::GenericLanguage;
use super::registry::{Extractor, LanguageDef, LanguageRegistry};
use super::resolver::ResolverStrategy;

pub const MANIFEST_FILE: &str = "language.json";
pub const DEFAULT_TAGS_FILE: &str = "tags.scm";
pub const DEFAULT_IMPORTS_FILE: &str = "imports.scm";
const SHARED_LIBRARY_EXTENSIONS: [&str; 2] = ["so", "dylib"];

/// `language.json`. Only `extensions` is required; everything else defaults
/// from the directory name and the conventional file names.
#[derive(Debug, Clone, Deserialize)]
pub struct GrammarManifest {
    /// Language name; defaults to the directory name.
    pub name: Option<String>,
    /// File extensions this grammar handles, with or without the leading dot.
    pub extensions: Vec<String>,
    /// Shared library file name inside the grammar directory; defaults to
    /// `<name>.so` (or `<name>.dylib`).
    pub library: Option<String>,
    /// Exported constructor symbol; defaults to `tree_sitter_<name>`.
    pub symbol: Option<String>,
    /// Tags query file name; defaults to `tags.scm` when that file exists.
    pub tags: Option<String>,
    /// Imports query file name; defaults to `imports.scm` when that file exists.
    pub imports: Option<String>,
    /// Import resolution strategies, tried in order.
    #[serde(default)]
    pub resolvers: Vec<ResolverStrategy>,
    /// Visibility assigned when the tags query has no `@visibility` capture.
    pub default_visibility: Option<Visibility>,
}

/// One grammar directory that could not be loaded, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrammarLoadError {
    pub dir: PathBuf,
    pub message: String,
}

/// Load every grammar directory under `root` into `registry`.
///
/// A broken grammar must not take the others down with it, so failures are
/// returned for the caller to report rather than aborting discovery. A missing
/// root is not an error — it simply means no host grammars are installed.
pub fn register_host_grammars(
    registry: &mut LanguageRegistry,
    root: &Path,
) -> Vec<GrammarLoadError> {
    let mut failures = Vec::new();
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return failures,
        Err(e) => {
            failures.push(GrammarLoadError {
                dir: root.to_path_buf(),
                message: format!("Cannot read grammars directory: {e}"),
            });
            return failures;
        }
    };

    let mut dirs: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .filter(|path| !file_name(path).starts_with('.'))
        .collect();
    dirs.sort();

    for dir in dirs {
        match load_grammar_dir(&dir) {
            Ok(def) => registry.register(def),
            Err(message) => failures.push(GrammarLoadError { dir, message }),
        }
    }
    failures
}

/// Load a single grammar directory into a registrable language definition.
pub fn load_grammar_dir(dir: &Path) -> Result<LanguageDef, String> {
    let manifest_path = dir.join(MANIFEST_FILE);
    let raw = fs::read_to_string(&manifest_path)
        .map_err(|e| format!("Cannot read {}: {e}", manifest_path.display()))?;
    let manifest: GrammarManifest = sonic_rs::from_str(&raw)
        .map_err(|e| format!("Invalid {}: {e}", manifest_path.display()))?;

    let name = manifest.name.clone().unwrap_or_else(|| file_name(dir));
    if name.is_empty() {
        return Err(format!(
            "{}: `name` must not be empty",
            manifest_path.display()
        ));
    }

    let extensions: Vec<String> = manifest
        .extensions
        .iter()
        .map(|ext| ext.trim())
        .filter(|ext| !ext.is_empty())
        .map(|ext| {
            if ext.starts_with('.') {
                ext.to_string()
            } else {
                format!(".{ext}")
            }
        })
        .collect();
    if extensions.is_empty() {
        return Err(format!(
            "{}: `extensions` must list at least one file extension",
            manifest_path.display()
        ));
    }

    let library_path = resolve_library_path(dir, &name, manifest.library.as_deref())?;
    let symbol = manifest
        .symbol
        .clone()
        .unwrap_or_else(|| language_symbol(&name));
    let grammar = load_grammar(&library_path, &symbol)?;

    let tags = read_query(dir, manifest.tags.as_deref(), DEFAULT_TAGS_FILE)?;
    let imports = read_query(dir, manifest.imports.as_deref(), DEFAULT_IMPORTS_FILE)?;
    if tags.is_none() && imports.is_none() {
        return Err(format!(
            "{}: neither {DEFAULT_TAGS_FILE} nor {DEFAULT_IMPORTS_FILE} found; a grammar \
             without queries cannot extract anything",
            dir.display()
        ));
    }

    let generic = GenericLanguage::new(
        &grammar,
        tags.as_deref(),
        imports.as_deref(),
        manifest.resolvers.clone(),
        manifest.default_visibility.unwrap_or(Visibility::Private),
    )
    .map_err(|e| format!("{}: {e}", dir.display()))?;

    Ok(LanguageDef {
        name,
        extensions,
        grammar,
        extractor: Extractor::Generic(Arc::new(generic)),
    })
}

/// The shared library must be a plain file name inside the grammar directory —
/// never a path that reaches outside it.
fn resolve_library_path(dir: &Path, name: &str, declared: Option<&str>) -> Result<PathBuf, String> {
    if let Some(declared) = declared {
        let mut components = Path::new(declared).components();
        match (components.next(), components.next()) {
            (Some(Component::Normal(_)), None) => {}
            _ => {
                return Err(format!(
                    "{}: `library` must be a file name inside the grammar directory, got {declared:?}",
                    dir.display()
                ));
            }
        }
        return Ok(dir.join(declared));
    }

    let candidates: Vec<PathBuf> = SHARED_LIBRARY_EXTENSIONS
        .iter()
        .map(|ext| dir.join(format!("{name}.{ext}")))
        .collect();
    candidates
        .iter()
        .find(|path| path.is_file())
        .cloned()
        .ok_or_else(|| {
            format!(
                "{}: no grammar library found; expected one of {} (or set `library` in {MANIFEST_FILE})",
                dir.display(),
                candidates
                    .iter()
                    .map(|p| file_name(p))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
}

/// An explicitly declared query file must exist; the conventional default is optional.
fn read_query(dir: &Path, declared: Option<&str>, default: &str) -> Result<Option<String>, String> {
    let path = dir.join(declared.unwrap_or(default));
    match fs::read_to_string(&path) {
        Ok(source) => Ok(Some(source)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && declared.is_none() => Ok(None),
        Err(e) => Err(format!("Cannot read query {}: {e}", path.display())),
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "manifest_tests.rs"]
mod tests;
