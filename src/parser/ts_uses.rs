//! Where one TypeScript or JavaScript file uses a symbol that another file
//! defines. This is parsing only, with no index: whoever holds the import
//! graph supplies the files to ask about.
//!
//! A use is matched by name. The file's imports say which local name the
//! symbol has there, and every identifier with that name counts, so a local
//! variable that shadows the import is counted too. Telling them apart needs
//! scope analysis, which this does not do.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use tree_sitter::Node;

use super::extractor::strip_quotes;
use super::registry::{Extractor, LanguageRegistry};
use super::resolver::normalize_path;
use super::ts_js::Importer;

/// The name a default export is imported and stored under.
const DEFAULT_EXPORT: &str = "default";

/// How one file uses a symbol of another.
pub struct FileUses {
    /// The 1-based lines that name the symbol, in order.
    pub lines: Vec<usize>,
    /// The names this file exports the symbol under, for files that import
    /// it from here in turn.
    pub re_exported_as: Vec<String>,
}

/// How `file_path` uses `symbol`, a name `target` exports (`default` for its
/// default export). `file_path` may be `target` itself.
pub fn uses_in_file(
    registry: &LanguageRegistry,
    file_path: &str,
    target: &str,
    symbol: &str,
) -> Result<FileUses, String> {
    let language = registry
        .for_path(file_path)
        .filter(|language| matches!(language.extractor, Extractor::TsJs))
        .ok_or_else(|| {
            format!("Finding uses is supported for TypeScript and JavaScript only: {file_path}")
        })?;
    let source =
        fs::read_to_string(file_path).map_err(|e| format!("Failed to read {file_path}: {e}"))?;
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&language.grammar)
        .map_err(|e| format!("Failed to set language {}: {e}", language.name))?;
    let tree = parser
        .parse(&source, None)
        .ok_or_else(|| format!("Failed to parse {file_path}"))?;

    let target = normalize_path(Path::new(target));
    let names = if normalize_path(Path::new(file_path)) == target {
        Names {
            locals: vec![symbol.to_string()],
            ..Names::default()
        }
    } else {
        let importer = Importer::new(file_path);
        let from_target = |specifier: &str| {
            importer
                .resolve(specifier)
                .is_some_and(|resolved| Path::new(&resolved) == target)
        };
        names_bound_to(tree.root_node(), source.as_bytes(), symbol, from_target)
    };

    let mut lines = names.lines.clone();
    for_each_node(tree.root_node(), |node| {
        if names.is_use(&node, source.as_bytes(), symbol) {
            lines.insert(node.start_position().row + 1);
        }
    });
    Ok(FileUses {
        lines: lines.into_iter().collect(),
        re_exported_as: names.re_exported_as,
    })
}

/// What a file calls a symbol it takes from the target.
#[derive(Default)]
struct Names {
    /// Local names bound directly to the symbol.
    locals: Vec<String>,
    /// Local names bound to the target as a whole (`import * as ns`).
    namespaces: Vec<String>,
    re_exported_as: Vec<String>,
    /// Lines that pass the symbol on without an identifier of a local name:
    /// `export { symbol as other } from "target"`.
    lines: BTreeSet<usize>,
}

impl Names {
    fn is_use(&self, node: &Node, source: &[u8], symbol: &str) -> bool {
        let text = |node: Node| node.utf8_text(source).unwrap_or("");
        let is_namespace = |node: Option<Node>| {
            node.is_some_and(|node| {
                node.kind() == "identifier" && self.namespaces.iter().any(|n| n == text(node))
            })
        };
        let is_symbol = |node: Option<Node>| node.is_some_and(|node| text(node) == symbol);
        match node.kind() {
            "identifier" | "type_identifier" | "shorthand_property_identifier" => {
                self.locals.iter().any(|local| local == text(*node))
            }
            // `ns.symbol` as a value, and as a type.
            "member_expression" => {
                is_namespace(node.child_by_field_name("object"))
                    && is_symbol(node.child_by_field_name("property"))
            }
            "nested_type_identifier" => {
                is_namespace(node.child_by_field_name("module"))
                    && is_symbol(node.child_by_field_name("name"))
            }
            _ => false,
        }
    }
}

