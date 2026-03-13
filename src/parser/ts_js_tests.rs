use super::*;

fn parse_ts(source: &str) -> tree_sitter::Tree {
    let mut parser = tree_sitter::Parser::new();
    let lang = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
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

    assert!(symbols
        .iter()
        .any(|s| s.name == "greet" && s.kind == SymbolKind::Function));
    assert!(symbols
        .iter()
        .any(|s| s.name == "x" && s.kind == SymbolKind::Const));
    assert!(symbols
        .iter()
        .any(|s| s.name == "Foo" && s.kind == SymbolKind::Class));
}

#[test]
fn test_ts_named_imports() {
    let dir = tempfile::TempDir::new().unwrap();
    let src = dir.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(
        src.join("utils.ts"),
        "export const a = 1; export const b = 2;",
    )
    .unwrap();
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

    let def = symbols
        .iter()
        .find(|s| s.visibility == Visibility::DefaultExport);
    assert!(def.is_some());
}

#[test]
fn test_ts_multiple_variable_declarators() {
    let source = r#"
export const WIDTH = 800, HEIGHT = 600;
"#;
    let tree = parse_ts(source);
    let (_, symbols) = extract_ts_js("/test.ts", source, tree.root_node(), "typescript");

    assert!(symbols
        .iter()
        .any(|s| s.name == "WIDTH" && s.kind == SymbolKind::Const));
    assert!(symbols
        .iter()
        .any(|s| s.name == "HEIGHT" && s.kind == SymbolKind::Const));
}

#[test]
fn test_ts_let_declaration() {
    let source = "let counter = 0;\n";
    let tree = parse_ts(source);
    let (_, symbols) = extract_ts_js("/test.ts", source, tree.root_node(), "typescript");

    let counter = symbols.iter().find(|s| s.name == "counter").unwrap();
    assert_eq!(counter.kind, SymbolKind::Let);
    assert_eq!(counter.visibility, Visibility::Private);
}

#[test]
fn test_ts_var_declaration() {
    let source = "var legacy = \"old\";\n";
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
