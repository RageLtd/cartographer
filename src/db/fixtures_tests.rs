//! Fixtures shared by the storage tests (`client_tests`, `queries_tests`,
//! `graph_tests`).

use tempfile::TempDir;

use super::client::{open, Db};
use super::queries::upsert_file;
use crate::types::{EdgeKind, ImportEdge, Symbol, SymbolKind, Visibility};

pub const PROJECT: &str = "/repo";
pub const OTHER_PROJECT: &str = "/other";
pub const STORE_FILE: &str = "index.db";

/// A fresh store in its own temporary directory. Keep the `TempDir` alive for
/// as long as the store is used.
pub fn store() -> (Db, TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let db = open(&dir.path().join(STORE_FILE)).unwrap();
    (db, dir)
}

pub fn symbol(name: &str) -> Symbol {
    Symbol {
        name: name.to_string(),
        kind: SymbolKind::Function,
        signature: format!("fn {name}()"),
        doc_comment: None,
        visibility: Visibility::Public,
        line: 1,
    }
}

pub fn symbols(names: &[&str]) -> Vec<Symbol> {
    names.iter().map(|name| symbol(name)).collect()
}

pub fn names(symbols: &[Symbol]) -> Vec<String> {
    symbols.iter().map(|s| s.name.clone()).collect()
}

pub fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

pub fn edge(source: &str, target: &str, specifier: &str, symbols: &[&str]) -> ImportEdge {
    ImportEdge {
        source: source.to_string(),
        target: target.to_string(),
        specifier: specifier.to_string(),
        symbols: strings(symbols),
        kind: EdgeKind::Import,
    }
}

/// The edge a Rust `mod <name>;` in `parent` makes to the file of `child`.
pub fn module_edge(parent: &str, child: &str, name: &str) -> ImportEdge {
    ImportEdge {
        kind: EdgeKind::ModuleDeclaration,
        ..edge(parent, child, &format!("mod {name}"), &[name])
    }
}

/// Upsert a Rust file with the given symbol names.
pub fn add_file(db: &Db, project: &str, file_path: &str, symbol_names: &[&str]) {
    upsert_file(
        db,
        project,
        file_path,
        "rust",
        &symbols(symbol_names),
        "hash",
    )
    .unwrap();
}

/// `path` under `PROJECT`: `p("src/a.rs")` is `/repo/src/a.rs`.
pub fn p(path: &str) -> String {
    format!("{PROJECT}/{path}")
}
