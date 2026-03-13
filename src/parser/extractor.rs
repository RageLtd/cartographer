use tree_sitter::{Node, TreeCursor};

use crate::types::{ImportEdge, Symbol, SymbolKind, Visibility};

use super::resolver::{resolve_rust_mod_decl, resolve_rust_module, resolve_ts_js_import};

// ============================================================================
// Shared helpers
// ============================================================================

fn strip_quotes(text: &str) -> &str {
    text.trim_matches(|c| c == '\'' || c == '"' || c == '`')
}

/// Extract the doc comment immediately preceding a node.
fn get_doc_comment(source: &str, node: &Node) -> Option<String> {
    let start_line = node.start_position().row;
    let lines: Vec<&str> = source.lines().collect();

    let mut comment_lines: Vec<&str> = Vec::new();

    if start_line == 0 {
        return None;
    }

    // Walk backwards collecting comment lines
    for i in (0..start_line).rev() {
        let trimmed = lines[i].trim();

        if trimmed.starts_with("/**")
            || trimmed.starts_with("* ")
            || trimmed.starts_with("*/")
            || trimmed == "*"
        {
            comment_lines.push(trimmed);
            if trimmed.starts_with("/**") {
                break;
            }
            continue;
        }

        if trimmed.starts_with("///") || trimmed.starts_with("//") {
            comment_lines.push(trimmed);
            continue;
        }

        break;
    }

    if comment_lines.is_empty() {
        return None;
    }

    comment_lines.reverse();

    let result: Vec<String> = comment_lines
        .iter()
        .map(|line| {
            let s = line.to_string();
            let s = s.trim_start_matches("/**").trim_start();
            let s = s.trim_start_matches("*/").trim_start();
            let s = s.trim_start_matches("* ").trim_start_matches('*').trim_start();
            let s = s.trim_start_matches("/// ").trim_start_matches("///");
            let s = s.trim_start_matches("// ").trim_start_matches("//");
            s.trim().to_string()
        })
        .filter(|s| !s.is_empty())
        .collect();

    if result.is_empty() {
        return None;
    }

    Some(result.join("\n"))
}

/// Get a concise signature string for a node — everything up to the opening brace.
fn get_signature(node: &Node, source: &[u8]) -> String {
    let text = node.utf8_text(source).unwrap_or("");
    if let Some(brace_idx) = text.find('{') {
        return text[..brace_idx].trim().to_string();
    }
    if let Some(newline_idx) = text.find('\n') {
        return text[..newline_idx].trim().to_string();
    }
    text.trim().to_string()
}

// ============================================================================
// TypeScript / JavaScript
// ============================================================================

pub fn extract_ts_js(
    file_path: &str,
    source: &str,
    root: Node,
    _language: &str,
) -> (Vec<ImportEdge>, Vec<Symbol>) {
    let mut imports = Vec::new();
    let mut symbols = Vec::new();
    let source_bytes = source.as_bytes();

    let mut cursor = root.walk();
    for node in root.children(&mut cursor) {
        match node.kind() {
            "import_statement" => {
                if let Some(edge) = extract_ts_js_import(file_path, &node, source_bytes) {
                    imports.push(edge);
                }
            }
            "export_statement" => {
                if let Some(re_export) = extract_ts_js_re_export(file_path, &node, source_bytes) {
                    imports.push(re_export);
                }
                // Check if this is `export default ...`
                if is_default_export(&node, source_bytes) {
                    symbols.push(Symbol {
                        name: "default".to_string(),
                        kind: infer_ts_js_kind(&node),
                        signature: get_signature(&node, source_bytes),
                        doc_comment: get_doc_comment(source, &node),
                        visibility: Visibility::DefaultExport,
                        line: node.start_position().row + 1,
                    });
                } else {
                    extract_ts_js_exported_symbols(source, &node, source_bytes, &mut symbols);
                }
            }
            "export_default_declaration" => {
                symbols.push(Symbol {
                    name: "default".to_string(),
                    kind: infer_ts_js_kind(&node),
                    signature: get_signature(&node, source_bytes),
                    doc_comment: get_doc_comment(source, &node),
                    visibility: Visibility::DefaultExport,
                    line: node.start_position().row + 1,
                });
            }
            _ => {
                if is_ts_js_declaration(&node) {
                    let syms = extract_ts_js_decl_symbols(source, &node, source_bytes, Visibility::Private);
                    symbols.extend(syms);
                }
            }
        }
    }

    let dynamic_imports = extract_dynamic_imports(file_path, &root, source_bytes);
    imports.extend(dynamic_imports);

    (imports, symbols)
}

