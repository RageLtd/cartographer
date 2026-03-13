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
            "method" => {
                if let Some(name_node) = child.child_by_field_name("name") {
                    let name = name_node.utf8_text(source_bytes).unwrap_or("");
                    symbols.push(Symbol {
                        name: name.to_string(),
                        kind: SymbolKind::Function,
                        signature: get_signature(&child, source_bytes),
                        doc_comment: get_ruby_doc_comment(source, &child),
                        visibility: Visibility::Public,
                        line: child.start_position().row + 1,
                    });
                }
            }
            "singleton_method" => {
                if let Some(name_node) = child.child_by_field_name("name") {
                    let name = name_node.utf8_text(source_bytes).unwrap_or("");
                    symbols.push(Symbol {
                        name: format!("self.{name}"),
                        kind: SymbolKind::Function,
                        signature: get_signature(&child, source_bytes),
                        doc_comment: get_ruby_doc_comment(source, &child),
                        visibility: Visibility::Public,
                        line: child.start_position().row + 1,
                    });
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
                extract_ruby_call(file_path, source, &child, source_bytes, imports, symbols);
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

fn extract_ruby_call(
    file_path: &str,
    source: &str,
    node: &Node,
    source_bytes: &[u8],
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
                                visibility: Visibility::Public,
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
mod tests {
    use super::*;

    fn parse_ruby(source: &str) -> tree_sitter::Tree {
        let mut parser = tree_sitter::Parser::new();
        let lang = tree_sitter_ruby::LANGUAGE.into();
        parser.set_language(&lang).unwrap();
        parser.parse(source, None).unwrap()
    }

    #[test]
    fn test_ruby_class_extraction() {
        let source = r#"
# A greeter class
class Greeter
  def greet(name)
    "Hello, #{name}"
  end
end
"#;
        let tree = parse_ruby(source);
        let (_, symbols) = extract_ruby("/test.rb", source, tree.root_node());

        assert!(symbols
            .iter()
            .any(|s| s.name == "Greeter" && s.kind == SymbolKind::Class));
        assert!(symbols
            .iter()
            .any(|s| s.name == "greet" && s.kind == SymbolKind::Function));
    }

    #[test]
    fn test_ruby_module_extraction() {
        let source = r#"
module Utils
  def self.helper
    true
  end
end
"#;
        let tree = parse_ruby(source);
        let (_, symbols) = extract_ruby("/test.rb", source, tree.root_node());

        assert!(symbols
            .iter()
            .any(|s| s.name == "Utils" && s.kind == SymbolKind::Module));
        assert!(symbols
            .iter()
            .any(|s| s.name == "self.helper" && s.kind == SymbolKind::Function));
    }

    #[test]
    fn test_ruby_constant_extraction() {
        let source = "MAX_SIZE = 1024\n";
        let tree = parse_ruby(source);
        let (_, symbols) = extract_ruby("/test.rb", source, tree.root_node());

        assert!(symbols
            .iter()
            .any(|s| s.name == "MAX_SIZE" && s.kind == SymbolKind::Const));
    }

    #[test]
    fn test_ruby_require_relative() {
        let dir = tempfile::TempDir::new().unwrap();
        let lib = dir.path().join("lib");
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::write(lib.join("helper.rb"), "module Helper; end").unwrap();
        let from_file = lib.join("main.rb").to_str().unwrap().to_string();

        let source = "require_relative 'helper'\n";
        let tree = parse_ruby(source);
        let (imports, _) = extract_ruby(&from_file, source, tree.root_node());

        assert_eq!(imports.len(), 1);
        assert!(imports[0].target.ends_with("helper.rb"));
    }

    #[test]
    fn test_ruby_doc_comment() {
        let source = r#"
# Calculate the sum of two numbers.
# Returns an integer.
def add(a, b)
  a + b
end
"#;
        let tree = parse_ruby(source);
        let (_, symbols) = extract_ruby("/test.rb", source, tree.root_node());

        let sym = symbols.iter().find(|s| s.name == "add").unwrap();
        assert!(sym.doc_comment.is_some());
        let doc = sym.doc_comment.as_ref().unwrap();
        assert!(doc.contains("Calculate the sum"));
    }
}
