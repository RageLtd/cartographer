use std::collections::HashMap;

use super::*;
use crate::db::client::Db;
use crate::db::fixtures_tests::*;
use crate::db::graph::get_file_detail;

/// The (target, sorted symbols) pairs `file_path` imports, sorted.
fn imports_of(db: &Db, project: &str, file_path: &str) -> Vec<(String, Vec<String>)> {
    let detail = get_file_detail(db, project, file_path)
        .unwrap()
        .unwrap_or_else(|| panic!("{file_path} should be indexed"));
    sorted_pairs(detail.imports)
}

/// The (source, sorted symbols) pairs importing `file_path`, sorted.
fn dependents_of(db: &Db, project: &str, file_path: &str) -> Vec<(String, Vec<String>)> {
    let detail = get_file_detail(db, project, file_path)
        .unwrap()
        .unwrap_or_else(|| panic!("{file_path} should be indexed"));
    sorted_pairs(detail.dependents)
}

fn sorted_pairs(mut pairs: Vec<(String, Vec<String>)>) -> Vec<(String, Vec<String>)> {
    for (_, symbols) in &mut pairs {
        symbols.sort();
    }
    pairs.sort();
    pairs
}

// ============================================================================
// Files
// ============================================================================

#[test]
fn upserted_file_reads_back() {
    let (db, _dir) = store();
    let a = p("src/a.rs");

    upsert_file(&db, PROJECT, &a, "rust", &symbols(&["alpha"]), "h1").unwrap();

    assert_eq!(
        get_file_hash(&db, PROJECT, &a).unwrap(),
        Some("h1".to_string())
    );
    assert_eq!(get_file_count(&db, PROJECT).unwrap(), 1);
    assert_eq!(
        get_language_counts(&db, PROJECT).unwrap(),
        HashMap::from([("rust".to_string(), 1)])
    );
    assert_eq!(
        get_project_stats(&db).unwrap(),
        vec![(PROJECT.to_string(), 1)]
    );
}

#[test]
fn unknown_file_has_no_hash_and_empty_store_counts_zero() {
    let (db, _dir) = store();

    assert_eq!(get_file_hash(&db, PROJECT, &p("src/a.rs")).unwrap(), None);
    assert_eq!(get_file_count(&db, PROJECT).unwrap(), 0);
    assert_eq!(get_import_count(&db, PROJECT).unwrap(), 0);
    assert!(get_language_counts(&db, PROJECT).unwrap().is_empty());
    assert!(get_project_stats(&db).unwrap().is_empty());
}

#[test]
fn upserting_the_same_path_replaces_it() {
    let (db, _dir) = store();
    let a = p("src/a.ts");

    upsert_file(&db, PROJECT, &a, "javascript", &symbols(&["old"]), "h1").unwrap();
    upsert_file(&db, PROJECT, &a, "typescript", &symbols(&["new"]), "h2").unwrap();

    assert_eq!(get_file_count(&db, PROJECT).unwrap(), 1);
    assert_eq!(
        get_file_hash(&db, PROJECT, &a).unwrap(),
        Some("h2".to_string())
    );
    assert_eq!(
        get_language_counts(&db, PROJECT).unwrap(),
        HashMap::from([("typescript".to_string(), 1)])
    );
    let detail = get_file_detail(&db, PROJECT, &a).unwrap().unwrap();
    assert_eq!(names(&detail.symbols), strings(&["new"]));
}

#[test]
fn removing_a_file_removes_it_and_its_edges() {
    let (db, _dir) = store();
    let (a, b, c) = (p("a.rs"), p("b.rs"), p("c.rs"));
    for file in [&a, &b, &c] {
        add_file(&db, PROJECT, file, &[]);
    }
    replace_imports(&db, PROJECT, &a, &[edge(&a, &b, "./b", &["B"])]).unwrap();
    replace_imports(&db, PROJECT, &b, &[edge(&b, &c, "./c", &["C"])]).unwrap();
    replace_imports(&db, PROJECT, &c, &[edge(&c, &a, "./a", &["A"])]).unwrap();

    remove_file(&db, PROJECT, &b).unwrap();

    assert_eq!(get_file_count(&db, PROJECT).unwrap(), 2);
    assert_eq!(get_file_hash(&db, PROJECT, &b).unwrap(), None);
    assert_eq!(get_import_count(&db, PROJECT).unwrap(), 1);
    assert!(imports_of(&db, PROJECT, &a).is_empty());
    assert!(dependents_of(&db, PROJECT, &c).is_empty());
    assert_eq!(
        dependents_of(&db, PROJECT, &a),
        vec![(c.clone(), strings(&["A"]))]
    );
}

