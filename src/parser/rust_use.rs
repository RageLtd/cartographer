//! Import edges of a Rust `use` declaration rooted at `crate`, `self` or
//! `super`. Each path is walked one module at a time from its starting module;
//! the first segment that is not a child module is an item of the file reached.

use std::path::{Path, PathBuf};

use tree_sitter::Node;

use crate::types::{EdgeKind, ImportEdge};

use super::rust_modules::{
    child_module_file, crate_root_module, module_of_file, parent_module, RootFileModules,
};

const PATH_FIELD: &str = "path";
const CRATE: &str = "crate";
const SELF: &str = "self";
const SUPER: &str = "super";
const WILDCARD: &str = "*";

/// One edge per file the declaration's paths reach, other than `file_path`
/// itself, each carrying the declaration's path as written. `root_files` is
/// shared by every declaration of the file being parsed.
pub fn use_edges(
    file_path: &str,
    use_declaration: &Node,
    source_bytes: &[u8],
    root_files: &mut RootFileModules,
) -> Vec<ImportEdge> {
    let Some(argument) = use_declaration.child_by_field_name("argument") else {
        return Vec::new();
    };
    let Ok(specifier) = argument.utf8_text(source_bytes) else {
        return Vec::new();
    };

    let mut paths = Vec::new();
    collect_paths(&argument, source_bytes, &[], &mut paths);

    let from_file = Path::new(file_path);
    let mut targets: Vec<(PathBuf, Vec<String>)> = Vec::new();
    for (target, symbol) in paths
        .iter()
        .filter_map(|p| walk_path(from_file, p, root_files))
    {
        if target == from_file {
            continue;
        }
        match targets.iter_mut().find(|(t, _)| *t == target) {
            Some((_, symbols)) if symbols.contains(&symbol) => {}
            Some((_, symbols)) => symbols.push(symbol),
            None => targets.push((target, vec![symbol])),
        }
    }

    targets
        .into_iter()
        .filter_map(|(target, symbols)| {
            Some(ImportEdge {
                source: file_path.to_string(),
                target: target.to_str()?.to_string(),
                specifier: specifier.to_string(),
                symbols,
                kind: EdgeKind::Import,
            })
        })
        .collect()
}

/// Flattens a use tree into one segment list per imported name, with `*` as
/// the last segment of a glob. Aliases are dropped: the edge names the item.
fn collect_paths(node: &Node, source_bytes: &[u8], prefix: &[String], out: &mut Vec<Vec<String>>) {
    match node.kind() {
        "scoped_use_list" => {
            let mut prefix = prefix.to_vec();
            if let Some(path) = node.child_by_field_name(PATH_FIELD) {
                let Some(segments) = path_segments(&path, source_bytes) else {
                    return;
                };
                prefix.extend(segments);
            }
            if let Some(list) = node.child_by_field_name("list") {
                collect_paths(&list, source_bytes, &prefix, out);
            }
        }
        "use_list" => {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                collect_paths(&child, source_bytes, prefix, out);
            }
        }
        "use_as_clause" => {
            if let Some(path) = node.child_by_field_name(PATH_FIELD) {
                collect_paths(&path, source_bytes, prefix, out);
            }
        }
        "use_wildcard" => {
            let mut path = prefix.to_vec();
            if let Some(inner) = node.named_child(0) {
                let Some(segments) = path_segments(&inner, source_bytes) else {
                    return;
                };
                path.extend(segments);
            }
            path.push(WILDCARD.to_string());
            out.push(path);
        }
        _ => {
            if let Some(segments) = path_segments(node, source_bytes) {
                out.push([prefix, &segments].concat());
            }
        }
    }
}

/// The segments of a plain path such as `crate::net::tcp`. A path opened by
/// `::` has no `path` field, so its first segment is the external crate name.
fn path_segments(node: &Node, source_bytes: &[u8]) -> Option<Vec<String>> {
    match node.kind() {
        "scoped_identifier" => {
            let mut segments = match node.child_by_field_name(PATH_FIELD) {
                Some(path) => path_segments(&path, source_bytes)?,
                None => Vec::new(),
            };
            let name = node.child_by_field_name("name")?;
            segments.push(name.utf8_text(source_bytes).ok()?.to_string());
            Some(segments)
        }
        // The path keywords' node kinds are spelled as the keywords.
        "identifier" | CRATE | SELF | SUPER => {
            Some(vec![node.utf8_text(source_bytes).ok()?.to_string()])
        }
        _ => None,
    }
}

/// The file a path reaches and the name it imports from that file: the first
/// segment that is not a child module, or the last module's name. `None` when
/// the path ends in a module whose file is unknown.
fn walk_path(
    from_file: &Path,
    segments: &[String],
    root_files: &mut RootFileModules,
) -> Option<(PathBuf, String)> {
    let (first, rest) = segments.split_first()?;
    let mut module = match first.as_str() {
        CRATE => crate_root_module(from_file, root_files)?,
        SELF => module_of_file(from_file)?,
        SUPER => parent_module(from_file, root_files)?,
        _ => return None,
    };
    let mut name = first.clone();
    for segment in rest {
        match segment.as_str() {
            SELF => {}
            SUPER => {
                module = parent_module(module.file.as_deref()?, root_files)?;
                name = segment.clone();
            }
            WILDCARD => return Some((module.file?, segment.clone())),
            child => match child_module_file(&module.children_dir, child) {
                Some(child_file) => {
                    module = module_of_file(&child_file)?;
                    name = segment.clone();
                }
                None => return Some((module.file?, segment.clone())),
            },
        }
    }
    Some((module.file?, name))
}
