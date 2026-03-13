use tree_sitter::Node;

use crate::types::{ImportEdge, Symbol, SymbolKind, Visibility};

use super::extractor::get_signature;
use super::resolver::resolve_elixir_module;

fn get_elixir_doc_comment(source: &str, node: &Node) -> Option<String> {
    let start_line = node.start_position().row;
    let lines: Vec<&str> = source.lines().collect();
    if start_line == 0 {
        return None;
    }

    for i in (0..start_line).rev() {
        let trimmed = lines[i].trim();
        if trimmed.is_empty() {
            continue;
        }

        // Hit closing """ of a heredoc — scan backward for @doc/@moduledoc
        if trimmed == "\"\"\"" {
            // Find the opening @doc/@moduledoc """
            for j in (0..i).rev() {
                let attr_line = lines[j].trim();
                if attr_line.starts_with("@doc") || attr_line.starts_with("@moduledoc") {
                    let mut doc_lines = Vec::new();
                    for line in lines.iter().take(i).skip(j + 1) {
                        doc_lines.push(line.trim());
                    }
                    let result = doc_lines.join("\n").trim().to_string();
                    return if result.is_empty() {
                        None
                    } else {
                        Some(result)
                    };
                }
            }
            return None;
        }

        if trimmed.starts_with("@doc") || trimmed.starts_with("@moduledoc") {
            let attr_text = trimmed
                .trim_start_matches("@moduledoc")
                .trim_start_matches("@doc")
                .trim();
            if attr_text == "false" {
                return None;
            }
            if attr_text.starts_with('"') && !attr_text.starts_with("\"\"\"") {
                let content = attr_text.trim_matches('"');
                return if content.is_empty() {
                    None
                } else {
                    Some(content.to_string())
                };
            }
            return None;
        }

        if trimmed.starts_with('#') {
            let mut comment_lines = vec![trimmed];
            for j in (0..i).rev() {
                let ct = lines[j].trim();
                if ct.starts_with('#') {
                    comment_lines.push(ct);
                } else {
                    break;
                }
            }
            comment_lines.reverse();
            let result: Vec<String> = comment_lines
                .iter()
                .map(|l| l.trim_start_matches('#').trim_start().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            return if result.is_empty() {
                None
            } else {
                Some(result.join("\n"))
            };
        }
        break;
    }
    None
}

pub fn extract_elixir(file_path: &str, source: &str, root: Node) -> (Vec<ImportEdge>, Vec<Symbol>) {
    let mut imports = Vec::new();
    let mut symbols = Vec::new();
    let source_bytes = source.as_bytes();

    extract_elixir_recursive(
        file_path,
        source,
        &root,
        source_bytes,
        &mut imports,
        &mut symbols,
    );

    (imports, symbols)
}

fn extract_elixir_recursive(
    file_path: &str,
    source: &str,
    node: &Node,
    source_bytes: &[u8],
    imports: &mut Vec<ImportEdge>,
    symbols: &mut Vec<Symbol>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "call" {
            extract_elixir_call(file_path, source, &child, source_bytes, imports, symbols);
        } else {
            extract_elixir_recursive(file_path, source, &child, source_bytes, imports, symbols);
        }
    }
}

