//! Tools about the project itself rather than its code: rules files and root path.

use std::path::Path;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content};
use rmcp::{tool, tool_router, ErrorData as McpError};

use crate::server_types::{EmptyInput, RulesInput};

use super::CartographerServer;

#[tool_router(router = project_tools_router, vis = "pub(crate)")]
impl CartographerServer {
    #[tool(
        name = "cartographer_rules",
        description = "Load project rules and conventions files. Scans for CLAUDE.md, AGENTS.md, .cursorrules, .claude/rules/**/*.md, and .claude/settings.json. Returns all found rules with their source paths. Call this at session start to understand project conventions."
    )]
    fn rules(&self, Parameters(input): Parameters<RulesInput>) -> Result<CallToolResult, McpError> {
        let project = match input.project {
            Some(p) if !p.is_empty() => p,
            _ => {
                let mut resolved = None;
                for var in ["PROJECT_ROOT", "ZED_WORKTREE_ROOT"] {
                    if let Ok(root) = std::env::var(var) {
                        if !root.is_empty() && root != "/" {
                            resolved = Some(root);
                            break;
                        }
                    }
                }
                resolved.unwrap_or_else(|| {
                    std::env::current_dir()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_else(|_| "/".to_string())
                })
            }
        };

        if project == "/" || project == "unknown" {
            return Ok(CallToolResult::error(vec![Content::text(
                "Cannot determine project root. Provide the project path explicitly.",
            )]));
        }

        let root = Path::new(&project);
        let mut rules: Vec<(String, String)> = Vec::new();

        // Root-level rules files
        for name in [
            "CLAUDE.md",
            "AGENTS.md",
            ".cursorrules",
            ".github/copilot-instructions.md",
        ] {
            let path = root.join(name);
            if path.is_file() {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    rules.push((name.to_string(), content));
                }
            }
        }

        // .claude/settings.json
        let settings_path = root.join(".claude/settings.json");
        if settings_path.is_file() {
            if let Ok(content) = std::fs::read_to_string(&settings_path) {
                rules.push((".claude/settings.json".to_string(), content));
            }
        }

        // .claude/rules/**/*.md (recursive)
        let rules_dir = root.join(".claude/rules");
        if rules_dir.is_dir() {
            collect_md_files(root, &rules_dir, &mut rules);
        }

        // .claude/skills/**/*.md (recursive)
        let skills_dir = root.join(".claude/skills");
        if skills_dir.is_dir() {
            collect_md_files(root, &skills_dir, &mut rules);
        }

        if rules.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(
                "No rules files found in project.",
            )]));
        }

        let mut output = String::new();
        for (path, content) in &rules {
            output.push_str(&format!("--- {} ---\n{}\n\n", path, content));
        }

        tracing::info!(
            "cartographer_rules: found {} rules files in {}",
            rules.len(),
            project
        );

        Ok(CallToolResult::success(vec![Content::text(output)]))
    }

    #[tool(
        name = "cartographer_project_path",
        description = "Get the current project's root directory path. Call this first if you need to know the project path for other Cartographer tools."
    )]
    fn project_path(
        &self,
        #[allow(unused)] Parameters(_input): Parameters<EmptyInput>,
    ) -> Result<CallToolResult, McpError> {
        // Try env vars in order of specificity
        for var in ["PROJECT_ROOT", "ZED_WORKTREE_ROOT"] {
            if let Ok(root) = std::env::var(var) {
                if !root.is_empty() && root != "/" {
                    tracing::info!("project_path from {var}: {root}");
                    return Ok(CallToolResult::success(vec![Content::text(root)]));
                }
            }
        }
        // Fall back to CWD (Zed should set this to project root for MCP servers)
        let cwd = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| "/".to_string());
        tracing::info!("project_path from CWD: {cwd}");
        Ok(CallToolResult::success(vec![Content::text(cwd)]))
    }
}

/// Recursively collect .md files from a directory, storing project-relative paths
/// like ".claude/rules/coding/style.md".
fn collect_md_files(base: &Path, dir: &Path, out: &mut Vec<(String, String)>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_md_files(base, &path, out);
        } else if path.extension().is_some_and(|e| e == "md") {
            if let Ok(content) = std::fs::read_to_string(&path) {
                let rel = path
                    .strip_prefix(base)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .to_string();
                out.push((rel, content));
            }
        }
    }
}
