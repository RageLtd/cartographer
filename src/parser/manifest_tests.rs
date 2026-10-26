use std::fs;
use std::path::{Path, PathBuf};

use super::*;
use crate::parser::fixtures::compile_tiny_grammar;
use crate::parser::parse_file;
use crate::parser::registry::BuiltinLanguage;
use crate::types::SymbolKind;

const TINY_TAGS: &str = r#"
(
  (comment)* @doc
  .
  (function_definition visibility: (pub)? @visibility name: (identifier) @name) @definition.function
  (#strip! @doc "^#\\s*")
  (#select-adjacent! @doc @definition.function)
)
"#;

const TINY_IMPORTS: &str = "(import_statement path: (string) @import.specifier)";

const TINY_MANIFEST: &str = r#"{
  "extensions": ["tiny", ".tny"],
  "resolvers": [
    { "relative-path": { "extensions": [".tiny"] } }
  ]
}"#;

/// Create `<root>/tiny/` with a compiled library plus the given files.
/// Returns `None` when the fixture cannot be compiled on this host.
fn grammar_dir(root: &Path, files: &[(&str, &str)]) -> Option<PathBuf> {
    let dir = root.join("tiny");
    fs::create_dir_all(&dir).unwrap();
    compile_tiny_grammar(&dir)?;
    for (name, content) in files {
        fs::write(dir.join(name), content).unwrap();
    }
    Some(dir)
}

fn full_grammar_dir(root: &Path) -> Option<PathBuf> {
    grammar_dir(
        root,
        &[
            (MANIFEST_FILE, TINY_MANIFEST),
            (DEFAULT_TAGS_FILE, TINY_TAGS),
            (DEFAULT_IMPORTS_FILE, TINY_IMPORTS),
        ],
    )
}

#[test]
fn discovered_grammar_parses_files_end_to_end() {
    let grammars = tempfile::tempdir().unwrap();
    let Some(_) = full_grammar_dir(grammars.path()) else {
        return;
    };
    // Noise that discovery must ignore: a stray file and a dot-directory.
    fs::write(grammars.path().join("README.md"), "ignored").unwrap();
    fs::create_dir_all(grammars.path().join(".cache")).unwrap();

    let mut registry = LanguageRegistry::builtin();
    let failures = register_host_grammars(&mut registry, grammars.path());
    assert!(failures.is_empty(), "{failures:?}");
    assert_eq!(registry.languages().len(), BuiltinLanguage::ALL.len() + 1);

    let def = registry.for_path("/p/main.tiny").expect(".tiny registered");
    assert_eq!(def.name, "tiny");
    assert_eq!(
        def.extensions,
        vec![".tiny", ".tny"],
        "extensions are dot-normalised"
    );
    assert!(registry.supports_path("/p/other.tny"));
    assert!(registry.supports_path("/p/still.rs"), "builtins untouched");

    let project = tempfile::tempdir().unwrap();
    fs::write(project.path().join("util.tiny"), "fn util() {}").unwrap();
    let main = project.path().join("main.tiny");
    fs::write(&main, "# Entry\npub fn main() {}\nuse \"./util\";\n").unwrap();

    let result = parse_file(&registry, main.to_str().unwrap(), None).expect("parses");
    assert_eq!(result.language, "tiny");
    assert_eq!(result.symbols.len(), 1);
    assert_eq!(result.symbols[0].name, "main");
    assert_eq!(result.symbols[0].kind, SymbolKind::Function);
    assert_eq!(result.symbols[0].visibility, Visibility::Public);
    assert_eq!(result.symbols[0].doc_comment.as_deref(), Some("Entry"));
    assert_eq!(result.imports.len(), 1);
    assert_eq!(
        result.imports[0].target,
        project.path().join("util.tiny").to_str().unwrap()
    );
}

#[test]
fn missing_grammars_root_is_not_a_failure() {
    let mut registry = LanguageRegistry::builtin();
    let failures = register_host_grammars(&mut registry, Path::new("/nonexistent/grammars"));
    assert!(failures.is_empty());
    assert_eq!(registry.languages().len(), BuiltinLanguage::ALL.len());
}

#[test]
fn one_broken_grammar_does_not_block_the_others() {
    let grammars = tempfile::tempdir().unwrap();
    let Some(_) = full_grammar_dir(grammars.path()) else {
        return;
    };
    let broken = grammars.path().join("broken");
    fs::create_dir_all(&broken).unwrap();
    fs::write(broken.join(MANIFEST_FILE), "{ not json").unwrap();

    let mut registry = LanguageRegistry::builtin();
    let failures = register_host_grammars(&mut registry, grammars.path());

    assert!(
        registry.supports_path("/p/main.tiny"),
        "healthy grammar still loads"
    );
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].dir, broken);
    assert!(
        failures[0].message.starts_with("Invalid "),
        "{}",
        failures[0].message
    );
    assert!(
        failures[0].message.contains(MANIFEST_FILE),
        "{}",
        failures[0].message
    );
}

