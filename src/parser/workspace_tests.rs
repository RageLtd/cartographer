use std::fs;
use std::path::Path;

use tempfile::TempDir;

use super::*;

/// The file every test imports from.
const IMPORTER: &str = "packages/app/src/main.ts";

const ROOT_MANIFEST: (&str, &str) = ("package.json", r#"{ "workspaces": ["packages/*"] }"#);
const APP_MANIFEST: (&str, &str) = ("packages/app/package.json", r#"{ "name": "app" }"#);

/// Writes each `(relative path, contents)` pair under a fresh directory, with
/// the importing file added.
fn workspace(files: &[(&str, &str)]) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (rel, contents) in files.iter().chain(&[(IMPORTER, "")]) {
        let path = dir.path().join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }
    dir
}

/// What `specifier` resolves to when imported from `IMPORTER`, relative to
/// the workspace root.
fn resolve(dir: &TempDir, specifier: &str) -> Option<String> {
    let resolved = Workspace::containing(&dir.path().join(IMPORTER))?.resolve(specifier)?;
    let relative = Path::new(&resolved)
        .strip_prefix(dir.path())
        .unwrap_or_else(|_| panic!("{resolved} is outside the workspace"));
    Some(relative.to_str().unwrap().to_string())
}

fn some(path: &str) -> Option<String> {
    Some(path.to_string())
}

/// A workspace whose `@scope/core` package has the given manifest and files.
fn with_core(manifest: &str, files: &[&str]) -> TempDir {
    let mut all = vec![
        ROOT_MANIFEST,
        APP_MANIFEST,
        ("packages/core/package.json", manifest),
    ];
    all.extend(files.iter().map(|file| (*file, "export const x = 1;\n")));
    workspace(&all)
}

// ============================================================================
// A package with an exports map
// ============================================================================

const CORE_EXPORTS: &str = r#"{
    "name": "@scope/core",
    "exports": {
        ".": "./src/index.ts",
        "./util": "./src/util.ts",
        "./skills/shared": "./src/skills/shared.ts"
    }
}"#;

const CORE_FILES: [&str; 4] = [
    "packages/core/src/index.ts",
    "packages/core/src/util.ts",
    "packages/core/src/skills/shared.ts",
    "packages/core/src/private.ts",
];

#[test]
fn an_exposed_subpath_resolves_to_the_file_it_names() {
    let dir = with_core(CORE_EXPORTS, &CORE_FILES);

    assert_eq!(
        resolve(&dir, "@scope/core/util"),
        some("packages/core/src/util.ts")
    );
    assert_eq!(
        resolve(&dir, "@scope/core/skills/shared"),
        some("packages/core/src/skills/shared.ts")
    );
}

#[test]
fn the_bare_package_name_resolves_to_its_entry() {
    let dir = with_core(CORE_EXPORTS, &CORE_FILES);

    assert_eq!(
        resolve(&dir, "@scope/core"),
        some("packages/core/src/index.ts")
    );
}

#[test]
fn a_subpath_the_package_does_not_expose_has_no_edge() {
    let dir = with_core(CORE_EXPORTS, &CORE_FILES);

    // The file exists, but the exports map does not list it.
    assert_eq!(resolve(&dir, "@scope/core/private"), None);
    assert_eq!(resolve(&dir, "@scope/core/src/private.ts"), None);
    assert_eq!(resolve(&dir, "@scope/core/nothing"), None);
}

#[test]
fn a_package_outside_the_workspace_has_no_edge() {
    let dir = with_core(CORE_EXPORTS, &CORE_FILES);

    assert_eq!(resolve(&dir, "react"), None);
    assert_eq!(resolve(&dir, "@scope/other/util"), None);
    // A name that only starts like a workspace package's is another package.
    assert_eq!(resolve(&dir, "@scope/core-extras"), None);
    assert_eq!(resolve(&dir, "@scope/core-extras/util"), None);
}

#[test]
fn an_exposed_target_that_is_not_on_disk_has_no_edge() {
    let dir = with_core(CORE_EXPORTS, &["packages/core/src/index.ts"]);

    assert_eq!(resolve(&dir, "@scope/core/util"), None);
}

#[test]
fn a_javascript_target_means_its_typescript_source() {
    let manifest = r#"{ "name": "@scope/core", "exports": { "./util": "./src/util.js" } }"#;
    let dir = with_core(manifest, &["packages/core/src/util.ts"]);

    assert_eq!(
        resolve(&dir, "@scope/core/util"),
        some("packages/core/src/util.ts")
    );
}

