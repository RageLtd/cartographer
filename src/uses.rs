//! Every use of a symbol across a project: the stored import graph says which
//! files could use it, and parsing each of them says where.

use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};

use crate::db::client::Db;
use crate::db::graph::get_file_detail;
use crate::parser::registry::LanguageRegistry;
use crate::parser::ts_uses::uses_in_file;

/// One file's uses of a symbol.
pub struct FileUse {
    pub file_path: String,
    /// The 1-based lines that name the symbol, in order.
    pub lines: Vec<usize>,
}

pub enum SymbolUses {
    FileNotIndexed,
    /// The file is indexed and has no symbol of that name; `defined` is what
    /// it does have.
    SymbolNotDefined {
        defined: Vec<String>,
    },
    /// Sorted by path. The defining file is among them when it names the
    /// symbol, so a symbol nothing else uses still has an answer.
    Found(Vec<FileUse>),
}

/// The uses of `symbol`, which `file_path` defines, anywhere in `project`.
///
/// A file that re-exports the symbol passes it on, under the same name or
/// another, to the files that import from it in turn; those are followed
/// until no new file and name turns up.
///
/// The answer is as current as the index for which files import which, and
/// as current as the disk for the lines within them.
pub fn find_uses(
    db: &Db,
    registry: &LanguageRegistry,
    project: &str,
    file_path: &str,
    symbol: &str,
) -> Result<SymbolUses, String> {
    let Some(defining) = get_file_detail(db, project, file_path)? else {
        return Ok(SymbolUses::FileNotIndexed);
    };
    if !defining.symbols.iter().any(|s| s.name == symbol) {
        return Ok(SymbolUses::SymbolNotDefined {
            defined: defining.symbols.into_iter().map(|s| s.name).collect(),
        });
    }

    let mut found: BTreeMap<String, BTreeSet<usize>> = BTreeMap::new();
    let own = uses_in_file(registry, file_path, file_path, symbol)?;
    found.insert(file_path.to_string(), own.lines.into_iter().collect());

    // Each entry is a file and the name it exports the symbol under.
    let start = (file_path.to_string(), symbol.to_string());
    let mut seen: HashSet<(String, String)> = HashSet::from([start.clone()]);
    let mut pending = VecDeque::from([start]);
    while let Some((exporter, name)) = pending.pop_front() {
        let Some(detail) = get_file_detail(db, project, &exporter)? else {
            continue;
        };
        let importers: BTreeSet<String> = detail.dependents.into_iter().map(|(f, _)| f).collect();
        for importer in importers {
            let uses = match uses_in_file(registry, &importer, &exporter, &name) {
                Ok(uses) => uses,
                // Not a file this can read: another language, or gone from
                // disk since it was indexed.
                Err(e) => {
                    tracing::warn!("Skipping {importer} while finding uses of {symbol}: {e}");
                    continue;
                }
            };
            found
                .entry(importer.clone())
                .or_default()
                .extend(uses.lines);
            for exported in uses.re_exported_as {
                let next = (importer.clone(), exported);
                if seen.insert(next.clone()) {
                    pending.push_back(next);
                }
            }
        }
    }

    Ok(SymbolUses::Found(
        found
            .into_iter()
            .filter(|(_, lines)| !lines.is_empty())
            .map(|(file_path, lines)| FileUse {
                file_path,
                lines: lines.into_iter().collect(),
            })
            .collect(),
    ))
}

#[cfg(test)]
#[path = "uses_tests.rs"]
mod tests;
