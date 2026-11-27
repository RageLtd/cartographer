use std::fs;

use tempfile::TempDir;

use super::*;

const SHARED: &str = "src/shared.ts";
const SHARED_SOURCE: &str = "export function helper() {}\n\
                             export function other() {}\n\
                             export type Shape = number;\n\
                             export default class Thing {}\n";

/// A project holding `src/shared.ts` and the given `(relative path, source)` files.
fn project(files: &[(&str, &str)]) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (rel, source) in files.iter().chain(&[(SHARED, SHARED_SOURCE)]) {
        let path = dir.path().join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
    }
    dir
}

/// How `file` uses `symbol` of `target`, both relative to the project.
fn uses(dir: &TempDir, file: &str, target: &str, symbol: &str) -> FileUses {
    let path = |rel: &str| dir.path().join(rel).to_str().unwrap().to_string();
    uses_in_file(
        &LanguageRegistry::builtin(),
        &path(file),
        &path(target),
        symbol,
    )
    .unwrap()
}

/// The lines on which a single file with this source uses `helper` of `src/shared.ts`.
fn lines_using_helper(source: &str) -> Vec<usize> {
    let dir = project(&[("src/main.ts", source)]);
    uses(&dir, "src/main.ts", SHARED, "helper").lines
}

fn names(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

// ============================================================================
// Where an imported name is used
// ============================================================================

#[test]
fn a_named_import_is_used_wherever_its_name_is_an_identifier() {
    let source = "import { helper, other } from \"./shared\";\n\
                  \n\
                  // helper is mentioned in a comment\n\
                  const text = \"helper in a string\";\n\
                  helper();\n\
                  const table = { helper, other };\n\
                  const unrelated = { helper: 1 };\n\
                  console.log(text.helper, unrelated);\n\
                  export function run(fn: typeof helper) {\n  return fn;\n}\n";

    // The import, the call, the shorthand property and the type query. Not
    // the comment, the string, the property key or the property access.
    assert_eq!(lines_using_helper(source), vec![1, 5, 6, 9]);
}

#[test]
fn a_type_is_used_in_type_positions() {
    let dir = project(&[(
        "src/main.ts",
        "import type { Shape } from \"./shared\";\n\
         \n\
         const one: Shape = 1;\n\
         function double(input: Shape[]): Shape {\n  return input[0] * 2;\n}\n",
    )]);

    assert_eq!(
        uses(&dir, "src/main.ts", SHARED, "Shape").lines,
        vec![1, 3, 4]
    );
}

#[test]
fn a_renamed_import_is_used_under_its_new_name() {
    let source = "import { helper as h } from \"./shared\";\n\
                  \n\
                  h();\n\
                  const helper = 1;\n\
                  console.log(helper);\n";

    // The import and the call through `h`. The local `helper` is another thing.
    assert_eq!(lines_using_helper(source), vec![1, 3]);
}

#[test]
fn the_same_name_imported_from_another_file_is_not_a_use() {
    let dir = project(&[
        ("src/elsewhere.ts", "export function helper() {}\n"),
        (
            "src/main.ts",
            "import { helper } from \"./elsewhere\";\n\nhelper();\n",
        ),
    ]);

    let found = uses(&dir, "src/main.ts", SHARED, "helper");

    assert_eq!(found.lines, Vec::<usize>::new());
    assert_eq!(found.re_exported_as, Vec::<String>::new());
    assert_eq!(
        uses(&dir, "src/main.ts", "src/elsewhere.ts", "helper").lines,
        vec![1, 3]
    );
}

#[test]
fn a_namespace_import_is_used_where_the_symbol_is_read_from_it() {
    let dir = project(&[(
        "src/main.ts",
        "import * as shared from \"./shared\";\n\
         \n\
         shared.helper();\n\
         shared.other();\n\
         const size: shared.Shape = 1;\n\
         console.log(size.helper);\n",
    )]);

    assert_eq!(uses(&dir, "src/main.ts", SHARED, "helper").lines, vec![3]);
    assert_eq!(uses(&dir, "src/main.ts", SHARED, "Shape").lines, vec![5]);
}

#[test]
fn the_default_export_is_used_under_the_name_it_was_imported_as() {
    let dir = project(&[(
        "src/main.ts",
        "import Whatever, { helper } from \"./shared\";\n\
         \n\
         helper();\n\
         export const made = new Whatever();\n",
    )]);

    assert_eq!(
        uses(&dir, "src/main.ts", SHARED, "default").lines,
        vec![1, 4]
    );
}

#[test]
fn a_component_is_used_where_it_is_rendered() {
    let dir = project(&[
        (
            "src/widget.tsx",
            "export function Widget() {\n  return null;\n}\n",
        ),
        (
            "src/page.tsx",
            "import { Widget } from \"./widget\";\n\
             \n\
             export function Page() {\n  return <Widget />;\n}\n",
        ),
    ]);

    assert_eq!(
        uses(&dir, "src/page.tsx", "src/widget.tsx", "Widget").lines,
        vec![1, 4]
    );
}

#[test]
fn an_import_by_workspace_package_name_is_followed() {
    let dir = project(&[
        ("package.json", r#"{ "workspaces": ["packages/*"] }"#),
        (
            "packages/core/package.json",
            r#"{ "name": "@scope/core", "exports": { "./util": "./src/util.ts" } }"#,
        ),
        ("packages/core/src/util.ts", "export function helper() {}\n"),
        ("packages/app/package.json", r#"{ "name": "app" }"#),
        (
            "packages/app/src/main.ts",
            "import { helper } from \"@scope/core/util\";\n\nhelper();\n",
        ),
    ]);

    assert_eq!(
        uses(
            &dir,
            "packages/app/src/main.ts",
            "packages/core/src/util.ts",
            "helper"
        )
        .lines,
        vec![1, 3]
    );
}

#[test]
fn the_defining_file_uses_its_own_symbol_where_it_names_it() {
    let dir = project(&[(
        "src/own.ts",
        "export function helper() {}\n\
         \n\
         export function twice() {\n  helper();\n  helper();\n}\n",
    )]);

    assert_eq!(
        uses(&dir, "src/own.ts", "src/own.ts", "helper").lines,
        vec![1, 4, 5]
    );
}

// Inside its own file a default export has no name `default`: it is whatever
// the file calls the thing it exports.
#[test]
fn the_defining_file_uses_its_default_export_under_the_name_it_declares() {
    let dir = project(&[
        (
            "src/declared.ts",
            "export default function Foo() {}\n\nFoo();\n",
        ),
        ("src/named.ts", "class Bar {}\n\nexport default Bar;\n"),
        (
            "src/listed.ts",
            "const baz = 1;\n\nexport { baz as default };\n",
        ),
        ("src/anonymous.ts", "export default { a: 1 };\n"),
    ]);
    let own_default = |file: &str| uses(&dir, file, file, "default").lines;

    assert_eq!(own_default("src/declared.ts"), vec![1, 3]);
    assert_eq!(own_default("src/named.ts"), vec![1, 3]);
    assert_eq!(own_default("src/listed.ts"), vec![1, 3]);
    assert_eq!(own_default("src/anonymous.ts"), Vec::<usize>::new());
}

// ============================================================================
// Re-exports
// ============================================================================

#[test]
fn a_named_re_export_passes_the_symbol_on_under_the_name_it_gives() {
    let dir = project(&[
        ("src/same.ts", "export { helper } from \"./shared\";\n"),
        (
            "src/renamed.ts",
            "export { other } from \"./shared\";\nexport { helper as aid } from \"./shared\";\n",
        ),
    ]);

    let same = uses(&dir, "src/same.ts", SHARED, "helper");
    assert_eq!(same.lines, vec![1]);
    assert_eq!(same.re_exported_as, names(&["helper"]));

    let renamed = uses(&dir, "src/renamed.ts", SHARED, "helper");
    assert_eq!(renamed.lines, vec![2]);
    assert_eq!(renamed.re_exported_as, names(&["aid"]));
}

#[test]
fn a_star_re_export_passes_every_named_symbol_on_without_using_it() {
    let dir = project(&[("src/index.ts", "export * from \"./shared\";\n")]);

    let helper = uses(&dir, "src/index.ts", SHARED, "helper");
    assert_eq!(helper.lines, Vec::<usize>::new());
    assert_eq!(helper.re_exported_as, names(&["helper"]));

    // `export *` does not pass on the default export.
    let default = uses(&dir, "src/index.ts", SHARED, "default");
    assert_eq!(default.re_exported_as, Vec::<String>::new());
}

#[test]
fn an_imported_symbol_exported_again_is_passed_on() {
    let dir = project(&[(
        "src/index.ts",
        "import { helper, other } from \"./shared\";\n\
         \n\
         helper();\n\
         export { helper as aid, other };\n",
    )]);

    let helper = uses(&dir, "src/index.ts", SHARED, "helper");
    assert_eq!(helper.lines, vec![1, 3, 4]);
    assert_eq!(helper.re_exported_as, names(&["aid"]));

    let other = uses(&dir, "src/index.ts", SHARED, "other");
    assert_eq!(other.re_exported_as, names(&["other"]));
}

#[test]
fn an_imported_symbol_exported_as_the_default_is_passed_on_as_default() {
    let dir = project(&[
        (
            "src/main.ts",
            "import { helper } from \"./shared\";\n\nexport default helper;\n",
        ),
        (
            "src/other.ts",
            "import { other } from \"./shared\";\n\nexport default other;\n",
        ),
    ]);

    let passed_on = uses(&dir, "src/main.ts", SHARED, "helper");
    assert_eq!(passed_on.lines, vec![1, 3]);
    assert_eq!(passed_on.re_exported_as, names(&["default"]));

    // A default export of something else passes nothing on.
    let not_passed_on = uses(&dir, "src/other.ts", SHARED, "helper");
    assert_eq!(not_passed_on.re_exported_as, Vec::<String>::new());
}

// ============================================================================
// What is not answered
// ============================================================================

#[test]
fn a_file_that_does_not_import_the_target_uses_nothing_of_it() {
    let source = "const helper = 1;\nconsole.log(helper);\n";

    assert_eq!(lines_using_helper(source), Vec::<usize>::new());
}

#[test]
fn a_file_in_another_language_is_refused() {
    let dir = project(&[("src/lib.rs", "pub fn helper() {}\n")]);
    let path = |rel: &str| dir.path().join(rel).to_str().unwrap().to_string();

    let refused = uses_in_file(
        &LanguageRegistry::builtin(),
        &path("src/lib.rs"),
        &path("src/lib.rs"),
        "helper",
    );

    let message = refused.err().expect("a Rust file is not supported");
    assert!(message.contains("TypeScript and JavaScript"), "{message}");
}

#[test]
fn a_file_that_cannot_be_read_is_an_error() {
    let dir = project(&[]);
    let path = |rel: &str| dir.path().join(rel).to_str().unwrap().to_string();

    let missing = uses_in_file(
        &LanguageRegistry::builtin(),
        &path("src/missing.ts"),
        &path(SHARED),
        "helper",
    );

    assert!(missing.is_err());
}