#[test]
fn a_string_exports_field_is_the_entry_and_exposes_nothing_else() {
    let manifest = r#"{ "name": "@scope/core", "exports": "./src/index.ts" }"#;
    let dir = with_core(manifest, &CORE_FILES);

    assert_eq!(
        resolve(&dir, "@scope/core"),
        some("packages/core/src/index.ts")
    );
    assert_eq!(resolve(&dir, "@scope/core/util"), None);
}

#[test]
fn a_pattern_with_one_star_exposes_every_file_it_matches() {
    let manifest = r#"{
        "name": "@scope/core",
        "exports": { "./*": "./src/*.ts", "./skills/*": "./src/skills/*.ts" }
    }"#;
    let dir = with_core(manifest, &CORE_FILES);

    assert_eq!(
        resolve(&dir, "@scope/core/util"),
        some("packages/core/src/util.ts")
    );
    // The longer pattern wins where both match.
    assert_eq!(
        resolve(&dir, "@scope/core/skills/shared"),
        some("packages/core/src/skills/shared.ts")
    );
    assert_eq!(resolve(&dir, "@scope/core/missing"), None);
}

#[test]
fn an_exact_subpath_wins_over_a_pattern() {
    let manifest = r#"{
        "name": "@scope/core",
        "exports": { "./*": "./src/*.ts", "./util": "./src/index.ts" }
    }"#;
    let dir = with_core(manifest, &CORE_FILES);

    assert_eq!(
        resolve(&dir, "@scope/core/util"),
        some("packages/core/src/index.ts")
    );
}

#[test]
fn conditions_give_the_first_target_in_written_order_that_exists() {
    // Named so that sorting the keys would reverse them.
    let manifest = r#"{
        "name": "@scope/core",
        "exports": {
            "./util": { "z-built": "./dist/util.js", "m-source": "./src/util.ts", "a-other": "./src/index.ts" },
            "./both": { "z-first": "./src/index.ts", "a-second": "./src/util.ts" },
            "./nested": { "node": { "import": "./dist/x.js", "default": "./src/private.ts" } }
        }
    }"#;
    let dir = with_core(manifest, &CORE_FILES);

    // The built file is not on disk, so the next condition is used.
    assert_eq!(
        resolve(&dir, "@scope/core/util"),
        some("packages/core/src/util.ts")
    );
    assert_eq!(
        resolve(&dir, "@scope/core/both"),
        some("packages/core/src/index.ts")
    );
    assert_eq!(
        resolve(&dir, "@scope/core/nested"),
        some("packages/core/src/private.ts")
    );
}

#[test]
fn conditions_directly_under_exports_are_the_entry() {
    let manifest = r#"{
        "name": "@scope/core",
        "exports": { "import": "./dist/index.js", "default": "./src/index.ts" }
    }"#;
    let dir = with_core(manifest, &CORE_FILES);

    assert_eq!(
        resolve(&dir, "@scope/core"),
        some("packages/core/src/index.ts")
    );
    assert_eq!(resolve(&dir, "@scope/core/util"), None);
}

#[test]
fn a_target_outside_its_package_has_no_edge() {
    let manifest = r#"{
        "name": "@scope/core",
        "exports": { "./up": "./../app/src/main.ts", "./bare": "src/util.ts" }
    }"#;
    let dir = with_core(manifest, &CORE_FILES);

    assert_eq!(resolve(&dir, "@scope/core/up"), None);
    // A target must be written relative to the package, starting with `./`.
    assert_eq!(resolve(&dir, "@scope/core/bare"), None);
}

// ============================================================================
// A package without an exports map
// ============================================================================

#[test]
fn without_exports_the_bare_name_resolves_through_module_then_main() {
    let files = ["packages/core/lib/entry.ts", "packages/core/lib/old.ts"];
    let both = r#"{ "name": "@scope/core", "module": "lib/entry.ts", "main": "./lib/old" }"#;
    let main_only = r#"{ "name": "@scope/core", "main": "./lib/old" }"#;

    assert_eq!(
        resolve(&with_core(both, &files), "@scope/core"),
        some("packages/core/lib/entry.ts")
    );
    assert_eq!(
        resolve(&with_core(main_only, &files), "@scope/core"),
        some("packages/core/lib/old.ts")
    );
}

#[test]
fn without_exports_or_an_entry_field_the_bare_name_is_the_index_file() {
    let manifest = r#"{ "name": "@scope/core" }"#;
    let dir = with_core(manifest, &["packages/core/index.ts"]);

    assert_eq!(resolve(&dir, "@scope/core"), some("packages/core/index.ts"));
}

