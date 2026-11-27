use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

use super::*;
use crate::db::client::open;
use crate::project_fixture_tests::*;

// ============================================================================
// File mentions in a prompt
// ============================================================================

#[test]
fn test_extract_file_mentions_basic() {
    let mentions = extract_file_mentions("look at src/main.rs and src/server.rs");
    assert!(mentions.contains(&"src/main.rs".to_string()));
    assert!(mentions.contains(&"src/server.rs".to_string()));
}

#[test]
fn test_extract_file_mentions_backticks() {
    let mentions = extract_file_mentions("check `src/parser/mod.rs` for the issue");
    assert!(mentions.contains(&"src/parser/mod.rs".to_string()));
}

#[test]
fn test_extract_file_mentions_no_false_positives() {
    let mentions = extract_file_mentions("the version is 1.2.3 and we should upgrade");
    assert!(mentions.is_empty());
}

#[test]
fn test_extract_file_mentions_typescript() {
    let mentions = extract_file_mentions("update components/Button.tsx and utils/helpers.ts");
    assert!(mentions.contains(&"components/Button.tsx".to_string()));
    assert!(mentions.contains(&"utils/helpers.ts".to_string()));
}

#[test]
fn test_extract_file_mentions_dedup() {
    let mentions = extract_file_mentions("src/main.rs and src/main.rs again");
    assert_eq!(mentions.len(), 1);
}

// ============================================================================
// Hooks answering from a store file
// ============================================================================

const PROMPT_EVENT: &str = "UserPromptSubmit";
const TOOL_EVENT: &str = "PreToolUse";
const READING: &str = "Reading";

/// The test project, indexed into its store file, with the store closed
/// again so the hooks open the file themselves.
fn indexed_project() -> TestProject {
    let project = test_project();
    let db = open(&project.store_path).unwrap();
    index(&db, &project);
    project
}

fn input(cwd: &str) -> HookInput {
    HookInput {
        cwd: cwd.to_string(),
        prompt: None,
        tool_input: None,
    }
}

fn prompt_input(cwd: &str, prompt: &str) -> HookInput {
    HookInput {
        prompt: Some(prompt.to_string()),
        ..input(cwd)
    }
}

fn tool_input(cwd: &str, file_path: &str) -> HookInput {
    let json =
        sonic_rs::to_string(&std::collections::HashMap::from([("file_path", file_path)])).unwrap();
    HookInput {
        tool_input: Some(sonic_rs::from_str(&json).unwrap()),
        ..input(cwd)
    }
}

/// The context a hook produced for `event`, failing when it produced none or
/// produced it for another event.
fn context_for(event: &str, output: Option<(&'static str, String)>) -> String {
    let (produced, context) = output.unwrap_or_else(|| panic!("{event} produced no context"));
    assert_eq!(produced, event);
    context
}

/// The one line of `context` starting with `prefix`.
fn line_starting<'a>(context: &'a str, prefix: &str) -> &'a str {
    context
        .lines()
        .find(|line| line.starts_with(prefix))
        .unwrap_or_else(|| panic!("no line starts with {prefix:?} in:\n{context}"))
}