const TS_JS_DECL_TYPES: &[&str] = &[
    "function_declaration",
    "class_declaration",
    "lexical_declaration",
    "variable_declaration",
    "interface_declaration",
    "type_alias_declaration",
    "enum_declaration",
];

fn is_ts_js_declaration(node: &Node) -> bool {
    TS_JS_DECL_TYPES.contains(&node.kind())
}

fn is_default_export(node: &Node, source_bytes: &[u8]) -> bool {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.utf8_text(source_bytes).ok() == Some("default") {
            return true;
        }
    }
    false
}

fn infer_ts_js_kind(node: &Node) -> SymbolKind {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "function_declaration" | "function" => return SymbolKind::Function,
            "class_declaration" | "class" => return SymbolKind::Class,
            _ => {}
        }
    }
    SymbolKind::Unknown
}

fn ts_js_kind_from_type(node_type: &str) -> Option<SymbolKind> {
    match node_type {
        "function_declaration" => Some(SymbolKind::Function),
        "class_declaration" => Some(SymbolKind::Class),
        "interface_declaration" => Some(SymbolKind::Interface),
        "type_alias_declaration" => Some(SymbolKind::Type),
        "enum_declaration" => Some(SymbolKind::Enum),
        _ => None,
    }
}

fn extract_ts_js_exported_symbols(
    source: &str,
    export_node: &Node,
    source_bytes: &[u8],
    out: &mut Vec<Symbol>,
) {
    let mut cursor = export_node.walk();
    for child in export_node.children(&mut cursor) {
        if TS_JS_DECL_TYPES.contains(&child.kind()) {
            let syms = extract_ts_js_decl_symbols(source, &child, source_bytes, Visibility::Exported);
            for mut sym in syms {
                // Override doc comment to look at the export node level
                if let Some(doc) = get_doc_comment(source, export_node) {
                    sym.doc_comment = Some(doc);
                }
                sym.visibility = Visibility::Exported;
                out.push(sym);
            }
            continue;
        }

        // export { foo, bar, baz as qux }
        if child.kind() == "export_clause" {
            let mut spec_cursor = child.walk();
            for spec in child.children(&mut spec_cursor) {
                if spec.kind() == "export_specifier" {
                    let alias = spec.child_by_field_name("alias");
                    let name_node = spec.child_by_field_name("name");
                    let symbol_name = alias
                        .or(name_node)
                        .and_then(|n| n.utf8_text(source_bytes).ok());
                    if let Some(name) = symbol_name {
                        out.push(Symbol {
                            name: name.to_string(),
                            kind: SymbolKind::Unknown,
                            signature: spec.utf8_text(source_bytes).unwrap_or("").to_string(),
                            doc_comment: None,
                            visibility: Visibility::Exported,
                            line: spec.start_position().row + 1,
                        });
                    }
                }
            }
        }
    }
}

