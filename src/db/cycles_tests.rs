use super::*;
use crate::db::fixtures_tests::*;
use crate::db::graph::{get_file_detail, walk_import_graph};
use crate::db::queries::replace_imports;

/// Stores `source` as importing each of `targets`, replacing what it imported.
fn imports(db: &Db, project: &str, source: &str, targets: &[&str]) {
    let edges: Vec<_> = targets
        .iter()
        .map(|target| edge(source, target, target, &[]))
        .collect();
    replace_imports(db, project, source, &edges).unwrap();
}

/// `count` files in `PROJECT`, each importing the next and the last the first.
fn ring(db: &Db, count: usize) -> Vec<String> {
    let files: Vec<String> = (0..count).map(|i| p(&format!("ring/f{i:03}.rs"))).collect();
    for (i, file) in files.iter().enumerate() {
        imports(db, PROJECT, file, &[&files[(i + 1) % count]]);
    }
    files
}

#[test]
fn two_files_that_import_each_other_are_one_group() {
    let (db, _dir) = store();
    let (a, b) = (p("a.rs"), p("b.rs"));
    imports(&db, PROJECT, &a, &[&b]);
    imports(&db, PROJECT, &b, &[&a]);

    assert_eq!(find_cycles(&db, PROJECT).unwrap(), vec![vec![a, b]]);
}

#[test]
fn a_ring_of_three_files_is_one_group() {
    let (db, _dir) = store();
    let files = ring(&db, 3);

    assert_eq!(find_cycles(&db, PROJECT).unwrap(), vec![files]);
}

// The search this replaced reported only the three pairs here and missed that
// all three files are tangled together.
#[test]
fn three_files_that_all_import_each_other_are_one_group_of_three() {
    let (db, _dir) = store();
    let (b, c, d) = (p("b.rs"), p("c.rs"), p("d.rs"));
    imports(&db, PROJECT, &b, &[&c, &d]);
    imports(&db, PROJECT, &c, &[&b, &d]);
    imports(&db, PROJECT, &d, &[&b, &c]);

    assert_eq!(find_cycles(&db, PROJECT).unwrap(), vec![vec![b, c, d]]);
}

// The search this replaced gave up on any cycle through more than 20 files.
#[test]
fn a_ring_of_twenty_five_files_is_one_group() {
    let (db, _dir) = store();
    let files = ring(&db, 25);

    assert_eq!(find_cycles(&db, PROJECT).unwrap(), vec![files]);
}

#[test]
fn separate_cycles_are_separate_groups_and_files_on_the_way_are_in_none() {
    let (db, _dir) = store();
    let (a, b, c, d) = (p("a.rs"), p("b.rs"), p("c.rs"), p("d.rs"));
    let (leads_in, leads_out) = (p("in.rs"), p("out.rs"));
    imports(&db, PROJECT, &leads_in, &[&a]);
    imports(&db, PROJECT, &a, &[&b]);
    // b is in a cycle with a, and also imports c one way.
    imports(&db, PROJECT, &b, &[&a, &c]);
    imports(&db, PROJECT, &c, &[&d]);
    imports(&db, PROJECT, &d, &[&c, &leads_out]);

    assert_eq!(
        find_cycles(&db, PROJECT).unwrap(),
        vec![vec![a, b], vec![c, d]]
    );
}

#[test]
fn a_project_without_cycles_has_no_groups() {
    let (db, _dir) = store();
    let (a, b, c) = (p("a.rs"), p("b.rs"), p("c.rs"));
    imports(&db, PROJECT, &a, &[&b, &c]);
    imports(&db, PROJECT, &b, &[&c]);
    // The same files form a cycle in another project.
    imports(&db, OTHER_PROJECT, &c, &[&a]);
    imports(&db, OTHER_PROJECT, &a, &[&c]);

    assert_eq!(
        find_cycles(&db, PROJECT).unwrap(),
        Vec::<Vec<String>>::new()
    );
    assert_eq!(find_cycles(&db, OTHER_PROJECT).unwrap(), vec![vec![a, c]]);
}

// A Rust parent must declare `mod child;` for the child to exist. That says
// where the child lives, not that the parent depends on it.
#[test]
fn a_module_declaration_and_the_childs_use_of_its_parent_are_not_a_cycle() {
    let (db, _dir) = store();
    let (parent, child) = (p("src/parser/mod.rs"), p("src/parser/tests.rs"));
    add_file(&db, PROJECT, &parent, &["parse_file"]);
    add_file(&db, PROJECT, &child, &[]);
    replace_imports(
        &db,
        PROJECT,
        &parent,
        &[module_edge(&parent, &child, "tests")],
    )
    .unwrap();
    replace_imports(
        &db,
        PROJECT,
        &child,
        &[edge(&child, &parent, "super::parse_file", &["parse_file"])],
    )
    .unwrap();

    assert_eq!(
        find_cycles(&db, PROJECT).unwrap(),
        Vec::<Vec<String>>::new()
    );
    // The declaration is still an edge everywhere else: removing or renaming
    // the child breaks the parent.
    let detail = get_file_detail(&db, PROJECT, &child).unwrap().unwrap();
    assert_eq!(
        detail.dependents,
        vec![(parent.clone(), strings(&["tests"]))]
    );
    let walk = walk_import_graph(
        &db,
        PROJECT,
        std::slice::from_ref(&parent),
        Some(1),
        Some(50),
    )
    .unwrap();
    let dependencies: Vec<&str> = walk
        .iter()
        .filter(|file| file.reason == "dependency")
        .map(|file| file.file_path.as_str())
        .collect();
    assert_eq!(dependencies, vec![child.as_str()]);
}

#[test]
fn a_parent_that_uses_its_child_which_uses_it_back_is_a_group() {
    let (db, _dir) = store();
    let (parent, child) = (p("src/parser/mod.rs"), p("src/parser/registry.rs"));
    replace_imports(
        &db,
        PROJECT,
        &parent,
        &[
            module_edge(&parent, &child, "registry"),
            edge(&parent, &child, "self::registry::Registry", &["Registry"]),
        ],
    )
    .unwrap();
    replace_imports(
        &db,
        PROJECT,
        &child,
        &[edge(&child, &parent, "super::parse_file", &["parse_file"])],
    )
    .unwrap();

    assert_eq!(
        find_cycles(&db, PROJECT).unwrap(),
        vec![vec![parent, child]]
    );
}

// An import chain can be far deeper than the call stack allows a recursive
// search to go.
#[test]
fn a_very_long_chain_and_a_very_long_ring_are_handled() {
    const LENGTH: usize = 100_000;
    let name = |i: usize| format!("f{i:06}");
    let chain: Vec<(String, String)> = (0..LENGTH - 1).map(|i| (name(i), name(i + 1))).collect();

    assert_eq!(mutually_reachable_groups(&chain), Vec::<Vec<String>>::new());

    let mut ring = chain;
    ring.push((name(LENGTH - 1), name(0)));
    let groups = mutually_reachable_groups(&ring);

    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].len(), LENGTH);
    assert_eq!(groups[0][0], name(0));
    assert_eq!(groups[0][LENGTH - 1], name(LENGTH - 1));
}