#[test]
fn manifest_without_extensions_is_rejected() {
    let grammars = tempfile::tempdir().unwrap();
    let Some(dir) = grammar_dir(
        grammars.path(),
        &[
            (MANIFEST_FILE, r#"{ "extensions": [" "] }"#),
            (DEFAULT_TAGS_FILE, TINY_TAGS),
        ],
    ) else {
        return;
    };
    let err = load_grammar_dir(&dir).expect_err("blank extensions must fail");
    assert!(err.contains("`extensions`"), "{err}");
}

#[test]
fn missing_library_names_the_expected_files() {
    let grammars = tempfile::tempdir().unwrap();
    let dir = grammars.path().join("tiny");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join(MANIFEST_FILE), TINY_MANIFEST).unwrap();
    fs::write(dir.join(DEFAULT_TAGS_FILE), TINY_TAGS).unwrap();

    let err = load_grammar_dir(&dir).expect_err("no .so present");
    assert!(err.contains("tiny.so"), "{err}");
    assert!(err.contains("tiny.dylib"), "{err}");
}

#[test]
fn library_path_must_stay_inside_the_grammar_dir() {
    let grammars = tempfile::tempdir().unwrap();
    let dir = grammars.path().join("tiny");
    fs::create_dir_all(&dir).unwrap();
    for escape in ["../evil.so", "/usr/lib/evil.so", "sub/dir.so"] {
        let manifest = format!(r#"{{ "extensions": ["tiny"], "library": "{escape}" }}"#);
        fs::write(dir.join(MANIFEST_FILE), manifest).unwrap();
        let err = load_grammar_dir(&dir).expect_err("escaping library path must fail");
        assert!(
            err.contains("`library` must be a file name"),
            "{escape}: {err}"
        );
    }
}

#[test]
fn declared_query_file_must_exist_but_defaults_are_optional() {
    let grammars = tempfile::tempdir().unwrap();
    let Some(dir) = grammar_dir(
        grammars.path(),
        &[(
            MANIFEST_FILE,
            r#"{ "extensions": ["tiny"], "tags": "custom-tags.scm" }"#,
        )],
    ) else {
        return;
    };
    let err = load_grammar_dir(&dir).expect_err("declared tags file missing");
    assert!(err.contains("custom-tags.scm"), "{err}");

    // Only an imports query, no tags: allowed.
    fs::write(
        dir.join(MANIFEST_FILE),
        r#"{ "extensions": ["tiny"], "resolvers": [] }"#,
    )
    .unwrap();
    fs::write(dir.join(DEFAULT_IMPORTS_FILE), TINY_IMPORTS).unwrap();
    load_grammar_dir(&dir).expect("imports-only grammar loads");
}

#[test]
fn grammar_with_no_queries_is_rejected() {
    let grammars = tempfile::tempdir().unwrap();
    let Some(dir) = grammar_dir(
        grammars.path(),
        &[(MANIFEST_FILE, r#"{ "extensions": ["tiny"] }"#)],
    ) else {
        return;
    };
    let err = load_grammar_dir(&dir).expect_err("no queries");
    assert!(err.contains("neither tags.scm nor imports.scm"), "{err}");
}

#[test]
fn manifest_deserializes_every_field_including_resolvers() {
    let json = r#"{
      "name": "python",
      "extensions": [".py"],
      "library": "python.so",
      "symbol": "tree_sitter_python",
      "tags": "tags.scm",
      "imports": "imports.scm",
      "default_visibility": "public",
      "resolvers": [
        { "relative-path": { "extensions": [".py"], "index_names": ["__init__"] } },
        { "module-path": {
            "separator": ".",
            "extensions": [".py"],
            "root_marker": "pyproject.toml",
            "source_dirs": ["src", ""],
            "index_names": ["__init__"],
            "strip_prefixes": []
        } }
      ]
    }"#;
    let manifest: GrammarManifest = sonic_rs::from_str(json).expect("valid manifest");
    assert_eq!(manifest.name.as_deref(), Some("python"));
    assert_eq!(manifest.default_visibility, Some(Visibility::Public));
    assert_eq!(
        manifest.resolvers,
        vec![
            ResolverStrategy::RelativePath {
                extensions: vec![".py".to_string()],
                index_names: vec!["__init__".to_string()],
            },
            ResolverStrategy::ModulePath {
                separator: ".".to_string(),
                extensions: vec![".py".to_string()],
                root_marker: Some("pyproject.toml".to_string()),
                source_dirs: vec!["src".to_string(), String::new()],
                index_names: vec!["__init__".to_string()],
                strip_prefixes: vec![],
            },
        ]
    );
}
