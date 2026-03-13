use super::*;
use crate::db::queries::{replace_imports, upsert_file};
use crate::db::setup::create_database;
use crate::types::{ImportEdge, SymbolKind, Visibility};

fn test_db() -> rusqlite::Connection {
    create_database(":memory:").unwrap()
}

fn make_symbol(name: &str, kind: SymbolKind) -> Symbol {
    Symbol {
        name: name.to_string(),
        kind,
        signature: format!("fn {name}()"),
        doc_comment: None,
        visibility: Visibility::Exported,
        line: 1,
    }
}

/// Seed a diamond-shaped graph:
///   A → B → D
///   A → C → D
fn seed_diamond(db: &Connection) {
    let proj = "/proj";
    for f in &["/proj/a.ts", "/proj/b.ts", "/proj/c.ts", "/proj/d.ts"] {
        upsert_file(db, proj, f, "typescript", &[], "h").unwrap();
    }
    replace_imports(
        db,
        proj,
        "/proj/a.ts",
        &[
            ImportEdge {
                source: "/proj/a.ts".into(),
                target: "/proj/b.ts".into(),
                specifier: "./b".into(),
                symbols: vec![],
            },
            ImportEdge {
                source: "/proj/a.ts".into(),
                target: "/proj/c.ts".into(),
                specifier: "./c".into(),
                symbols: vec![],
            },
        ],
    )
    .unwrap();
    replace_imports(
        db,
        proj,
        "/proj/b.ts",
        &[ImportEdge {
            source: "/proj/b.ts".into(),
            target: "/proj/d.ts".into(),
            specifier: "./d".into(),
            symbols: vec![],
        }],
    )
    .unwrap();
    replace_imports(
        db,
        proj,
        "/proj/c.ts",
        &[ImportEdge {
            source: "/proj/c.ts".into(),
            target: "/proj/d.ts".into(),
            specifier: "./d".into(),
            symbols: vec![],
        }],
    )
    .unwrap();
}

#[test]
fn test_graph_walk_entry_only() {
    let db = test_db();
    seed_diamond(&db);

    let results =
        walk_import_graph(&db, "/proj", &["/proj/a.ts".into()], Some(0), Some(20)).unwrap();

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].file_path, "/proj/a.ts");
    assert_eq!(results[0].reason, "entry");
    assert_eq!(results[0].depth, 0);
}

#[test]
fn test_graph_walk_depth_1() {
    let db = test_db();
    seed_diamond(&db);

    let results =
        walk_import_graph(&db, "/proj", &["/proj/a.ts".into()], Some(1), Some(20)).unwrap();

    assert_eq!(results.len(), 3);

    let entry = results.iter().find(|r| r.reason == "entry").unwrap();
    assert_eq!(entry.file_path, "/proj/a.ts");

    let deps: Vec<&str> = results
        .iter()
        .filter(|r| r.reason == "dependency")
        .map(|r| r.file_path.as_str())
        .collect();
    assert!(deps.contains(&"/proj/b.ts"));
    assert!(deps.contains(&"/proj/c.ts"));
}

#[test]
fn test_graph_walk_depth_2_reaches_d() {
    let db = test_db();
    seed_diamond(&db);

    let results =
        walk_import_graph(&db, "/proj", &["/proj/a.ts".into()], Some(2), Some(20)).unwrap();

    assert_eq!(results.len(), 4);
    let d = results
        .iter()
        .find(|r| r.file_path == "/proj/d.ts")
        .unwrap();
    assert_eq!(d.reason, "dependency");
    assert_eq!(d.depth, 2);
}

#[test]
fn test_graph_walk_dependents() {
    let db = test_db();
    seed_diamond(&db);

    let results =
        walk_import_graph(&db, "/proj", &["/proj/d.ts".into()], Some(1), Some(20)).unwrap();

    assert_eq!(results.len(), 3);
    let dependents: Vec<&str> = results
        .iter()
        .filter(|r| r.reason == "dependent")
        .map(|r| r.file_path.as_str())
        .collect();
    assert!(dependents.contains(&"/proj/b.ts"));
    assert!(dependents.contains(&"/proj/c.ts"));
}

