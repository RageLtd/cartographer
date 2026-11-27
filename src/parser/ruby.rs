use tree_sitter::Node;

use crate::types::{ImportEdge, Symbol, SymbolKind, Visibility};

use super::extractor::{get_signature, strip_quotes};
use super::resolver::resolve_ruby_require;

fn get_ruby_doc_comment(source: &str, node: &Node) -> Option<String> {
    let start_line = node.start_position().row;
    let lines: Vec<&str> = source.lines().collect();
    if start_line == 0 {
        return None;
    }

    let mut comment_lines: Vec<&str> = Vec::new();
    for i in (0..start_line).rev() {
        let trimmed = lines[i].trim();
        if trimmed.starts_with('#') {
            comment_lines.push(trimmed);
        } else {
            break;
        }
    }

    if comment_lines.is_empty() {
        return None;
    }

    comment_lines.reverse();
    let result: Vec<String> = comment_lines
        .iter()
        .map(|line| line.trim_start_matches('#').trim_start().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    if result.is_empty() {
        return None;
    }

    Some(result.join("\n"))
}

pub fn extract_ruby(file_path: &str, source: &str, root: Node) -> (Vec<ImportEdge>, Vec<Symbol>) {
    let mut imports = Vec::new();
    let mut symbols = Vec::new();
    let source_bytes = source.as_bytes();

    extract_ruby_recursive(
        file_path,
        source,
        &root,
        source_bytes,
        &mut imports,
        &mut symbols,
    );

    (imports, symbols)
}

fn extract_ruby_recursive(
    file_path: &str,
    source: &str,
    node: &Node,
    source_bytes: &[u8],
    imports: &mut Vec<ImportEdge>,
    symbols: &mut Vec<Symbol>,
) {
    // `private` and its siblings apply from where they stand to the end of
    // the body they are written in.
    let mut default_visibility = Visibility::Public;
    // The symbols defined directly in this body: the only ones a later
    // `private :name` written here can refer to.
    let mut own_symbols: Vec<usize> = Vec::new();

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "class" => {
                if let Some(name_node) = child.child_by_field_name("name") {
                    let name = name_node.utf8_text(source_bytes).unwrap_or("");
                    symbols.push(Symbol {
                        name: name.to_string(),
                        kind: SymbolKind::Class,
                        signature: get_signature(&child, source_bytes),
                        doc_comment: get_ruby_doc_comment(source, &child),
                        visibility: Visibility::Public,
                        line: child.start_position().row + 1,
                    });
                }
                if let Some(body) = child.child_by_field_name("body") {
                    extract_ruby_recursive(
                        file_path,
                        source,
                        &body,
                        source_bytes,
                        imports,
                        symbols,
                    );
                }
            }
            "module" => {
                if let Some(name_node) = child.child_by_field_name("name") {
                    let name = name_node.utf8_text(source_bytes).unwrap_or("");
                    symbols.push(Symbol {
                        name: name.to_string(),
                        kind: SymbolKind::Module,
                        signature: get_signature(&child, source_bytes),
                        doc_comment: get_ruby_doc_comment(source, &child),
                        visibility: Visibility::Public,
                        line: child.start_position().row + 1,
                    });
                }
                if let Some(body) = child.child_by_field_name("body") {
                    extract_ruby_recursive(
                        file_path,
                        source,
                        &body,
                        source_bytes,
                        imports,
                        symbols,
                    );
                }
            }
            "method" | "singleton_method" => {
                let visibility = ruby_definition_visibility(&child, default_visibility);
                if let Some(symbol) = ruby_method_symbol(source, &child, source_bytes, visibility) {
                    own_symbols.push(symbols.len());
                    symbols.push(symbol);
                }
            }
            "identifier" => {
                let word = child.utf8_text(source_bytes).unwrap_or("");
                if let Some(visibility) = ruby_visibility_keyword(word) {
                    default_visibility = visibility;
                }
            }
            "assignment" => {
                if let Some(left) = child.child(0) {
                    if left.kind() == "constant" {
                        let name = left.utf8_text(source_bytes).unwrap_or("");
                        symbols.push(Symbol {
                            name: name.to_string(),
                            kind: SymbolKind::Const,
                            signature: get_signature(&child, source_bytes),
                            doc_comment: get_ruby_doc_comment(source, &child),
                            visibility: Visibility::Public,
                            line: child.start_position().row + 1,
                        });
                    }
                }
            }
            "call" => {
                let first_new = symbols.len();
                let method_name = ruby_bare_call_name(&child, source_bytes);
                let arguments = child.child_by_field_name("arguments");
                match (ruby_visibility_keyword(method_name), arguments) {
                    // `private def x`, `private attr_reader :y`, `private :z`
                    (Some(visibility), Some(args)) => {
                        let mut arg_cursor = args.walk();
                        for arg in args.children(&mut arg_cursor) {
                            match arg.kind() {
                                "method" | "singleton_method" => {
                                    let visibility = ruby_definition_visibility(&arg, visibility);
                                    symbols.extend(ruby_method_symbol(
                                        source,
                                        &arg,
                                        source_bytes,
                                        visibility,
                                    ));
                                }
                                "call" => extract_ruby_call(
                                    file_path,
                                    source,
                                    &arg,
                                    source_bytes,
                                    visibility,
                                    imports,
                                    symbols,
                                ),
                                _ => {
                                    if let Some(name) = ruby_name_argument(&arg, source_bytes) {
                                        set_ruby_visibility(
                                            symbols,
                                            &own_symbols,
                                            name,
                                            visibility,
                                        );
                                    }
                                }
                            }
                        }
                    }
                    (None, Some(args)) if method_name == RUBY_PRIVATE_CLASS_METHOD => {
                        let mut arg_cursor = args.walk();
                        for arg in args.children(&mut arg_cursor) {
                            if let Some(name) = ruby_name_argument(&arg, source_bytes) {
                                set_ruby_visibility(
                                    symbols,
                                    &own_symbols,
                                    &format!("{RUBY_SINGLETON_PREFIX}{name}"),
                                    Visibility::Private,
                                );
                            }
                        }
                    }
                    _ => extract_ruby_call(
                        file_path,
                        source,
                        &child,
                        source_bytes,
                        default_visibility,
                        imports,
                        symbols,
                    ),
                }
                own_symbols.extend(first_new..symbols.len());
            }
            _ => {
                if child.kind() == "body_statement" || child.kind() == "program" {
                    extract_ruby_recursive(
                        file_path,
                        source,
                        &child,
                        source_bytes,
                        imports,
                        symbols,
                    );
                }
            }
        }
    }
}