/// Reads the file's top-level imports and exports for what they bind to
/// `symbol` of the target. `from_target` says whether a specifier names it.
fn names_bound_to(
    root: Node,
    source: &[u8],
    symbol: &str,
    from_target: impl Fn(&str) -> bool,
) -> Names {
    let text = |node: Node| node.utf8_text(source).unwrap_or("").to_string();
    let names_target = |statement: &Node| {
        statement
            .child_by_field_name("source")
            .is_some_and(|specifier| from_target(strip_quotes(&text(specifier))))
    };
    // An import or export specifier as (name, name it is given).
    let specifier_names = |specifier: Node| {
        let name = specifier.child_by_field_name("name").map(text)?;
        let given = specifier.child_by_field_name("alias").map(text);
        Some((name.clone(), given.unwrap_or(name)))
    };

    let mut names = Names::default();
    let mut cursor = root.walk();
    let statements: Vec<Node> = root.children(&mut cursor).collect();

    for statement in statements
        .iter()
        .filter(|s| s.kind() == "import_statement" && names_target(s))
    {
        for_each_node(*statement, |node| match node.kind() {
            "import_specifier" => {
                if let Some((name, local)) = specifier_names(node) {
                    if name == symbol {
                        names.locals.push(local);
                    }
                }
            }
            "namespace_import" => {
                let mut cursor = node.walk();
                let namespace = node
                    .children(&mut cursor)
                    .find(|c| c.kind() == "identifier");
                names.namespaces.extend(namespace.map(text));
            }
            // `import Thing from "target"`: the default export, under any name.
            "import_clause" if symbol == DEFAULT_EXPORT => {
                let mut cursor = node.walk();
                let default = node
                    .children(&mut cursor)
                    .find(|c| c.kind() == "identifier");
                names.locals.extend(default.map(text));
            }
            _ => {}
        });
    }

    // Exports second: `export { local }` passes on a name an import bound.
    for statement in statements.iter().filter(|s| s.kind() == "export_statement") {
        let mut cursor = statement.walk();
        let children: Vec<Node> = statement.children(&mut cursor).collect();
        let clause = children.iter().find(|c| c.kind() == "export_clause");
        let from_the_target = names_target(statement);
        let has_source = statement.child_by_field_name("source").is_some();

        match clause {
            Some(clause) => {
                let mut cursor = clause.walk();
                for specifier in clause.children(&mut cursor) {
                    let Some((name, given)) = specifier_names(specifier) else {
                        continue;
                    };
                    if from_the_target && name == symbol {
                        names.lines.insert(specifier.start_position().row + 1);
                        names.re_exported_as.push(given);
                    } else if !has_source && names.locals.contains(&name) {
                        names.re_exported_as.push(given);
                    }
                }
            }
            // `export * from "target"` passes on every named export, and not
            // the default one. `export * as ns from` exports a namespace.
            None => {
                let is_star = children.iter().any(|c| c.kind() == "*");
                let as_namespace = children.iter().any(|c| c.kind() == "namespace_export");
                if from_the_target && is_star && !as_namespace && symbol != DEFAULT_EXPORT {
                    names.re_exported_as.push(symbol.to_string());
                }
            }
        }
    }
    names
}

/// Visits `root` and everything under it, without recursion: a tree can be
/// deeper than the call stack.
fn for_each_node<'tree>(root: Node<'tree>, mut visit: impl FnMut(Node<'tree>)) {
    let mut cursor = root.walk();
    loop {
        visit(cursor.node());
        if cursor.goto_first_child() {
            continue;
        }
        loop {
            if cursor.node() == root {
                return;
            }
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                return;
            }
        }
    }
}

#[cfg(test)]
#[path = "ts_uses_tests.rs"]
mod tests;
