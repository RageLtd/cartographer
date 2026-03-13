use tree_sitter::Node;

use crate::types::{ImportEdge, Symbol, SymbolKind, Visibility};

use super::extractor::{get_doc_comment, get_signature};
use super::resolver::{resolve_rust_mod_decl, resolve_rust_module};

pub fn extract_rust(
    file_path: &str,
    source: &str,
    root: Node,
    crate_root: &str,
) -> (Vec<ImportEdge>, Vec<Symbol>) {
    let mut imports = Vec::new();
    let mut symbols = Vec::new();
    let source_bytes = source.as_bytes();

    let mut cursor = root.walk();
    for node in root.children(&mut cursor) {
        match node.kind() {
            "use_declaration" => {
                let edges = extract_rust_use(file_path, &node, source_bytes, crate_root);
                imports.extend(edges);
            }
            "mod_item" => {
                if let Some(edge) = extract_rust_mod(file_path, &node, source_bytes) {
                    imports.push(edge);
                }
            }
            _ => {}
        }

        if let Some(sym) = extract_rust_symbol(source, &node, source_bytes) {
            symbols.push(sym);
        }
    }

    (imports, symbols)
}

fn rust_kind_from_type(node_type: &str) -> Option<SymbolKind> {
    match node_type {
        "function_item" => Some(SymbolKind::Function),
        "struct_item" => Some(SymbolKind::Struct),
        "enum_item" => Some(SymbolKind::Enum),
        "trait_item" => Some(SymbolKind::Trait),
        "impl_item" => Some(SymbolKind::Impl),
        "type_item" => Some(SymbolKind::Type),
        "const_item" | "static_item" => Some(SymbolKind::Const),
        "macro_definition" => Some(SymbolKind::Macro),
        "mod_item" => Some(SymbolKind::Module),
        _ => None,
    }
}

fn extract_rust_symbol(source: &str, node: &Node, source_bytes: &[u8]) -> Option<Symbol> {
    let kind = rust_kind_from_type(node.kind())?;

    let name = if let Some(name_node) = node.child_by_field_name("name") {
        name_node.utf8_text(source_bytes).ok()?.to_string()
    } else if kind == SymbolKind::Impl {
        get_impl_name(node, source_bytes)?
    } else {
        return None;
    };

    Some(Symbol {
        name,
        kind,
        signature: get_signature(node, source_bytes),
        doc_comment: get_doc_comment(source, node),
        visibility: get_rust_visibility(node, source_bytes),
        line: node.start_position().row + 1,
    })
}

fn get_rust_visibility(node: &Node, source_bytes: &[u8]) -> Visibility {
    let first_child = match node.child(0) {
        Some(c) => c,
        None => return Visibility::Private,
    };

    if first_child.kind() != "visibility_modifier" {
        return Visibility::Private;
    }

    let text = first_child.utf8_text(source_bytes).unwrap_or("");
    if text == "pub" {
        Visibility::Public
    } else if text.contains("crate") {
        Visibility::PubCrate
    } else if text.contains("super") {
        Visibility::PubSuper
    } else {
        Visibility::Public
    }
}

fn get_impl_name(node: &Node, source_bytes: &[u8]) -> Option<String> {
    if let Some(type_node) = node.child_by_field_name("type") {
        return type_node
            .utf8_text(source_bytes)
            .ok()
            .map(|s| s.to_string());
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "type_identifier" {
            return child.utf8_text(source_bytes).ok().map(|s| s.to_string());
        }
    }
    None
}