/// Puts the project under git with every file committed, then appends one
/// line to `SERVER`, so `git diff --numstat HEAD` reports `1 0 src/server.ts`.
/// False when git is unavailable here.
fn commit_then_edit_server(project: &TestProject) -> bool {
    let git = |args: &[&str]| {
        Command::new("git")
            .args([
                "-c",
                "user.name=Cartographer Test",
                "-c",
                "user.email=test@example.invalid",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .current_dir(&project.root)
            .output()
            .is_ok_and(|o| o.status.success())
    };
    let committed = git(&["init", "-q"])
        && git(&["add", "-A"])
        && git(&["commit", "-q", "--no-verify", "-m", "fixture"]);
    if !committed {
        eprintln!("skipping: could not commit the test project with git");
        return false;
    }
    let mut server = OpenOptions::new()
        .append(true)
        .open(project.path(SERVER))
        .unwrap();
    server.write_all(b"// edited\n").unwrap();
    true
}

/// Store paths that hold no index: one not created yet, one created empty,
/// and one that cannot be opened because its directory is a regular file.
/// Keep the project alive while they are used.
fn stores_without_an_index(project: &TestProject) -> [PathBuf; 3] {
    let dir = project.store_path.parent().unwrap();
    let empty = dir.join("empty.db");
    drop(open(&empty).unwrap());
    let not_a_dir = dir.join("plain-file");
    std::fs::write(&not_a_dir, "not a directory").unwrap();
    [dir.join("missing.db"), empty, not_a_dir.join("store.db")]
}

#[test]
fn session_start_reports_the_index_counts_from_the_store() {
    let project = indexed_project();

    let context = context_for(
        "SessionStart",
        session_start(&project.store_path, input(&project.root)),
    );

    assert!(context.contains("Graph-First Navigation"), "{context}");
    assert!(
        context.contains(&format!("**Project**: {}", project.root)),
        "{context}"
    );
    assert!(
        context.contains(&format!("**Files indexed**: {FILE_COUNT}")),
        "{context}"
    );
    assert!(
        context.contains(&format!("**Import edges**: {IMPORT_COUNT}")),
        "{context}"
    );
}

#[test]
fn session_start_without_an_index_still_gives_its_guidance() {
    let project = test_project();
    let [missing, empty, unopenable] = stores_without_an_index(&project);

    for store in [&missing, &empty] {
        let context = context_for("SessionStart", session_start(store, input(&project.root)));
        assert!(context.contains("Graph-First Navigation"), "{context}");
        assert!(context.contains("Project not yet indexed"), "{context}");
        assert!(!context.contains("**Files indexed**"), "{context}");
    }

    let context = context_for(
        "SessionStart",
        session_start(&unopenable, input(&project.root)),
    );
    assert!(context.contains("Graph-First Navigation"), "{context}");
    assert!(!context.contains("**Files indexed**"), "{context}");
}

#[test]
fn prompt_mentioning_a_file_reports_its_imports_and_dependents() {
    let project = indexed_project();

    let context = context_for(
        PROMPT_EVENT,
        prompt_submit(
            &project.store_path,
            prompt_input(&project.root, &format!("why does {SERVER} loop?")),
        ),
    );

    assert!(context.contains(&format!("### {SERVER}")), "{context}");
    assert_eq!(
        line_starting(&context, "**Imports:**"),
        format!("**Imports:** {ROUTER}")
    );
    let imported_by = line_starting(&context, "**Imported by:**");
    assert!(imported_by.contains(MAIN), "{context}");
    assert!(imported_by.contains(ROUTER), "{context}");
}

#[test]
fn reading_an_indexed_file_reports_its_imports_and_dependents() {
    let project = indexed_project();

    let context = context_for(
        TOOL_EVENT,
        file_tool(
            &project.store_path,
            tool_input(&project.root, SERVER),
            READING,
        ),
    );

    assert!(
        context.starts_with(&format!("## [cartographer] {READING} `{SERVER}`")),
        "{context}"
    );
    assert!(context.contains("**Dependents:** 2"), "{context}");
    assert_eq!(
        line_starting(&context, "**Imports:**"),
        format!("**Imports:** {ROUTER}")
    );
    let imported_by = line_starting(&context, "**Imported by:**");
    assert!(imported_by.contains(MAIN), "{context}");
    assert!(imported_by.contains(ROUTER), "{context}");
}

#[test]
fn reading_by_absolute_path_reports_the_same_file() {
    let project = indexed_project();

    let context = context_for(
        TOOL_EVENT,
        file_tool(
            &project.store_path,
            tool_input(&project.root, &project.path(SERVER)),
            READING,
        ),
    );

    assert!(context.contains(&format!("`{SERVER}`")), "{context}");
    assert!(context.contains("**Dependents:** 2"), "{context}");
}

#[test]
fn post_edit_reports_the_edited_files_dependents() {
    let project = indexed_project();
    if !commit_then_edit_server(&project) {
        return;
    }

    let context = context_for(
        "PostToolUse",
        post_edit(&project.store_path, input(&project.root)),
    );

    assert!(
        context.contains(&format!("- `{SERVER}` (+1/-0) — 2 dependents")),
        "{context}"
    );
}

#[test]
fn post_compact_reports_the_edited_files_structure() {
    let project = indexed_project();
    if !commit_then_edit_server(&project) {
        return;
    }

    let context = context_for(
        "PostCompact",
        post_compact(&project.store_path, input(&project.root)),
    );

    assert!(
        context.contains(&format!("### {SERVER} (+1/-0)")),
        "{context}"
    );
    let dependents = line_starting(&context, "**Dependents (2):**");
    assert!(dependents.contains(MAIN), "{context}");
    assert!(dependents.contains(ROUTER), "{context}");
    assert_eq!(
        line_starting(&context, "**Imports:**"),
        format!("**Imports:** {ROUTER}")
    );
    assert!(
        context.contains("**Symbols:** 1 total, 1 public"),
        "{context}"
    );
}

#[test]
fn hooks_that_need_an_index_produce_nothing_without_one() {
    let project = test_project();
    let git_edit = commit_then_edit_server(&project);
    let stores = stores_without_an_index(&project);

    for store in &stores {
        let cwd = project.root.as_str();
        assert!(
            prompt_submit(store, prompt_input(cwd, &format!("look at {SERVER}"))).is_none(),
            "prompt with {store:?}"
        );
        assert!(
            file_tool(store, tool_input(cwd, SERVER), READING).is_none(),
            "file tool with {store:?}"
        );
        // Without git there is nothing edited, so these say nothing either way.
        if git_edit {
            assert!(
                post_edit(store, input(cwd)).is_none(),
                "post edit with {store:?}"
            );
            assert!(
                post_compact(store, input(cwd)).is_none(),
                "post compact with {store:?}"
            );
        }
    }
}

#[test]
fn hooks_ignore_an_index_of_another_project() {
    let project = indexed_project();
    let other = test_project();

    let context = context_for(
        "SessionStart",
        session_start(&project.store_path, input(&other.root)),
    );
    assert!(context.contains("Project not yet indexed"), "{context}");
    assert!(file_tool(
        &project.store_path,
        tool_input(&other.root, SERVER),
        READING
    )
    .is_none());
}