#[test]
fn test_graph_walk_bidirectional() {
    let db = test_db();
    seed_diamond(&db);

    let results =
        walk_import_graph(&db, "/proj", &["/proj/b.ts".into()], Some(1), Some(20)).unwrap();

    let paths: Vec<&str> = results.iter().map(|r| r.file_path.as_str()).collect();
    assert!(paths.contains(&"/proj/b.ts"));
    assert!(paths.contains(&"/proj/a.ts"));
    assert!(paths.contains(&"/proj/d.ts"));
}

#[test]
fn test_graph_walk_max_results_limit() {
    let db = test_db();
    seed_diamond(&db);

    let results =
        walk_import_graph(&db, "/proj", &["/proj/a.ts".into()], Some(5), Some(2)).unwrap();

    assert_eq!(results.len(), 2);
}

#[test]
fn test_graph_walk_empty_entry_points() {
    let db = test_db();
    let results = walk_import_graph(&db, "/proj", &[], Some(2), Some(20)).unwrap();
    assert!(results.is_empty());
}

#[test]
fn test_graph_walk_relative_path() {
    let db = test_db();
    upsert_file(&db, "/proj", "/proj/src/a.ts", "typescript", &[], "h").unwrap();

    let results =
        walk_import_graph(&db, "/proj", &["/proj/src/a.ts".into()], Some(0), Some(20)).unwrap();

    assert_eq!(results[0].relative_path, "src/a.ts");
}

#[test]
fn test_graph_walk_preserves_symbols() {
    let db = test_db();
    let syms = vec![
        make_symbol("validate", SymbolKind::Function),
        make_symbol("Config", SymbolKind::Interface),
    ];
    upsert_file(&db, "/p", "/p/a.ts", "typescript", &syms, "h").unwrap();

    let results = walk_import_graph(&db, "/p", &["/p/a.ts".into()], Some(0), Some(20)).unwrap();

    assert_eq!(results[0].symbols.len(), 2);
    assert!(results[0].symbols.iter().any(|s| s.name == "validate"));
    assert!(results[0].symbols.iter().any(|s| s.name == "Config"));
}

#[test]
fn test_fts_search_by_file_path() {
    let db = test_db();
    upsert_file(
        &db,
        "/p",
        "/p/src/auth/middleware.ts",
        "typescript",
        &[],
        "h",
    )
    .unwrap();
    upsert_file(&db, "/p", "/p/src/db/queries.ts", "typescript", &[], "h").unwrap();

    let results = search_files(&db, "/p", "middleware", 10).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].0, "/p/src/auth/middleware.ts");
}

#[test]
fn test_fts_search_by_symbol_name() {
    let db = test_db();
    let syms = vec![
        make_symbol("validateToken", SymbolKind::Function),
        make_symbol("refreshToken", SymbolKind::Function),
    ];
    upsert_file(&db, "/p", "/p/src/auth.ts", "typescript", &syms, "h").unwrap();

    let other_syms = vec![make_symbol("query", SymbolKind::Function)];
    upsert_file(&db, "/p", "/p/src/db.ts", "typescript", &other_syms, "h").unwrap();

    let results = search_files(&db, "/p", "validateToken", 10).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].0, "/p/src/auth.ts");
}

#[test]
fn test_fts_search_no_json_noise() {
    let db = test_db();
    let syms = vec![make_symbol("doStuff", SymbolKind::Function)];
    upsert_file(&db, "/p", "/p/a.ts", "typescript", &syms, "h").unwrap();

    let results = search_files(&db, "/p", "signature", 10).unwrap();
    assert!(results.is_empty());
}

#[test]
fn test_fts_respects_project_filter() {
    let db = test_db();
    upsert_file(&db, "/p1", "/p1/a.ts", "typescript", &[], "h").unwrap();
    upsert_file(&db, "/p2", "/p2/a.ts", "typescript", &[], "h").unwrap();

    let results = search_files(&db, "/p1", "a", 10).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].0, "/p1/a.ts");
}