const RUBY_PRIVATE_CLASS_METHOD: &str = "private_class_method";
const RUBY_SINGLETON_PREFIX: &str = "self.";
const RUBY_SINGLETON_METHOD: &str = "singleton_method";

/// The visibility that `public`, `private` or `protected` switches to.
/// `Visibility` has no protected level; a protected method cannot be called
/// from outside its class, so it is reported as private.
fn ruby_visibility_keyword(word: &str) -> Option<Visibility> {
    match word {
        "public" => Some(Visibility::Public),
        "private" | "protected" => Some(Visibility::Private),
        _ => None,
    }
}

/// `private` governs instance methods only; a `def self.x` stays public until
/// `private_class_method` names it.
fn ruby_definition_visibility(definition: &Node, current: Visibility) -> Visibility {
    if definition.kind() == RUBY_SINGLETON_METHOD {
        Visibility::Public
    } else {
        current
    }
}

/// The method a call names, or `""` when it is called on a receiver: `x.private`
/// is not the visibility keyword.
fn ruby_bare_call_name<'a>(call: &Node, source_bytes: &'a [u8]) -> &'a str {
    if call.child_by_field_name("receiver").is_some() {
        return "";
    }
    call.child_by_field_name("method")
        .and_then(|n| n.utf8_text(source_bytes).ok())
        .unwrap_or("")
}

