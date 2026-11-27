use std::fs;

use tempfile::TempDir;

use super::*;
use crate::db::client::open;
use crate::indexer::full_index;

const SHARED: &str = "src/shared.ts";

struct Project {
    root: String,
    db: Db,
    _dir: TempDir,
}

/// Writes the files under a fresh directory and indexes them.
fn indexed(files: &[(&str, &str)]) -> Project {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("project");
    for (rel, source) in files {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
    }
    let root = root.to_str().unwrap().to_string();
    let db = open(&dir.path().join("cartographer.db")).unwrap();
    full_index(&db, &LanguageRegistry::builtin(), &root, &root).unwrap();
    Project {
        root,
        db,
        _dir: dir,
    }
}

fn ask(project: &Project, file: &str, symbol: &str) -> SymbolUses {
    find_uses(
        &project.db,
        &LanguageRegistry::builtin(),
        &project.root,
        &format!("{}/{file}", project.root),
        symbol,
    )
    .unwrap()
}

/// The uses of `symbol` of `file` as (path relative to the root, lines).
fn uses(project: &Project, file: &str, symbol: &str) -> Vec<(String, Vec<usize>)> {
    match ask(project, file, symbol) {
        SymbolUses::Found(found) => found
            .into_iter()
            .map(|file_use| {
                let prefix = format!("{}/", project.root);
                let path = file_use
                    .file_path
                    .strip_prefix(&prefix)
                    .unwrap()
                    .to_string();
                (path, file_use.lines)
            })
            .collect(),
        SymbolUses::FileNotIndexed => panic!("{file} is not indexed"),
        SymbolUses::SymbolNotDefined { defined } => {
            panic!("{file} does not define {symbol}; it defines {defined:?}")
        }
    }
}

fn used(file: &str, lines: &[usize]) -> (String, Vec<usize>) {
    (file.to_string(), lines.to_vec())
}

#[test]
fn every_file_that_uses_the_symbol_is_listed_with_its_lines() {
    let project = indexed(&[
        (
            SHARED,
            "export const helper = () => 1;\nexport const other = () => helper();\n",
        ),
        (
            "src/a.ts",
            "import { helper } from \"./shared\";\n\nhelper();\nhelper();\n",
        ),
        (
            "src/deep/b.ts",
            "import { helper as h, other } from \"../shared\";\n\nother();\nh();\n",
        ),
        (
            "src/c.ts",
            "import * as shared from \"./shared\";\n\nshared.other();\nshared.helper();\n",
        ),
        (
            "src/unrelated.ts",
            "import { other } from \"./shared\";\n\nother();\n",
        ),
    ]);

    assert_eq!(
        uses(&project, SHARED, "helper"),
        vec![
            used("src/a.ts", &[1, 3, 4]),
            used("src/c.ts", &[4]),
            used("src/deep/b.ts", &[1, 4]),
            used(SHARED, &[1, 2]),
        ]
    );
}

#[test]
fn a_symbol_of_the_same_name_defined_elsewhere_is_not_a_use() {
    let project = indexed(&[
        (SHARED, "export const helper = 1;\n"),
        ("src/twin.ts", "export const helper = 2;\n"),
        (
            "src/a.ts",
            "import { helper } from \"./shared\";\n\nconsole.log(helper);\n",
        ),
        (
            "src/b.ts",
            "import { helper } from \"./twin\";\n\nconsole.log(helper);\n",
        ),
    ]);

    assert_eq!(
        uses(&project, SHARED, "helper"),
        vec![used("src/a.ts", &[1, 3]), used(SHARED, &[1])]
    );
    assert_eq!(
        uses(&project, "src/twin.ts", "helper"),
        vec![used("src/b.ts", &[1, 3]), used("src/twin.ts", &[1])]
    );
}

