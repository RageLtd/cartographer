use std::io::{self, Read};

use rusqlite::Connection;

use crate::constants::default_db_path;
use crate::db::graph::get_file_detail;
use crate::db::queries::{get_file_count, get_import_count};
use crate::db::setup::create_database;

/// Hook input from Claude Code (subset of fields we care about)
#[derive(serde::Deserialize)]
struct HookInput {
    #[serde(default)]
    cwd: String,
    #[serde(default)]
    prompt: Option<String>,
}

/// Hook output to Claude Code
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct HookOutput {
    #[serde(rename = "continue")]
    continue_: bool,
    suppress_output: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    hook_specific_output: Option<HookSpecificOutput>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct HookSpecificOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    hook_event_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    additional_context: Option<String>,
}

fn read_stdin() -> io::Result<String> {
    let mut buf = String::new();
    io::stdin().read_to_string(&mut buf)?;
    Ok(buf)
}

fn emit(output: &HookOutput) {
    if let Ok(json) = sonic_rs::to_string(output) {
        print!("{json}");
    }
}

fn empty_response() -> HookOutput {
    HookOutput {
        continue_: true,
        suppress_output: true,
        hook_specific_output: None,
    }
}

fn context_response(event: &str, context: String) -> HookOutput {
    HookOutput {
        continue_: true,
        suppress_output: true,
        hook_specific_output: Some(HookSpecificOutput {
            hook_event_name: Some(event.to_string()),
            additional_context: Some(context),
        }),
    }
}

fn open_db() -> Option<Connection> {
    let path = default_db_path().ok()?;
    let path_str = path.to_str()?;
    if !path.exists() {
        return None;
    }
    create_database(path_str).ok()
}

/// SessionStart hook — inject graph-first guidance and index status.
pub fn hook_context() {
    let input: HookInput = match read_stdin().ok().and_then(|s| sonic_rs::from_str(&s).ok()) {
        Some(i) => i,
        None => {
            emit(&empty_response());
            return;
        }
    };

    let mut context = String::from(
        "# [cartographer] codebase structure\n\
         \n\
         ## Graph-First Navigation\n\
         \n\
         When you need to understand file relationships, dependencies, or the blast radius of changes:\n\
         - **Use `cartographer_query` BEFORE using Grep or Glob** for structural questions\n\
         - Use `cartographer_query` with `entry_points` to find dependencies and dependents\n\
         - Use `cartographer_search` to find files by name or symbol\n\
         - Only fall back to Grep/Glob when searching for specific string literals or patterns\n\
         \n\
         ## When to Query the Graph\n\
         - \"What does X depend on?\" → `cartographer_query`\n\
         - \"What would break if I change X?\" → `cartographer_query`\n\
         - \"Where is X defined/exported?\" → `cartographer_search`\n\
         - \"How are these modules connected?\" → `cartographer_query` from both files\n\
         - Understanding architecture or module boundaries → `cartographer_query`\n\
         \n\
         ## When Grep/Glob is Still Better\n\
         - Searching for specific error messages or string literals\n\
         - Finding TODO/FIXME comments\n\
         - Pattern matching across file contents (not structure)",
    );

    if let Some(db) = open_db() {
        let cwd = &input.cwd;
        if !cwd.is_empty() {
            let file_count = get_file_count(&db, cwd).unwrap_or(0);
            if file_count == 0 {
                context.push_str(&format!(
                    "\n\n## Index Status\n\
                     - Project not yet indexed. Run `cartographer_index_project` with project path `{cwd}` to build the import graph."
                ));
            } else {
                let import_count = get_import_count(&db, cwd).unwrap_or(0);
                context.push_str(&format!(
                    "\n\n## Index Status\n\
                     - **Project**: {cwd}\n\
                     - **Files indexed**: {file_count}\n\
                     - **Import edges**: {import_count}\n\
                     - Run `cartographer_detect_changes` if files have changed since last index",
                ));
            }
        }
    }

    emit(&context_response("SessionStart", context));
}

