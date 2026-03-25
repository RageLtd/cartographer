use std::io::{self, Read};
use std::process::Command;

use sonic_rs::JsonValueTrait;
use tokio::runtime::Runtime;

use crate::db::client::{connect, Db};
use crate::db::graph::{get_file_detail, FileDetail};
use crate::db::queries::{get_file_count, get_import_count};

// --- Hook I/O types ---

#[derive(serde::Deserialize)]
struct HookInput {
    #[serde(default)]
    cwd: String,
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    tool_input: Option<sonic_rs::Value>,
}

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

// --- Shared helpers ---

fn parse_input() -> Option<HookInput> {
    let mut buf = String::new();
    io::stdin().read_to_string(&mut buf).ok()?;
    sonic_rs::from_str(&buf).ok()
}

fn run_hook(handler: impl FnOnce(HookInput) -> Option<(&'static str, String)>) {
    let result = parse_input().and_then(handler);
    let output = HookOutput {
        continue_: true,
        suppress_output: true,
        hook_specific_output: result.map(|(event, context)| HookSpecificOutput {
            hook_event_name: Some(event.to_string()),
            additional_context: Some(context),
        }),
    };
    if let Ok(json) = sonic_rs::to_string(&output) {
        print!("{json}");
    }
}

fn open_db() -> Option<Db> {
    let rt = Runtime::new().ok()?;
    rt.block_on(connect()).ok()
}

fn require_indexed_db(cwd: &str) -> Option<(Db, Runtime)> {
    let rt = Runtime::new().ok()?;
    let db = rt.block_on(connect()).ok()?;
    let count = rt.block_on(get_file_count(&db, cwd)).unwrap_or(0);
    (count > 0).then_some((db, rt))
}

fn rel_path<'a>(path: &'a str, cwd: &str) -> &'a str {
    path.strip_prefix(cwd)
        .unwrap_or(path)
        .trim_start_matches('/')
}

fn resolve_abs_path(file: &str, cwd: &str) -> String {
    if file.starts_with('/') {
        file.to_string()
    } else {
        format!("{cwd}/{file}")
    }
}

fn extract_tool_file_path(input: &HookInput, cwd: &str) -> Option<String> {
    let tool_input = input.tool_input.as_ref()?;
    let file_path = tool_input
        .get("file_path")?
        .as_str()
        .filter(|p| !p.is_empty())?;
    Some(resolve_abs_path(file_path, cwd))
}

fn lookup_detail(db: &Db, rt: &Runtime, cwd: &str, path: &str) -> Option<FileDetail> {
    rt.block_on(get_file_detail(db, cwd, path)).ok().flatten()
}

fn find_files_by_suffix(db: &Db, rt: &Runtime, project: &str, suffix: &str) -> Vec<String> {
    #[derive(serde::Deserialize)]
    struct Row {
        file_path: String,
    }

    let pattern = format!("%{suffix}");
    let project_owned = project.to_string();
    let result = rt.block_on(async {
        let mut res = db
            .query("SELECT file_path FROM cart_file WHERE project = $project AND file_path LIKE $pattern LIMIT 3")
            .bind(("project", project_owned))
            .bind(("pattern", pattern))
            .await
            .ok()?;
        let rows: Vec<Row> = res.take(0).ok()?;
        Some(rows)
    });

    result
        .unwrap_or_default()
        .into_iter()
        .map(|r| r.file_path)
        .collect()
}

// --- Hook handlers ---