fn extract_ts_js_decl_symbols(
    source: &str,
    node: &Node,
    source_bytes: &[u8],
    visibility: Visibility,
) -> Vec<Symbol> {
    let mut symbols = Vec::new();

    // Variable declarations can have multiple declarators
    if node.kind() == "lexical_declaration" || node.kind() == "variable_declaration" {
        let var_kind = match node.child(0).and_then(|c| c.utf8_text(source_bytes).ok()) {
            Some("const") => SymbolKind::Const,
            Some("var") => SymbolKind::Var,
            _ => SymbolKind::Let,
        };

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "variable_declarator" {
                if let Some(name_node) = child.child_by_field_name("name") {
                    if let Ok(name) = name_node.utf8_text(source_bytes) {
                        symbols.push(Symbol {
                            name: name.to_string(),
                            kind: var_kind,
                            signature: get_signature(node, source_bytes),
                            doc_comment: get_doc_comment(source, node),
                            visibility,
                            line: node.start_position().row + 1,
                        });
                    }
                }
            }
        }
        return symbols;
    }

    if let Some(kind) = ts_js_kind_from_type(node.kind()) {
        if let Some(name_node) = node.child_by_field_name("name") {
            if let Ok(name) = name_node.utf8_text(source_bytes) {
                symbols.push(Symbol {
                    name: name.to_string(),
                    kind,
                    signature: get_signature(node, source_bytes),
                    doc_comment: get_doc_comment(source, node),
                    visibility,
                    line: node.start_position().row + 1,
                });
            }
        }
    }

    symbols
}

// ============================================================================
// TS/JS imports
// ============================================================================

fn extract_ts_js_import(file_path: &str, node: &Node, source_bytes: &[u8]) -> Option<ImportEdge> {
    let source_node = node.child_by_field_name("source")?;
    let specifier_raw = source_node.utf8_text(source_bytes).ok()?;
    let specifier = strip_quotes(specifier_raw);
    let resolved = resolve_ts_js_import(specifier, file_path)?;
    let syms = extract_import_symbols(node, source_bytes);

    Some(ImportEdge {
        source: file_path.to_string(),
        target: resolved,
        specifier: specifier.to_string(),
        symbols: syms,
    })
}

fn extract_ts_js_re_export(file_path: &str, node: &Node, source_bytes: &[u8]) -> Option<ImportEdge> {
    let source_node = node.child_by_field_name("source")?;
    let specifier_raw = source_node.utf8_text(source_bytes).ok()?;
    let specifier = strip_quotes(specifier_raw);
    let resolved = resolve_ts_js_import(specifier, file_path)?;

    Some(ImportEdge {
        source: file_path.to_string(),
        target: resolved,
        specifier: specifier.to_string(),
        symbols: vec!["*".to_string()],
    })
}

fn extract_import_symbols(node: &Node, source_bytes: &[u8]) -> Vec<String> {
    let mut symbols = Vec::new();
    collect_import_symbols_recursive(node, source_bytes, &mut symbols);
    symbols
}

fn collect_import_symbols_recursive(node: &Node, source_bytes: &[u8], symbols: &mut Vec<String>) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "identifier" => {
                // A bare identifier in an import clause means default import
                // But skip "from" keyword and the import keyword itself
                let text = child.utf8_text(source_bytes).unwrap_or("");
                if text != "from" && text != "import" {
                    symbols.push("default".to_string());
                }
            }
            "namespace_import" => symbols.push("*".to_string()),
            "named_imports" => {
                let mut spec_cursor = child.walk();
                for spec in child.children(&mut spec_cursor) {
                    if spec.kind() == "import_specifier" {
                        if let Some(name) = spec.child_by_field_name("name") {
                            if let Ok(text) = name.utf8_text(source_bytes) {
                                symbols.push(text.to_string());
                            }
                        }
                    }
                }
            }
            // Recurse into import_clause wrapper
            "import_clause" => {
                collect_import_symbols_recursive(&child, source_bytes, symbols);
            }
            _ => {}
        }
    }
}

fn extract_dynamic_imports(file_path: &str, root: &Node, source_bytes: &[u8]) -> Vec<ImportEdge> {
    let mut edges = Vec::new();
    let mut cursor = root.walk();
    visit_dynamic_imports(&mut cursor, file_path, source_bytes, &mut edges);
    edges
}

