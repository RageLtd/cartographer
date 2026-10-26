use std::fs;

use super::*;
use crate::parser::dynamic::{language_symbol, load_grammar};
use crate::parser::fixtures::{compile_tiny_grammar, TINY_GRAMMAR_NAME};
use crate::parser::registry::BuiltinLanguage;

fn parse(language: &Language, source: &str) -> tree_sitter::Tree {
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(language).expect("grammar accepted");
    parser.parse(source, None).expect("parse")
}

fn generic(
    language: &Language,
    tags: Option<&str>,
    imports: Option<&str>,
    resolvers: Vec<ResolverStrategy>,
) -> GenericLanguage {
    GenericLanguage::new(language, tags, imports, resolvers, Visibility::Private)
        .expect("queries compile")
}

const RUST_TAGS_WITH_DOCS: &str = r#"
(
  (line_comment)* @doc
  .
  (function_item name: (identifier) @name) @definition.function
  (#strip! @doc "^///\\s*")
  (#select-adjacent! @doc @definition.function)
)
(struct_item name: (type_identifier) @name) @definition.class
"#;

const RUST_SOURCE: &str = r#"
/// Greets.
/// Loudly.
pub fn greet() {}

// unrelated

fn helper() {}

struct Config {}
"#;

#[test]
fn tags_query_yields_symbols_with_adjacent_docs_stripped() {
    let language = BuiltinLanguage::Rust.grammar();
    let lang = generic(&language, Some(RUST_TAGS_WITH_DOCS), None, vec![]);
    let tree = parse(&language, RUST_SOURCE);

    let (imports, symbols) = extract_generic("/p/lib.rs", RUST_SOURCE, tree.root_node(), &lang);
    assert!(imports.is_empty());

    let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["greet", "helper", "Config"]);

    let greet = &symbols[0];
    assert_eq!(greet.kind, SymbolKind::Function);
    assert_eq!(greet.doc_comment.as_deref(), Some("Greets.\nLoudly."));
    assert_eq!(greet.signature, "pub fn greet()");
    assert_eq!(greet.line, 4);
    assert_eq!(
        greet.visibility,
        Visibility::Private,
        "no @visibility capture → default"
    );

    let helper = &symbols[1];
    assert_eq!(
        helper.doc_comment, None,
        "a blank line separates the comment from the definition"
    );

    assert_eq!(
        symbols[2].kind,
        SymbolKind::Class,
        "tags.scm convention maps structs to class"
    );
}

#[test]
fn visibility_capture_marks_public_definitions() {
    let language = BuiltinLanguage::Rust.grammar();
    let tags = r#"
(function_item (visibility_modifier)? @visibility name: (identifier) @name) @definition.function
"#;
    let source = "pub fn open() {}\npub(crate) fn inner() {}\nfn hidden() {}\n";
    let lang = generic(&language, Some(tags), None, vec![]);
    let tree = parse(&language, source);

    let (_, symbols) = extract_generic("/p/lib.rs", source, tree.root_node(), &lang);
    let vis: Vec<(&str, Visibility)> = symbols
        .iter()
        .map(|s| (s.name.as_str(), s.visibility))
        .collect();
    assert_eq!(
        vis,
        vec![
            ("open", Visibility::Public),
            ("inner", Visibility::PubCrate),
            ("hidden", Visibility::Private),
        ]
    );
}

#[test]
fn method_and_function_patterns_do_not_duplicate_a_symbol() {
    let language = BuiltinLanguage::Rust.grammar();
    // Mirrors tree-sitter-rust's real tags.scm, where a method matches both patterns.
    let tags = r#"
(declaration_list (function_item name: (identifier) @name) @definition.method)
(function_item name: (identifier) @name) @definition.function
"#;
    let source = "struct A;\nimpl A {\n    fn run(&self) {}\n}\nfn free() {}\n";
    let lang = generic(&language, Some(tags), None, vec![]);
    let tree = parse(&language, source);

    let (_, symbols) = extract_generic("/p/lib.rs", source, tree.root_node(), &lang);
    let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["run", "free"]);
}

#[test]
fn unknown_definition_tags_become_unknown_kind() {
    assert_eq!(kind_from_tag("widget"), SymbolKind::Unknown);
    assert_eq!(kind_from_tag("method"), SymbolKind::Function);
    assert_eq!(kind_from_tag("constant"), SymbolKind::Const);
}

#[test]
fn invalid_query_is_reported_with_its_role() {
    let language = BuiltinLanguage::Rust.grammar();
    let err = GenericLanguage::new(
        &language,
        Some("(no_such_node) @definition.function"),
        None,
        vec![],
        Visibility::Private,
    )
    .expect_err("bad node name must fail");
    assert!(err.starts_with("tags query:"), "{err}");

    let err = GenericLanguage::new(
        &language,
        None,
        Some("(use_declaration"),
        vec![],
        Visibility::Private,
    )
    .expect_err("unbalanced query must fail");
    assert!(err.starts_with("imports query:"), "{err}");
}