pub fn hook_context() {
    run_hook(|input| {
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
                let rt = Runtime::new().ok()?;
                let file_count = rt.block_on(get_file_count(&db, cwd)).unwrap_or(0);
                if file_count == 0 {
                    context.push_str(&format!(
                        "\n\n## Index Status\n\
                         - Project not yet indexed. Run `cartographer_index_project` with project path `{cwd}` to build the import graph."
                    ));
                } else {
                    let import_count = rt.block_on(get_import_count(&db, cwd)).unwrap_or(0);
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

        Some(("SessionStart", context))
    });
}

pub fn hook_prompt() {
    run_hook(|input| {
        let prompt = input.prompt.as_deref().filter(|p| !p.is_empty())?;
        let cwd = &input.cwd;
        let (db, rt) = require_indexed_db(cwd)?;

        let file_mentions = extract_file_mentions(prompt);
        let context_parts: Vec<String> = file_mentions
            .iter()
            .flat_map(|mention| find_files_by_suffix(&db, &rt, cwd, mention))
            .filter_map(|path| {
                let detail = lookup_detail(&db, &rt, cwd, &path)?;
                format_prompt_file_context(cwd, &detail)
            })
            .collect();

        (!context_parts.is_empty()).then(|| {
            (
                "UserPromptSubmit",
                format!(
                    "## Relevant files\n{}\n\nUse `cartographer_query` for deeper dependency analysis.",
                    context_parts.join("\n")
                ),
            )
        })
    });
}

pub fn hook_pre_read() {
    hook_file_tool("Reading");
}

pub fn hook_pre_edit() {
    hook_file_tool("Editing");
}

fn hook_file_tool(action: &'static str) {
    run_hook(|input| {
        let cwd = &input.cwd;
        let file_path = extract_tool_file_path(&input, cwd)?;
        let (db, rt) = require_indexed_db(cwd)?;
        let detail = lookup_detail(&db, &rt, cwd, &file_path)?;
        let ctx = format_tool_file_context(cwd, &detail, action)?;
        Some(("PreToolUse", ctx))
    });
}

pub fn hook_post_edit() {
    run_hook(|input| {
        let cwd = &input.cwd;
        let (db, rt) = require_indexed_db(cwd)?;
        let changed_files = git_changed_files(cwd);

        let lines: Vec<String> = changed_files
            .iter()
            .map(|(added, removed, file)| {
                let abs_path = resolve_abs_path(file, cwd);
                let dep_count =
                    lookup_detail(&db, &rt, cwd, &abs_path).map_or(0, |d| d.dependents.len());
                let suffix = if dep_count > 0 {
                    format!(" — {dep_count} dependents")
                } else {
                    String::new()
                };
                format!("- `{file}` (+{added}/-{removed}){suffix}")
            })
            .collect();

        (!lines.is_empty()).then(|| {
            (
                "PostToolUse",
                format!(
                    "## [cartographer] Modified files\n{}\n\nRun `cartographer_detect_changes` to update the import graph.",
                    lines.join("\n")
                ),
            )
        })
    });
}

pub fn hook_post_compact() {
    run_hook(|input| {
        let cwd = &input.cwd;
        let (db, rt) = require_indexed_db(cwd)?;

        let parts: Vec<String> = git_changed_files(cwd)
            .iter()
            .filter_map(|(added, removed, file)| {
                let abs_path = resolve_abs_path(file, cwd);
                let detail = lookup_detail(&db, &rt, cwd, &abs_path)?;
                Some(format_compact_file_context(cwd, &detail, added, removed))
            })
            .collect();

        (!parts.is_empty()).then(|| {
            (
                "PostCompact",
                format!(
                    "# [cartographer] Structural context for modified files\n\
                     \n\
                     These files have been modified during this session. Preserve awareness of their graph relationships.\n\
                     \n\
                     {}\n\
                     \n\
                     Use `cartographer_query` to explore deeper dependency chains.",
                    parts.join("\n\n")
                ),
            )
        })
    });
}

// --- Formatting helpers ---

fn format_path_list(entries: &[(String, Vec<String>)], cwd: &str, limit: usize) -> String {
    entries
        .iter()
        .take(limit)
        .map(|(p, _)| rel_path(p, cwd).to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_prompt_file_context(cwd: &str, detail: &FileDetail) -> Option<String> {
    let rp = rel_path(&detail.file_path, cwd);
    let total_syms = detail.symbols.len();

    if detail.imports.is_empty() && detail.dependents.is_empty() && total_syms == 0 {
        return None;
    }

    let mut part = format!("### {rp}");

    let dep_count = detail.dependents.len();
    if dep_count > 5 {
        part.push_str(&format!(
            "\n**Warning:** High fan-in ({dep_count} dependents) — changes here have wide impact"
        ));
    }

    if total_syms > 0 && detail.symbols.iter().all(|s| s.doc_comment.is_none()) {
        part.push_str(&format!(
            "\n**Note:** {total_syms} symbols, none documented"
        ));
    }

    if !detail.imports.is_empty() {
        let list = format_path_list(&detail.imports, cwd, usize::MAX);
        part.push_str(&format!("\n**Imports:** {list}"));
    }

    if !detail.dependents.is_empty() {
        let list = format_path_list(&detail.dependents, cwd, usize::MAX);
        part.push_str(&format!("\n**Imported by:** {list}"));
    }

    Some(part)
}

fn format_tool_file_context(cwd: &str, detail: &FileDetail, action: &str) -> Option<String> {
    if detail.imports.is_empty() && detail.dependents.is_empty() && detail.symbols.is_empty() {
        return None;
    }

    let rp = rel_path(&detail.file_path, cwd);
    let dep_count = detail.dependents.len();
    let mut parts: Vec<String> = Vec::new();

    match dep_count {
        n if n > 5 => parts.push(format!(
            "**Warning:** High fan-in ({n} dependents) — changes here have wide impact"
        )),
        n if n > 0 => parts.push(format!("**Dependents:** {n}")),
        _ => {}
    }

    if !detail.imports.is_empty() {
        let list = format_path_list(&detail.imports, cwd, usize::MAX);
        parts.push(format!("**Imports:** {list}"));
    }

    if !detail.dependents.is_empty() {
        let list = format_path_list(&detail.dependents, cwd, 8);
        let mut dep_str = format!("**Imported by:** {list}");
        if dep_count > 8 {
            dep_str.push_str(&format!(" + {} more", dep_count - 8));
        }
        parts.push(dep_str);
    }

    (!parts.is_empty()).then(|| format!("## [cartographer] {action} `{rp}`\n{}", parts.join("\n")))
}

fn format_compact_file_context(
    cwd: &str,
    detail: &FileDetail,
    added: &str,
    removed: &str,
) -> String {
    let rp = rel_path(&detail.file_path, cwd);
    let mut part = format!("### {rp} (+{added}/-{removed})");

    let dep_count = detail.dependents.len();
    if dep_count > 0 {
        let dep_list = format_path_list(&detail.dependents, cwd, 5);
        part.push_str(&format!("\n**Dependents ({dep_count}):** {dep_list}"));
        if dep_count > 5 {
            part.push_str(&format!(" + {} more", dep_count - 5));
        }
    }

    if !detail.imports.is_empty() {
        let imp_list = format_path_list(&detail.imports, cwd, usize::MAX);
        part.push_str(&format!("\n**Imports:** {imp_list}"));
    }

    let sym_count = detail.symbols.len();
    if sym_count > 0 {
        let pub_count = detail
            .symbols
            .iter()
            .filter(|s| {
                matches!(
                    s.visibility,
                    crate::types::Visibility::Public
                        | crate::types::Visibility::Exported
                        | crate::types::Visibility::DefaultExport
                        | crate::types::Visibility::PubCrate
                )
            })
            .count();
        part.push_str(&format!(
            "\n**Symbols:** {sym_count} total, {pub_count} public"
        ));
    }

    part
}

// --- Git helpers ---

fn git_changed_files(cwd: &str) -> Vec<(String, String, String)> {
    Command::new("git")
        .args(["diff", "--numstat", "HEAD"])
        .current_dir(cwd)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter_map(|line| {
                    let p: Vec<&str> = line.split('\t').collect();
                    (p.len() >= 3 && p[0] != "-")
                        .then(|| (p[0].to_string(), p[1].to_string(), p[2].to_string()))
                })
                .collect()
        })
        .unwrap_or_default()
}

// --- Prompt parsing helpers ---

const FILE_EXTENSIONS: &[&str] = &[
    "ts", "tsx", "js", "jsx", "rs", "rb", "ex", "exs", "py", "go", "java", "c", "h", "cpp",
    "hpp", "css", "scss", "vue", "svelte", "json", "toml", "yaml", "yml", "md",
];

fn extract_file_mentions(text: &str) -> Vec<String> {
    let mut mentions: Vec<String> = text
        .split(|c: char| c.is_whitespace() || c == '`' || c == '\'' || c == '"')
        .map(|w| {
            w.trim_matches(|c: char| {
                !c.is_alphanumeric() && c != '.' && c != '/' && c != '_' && c != '-'
            })
        })
        .filter(|w| !w.is_empty())
        .filter(|w| {
            w.rfind('.').is_some_and(|pos| {
                let ext = &w[pos + 1..];
                FILE_EXTENSIONS.contains(&ext) && (w.contains('/') || w.contains('.'))
            })
        })
        .map(String::from)
        .collect();

    mentions.sort();
    mentions.dedup();
    mentions
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