fn visit_dynamic_imports(
    cursor: &mut TreeCursor,
    file_path: &str,
    source_bytes: &[u8],
    edges: &mut Vec<ImportEdge>,
) {
    if cursor.node().kind() == "call_expression" {
        let node = cursor.node();
        if let Some(func) = node.child_by_field_name("function") {
            if func.kind() == "import" {
                if let Some(args) = node.child_by_field_name("arguments") {
                    // First real argument (skip open paren)
                    if let Some(first_arg) = args.child(0).and_then(|c| c.next_sibling()) {
                        if first_arg.kind() == "string" {
                            if let Ok(text) = first_arg.utf8_text(source_bytes) {
                                let specifier = strip_quotes(text);
                                if let Some(resolved) = resolve_ts_js_import(specifier, file_path) {
                                    edges.push(ImportEdge {
                                        source: file_path.to_string(),
                                        target: resolved,
                                        specifier: specifier.to_string(),
                                        symbols: vec!["*".to_string()],
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    if cursor.goto_first_child() {
        visit_dynamic_imports(cursor, file_path, source_bytes, edges);
        while cursor.goto_next_sibling() {
            visit_dynamic_imports(cursor, file_path, source_bytes, edges);
        }
        cursor.goto_parent();
    }
}

// ============================================================================
// Rust
// ============================================================================

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
        return type_node.utf8_text(source_bytes).ok().map(|s| s.to_string());
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "type_identifier" {
            return child.utf8_text(source_bytes).ok().map(|s| s.to_string());
        }
    }
    None
}

// ============================================================================
// Rust imports
// ============================================================================

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
    // Skip inline mod blocks (have a declaration_list body)
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

    fn parse_ts(source: &str) -> tree_sitter::Tree {
        let mut parser = tree_sitter::Parser::new();
        let lang = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
        parser.set_language(&lang).unwrap();
        parser.parse(source, None).unwrap()
    }

    fn parse_rust_code(source: &str) -> tree_sitter::Tree {
        let mut parser = tree_sitter::Parser::new();
        let lang = tree_sitter_rust::LANGUAGE.into();
        parser.set_language(&lang).unwrap();
        parser.parse(source, None).unwrap()
    }

    #[test]
    fn test_extract_ts_symbols() {
        let source = r#"
export function greet(name: string): string {
    return `Hello, ${name}`;
}

const x = 42;

export class Foo {
    bar() {}
}
"#;
        let tree = parse_ts(source);
        let (_, symbols) = extract_ts_js("/test.ts", source, tree.root_node(), "typescript");

        assert!(symbols.iter().any(|s| s.name == "greet" && s.kind == SymbolKind::Function));
        assert!(symbols.iter().any(|s| s.name == "x" && s.kind == SymbolKind::Const));
        assert!(symbols.iter().any(|s| s.name == "Foo" && s.kind == SymbolKind::Class));
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

        assert!(symbols.iter().any(|s| s.name == "greet" && s.kind == SymbolKind::Function));
        assert!(symbols.iter().any(|s| s.name == "Config" && s.kind == SymbolKind::Struct));
        assert!(symbols.iter().any(|s| s.name == "Config" && s.kind == SymbolKind::Impl));
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

    // ====================================================================
    // TS/JS: Import extraction
    // ====================================================================

    #[test]
    fn test_ts_named_imports() {
        let dir = tempfile::TempDir::new().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("utils.ts"), "export const a = 1; export const b = 2;").unwrap();
        let from_file = src.join("index.ts").to_str().unwrap().to_string();

        let source = r#"import { a, b } from "./utils";"#;
        let tree = parse_ts(source);
        let (imports, _) = extract_ts_js(&from_file, source, tree.root_node(), "typescript");

        assert_eq!(imports.len(), 1);
        assert!(imports[0].target.ends_with("utils.ts"));
        assert_eq!(imports[0].specifier, "./utils");
        assert!(imports[0].symbols.contains(&"a".to_string()));
        assert!(imports[0].symbols.contains(&"b".to_string()));
    }

    #[test]
    fn test_ts_default_import() {
        let dir = tempfile::TempDir::new().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("config.ts"), "export default {};").unwrap();
        let from_file = src.join("index.ts").to_str().unwrap().to_string();

        let source = r#"import config from "./config";"#;
        let tree = parse_ts(source);
        let (imports, _) = extract_ts_js(&from_file, source, tree.root_node(), "typescript");

        assert_eq!(imports.len(), 1);
        assert!(imports[0].symbols.contains(&"default".to_string()));
    }

    #[test]
    fn test_ts_namespace_import() {
        let dir = tempfile::TempDir::new().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("utils.ts"), "").unwrap();
        let from_file = src.join("index.ts").to_str().unwrap().to_string();

        let source = r#"import * as utils from "./utils";"#;
        let tree = parse_ts(source);
        let (imports, _) = extract_ts_js(&from_file, source, tree.root_node(), "typescript");

        assert_eq!(imports.len(), 1);
        assert!(imports[0].symbols.contains(&"*".to_string()));
    }

    #[test]
    fn test_ts_bare_import_skipped() {
        let source = r#"import express from "express";"#;
        let tree = parse_ts(source);
        let (imports, _) = extract_ts_js("/test.ts", source, tree.root_node(), "typescript");
        assert!(imports.is_empty());
    }

    #[test]
    fn test_ts_re_export() {
        let dir = tempfile::TempDir::new().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("types.ts"), "export type Foo = {};").unwrap();
        let from_file = src.join("index.ts").to_str().unwrap().to_string();

        let source = r#"export { Foo } from "./types";"#;
        let tree = parse_ts(source);
        let (imports, _) = extract_ts_js(&from_file, source, tree.root_node(), "typescript");

        assert_eq!(imports.len(), 1);
        assert!(imports[0].target.ends_with("types.ts"));
        assert!(imports[0].symbols.contains(&"*".to_string()));
    }

    // ====================================================================
    // TS/JS: Symbol extraction — interfaces, types, enums, export clause
    // ====================================================================

    #[test]
    fn test_ts_interface_extraction() {
        let source = r#"
export interface Config {
    debug: boolean;
    port: number;
}
"#;
        let tree = parse_ts(source);
        let (_, symbols) = extract_ts_js("/test.ts", source, tree.root_node(), "typescript");

        let config = symbols.iter().find(|s| s.name == "Config").unwrap();
        assert_eq!(config.kind, SymbolKind::Interface);
        assert_eq!(config.visibility, Visibility::Exported);
    }

    #[test]
    fn test_ts_type_alias_extraction() {
        let source = r#"
export type Result<T> = { ok: true; value: T } | { ok: false; error: Error };
"#;
        let tree = parse_ts(source);
        let (_, symbols) = extract_ts_js("/test.ts", source, tree.root_node(), "typescript");

        let result = symbols.iter().find(|s| s.name == "Result").unwrap();
        assert_eq!(result.kind, SymbolKind::Type);
        assert_eq!(result.visibility, Visibility::Exported);
    }

    #[test]
    fn test_ts_enum_extraction() {
        let source = r#"
export enum Direction {
    Up,
    Down,
    Left,
    Right,
}
"#;
        let tree = parse_ts(source);
        let (_, symbols) = extract_ts_js("/test.ts", source, tree.root_node(), "typescript");

        let dir = symbols.iter().find(|s| s.name == "Direction").unwrap();
        assert_eq!(dir.kind, SymbolKind::Enum);
    }

    #[test]
    fn test_ts_export_clause() {
        let source = r#"
const foo = 1;
const bar = 2;
export { foo, bar };
"#;
        let tree = parse_ts(source);
        let (_, symbols) = extract_ts_js("/test.ts", source, tree.root_node(), "typescript");

        // Should have private foo/bar + exported foo/bar from the export clause
        let exported: Vec<_> = symbols
            .iter()
            .filter(|s| s.visibility == Visibility::Exported)
            .collect();
        assert!(exported.iter().any(|s| s.name == "foo"));
        assert!(exported.iter().any(|s| s.name == "bar"));
    }

    #[test]
    fn test_ts_export_default() {
        let source = r#"
export default function main() {
    return 42;
}
"#;
        let tree = parse_ts(source);
        let (_, symbols) = extract_ts_js("/test.ts", source, tree.root_node(), "typescript");

        let def = symbols.iter().find(|s| s.visibility == Visibility::DefaultExport);
        assert!(def.is_some());
    }

    #[test]
    fn test_ts_multiple_variable_declarators() {
        let source = r#"
export const WIDTH = 800, HEIGHT = 600;
"#;
        let tree = parse_ts(source);
        let (_, symbols) = extract_ts_js("/test.ts", source, tree.root_node(), "typescript");

        assert!(symbols.iter().any(|s| s.name == "WIDTH" && s.kind == SymbolKind::Const));
        assert!(symbols.iter().any(|s| s.name == "HEIGHT" && s.kind == SymbolKind::Const));
    }

    #[test]
    fn test_ts_let_declaration() {
        let source = r#"
let counter = 0;
"#;
        let tree = parse_ts(source);
        let (_, symbols) = extract_ts_js("/test.ts", source, tree.root_node(), "typescript");

        let counter = symbols.iter().find(|s| s.name == "counter").unwrap();
        assert_eq!(counter.kind, SymbolKind::Let);
        assert_eq!(counter.visibility, Visibility::Private);
    }

    #[test]
    fn test_ts_var_declaration() {
        let source = r#"
var legacy = "old";
"#;
        let tree = parse_ts(source);
        let (_, symbols) = extract_ts_js("/test.ts", source, tree.root_node(), "typescript");

        let legacy = symbols.iter().find(|s| s.name == "legacy").unwrap();
        assert_eq!(legacy.kind, SymbolKind::Var);
        assert_eq!(legacy.visibility, Visibility::Private);
    }

    #[test]
    fn test_ts_jsdoc_comment() {
        let source = r#"
/**
 * Validate the given token.
 * @param token - JWT string
 * @returns true if valid
 */
export function validateToken(token: string): boolean {
    return true;
}
"#;
        let tree = parse_ts(source);
        let (_, symbols) = extract_ts_js("/test.ts", source, tree.root_node(), "typescript");

        let sym = symbols.iter().find(|s| s.name == "validateToken").unwrap();
        assert!(sym.doc_comment.is_some());
        let doc = sym.doc_comment.as_ref().unwrap();
        assert!(doc.contains("Validate the given token"));
    }

    #[test]
    fn test_ts_signature_stops_at_brace() {
        let source = r#"
export function greet(name: string): string {
    return name;
}
"#;
        let tree = parse_ts(source);
        let (_, symbols) = extract_ts_js("/test.ts", source, tree.root_node(), "typescript");

        let sym = symbols.iter().find(|s| s.name == "greet").unwrap();
        assert!(sym.signature.contains("greet"));
        assert!(!sym.signature.contains("return"));
    }

    // ====================================================================
    // Rust: All symbol kinds
    // ====================================================================

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
        let source = r#"
pub const MAX_SIZE: usize = 1024;
"#;
        let tree = parse_rust_code(source);
        let (_, symbols) = extract_rust("/test.rs", source, tree.root_node(), "/");

        let c = symbols.iter().find(|s| s.name == "MAX_SIZE").unwrap();
        assert_eq!(c.kind, SymbolKind::Const);
    }

    #[test]
    fn test_rust_type_alias() {
        let source = r#"
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
"#;
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
        let source = r#"
pub(super) fn parent_visible() {}
"#;
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

        // Inline mod should produce a module symbol but no import edge
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
