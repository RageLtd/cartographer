use tree_sitter::{Node, TreeCursor};

use crate::types::{ImportEdge, Symbol, SymbolKind, Visibility};

use super::extractor::{get_doc_comment, get_signature, strip_quotes};
use super::resolver::resolve_ts_js_import;

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
                    let syms = extract_ts_js_decl_symbols(
                        source,
                        &node,
                        source_bytes,
                        Visibility::Private,
                    );
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
            let syms =
                extract_ts_js_decl_symbols(source, &child, source_bytes, Visibility::Exported);
            for mut sym in syms {
                if let Some(doc) = get_doc_comment(source, export_node) {
                    sym.doc_comment = Some(doc);
                }
                sym.visibility = Visibility::Exported;
                out.push(sym);
            }
            continue;
        }

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

fn extract_ts_js_re_export(
    file_path: &str,
    node: &Node,
    source_bytes: &[u8],
) -> Option<ImportEdge> {
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

#[cfg(test)]
#[path = "ts_js_tests.rs"]
mod tests;