fn ruby_method_symbol(
    source: &str,
    definition: &Node,
    source_bytes: &[u8],
    visibility: Visibility,
) -> Option<Symbol> {
    let name = definition
        .child_by_field_name("name")?
        .utf8_text(source_bytes)
        .ok()?;
    let name = if definition.kind() == RUBY_SINGLETON_METHOD {
        format!("{RUBY_SINGLETON_PREFIX}{name}")
    } else {
        name.to_string()
    };
    Some(Symbol {
        name,
        kind: SymbolKind::Function,
        signature: get_signature(definition, source_bytes),
        doc_comment: get_ruby_doc_comment(source, definition),
        visibility,
        line: definition.start_position().row + 1,
    })
}

/// A method name given to `private :a, "b"` as a symbol or a string.
fn ruby_name_argument<'a>(arg: &Node, source_bytes: &'a [u8]) -> Option<&'a str> {
    let text = arg.utf8_text(source_bytes).ok()?;
    match arg.kind() {
        "simple_symbol" => Some(text.trim_start_matches(':')),
        "string" => Some(strip_quotes(text)),
        _ => None,
    }
}

fn set_ruby_visibility(
    symbols: &mut [Symbol],
    own_symbols: &[usize],
    name: &str,
    visibility: Visibility,
) {
    for &index in own_symbols {
        if symbols[index].name == name {
            symbols[index].visibility = visibility;
        }
    }
}

fn extract_ruby_call(
    file_path: &str,
    source: &str,
    node: &Node,
    source_bytes: &[u8],
    attr_visibility: Visibility,
    imports: &mut Vec<ImportEdge>,
    symbols: &mut Vec<Symbol>,
) {
    let method_name = node
        .child_by_field_name("method")
        .and_then(|n| n.utf8_text(source_bytes).ok())
        .unwrap_or("");

    match method_name {
        "require" | "require_relative" => {
            if let Some(args) = node.child_by_field_name("arguments") {
                let mut arg_cursor = args.walk();
                for arg in args.children(&mut arg_cursor) {
                    if arg.kind() == "string" || arg.kind() == "string_content" {
                        let raw = arg.utf8_text(source_bytes).unwrap_or("");
                        let specifier = strip_quotes(raw);
                        if !specifier.is_empty() {
                            let resolved = if method_name == "require_relative" {
                                resolve_ruby_require(specifier, file_path, true)
                            } else {
                                resolve_ruby_require(specifier, file_path, false)
                            };
                            if let Some(target) = resolved {
                                imports.push(ImportEdge {
                                    source: file_path.to_string(),
                                    target,
                                    specifier: specifier.to_string(),
                                    symbols: vec!["*".to_string()],
                                });
                            }
                        }
                    }
                }
            }
        }
        "include" | "extend" | "prepend" => {
            if let Some(args) = node.child_by_field_name("arguments") {
                let mut arg_cursor = args.walk();
                for arg in args.children(&mut arg_cursor) {
                    if arg.kind() == "constant" || arg.kind() == "scope_resolution" {
                        let name = arg.utf8_text(source_bytes).unwrap_or("");
                        if !name.is_empty() {
                            symbols.push(Symbol {
                                name: format!("{method_name} {name}"),
                                kind: SymbolKind::Unknown,
                                signature: node.utf8_text(source_bytes).unwrap_or("").to_string(),
                                doc_comment: None,
                                visibility: Visibility::Private,
                                line: node.start_position().row + 1,
                            });
                        }
                    }
                }
            }
        }
        "attr_accessor" | "attr_reader" | "attr_writer" => {
            if let Some(args) = node.child_by_field_name("arguments") {
                let mut arg_cursor = args.walk();
                for arg in args.children(&mut arg_cursor) {
                    if arg.kind() == "simple_symbol" || arg.kind() == "hash_key_symbol" {
                        let raw = arg.utf8_text(source_bytes).unwrap_or("");
                        let name = raw.trim_start_matches(':');
                        if !name.is_empty() {
                            symbols.push(Symbol {
                                name: name.to_string(),
                                kind: SymbolKind::Function,
                                signature: format!("{method_name} :{name}"),
                                doc_comment: get_ruby_doc_comment(source, node),
                                visibility: attr_visibility,
                                line: node.start_position().row + 1,
                            });
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
#[path = "ruby_tests.rs"]
mod tests;