#[test]
fn a_use_through_a_workspace_package_name_is_found() {
    let project = indexed(&[
        ("package.json", r#"{ "workspaces": ["packages/*"] }"#),
        (
            "packages/core/package.json",
            r#"{ "name": "@scope/core", "exports": { "./shared": "./src/shared.ts" } }"#,
        ),
        ("packages/core/src/shared.ts", "export const helper = 1;\n"),
        ("packages/app/package.json", r#"{ "name": "app" }"#),
        (
            "packages/app/src/main.ts",
            "import { helper } from \"@scope/core/shared\";\n\nconsole.log(helper);\n",
        ),
    ]);

    assert_eq!(
        uses(&project, "packages/core/src/shared.ts", "helper"),
        vec![
            used("packages/app/src/main.ts", &[1, 3]),
            used("packages/core/src/shared.ts", &[1]),
        ]
    );
}

#[test]
fn a_re_export_is_followed_to_the_files_that_import_from_it() {
    let project = indexed(&[
        (
            SHARED,
            "export const helper = 1;\nexport const other = 2;\n",
        ),
        // A barrel that passes everything on.
        ("src/index.ts", "export * from \"./shared\";\n"),
        // A second hop that renames it.
        (
            "src/public.ts",
            "export { helper as aid } from \"./index\";\n",
        ),
        (
            "src/via-barrel.ts",
            "import { helper } from \"./index\";\n\nconsole.log(helper);\n",
        ),
        (
            "src/via-rename.ts",
            "import { aid } from \"./public\";\n\nconsole.log(aid);\n",
        ),
        (
            "src/other-only.ts",
            "import { other } from \"./index\";\n\nconsole.log(other);\n",
        ),
    ]);

    assert_eq!(
        uses(&project, SHARED, "helper"),
        vec![
            used("src/public.ts", &[1]),
            used(SHARED, &[1]),
            used("src/via-barrel.ts", &[1, 3]),
            used("src/via-rename.ts", &[1, 3]),
        ]
    );
}

#[test]
fn a_symbol_re_exported_as_a_default_is_followed_to_its_importers() {
    let project = indexed(&[
        (SHARED, "export const helper = 1;\n"),
        (
            "src/wrapper.ts",
            "import { helper } from \"./shared\";\n\nexport default helper;\n",
        ),
        (
            "src/user.ts",
            "import anything from \"./wrapper\";\n\nconsole.log(anything);\n",
        ),
    ]);

    assert_eq!(
        uses(&project, SHARED, "helper"),
        vec![
            used(SHARED, &[1]),
            used("src/user.ts", &[1, 3]),
            used("src/wrapper.ts", &[1, 3]),
        ]
    );
}

#[test]
fn files_that_re_export_each_other_do_not_loop() {
    let project = indexed(&[
        (SHARED, "export const helper = 1;\n"),
        (
            "src/a.ts",
            "export * from \"./shared\";\nexport * from \"./b\";\n",
        ),
        ("src/b.ts", "export * from \"./a\";\n"),
        (
            "src/user.ts",
            "import { helper } from \"./b\";\n\nconsole.log(helper);\n",
        ),
    ]);

    assert_eq!(
        uses(&project, SHARED, "helper"),
        vec![used(SHARED, &[1]), used("src/user.ts", &[1, 3])]
    );
}

#[test]
fn a_symbol_with_no_uses_elsewhere_lists_only_its_definition() {
    let project = indexed(&[
        (
            SHARED,
            "export const helper = 1;\nexport const lonely = 2;\n",
        ),
        (
            "src/a.ts",
            "import { helper } from \"./shared\";\n\nconsole.log(helper);\n",
        ),
    ]);

    assert_eq!(uses(&project, SHARED, "lonely"), vec![used(SHARED, &[2])]);
}

#[test]
fn a_symbol_the_file_does_not_define_is_reported_as_such() {
    let project = indexed(&[(
        SHARED,
        "export const helper = 1;\nexport const other = 2;\n",
    )]);

    match ask(&project, SHARED, "missing") {
        SymbolUses::SymbolNotDefined { defined } => {
            assert_eq!(defined, vec!["helper".to_string(), "other".to_string()]);
        }
        SymbolUses::Found(found) => panic!("found {} files", found.len()),
        SymbolUses::FileNotIndexed => panic!("the file is indexed"),
    }
}

#[test]
fn a_file_that_is_not_indexed_is_reported_as_such() {
    let project = indexed(&[(SHARED, "export const helper = 1;\n")]);

    assert!(matches!(
        ask(&project, "src/nowhere.ts", "helper"),
        SymbolUses::FileNotIndexed
    ));
}

#[test]
fn a_file_in_another_language_is_refused() {
    let project = indexed(&[("src/lib.rs", "pub fn helper() {}\n")]);

    let refused = find_uses(
        &project.db,
        &LanguageRegistry::builtin(),
        &project.root,
        &format!("{}/src/lib.rs", project.root),
        "helper",
    );

    let message = refused.err().expect("a Rust file is not supported");
    assert!(message.contains("TypeScript and JavaScript"), "{message}");
}
