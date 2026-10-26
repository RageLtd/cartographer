//! `cartographer parse <file>`: parse one file with the assembled registry
//! (built-in plus host grammars) and print the extraction as JSON.
//!
//! This is the smoke test for a freshly installed host grammar — it needs no
//! MCP client and no database, and it exercises exactly the path the indexer
//! and the MCP tools use.

use serde::Serialize;

use crate::grammars;
use crate::parser::parse_file;
use crate::parser::registry::LanguageRegistry;
use crate::types::{ImportEdge, Symbol};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParseReport {
    pub file_path: String,
    pub language: String,
    pub imports: Vec<ImportEdge>,
    pub symbols: Vec<Symbol>,
}

pub fn parse_report(registry: &LanguageRegistry, file_path: &str) -> Result<ParseReport, String> {
    let result = parse_file(registry, file_path, None)?;
    Ok(ParseReport {
        file_path: file_path.to_string(),
        language: result.language,
        imports: result.imports,
        symbols: result.symbols,
    })
}

/// Run the subcommand; returns the process exit code. Grammar directories that
/// fail to load are reported on stderr but do not prevent parsing with the rest.
pub fn run_cli(args: &[String]) -> i32 {
    let Some(file_path) = args.first() else {
        eprintln!("Usage: cartographer parse <file>");
        return 2;
    };

    let report = grammars::load_registry();
    for failure in &report.failures {
        eprintln!(
            "warning: skipping grammar {}: {}",
            failure.dir.display(),
            failure.message
        );
    }

    let json = parse_report(&report.registry, file_path)
        .and_then(|parsed| sonic_rs::to_string_pretty(&parsed).map_err(|e| e.to_string()));
    match json {
        Ok(json) => {
            println!("{json}");
            0
        }
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::types::{SymbolKind, Visibility};

    #[test]
    fn reports_language_symbols_and_imports_for_a_builtin_grammar() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(dir.path().join("Cargo.toml"), "[package]\nname = \"x\"").unwrap();
        fs::create_dir(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/util.rs"), "pub fn helper() {}").unwrap();
        let lib = dir.path().join("src/lib.rs");
        fs::write(&lib, "mod util;\n\n/// Says hello.\npub fn hi() {}\n").unwrap();

        let report =
            parse_report(&LanguageRegistry::builtin(), lib.to_str().unwrap()).expect("rust parses");

        assert_eq!(report.language, "rust");
        assert_eq!(report.file_path, lib.to_str().unwrap());
        let hi = report
            .symbols
            .iter()
            .find(|s| s.name == "hi")
            .expect("hi symbol");
        assert_eq!(hi.kind, SymbolKind::Function);
        assert_eq!(hi.visibility, Visibility::Public);
        assert_eq!(hi.doc_comment.as_deref(), Some("Says hello."));
        assert_eq!(report.imports.len(), 1);
        assert!(report.imports[0].target.ends_with("src/util.rs"));

        let json = sonic_rs::to_string(&report).expect("serializes");
        assert!(json.contains("\"filePath\""), "camelCase keys: {json}");
    }

    #[test]
    fn unsupported_extension_is_an_error_not_an_empty_report() {
        let dir = tempfile::tempdir().expect("tempdir");
        let readme = dir.path().join("README.md");
        fs::write(&readme, "# hi").unwrap();

        let err = parse_report(&LanguageRegistry::builtin(), readme.to_str().unwrap())
            .expect_err("markdown has no grammar");
        assert!(err.contains(".md"), "{err}");
    }
}