/// UserPromptSubmit hook — extract file mentions, look up their graph neighborhood.
pub fn hook_prompt() {
    let input: HookInput = match read_stdin().ok().and_then(|s| sonic_rs::from_str(&s).ok()) {
        Some(i) => i,
        None => {
            emit(&empty_response());
            return;
        }
    };

    let prompt = match &input.prompt {
        Some(p) if !p.is_empty() => p,
        _ => {
            emit(&empty_response());
            return;
        }
    };

    let cwd = &input.cwd;
    if cwd.is_empty() {
        emit(&empty_response());
        return;
    }

    let db = match open_db() {
        Some(db) => db,
        None => {
            emit(&empty_response());
            return;
        }
    };

    // Check if project is indexed
    let file_count = get_file_count(&db, cwd).unwrap_or(0);
    if file_count == 0 {
        emit(&empty_response());
        return;
    }

    // Extract file-like mentions from the prompt
    let file_mentions = extract_file_mentions(prompt);
    if file_mentions.is_empty() {
        emit(&empty_response());
        return;
    }

    // Look up each mentioned file using full detail
    let mut context_parts: Vec<String> = Vec::new();

    for mention in &file_mentions {
        let matches = find_files_by_suffix(&db, cwd, mention);
        for file_path in matches {
            let detail = match get_file_detail(&db, cwd, &file_path) {
                Ok(Some(d)) => d,
                _ => continue,
            };

            let rel_path = detail
                .file_path
                .strip_prefix(cwd)
                .unwrap_or(&detail.file_path)
                .trim_start_matches('/');

            let mut part = format!("### {rel_path}");

            // Impact warning
            let dep_count = detail.dependents.len();
            if dep_count > 5 {
                part.push_str(&format!(
                    "\n**Warning:** High fan-in ({dep_count} dependents) — changes here have wide impact"
                ));
            }

            // Doc coverage
            let total_syms = detail.symbols.len();
            if total_syms > 0 {
                let undocumented = detail
                    .symbols
                    .iter()
                    .filter(|s| s.doc_comment.is_none())
                    .count();
                if undocumented == total_syms {
                    part.push_str(&format!(
                        "\n**Note:** {total_syms} symbols, none documented"
                    ));
                }
            }

            if !detail.imports.is_empty() {
                let dep_list: Vec<String> = detail
                    .imports
                    .iter()
                    .map(|(t, _)| {
                        t.strip_prefix(cwd)
                            .unwrap_or(t)
                            .trim_start_matches('/')
                            .to_string()
                    })
                    .collect();
                part.push_str(&format!("\n**Imports:** {}", dep_list.join(", ")));
            }

            if !detail.dependents.is_empty() {
                let dep_list: Vec<String> = detail
                    .dependents
                    .iter()
                    .map(|(s, _)| {
                        s.strip_prefix(cwd)
                            .unwrap_or(s)
                            .trim_start_matches('/')
                            .to_string()
                    })
                    .collect();
                part.push_str(&format!("\n**Imported by:** {}", dep_list.join(", ")));
            }

            if detail.imports.is_empty() && detail.dependents.is_empty() && total_syms == 0 {
                continue;
            }

            context_parts.push(part);
        }
    }

    if context_parts.is_empty() {
        emit(&empty_response());
        return;
    }

    let context = format!(
        "## Relevant files\n{}\n\nUse `cartographer_query` for deeper dependency analysis.",
        context_parts.join("\n")
    );

    emit(&context_response("UserPromptSubmit", context));
}

/// Extract file-path-like mentions from user prompt text.
fn extract_file_mentions(text: &str) -> Vec<String> {
    let mut mentions = Vec::new();

    for word in text.split(|c: char| c.is_whitespace() || c == '`' || c == '\'' || c == '"') {
        let word = word.trim_matches(|c: char| {
            !c.is_alphanumeric() && c != '.' && c != '/' && c != '_' && c != '-'
        });
        if word.is_empty() {
            continue;
        }

        // Must contain a dot followed by a plausible file extension
        if let Some(dot_pos) = word.rfind('.') {
            let ext = &word[dot_pos + 1..];
            if matches!(
                ext,
                "ts" | "tsx"
                    | "js"
                    | "jsx"
                    | "rs"
                    | "py"
                    | "go"
                    | "java"
                    | "c"
                    | "h"
                    | "cpp"
                    | "hpp"
                    | "css"
                    | "scss"
                    | "vue"
                    | "svelte"
                    | "json"
                    | "toml"
                    | "yaml"
                    | "yml"
                    | "md"
            ) {
                // Must look like a path (contain / or at least a directory-like prefix)
                if word.contains('/') || word.contains('.') {
                    mentions.push(word.to_string());
                }
            }
        }
    }

    mentions.sort();
    mentions.dedup();
    mentions
}

fn find_files_by_suffix(db: &Connection, project: &str, suffix: &str) -> Vec<String> {
    let pattern = format!("%{suffix}");
    let mut stmt = db
        .prepare_cached(
            "SELECT file_path FROM files WHERE project = ?1 AND file_path LIKE ?2 LIMIT 3",
        )
        .unwrap_or_else(|_| panic!("Failed to prepare query"));

    let rows = stmt
        .query_map(rusqlite::params![project, pattern], |row| {
            row.get::<_, String>(0)
        })
        .unwrap_or_else(|_| panic!("Failed to query"));

    rows.filter_map(|r| r.ok()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

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
        // 1.2.3 should not match — no valid extension
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
}