fn extract_elixir_call(
    file_path: &str,
    source: &str,
    node: &Node,
    source_bytes: &[u8],
    imports: &mut Vec<ImportEdge>,
    symbols: &mut Vec<Symbol>,
) {
    let callee = match node.child(0) {
        Some(c) if c.kind() == "identifier" => c.utf8_text(source_bytes).unwrap_or(""),
        _ => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "do_block" || child.kind() == "arguments" {
                    extract_elixir_recursive(
                        file_path,
                        source,
                        &child,
                        source_bytes,
                        imports,
                        symbols,
                    );
                }
            }
            return;
        }
    };

    match callee {
        "defmodule" => {
            let module_name = get_elixir_module_name(node, source_bytes);
            if let Some(name) = &module_name {
                symbols.push(Symbol {
                    name: name.clone(),
                    kind: SymbolKind::Module,
                    signature: format!("defmodule {name}"),
                    doc_comment: get_elixir_doc_comment(source, node),
                    visibility: Visibility::Public,
                    line: node.start_position().row + 1,
                });
            }
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "do_block" {
                    extract_elixir_recursive(
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
        "def" | "defp" => {
            let func_name = get_elixir_def_name(node, source_bytes);
            if let Some(name) = func_name {
                symbols.push(Symbol {
                    name,
                    kind: SymbolKind::Function,
                    signature: get_signature(node, source_bytes),
                    doc_comment: get_elixir_doc_comment(source, node),
                    visibility: if callee == "def" {
                        Visibility::Public
                    } else {
                        Visibility::Private
                    },
                    line: node.start_position().row + 1,
                });
            }
        }
        "defmacro" | "defmacrop" => {
            let func_name = get_elixir_def_name(node, source_bytes);
            if let Some(name) = func_name {
                symbols.push(Symbol {
                    name,
                    kind: SymbolKind::Macro,
                    signature: get_signature(node, source_bytes),
                    doc_comment: get_elixir_doc_comment(source, node),
                    visibility: if callee == "defmacro" {
                        Visibility::Public
                    } else {
                        Visibility::Private
                    },
                    line: node.start_position().row + 1,
                });
            }
        }
        "defstruct" => {
            symbols.push(Symbol {
                name: "defstruct".to_string(),
                kind: SymbolKind::Struct,
                signature: get_signature(node, source_bytes),
                doc_comment: None,
                visibility: Visibility::Public,
                line: node.start_position().row + 1,
            });
        }
        "alias" | "import" | "use" | "require" => {
            let module_name = get_elixir_directive_module(node, source_bytes);
            if let Some(name) = module_name {
                if let Some(target) = resolve_elixir_module(&name, file_path) {
                    imports.push(ImportEdge {
                        source: file_path.to_string(),
                        target,
                        specifier: name,
                        symbols: vec!["*".to_string()],
                    });
                }
            }
        }
        _ => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "do_block" || child.kind() == "arguments" {
                    extract_elixir_recursive(
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

fn get_elixir_module_name(node: &Node, source_bytes: &[u8]) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "arguments" {
            let mut arg_cursor = child.walk();
            for arg in child.children(&mut arg_cursor) {
                if arg.kind() == "alias" {
                    return arg.utf8_text(source_bytes).ok().map(|s| s.to_string());
                }
            }
        }
    }
    None
}

fn get_elixir_def_name(node: &Node, source_bytes: &[u8]) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "arguments" {
            let mut arg_cursor = child.walk();
            for arg in child.children(&mut arg_cursor) {
                match arg.kind() {
                    "call" => {
                        if let Some(name_node) = arg.child(0) {
                            if name_node.kind() == "identifier" {
                                return name_node
                                    .utf8_text(source_bytes)
                                    .ok()
                                    .map(|s| s.to_string());
                            }
                        }
                    }
                    "identifier" => {
                        return arg.utf8_text(source_bytes).ok().map(|s| s.to_string());
                    }
                    "binary_operator" => {
                        if let Some(left) = arg.child(0) {
                            if left.kind() == "call" {
                                if let Some(name_node) = left.child(0) {
                                    if name_node.kind() == "identifier" {
                                        return name_node
                                            .utf8_text(source_bytes)
                                            .ok()
                                            .map(|s| s.to_string());
                                    }
                                }
                            } else if left.kind() == "identifier" {
                                return left.utf8_text(source_bytes).ok().map(|s| s.to_string());
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    None
}

fn get_elixir_directive_module(node: &Node, source_bytes: &[u8]) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "arguments" {
            let mut arg_cursor = child.walk();
            for arg in child.children(&mut arg_cursor) {
                if arg.kind() == "alias" {
                    return arg.utf8_text(source_bytes).ok().map(|s| s.to_string());
                }
            }
        }
        if child.kind() == "alias" {
            return child.utf8_text(source_bytes).ok().map(|s| s.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_elixir(source: &str) -> tree_sitter::Tree {
        let mut parser = tree_sitter::Parser::new();
        let lang = tree_sitter_elixir::LANGUAGE.into();
        parser.set_language(&lang).unwrap();
        parser.parse(source, None).unwrap()
    }

    #[test]
    fn test_elixir_module_extraction() {
        let source = r#"
defmodule MyApp.Greeter do
  def greet(name) do
    "Hello, #{name}"
  end

  defp internal_helper do
    :ok
  end
end
"#;
        let tree = parse_elixir(source);
        let (_, symbols) = extract_elixir("/test.ex", source, tree.root_node());

        assert!(symbols
            .iter()
            .any(|s| s.name == "MyApp.Greeter" && s.kind == SymbolKind::Module));
        assert!(symbols.iter().any(|s| s.name == "greet"
            && s.kind == SymbolKind::Function
            && s.visibility == Visibility::Public));
        assert!(symbols.iter().any(|s| s.name == "internal_helper"
            && s.kind == SymbolKind::Function
            && s.visibility == Visibility::Private));
    }

    #[test]
    fn test_elixir_macro_extraction() {
        let source = r#"
defmodule MyApp.DSL do
  defmacro my_macro(arg) do
    quote do: unquote(arg)
  end
end
"#;
        let tree = parse_elixir(source);
        let (_, symbols) = extract_elixir("/test.ex", source, tree.root_node());

        assert!(symbols
            .iter()
            .any(|s| s.name == "my_macro" && s.kind == SymbolKind::Macro));
    }

    #[test]
    fn test_elixir_doc_comment() {
        let source = r#"
@doc """
Greets a person by name.
Returns a greeting string.
"""
def greet(name) do
  "Hello, #{name}"
end
"#;
        let tree = parse_elixir(source);
        let (_, symbols) = extract_elixir("/test.ex", source, tree.root_node());

        let sym = symbols.iter().find(|s| s.name == "greet").unwrap();
        assert!(sym.doc_comment.is_some());
        let doc = sym.doc_comment.as_ref().unwrap();
        assert!(doc.contains("Greets a person by name"));
    }

    #[test]
    fn test_elixir_defstruct() {
        let source = r#"
defmodule MyApp.User do
  defstruct [:name, :email]
end
"#;
        let tree = parse_elixir(source);
        let (_, symbols) = extract_elixir("/test.ex", source, tree.root_node());

        assert!(symbols.iter().any(|s| s.kind == SymbolKind::Struct));
    }
}