// ============================================================================
// Imports
// ============================================================================

#[test]
fn replacing_imports_replaces_only_that_files_edges() {
    let (db, _dir) = store();
    let (a, b, c, d) = (p("a.rs"), p("b.rs"), p("c.rs"), p("d.rs"));
    for file in [&a, &b, &c, &d] {
        add_file(&db, PROJECT, file, &[]);
    }
    replace_imports(
        &db,
        PROJECT,
        &a,
        &[edge(&a, &b, "./b", &["B"]), edge(&a, &c, "./c", &["C"])],
    )
    .unwrap();
    replace_imports(&db, PROJECT, &d, &[edge(&d, &b, "./b", &["B"])]).unwrap();

    replace_imports(&db, PROJECT, &a, &[edge(&a, &d, "./d", &["D"])]).unwrap();

    assert_eq!(get_import_count(&db, PROJECT).unwrap(), 2);
    assert_eq!(
        imports_of(&db, PROJECT, &a),
        vec![(d.clone(), strings(&["D"]))]
    );
    assert_eq!(
        dependents_of(&db, PROJECT, &b),
        vec![(d.clone(), strings(&["B"]))]
    );
    assert!(dependents_of(&db, PROJECT, &c).is_empty());
}

// The indexer stores a file's imports as it parses it, before the files they
// point at have been indexed.
#[test]
fn an_edge_to_a_file_not_yet_indexed_is_stored() {
    let (db, _dir) = store();
    let (a, b) = (p("a.rs"), p("b.rs"));
    add_file(&db, PROJECT, &a, &[]);

    replace_imports(&db, PROJECT, &a, &[edge(&a, &b, "./b", &["B"])]).unwrap();

    assert_eq!(get_import_count(&db, PROJECT).unwrap(), 1);
    assert_eq!(
        imports_of(&db, PROJECT, &a),
        vec![(b.clone(), strings(&["B"]))]
    );
}

// Re-indexing one file must not drop the edges other files hold to it.
#[test]
fn upserting_a_file_again_keeps_the_edges_into_and_out_of_it() {
    let (db, _dir) = store();
    let (a, b, c) = (p("a.rs"), p("b.rs"), p("c.rs"));
    for file in [&a, &b, &c] {
        add_file(&db, PROJECT, file, &[]);
    }
    replace_imports(&db, PROJECT, &a, &[edge(&a, &b, "./b", &["B"])]).unwrap();
    replace_imports(&db, PROJECT, &b, &[edge(&b, &c, "./c", &["C"])]).unwrap();

    upsert_file(&db, PROJECT, &b, "rust", &symbols(&["changed"]), "h2").unwrap();

    assert_eq!(get_import_count(&db, PROJECT).unwrap(), 2);
    assert_eq!(
        dependents_of(&db, PROJECT, &b),
        vec![(a.clone(), strings(&["B"]))]
    );
    assert_eq!(
        imports_of(&db, PROJECT, &b),
        vec![(c.clone(), strings(&["C"]))]
    );
}

#[test]
fn replacing_imports_with_none_clears_them() {
    let (db, _dir) = store();
    let (a, b) = (p("a.rs"), p("b.rs"));
    add_file(&db, PROJECT, &a, &[]);
    add_file(&db, PROJECT, &b, &[]);
    replace_imports(&db, PROJECT, &a, &[edge(&a, &b, "./b", &["B"])]).unwrap();

    replace_imports(&db, PROJECT, &a, &[]).unwrap();

    assert_eq!(get_import_count(&db, PROJECT).unwrap(), 0);
    assert!(imports_of(&db, PROJECT, &a).is_empty());
}