fn extract_rust_use(
    file_path: &str,
    node: &Node,
    source_bytes: &[u8],
    crate_root: &str,
) -> Vec<ImportEdge> {
    let mut edges = Vec::new();

    let path_text = match extract_rust_use_path(node, source_bytes) {
        Some(p) => p,
        None => return edges,
    };

    if let Some(resolved) = resolve_rust_module(&path_text, file_path, crate_root) {
        let segments: Vec<&str> = path_text.split("::").collect();
        let symbol = segments.last().copied().unwrap_or("");
        edges.push(ImportEdge {
            source: file_path.to_string(),
            target: resolved,
            specifier: path_text.clone(),
            symbols: if symbol == "*" {
                vec!["*".to_string()]
            } else {
                vec![symbol.to_string()]
            },
        });
    }

    edges
}

fn extract_rust_use_path(node: &Node, source_bytes: &[u8]) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "scoped_identifier" | "scoped_use_list" | "use_wildcard" | "identifier"
            | "use_as_clause" => {
                return child.utf8_text(source_bytes).ok().map(|s| s.to_string());
            }
            _ => {}
        }
    }
    None
}

fn extract_rust_mod(file_path: &str, node: &Node, source_bytes: &[u8]) -> Option<ImportEdge> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "declaration_list" {
            return None;
        }
    }

    let name_node = node.child_by_field_name("name")?;
    let mod_name = name_node.utf8_text(source_bytes).ok()?;
    let resolved = resolve_rust_mod_decl(mod_name, file_path)?;

    Some(ImportEdge {
        source: file_path.to_string(),
        target: resolved,
        specifier: format!("mod {mod_name}"),
        symbols: vec![mod_name.to_string()],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_rust_code(source: &str) -> tree_sitter::Tree {
        let mut parser = tree_sitter::Parser::new();
        let lang = tree_sitter_rust::LANGUAGE.into();
        parser.set_language(&lang).unwrap();
        parser.parse(source, None).unwrap()
    }

    #[test]
    fn test_extract_rust_symbols() {
        let source = r#"
/// A greeter function
pub fn greet(name: &str) -> String {
    format!("Hello, {}", name)
}

struct Config {
    debug: bool,
}

impl Config {
    fn new() -> Self {
        Config { debug: false }
    }
}
"#;
        let tree = parse_rust_code(source);
        let (_, symbols) = extract_rust("/test.rs", source, tree.root_node(), "/");

        assert!(symbols
            .iter()
            .any(|s| s.name == "greet" && s.kind == SymbolKind::Function));
        assert!(symbols
            .iter()
            .any(|s| s.name == "Config" && s.kind == SymbolKind::Struct));
        assert!(symbols
            .iter()
            .any(|s| s.name == "Config" && s.kind == SymbolKind::Impl));
    }

    #[test]
    fn test_doc_comment_extraction() {
        let source = r#"
/// This is a doc comment
/// with multiple lines
pub fn documented() {}
"#;
        let tree = parse_rust_code(source);
        let (_, symbols) = extract_rust("/test.rs", source, tree.root_node(), "/");

        let sym = symbols.iter().find(|s| s.name == "documented").unwrap();
        assert!(sym.doc_comment.is_some());
        let doc = sym.doc_comment.as_ref().unwrap();
        assert!(doc.contains("This is a doc comment"));
        assert!(doc.contains("with multiple lines"));
    }

    #[test]
    fn test_rust_visibility() {
        let source = r#"
pub fn public_fn() {}
pub(crate) fn crate_fn() {}
fn private_fn() {}
"#;
        let tree = parse_rust_code(source);
        let (_, symbols) = extract_rust("/test.rs", source, tree.root_node(), "/");

        let pub_fn = symbols.iter().find(|s| s.name == "public_fn").unwrap();
        assert_eq!(pub_fn.visibility, Visibility::Public);

        let crate_fn = symbols.iter().find(|s| s.name == "crate_fn").unwrap();
        assert_eq!(crate_fn.visibility, Visibility::PubCrate);

        let private_fn = symbols.iter().find(|s| s.name == "private_fn").unwrap();
        assert_eq!(private_fn.visibility, Visibility::Private);
    }

    #[test]
    fn test_rust_enum_extraction() {
        let source = r#"
pub enum Color {
    Red,
    Green,
    Blue,
}
"#;
        let tree = parse_rust_code(source);
        let (_, symbols) = extract_rust("/test.rs", source, tree.root_node(), "/");

        let color = symbols.iter().find(|s| s.name == "Color").unwrap();
        assert_eq!(color.kind, SymbolKind::Enum);
        assert_eq!(color.visibility, Visibility::Public);
    }

    #[test]
    fn test_rust_trait_extraction() {
        let source = r#"
pub trait Drawable {
    fn draw(&self);
}
"#;
        let tree = parse_rust_code(source);
        let (_, symbols) = extract_rust("/test.rs", source, tree.root_node(), "/");

        let t = symbols.iter().find(|s| s.name == "Drawable").unwrap();
        assert_eq!(t.kind, SymbolKind::Trait);
    }

    #[test]
    fn test_rust_const_extraction() {
        let source = "pub const MAX_SIZE: usize = 1024;\n";
        let tree = parse_rust_code(source);
        let (_, symbols) = extract_rust("/test.rs", source, tree.root_node(), "/");

        let c = symbols.iter().find(|s| s.name == "MAX_SIZE").unwrap();
        assert_eq!(c.kind, SymbolKind::Const);
    }

    #[test]
    fn test_rust_type_alias() {
        let source = "pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;\n";
        let tree = parse_rust_code(source);
        let (_, symbols) = extract_rust("/test.rs", source, tree.root_node(), "/");

        let t = symbols.iter().find(|s| s.name == "Result").unwrap();
        assert_eq!(t.kind, SymbolKind::Type);
    }

    #[test]
    fn test_rust_macro_extraction() {
        let source = r#"
macro_rules! say_hello {
    () => { println!("Hello!"); };
}
"#;
        let tree = parse_rust_code(source);
        let (_, symbols) = extract_rust("/test.rs", source, tree.root_node(), "/");

        let m = symbols.iter().find(|s| s.name == "say_hello").unwrap();
        assert_eq!(m.kind, SymbolKind::Macro);
    }

    #[test]
    fn test_rust_impl_trait_name() {
        let source = r#"
struct Foo;

impl std::fmt::Display for Foo {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Foo")
    }
}
"#;
        let tree = parse_rust_code(source);
        let (_, symbols) = extract_rust("/test.rs", source, tree.root_node(), "/");

        let impl_sym = symbols.iter().find(|s| s.kind == SymbolKind::Impl).unwrap();
        assert_eq!(impl_sym.name, "Foo");
    }

    #[test]
    fn test_rust_pub_super_visibility() {
        let source = "pub(super) fn parent_visible() {}\n";
        let tree = parse_rust_code(source);
        let (_, symbols) = extract_rust("/test.rs", source, tree.root_node(), "/");

        let sym = symbols.iter().find(|s| s.name == "parent_visible").unwrap();
        assert_eq!(sym.visibility, Visibility::PubSuper);
    }

    #[test]
    fn test_rust_inline_mod_not_import() {
        let source = r#"
mod tests {
    fn test_something() {}
}
"#;
        let tree = parse_rust_code(source);
        let (imports, symbols) = extract_rust("/test.rs", source, tree.root_node(), "/");

        assert!(imports.is_empty());
        let mod_sym = symbols.iter().find(|s| s.name == "tests").unwrap();
        assert_eq!(mod_sym.kind, SymbolKind::Module);
    }

    #[test]
    fn test_rust_line_numbers() {
        let source = "pub fn first() {}\n\npub fn third() {}\n";
        let tree = parse_rust_code(source);
        let (_, symbols) = extract_rust("/test.rs", source, tree.root_node(), "/");

        let first = symbols.iter().find(|s| s.name == "first").unwrap();
        assert_eq!(first.line, 1);

        let third = symbols.iter().find(|s| s.name == "third").unwrap();
        assert_eq!(third.line, 3);
    }
}