#[test]
fn without_exports_a_subpath_is_a_path_inside_the_package() {
    let manifest = r#"{ "name": "@scope/core" }"#;
    let dir = with_core(
        manifest,
        &[
            "packages/core/src/util.ts",
            "packages/core/src/skills/index.ts",
        ],
    );

    assert_eq!(
        resolve(&dir, "@scope/core/src/util"),
        some("packages/core/src/util.ts")
    );
    assert_eq!(
        resolve(&dir, "@scope/core/src/util.ts"),
        some("packages/core/src/util.ts")
    );
    assert_eq!(
        resolve(&dir, "@scope/core/src/skills"),
        some("packages/core/src/skills/index.ts")
    );
    assert_eq!(resolve(&dir, "@scope/core/src/missing"), None);
    assert_eq!(resolve(&dir, "@scope/core/../app/src/main"), None);
}

// ============================================================================
// Finding the workspace and its packages
// ============================================================================

#[test]
fn workspaces_may_be_an_object_with_a_packages_list() {
    let dir = workspace(&[
        (
            "package.json",
            r#"{ "workspaces": { "packages": ["packages/*"], "catalogs": {} } }"#,
        ),
        APP_MANIFEST,
        ("packages/core/package.json", CORE_EXPORTS),
        ("packages/core/src/util.ts", ""),
    ]);

    assert_eq!(
        resolve(&dir, "@scope/core/util"),
        some("packages/core/src/util.ts")
    );
}

#[test]
fn workspace_patterns_may_name_a_directory_or_match_part_of_a_name() {
    let dir = workspace(&[
        (
            "package.json",
            r#"{ "workspaces": ["packages/app", "tools/cli", "libs/lib-*"] }"#,
        ),
        APP_MANIFEST,
        ("tools/cli/package.json", r#"{ "name": "cli" }"#),
        ("tools/cli/index.ts", ""),
        ("tools/other/package.json", r#"{ "name": "other" }"#),
        ("tools/other/index.ts", ""),
        ("libs/lib-a/package.json", r#"{ "name": "lib-a" }"#),
        ("libs/lib-a/index.ts", ""),
        ("libs/extra/package.json", r#"{ "name": "extra" }"#),
        ("libs/extra/index.ts", ""),
    ]);

    assert_eq!(resolve(&dir, "cli"), some("tools/cli/index.ts"));
    assert_eq!(resolve(&dir, "lib-a"), some("libs/lib-a/index.ts"));
    // In the tree, but not named by any workspace pattern.
    assert_eq!(resolve(&dir, "other"), None);
    assert_eq!(resolve(&dir, "extra"), None);
}

#[test]
fn a_directory_without_a_usable_manifest_does_not_hide_the_other_packages() {
    let dir = workspace(&[
        ROOT_MANIFEST,
        APP_MANIFEST,
        ("packages/broken/package.json", "{ not json"),
        ("packages/unnamed/package.json", r#"{ "version": "1.0.0" }"#),
        ("packages/empty/readme.md", ""),
        ("packages/core/package.json", CORE_EXPORTS),
        ("packages/core/src/util.ts", ""),
    ]);

    assert_eq!(
        resolve(&dir, "@scope/core/util"),
        some("packages/core/src/util.ts")
    );
}

#[test]
fn a_file_outside_any_workspace_has_no_workspace() {
    let dir = workspace(&[
        ("package.json", r#"{ "name": "single-package" }"#),
        APP_MANIFEST,
        ("packages/core/package.json", CORE_EXPORTS),
        ("packages/core/src/util.ts", ""),
    ]);

    assert!(Workspace::containing(&dir.path().join(IMPORTER)).is_none());
}

#[test]
fn the_nearest_workspace_above_the_file_is_the_one_used() {
    let dir = workspace(&[
        ("package.json", r#"{ "workspaces": ["outer/*"] }"#),
        ("outer/far/package.json", r#"{ "name": "far" }"#),
        ("outer/far/index.ts", ""),
        (
            "packages/package.json",
            r#"{ "workspaces": ["app", "core"] }"#,
        ),
        APP_MANIFEST,
        ("packages/core/package.json", r#"{ "name": "near" }"#),
        ("packages/core/index.ts", ""),
    ]);

    assert_eq!(resolve(&dir, "near"), some("packages/core/index.ts"));
    assert_eq!(resolve(&dir, "far"), None);
}