// `import type { A } from "./b"` and `import { b } from "./b"` are two
// declarations with the same source, target and specifier.
#[test]
fn duplicate_edges_merge_into_one_carrying_all_symbols() {
    let (db, _dir) = store();
    let (a, b, c) = (p("a.ts"), p("b.ts"), p("c.ts"));
    for file in [&a, &b, &c] {
        add_file(&db, PROJECT, file, &[]);
    }

    replace_imports(
        &db,
        PROJECT,
        &a,
        &[
            edge(&a, &b, "./b", &["A"]),
            edge(&a, &b, "./b", &["b"]),
            edge(&a, &b, "./b", &["c"]),
            edge(&a, &b, "./b.js", &["d"]),
            edge(&a, &c, "./b", &["e"]),
        ],
    )
    .unwrap();

    assert_eq!(get_import_count(&db, PROJECT).unwrap(), 3);
    assert_eq!(
        imports_of(&db, PROJECT, &a),
        vec![
            (b.clone(), strings(&["A", "b", "c"])),
            (b.clone(), strings(&["d"])),
            (c.clone(), strings(&["e"])),
        ]
    );
}

#[test]
fn duplicate_edges_merge_across_a_second_replace() {
    let (db, _dir) = store();
    let (a, b) = (p("a.ts"), p("b.ts"));
    add_file(&db, PROJECT, &a, &[]);
    add_file(&db, PROJECT, &b, &[]);
    let edges = [edge(&a, &b, "./b", &["A"]), edge(&a, &b, "./b", &["b"])];

    replace_imports(&db, PROJECT, &a, &edges).unwrap();
    replace_imports(&db, PROJECT, &a, &edges).unwrap();

    assert_eq!(get_import_count(&db, PROJECT).unwrap(), 1);
    assert_eq!(
        imports_of(&db, PROJECT, &a),
        vec![(b.clone(), strings(&["A", "b"]))]
    );
}

// ============================================================================
// Project scoping
// ============================================================================

#[test]
fn the_same_path_in_two_projects_is_two_files() {
    let (db, _dir) = store();
    let (a, b) = (p("a.rs"), p("b.rs"));

    upsert_file(&db, PROJECT, &a, "rust", &symbols(&["one"]), "h1").unwrap();
    upsert_file(&db, OTHER_PROJECT, &a, "ruby", &symbols(&["two"]), "h2").unwrap();
    add_file(&db, OTHER_PROJECT, &b, &[]);
    replace_imports(&db, OTHER_PROJECT, &a, &[edge(&a, &b, "./b", &["B"])]).unwrap();

    assert_eq!(
        get_file_hash(&db, PROJECT, &a).unwrap(),
        Some("h1".to_string())
    );
    assert_eq!(
        get_file_hash(&db, OTHER_PROJECT, &a).unwrap(),
        Some("h2".to_string())
    );
    assert_eq!(get_file_count(&db, PROJECT).unwrap(), 1);
    assert_eq!(get_file_count(&db, OTHER_PROJECT).unwrap(), 2);
    assert_eq!(get_import_count(&db, PROJECT).unwrap(), 0);
    assert_eq!(get_import_count(&db, OTHER_PROJECT).unwrap(), 1);
    assert_eq!(
        get_language_counts(&db, PROJECT).unwrap(),
        HashMap::from([("rust".to_string(), 1)])
    );
    assert_eq!(
        get_language_counts(&db, OTHER_PROJECT).unwrap(),
        HashMap::from([("ruby".to_string(), 1), ("rust".to_string(), 1)])
    );
    let mut stats = get_project_stats(&db).unwrap();
    stats.sort();
    assert_eq!(
        stats,
        vec![(OTHER_PROJECT.to_string(), 2), (PROJECT.to_string(), 1)]
    );
    assert!(imports_of(&db, PROJECT, &a).is_empty());
    assert_eq!(
        find_files_by_suffix(&db, PROJECT, "b.rs", 10).unwrap(),
        Vec::<String>::new()
    );
}