#[test]
fn imports_resolve_through_a_module_path_strategy() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    fs::write(root.join("Cargo.toml"), "[package]\nname = \"x\"").unwrap();
    fs::create_dir_all(root.join("src/db")).unwrap();
    fs::write(root.join("src/auth.rs"), "pub fn check() {}").unwrap();
    fs::write(root.join("src/db/mod.rs"), "").unwrap();
    let lib_rs = root.join("src/lib.rs");
    let source = "use crate::auth::check;\nuse crate::db;\nuse std::fmt;\n";
    fs::write(&lib_rs, source).unwrap();

    let language = BuiltinLanguage::Rust.grammar();
    let imports_query = "(use_declaration argument: (_) @import.specifier)";
    let strategy = ResolverStrategy::ModulePath {
        separator: "::".to_string(),
        extensions: vec![".rs".to_string()],
        root_marker: Some("Cargo.toml".to_string()),
        source_dirs: vec!["src".to_string()],
        index_names: vec!["mod".to_string()],
        strip_prefixes: vec!["crate::".to_string()],
    };
    let lang = generic(&language, None, Some(imports_query), vec![strategy]);
    let tree = parse(&language, source);

    let (imports, symbols) =
        extract_generic(lib_rs.to_str().unwrap(), source, tree.root_node(), &lang);
    assert!(symbols.is_empty(), "no tags query → no symbols");

    let edges: Vec<(String, Vec<String>)> = imports
        .iter()
        .map(|i| {
            let rel = i.target.strip_prefix(root.to_str().unwrap()).unwrap();
            (rel.to_string(), i.symbols.clone())
        })
        .collect();
    assert_eq!(
        edges,
        vec![
            ("/src/auth.rs".to_string(), vec!["check".to_string()]),
            ("/src/db/mod.rs".to_string(), vec![]),
        ],
        "std::fmt is external and must not resolve"
    );
    assert_eq!(imports[0].specifier, "crate::auth::check");
}

#[test]
fn imports_resolve_through_a_relative_path_strategy_with_symbols() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    fs::write(root.join("util.ts"), "export const a = 1;").unwrap();
    let main_ts = root.join("main.ts");
    let source = "import { a, b } from \"./util\";\nimport x from \"lodash\";\n";
    fs::write(&main_ts, source).unwrap();

    let language = BuiltinLanguage::TypeScript.grammar();
    let imports_query = r#"
(import_statement
  (import_clause (named_imports (import_specifier name: (identifier) @import.symbol)))?
  source: (string) @import.specifier)
"#;
    let strategy = ResolverStrategy::RelativePath {
        extensions: vec![".ts".to_string()],
        index_names: vec!["index".to_string()],
    };
    let lang = generic(&language, None, Some(imports_query), vec![strategy]);
    let tree = parse(&language, source);

    let (imports, _) = extract_generic(main_ts.to_str().unwrap(), source, tree.root_node(), &lang);
    assert_eq!(imports.len(), 1, "bare specifiers are skipped: {imports:?}");
    assert_eq!(imports[0].target, root.join("util.ts").to_str().unwrap());
    assert_eq!(imports[0].specifier, "./util");
    assert_eq!(imports[0].symbols, vec!["a", "b"]);
}

#[test]
fn dynamically_loaded_fixture_grammar_extracts_end_to_end() {
    let build_dir = tempfile::tempdir().expect("tempdir");
    let Some(so) = compile_tiny_grammar(build_dir.path()) else {
        return;
    };
    let language =
        load_grammar(&so, &language_symbol(TINY_GRAMMAR_NAME)).expect("fixture grammar loads");

    let project = tempfile::tempdir().expect("tempdir");
    fs::write(project.path().join("util.tiny"), "fn util() {}").unwrap();
    let main = project.path().join("main.tiny");
    let source = "# Entry point\n# of the program\npub fn main() {}\nuse \"./util\";\n\n# detached\n\nfn helper() {}\n";
    fs::write(&main, source).unwrap();

    let tags = r#"
(
  (comment)* @doc
  .
  (function_definition visibility: (pub)? @visibility name: (identifier) @name) @definition.function
  (#strip! @doc "^#\\s*")
  (#select-adjacent! @doc @definition.function)
)
"#;
    let imports = "(import_statement path: (string) @import.specifier)";
    let strategy = ResolverStrategy::RelativePath {
        extensions: vec![".tiny".to_string()],
        index_names: vec![],
    };
    let lang = generic(&language, Some(tags), Some(imports), vec![strategy]);
    let tree = parse(&language, source);
    assert!(
        !tree.root_node().has_error(),
        "{}",
        tree.root_node().to_sexp()
    );

    let (imports, symbols) =
        extract_generic(main.to_str().unwrap(), source, tree.root_node(), &lang);

    assert_eq!(symbols.len(), 2);
    assert_eq!(symbols[0].name, "main");
    assert_eq!(symbols[0].visibility, Visibility::Public);
    assert_eq!(
        symbols[0].doc_comment.as_deref(),
        Some("Entry point\nof the program")
    );
    assert_eq!(symbols[0].line, 3);
    assert_eq!(symbols[1].name, "helper");
    assert_eq!(symbols[1].visibility, Visibility::Private);
    assert_eq!(symbols[1].doc_comment, None);

    assert_eq!(imports.len(), 1);
    assert_eq!(
        imports[0].target,
        project.path().join("util.tiny").to_str().unwrap()
    );
    assert_eq!(imports[0].specifier, "./util");
}