#[test]
fn test_find_cycles_simple() {
    let db = test_db();
    let proj = "/p";
    for f in &["/p/a.ts", "/p/b.ts", "/p/c.ts"] {
        upsert_file(&db, proj, f, "typescript", &[], "h").unwrap();
    }
    replace_imports(
        &db,
        proj,
        "/p/a.ts",
        &[ImportEdge {
            source: "/p/a.ts".into(),
            target: "/p/b.ts".into(),
            specifier: "./b".into(),
            symbols: vec![],
        }],
    )
    .unwrap();
    replace_imports(
        &db,
        proj,
        "/p/b.ts",
        &[ImportEdge {
            source: "/p/b.ts".into(),
            target: "/p/c.ts".into(),
            specifier: "./c".into(),
            symbols: vec![],
        }],
    )
    .unwrap();
    replace_imports(
        &db,
        proj,
        "/p/c.ts",
        &[ImportEdge {
            source: "/p/c.ts".into(),
            target: "/p/a.ts".into(),
            specifier: "./a".into(),
            symbols: vec![],
        }],
    )
    .unwrap();

    let cycles = find_cycles(&db, proj).unwrap();
    assert_eq!(cycles.len(), 1);
    assert_eq!(cycles[0].len(), 4);
    assert_eq!(cycles[0].first(), cycles[0].last());
}

#[test]
fn test_find_cycles_none() {
    let db = test_db();
    seed_diamond(&db);
    let cycles = find_cycles(&db, "/proj").unwrap();
    assert!(cycles.is_empty());
}

#[test]
fn test_find_cycles_self_loop() {
    let db = test_db();
    upsert_file(&db, "/p", "/p/a.ts", "typescript", &[], "h").unwrap();
    replace_imports(
        &db,
        "/p",
        "/p/a.ts",
        &[ImportEdge {
            source: "/p/a.ts".into(),
            target: "/p/a.ts".into(),
            specifier: "./a".into(),
            symbols: vec![],
        }],
    )
    .unwrap();

    let cycles = find_cycles(&db, "/p").unwrap();
    assert_eq!(cycles.len(), 1);
}

#[test]
fn test_get_file_detail() {
    let db = test_db();
    let syms = vec![
        make_symbol("handler", SymbolKind::Function),
        make_symbol("Config", SymbolKind::Interface),
    ];
    upsert_file(&db, "/p", "/p/a.ts", "typescript", &syms, "h").unwrap();
    upsert_file(&db, "/p", "/p/b.ts", "typescript", &[], "h").unwrap();
    upsert_file(&db, "/p", "/p/c.ts", "typescript", &[], "h").unwrap();

    replace_imports(
        &db,
        "/p",
        "/p/a.ts",
        &[ImportEdge {
            source: "/p/a.ts".into(),
            target: "/p/b.ts".into(),
            specifier: "./b".into(),
            symbols: vec!["foo".into()],
        }],
    )
    .unwrap();
    replace_imports(
        &db,
        "/p",
        "/p/c.ts",
        &[ImportEdge {
            source: "/p/c.ts".into(),
            target: "/p/a.ts".into(),
            specifier: "./a".into(),
            symbols: vec!["handler".into()],
        }],
    )
    .unwrap();

    let detail = get_file_detail(&db, "/p", "/p/a.ts").unwrap().unwrap();
    assert_eq!(detail.language, "typescript");
    assert_eq!(detail.symbols.len(), 2);
    assert_eq!(detail.imports.len(), 1);
    assert_eq!(detail.imports[0].0, "/p/b.ts");
    assert_eq!(detail.imports[0].1, vec!["foo"]);
    assert_eq!(detail.dependents.len(), 1);
    assert_eq!(detail.dependents[0].0, "/p/c.ts");
    assert_eq!(detail.dependents[0].1, vec!["handler"]);
}

#[test]
fn test_get_file_detail_not_found() {
    let db = test_db();
    let detail = get_file_detail(&db, "/p", "/p/nope.ts").unwrap();
    assert!(detail.is_none());
}