#[test]
fn file_paths_lists_only_the_projects_own_files() {
    let (db, _dir) = store();
    let (a, b, c) = (p("a.rs"), p("b.rs"), p("c.rs"));
    add_file(&db, PROJECT, &a, &[]);
    add_file(&db, PROJECT, &b, &[]);
    add_file(&db, OTHER_PROJECT, &c, &[]);
    assert_eq!(
        get_file_paths(&db, "/nothing-indexed").unwrap(),
        Vec::<String>::new()
    );

    let mut paths = get_file_paths(&db, PROJECT).unwrap();
    paths.sort();

    assert_eq!(paths, vec![a, b]);
}

#[test]
fn removing_a_file_leaves_the_other_projects_copy() {
    let (db, _dir) = store();
    let (a, b) = (p("a.rs"), p("b.rs"));
    for project in [PROJECT, OTHER_PROJECT] {
        add_file(&db, project, &a, &[]);
        add_file(&db, project, &b, &[]);
        replace_imports(&db, project, &a, &[edge(&a, &b, "./b", &["B"])]).unwrap();
    }

    remove_file(&db, OTHER_PROJECT, &a).unwrap();

    assert_eq!(get_file_count(&db, PROJECT).unwrap(), 2);
    assert_eq!(get_import_count(&db, PROJECT).unwrap(), 1);
    assert_eq!(get_file_count(&db, OTHER_PROJECT).unwrap(), 1);
    assert_eq!(get_import_count(&db, OTHER_PROJECT).unwrap(), 0);
}

// ============================================================================
// Suffix lookup
// ============================================================================

#[test]
fn suffix_lookup_returns_files_ending_with_the_suffix() {
    let (db, _dir) = store();
    for file in [
        "src/parser/rust_ext.rs",
        "src/parser/ruby.rs",
        "src/ext.rs.bak",
    ] {
        add_file(&db, PROJECT, &p(file), &[]);
    }

    let found = find_files_by_suffix(&db, PROJECT, "parser/rust_ext.rs", 10).unwrap();

    assert_eq!(found, vec![p("src/parser/rust_ext.rs")]);
}

#[test]
fn suffix_lookup_matches_whole_path_segments_only() {
    let (db, _dir) = store();
    for file in ["src/auth.rs", "src/oauth.rs", "src/web/auth.rs"] {
        add_file(&db, PROJECT, &p(file), &[]);
    }

    let mut by_name = find_files_by_suffix(&db, PROJECT, "auth.rs", 10).unwrap();
    by_name.sort();
    let by_full_path = find_files_by_suffix(&db, PROJECT, &p("src/auth.rs"), 10).unwrap();
    let from_the_project_root = find_files_by_suffix(&db, PROJECT, "/src/auth.rs", 10).unwrap();

    assert_eq!(by_name, vec![p("src/auth.rs"), p("src/web/auth.rs")]);
    assert_eq!(by_full_path, vec![p("src/auth.rs")]);
    assert_eq!(from_the_project_root, vec![p("src/auth.rs")]);
}

#[test]
fn suffix_lookup_respects_its_limit() {
    let (db, _dir) = store();
    for file in ["a/mod.rs", "b/mod.rs", "c/mod.rs"] {
        add_file(&db, PROJECT, &p(file), &[]);
    }

    let found = find_files_by_suffix(&db, PROJECT, "mod.rs", 2).unwrap();

    assert_eq!(found.len(), 2);
    assert!(found.iter().all(|f| f.ends_with("/mod.rs")), "{found:?}");
}

#[test]
fn suffix_lookup_treats_wildcards_literally() {
    let (db, _dir) = store();
    for file in [
        "src/rust_ext.rs",
        "src/rustXext.rs",
        "src/odd%.rs",
        "src/a.rs",
    ] {
        add_file(&db, PROJECT, &p(file), &[]);
    }

    assert_eq!(
        find_files_by_suffix(&db, PROJECT, "rust_ext.rs", 10).unwrap(),
        vec![p("src/rust_ext.rs")]
    );
    assert_eq!(
        find_files_by_suffix(&db, PROJECT, "odd%.rs", 10).unwrap(),
        vec![p("src/odd%.rs")]
    );
    assert_eq!(
        find_files_by_suffix(&db, PROJECT, "_.rs", 10).unwrap(),
        Vec::<String>::new()
    );
}
