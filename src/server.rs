use std::collections::HashMap;

use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content};
use rmcp::{tool, tool_router, ErrorData as McpError};
use tokio::runtime::Handle;

use crate::constants::{DEFAULT_MAX_DEPTH, DEFAULT_MAX_RESULTS};
use crate::db::client::Db;
use crate::db::graph::{find_cycles, get_file_detail, search_files, walk_import_graph};
use crate::db::queries::{
    get_file_count, get_import_count, get_language_counts, get_last_git_status, replace_imports,
    save_git_status, upsert_file,
};
use crate::indexer::{diff_git_status, full_index, get_current_git_status, incremental_index};
use crate::parser::{hash_file, parse_file};
use crate::server_types::{FileInfoInput, ParseFileInput, ProjectInput, QueryInput, SearchInput};

#[derive(Clone)]
pub struct CartographerServer {
    pub(crate) db: Db,
    pub(crate) tool_router: ToolRouter<Self>,
}

impl CartographerServer {
    pub fn new(db: Db) -> Self {
        Self {
            db,
            tool_router: Self::tool_router(),
        }
    }

    /// Bridge sync tool handlers to async DB calls
    fn run_async<F: std::future::Future>(&self, fut: F) -> F::Output {
        tokio::task::block_in_place(|| Handle::current().block_on(fut))
    }
}

#[tool_router]
impl CartographerServer {
    #[tool(
        name = "cartographer_parse_file",
        description = "Parse a file's AST using Tree-sitter to extract its imports and symbols. Stores the results in the import graph database. Supports: TypeScript, JavaScript, TSX, JSX, Rust."
    )]
    fn parse_file_tool(
        &self,
        Parameters(input): Parameters<ParseFileInput>,
    ) -> Result<CallToolResult, McpError> {
        let result = match parse_file(&input.file_path, None) {
            Ok(r) => r,
            Err(e) => {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Error parsing {}: {e}",
                    input.file_path
                ))]));
            }
        };

        let hash = match hash_file(&input.file_path) {
            Ok(h) => h,
            Err(e) => {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Error hashing {}: {e}",
                    input.file_path
                ))]));
            }
        };

        self.run_async(async {
            upsert_file(
                &self.db,
                &input.project,
                &input.file_path,
                &result.language,
                &result.symbols,
                &hash,
            )
            .await
            .map_err(|e| McpError::internal_error(e, None))?;

            replace_imports(&self.db, &input.project, &input.file_path, &result.imports)
                .await
                .map_err(|e| McpError::internal_error(e, None))?;

            Ok::<_, McpError>(())
        })?;

        #[derive(serde::Serialize)]
        #[serde(rename_all = "camelCase")]
        struct ImportOut {
            target: String,
            specifier: String,
            symbols: Vec<String>,
        }

        #[derive(serde::Serialize)]
        #[serde(rename_all = "camelCase")]
        struct ParseOut {
            file_path: String,
            language: String,
            imports: Vec<ImportOut>,
            symbols: Vec<crate::types::Symbol>,
        }

        let out = ParseOut {
            file_path: input.file_path,
            language: result.language,
            imports: result
                .imports
                .into_iter()
                .map(|i| ImportOut {
                    target: i.target,
                    specifier: i.specifier,
                    symbols: i.symbols,
                })
                .collect(),
            symbols: result.symbols,
        };

        let json = sonic_rs::to_string_pretty(&out)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        name = "cartographer_detect_changes",
        description = "Compare current git status against the last known snapshot to find changed files. Re-parses changed files and updates the import graph. Call this after file modifications."
    )]
    fn detect_changes(
        &self,
        Parameters(input): Parameters<ProjectInput>,
    ) -> Result<CallToolResult, McpError> {
        let last_status = self
            .run_async(get_last_git_status(&self.db, &input.project))
            .map_err(|e| McpError::internal_error(e, None))?;

        let current_status = get_current_git_status(&input.project);
        let (modified, deleted) = diff_git_status(&last_status, &current_status);

        if modified.is_empty() && deleted.is_empty() {
            self.run_async(save_git_status(&self.db, &input.project, &current_status))
                .map_err(|e| McpError::internal_error(e, None))?;
            return Ok(CallToolResult::success(vec![Content::text(
                "No changes detected.",
            )]));
        }

        let (indexed, removed) = self
            .run_async(incremental_index(&self.db, &input.project, &modified, &deleted))
            .map_err(|e| McpError::internal_error(e, None))?;

        self.run_async(save_git_status(&self.db, &input.project, &current_status))
            .map_err(|e| McpError::internal_error(e, None))?;

        #[derive(serde::Serialize)]
        struct ChangesOut {
            indexed: usize,
            removed: usize,
            modified: Vec<String>,
            deleted: Vec<String>,
        }

        let json = sonic_rs::to_string_pretty(&ChangesOut {
            indexed,
            removed,
            modified,
            deleted,
        })
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        name = "cartographer_query",
        description = "Walk the import graph outward from entry point files. Returns dependencies and dependents up to a configurable depth. Entry points can be absolute paths or search terms."
    )]
    fn query(&self, Parameters(input): Parameters<QueryInput>) -> Result<CallToolResult, McpError> {
        let mut resolved_paths: Vec<String> = Vec::new();
        for entry in &input.entry_points {
            if entry.starts_with('/') {
                resolved_paths.push(entry.clone());
            } else {
                let results = self
                    .run_async(search_files(&self.db, &input.project, entry, 5))
                    .map_err(|e| McpError::internal_error(e, None))?;
                for (path, _) in results {
                    resolved_paths.push(path);
                }
            }
        }

        if resolved_paths.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(
                "No matching files found in the index.",
            )]));
        }

        let max_depth = input.max_depth.unwrap_or(DEFAULT_MAX_DEPTH);
        let max_results = input.max_results.unwrap_or(DEFAULT_MAX_RESULTS);

        let files = self
            .run_async(walk_import_graph(
                &self.db,
                &input.project,
                &resolved_paths,
                Some(max_depth),
                Some(max_results),
            ))
            .map_err(|e| McpError::internal_error(e, None))?;

        #[derive(serde::Serialize)]
        struct QueryOut {
            path: String,
            reason: String,
            depth: i64,
            symbols: Vec<crate::types::Symbol>,
        }

        let out: Vec<QueryOut> = files
            .into_iter()
            .map(|f| QueryOut {
                path: f.relative_path,
                reason: f.reason,
                depth: f.depth,
                symbols: f.symbols,
            })
            .collect();

        let json = sonic_rs::to_string_pretty(&out)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        name = "cartographer_index_project",
        description = "Perform a full index of all supported files in a project directory. Walks the file tree, parses each file with Tree-sitter, and stores the import graph."
    )]
    fn index_project(
        &self,
        Parameters(input): Parameters<ProjectInput>,
    ) -> Result<CallToolResult, McpError> {
        let (indexed, skipped) = self
            .run_async(full_index(&self.db, &input.project, &input.project))
            .map_err(|e| McpError::internal_error(e, None))?;

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Indexed {indexed} files ({skipped} skipped)."
        ))]))
    }

    #[tool(
        name = "cartographer_stats",
        description = "Show statistics about the indexed codebase: file count, import edges, languages."
    )]
    fn stats(
        &self,
        Parameters(input): Parameters<ProjectInput>,
    ) -> Result<CallToolResult, McpError> {
        let total_files = self
            .run_async(get_file_count(&self.db, &input.project))
            .map_err(|e| McpError::internal_error(e, None))?;

        let import_count = self
            .run_async(get_import_count(&self.db, &input.project))
            .map_err(|e| McpError::internal_error(e, None))?;

        let languages = self
            .run_async(get_language_counts(&self.db, &input.project))
            .map_err(|e| McpError::internal_error(e, None))?;

        #[derive(serde::Serialize)]
        #[serde(rename_all = "camelCase")]
        struct StatsOut {
            total_files: i64,
            total_import_edges: i64,
            languages: HashMap<String, usize>,
        }

        let json = sonic_rs::to_string_pretty(&StatsOut {
            total_files,
            total_import_edges: import_count,
            languages,
        })
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        name = "cartographer_search",
        description = "Search indexed files by path or symbol name using full-text search. Returns matching files with their symbols, visibility, and doc comments."
    )]
    fn search(
        &self,
        Parameters(input): Parameters<SearchInput>,
    ) -> Result<CallToolResult, McpError> {
        let limit = input.limit.unwrap_or(10);
        let results = self
            .run_async(search_files(&self.db, &input.project, &input.query, limit))
            .map_err(|e| McpError::internal_error(e, None))?;

        if results.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(
                "No matching files found.",
            )]));
        }

        #[derive(serde::Serialize)]
        #[serde(rename_all = "camelCase")]
        struct SearchOut {
            file_path: String,
            relative_path: String,
            symbols: Vec<crate::types::Symbol>,
        }

        let out: Vec<SearchOut> = results
            .into_iter()
            .map(|(path, symbols)| {
                let relative_path = path
                    .strip_prefix(&input.project)
                    .unwrap_or(&path)
                    .trim_start_matches('/')
                    .to_string();
                SearchOut {
                    file_path: path,
                    relative_path,
                    symbols,
                }
            })
            .collect();

        let json = sonic_rs::to_string_pretty(&out)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        name = "cartographer_get_file_info",
        description = "Get detailed information about a specific file: its symbols (with visibility, signatures, doc comments), what it imports, and what imports it."
    )]
    fn get_file_info(
        &self,
        Parameters(input): Parameters<FileInfoInput>,
    ) -> Result<CallToolResult, McpError> {
        let detail = self
            .run_async(get_file_detail(&self.db, &input.project, &input.file_path))
            .map_err(|e| McpError::internal_error(e, None))?;

        let detail = match detail {
            Some(d) => d,
            None => {
                return Ok(CallToolResult::success(vec![Content::text(format!(
                    "File not found in index: {}",
                    input.file_path
                ))]));
            }
        };

        #[derive(serde::Serialize)]
        #[serde(rename_all = "camelCase")]
        struct ImportOut {
            target: String,
            symbols: Vec<String>,
        }

        #[derive(serde::Serialize)]
        #[serde(rename_all = "camelCase")]
        struct DependentOut {
            source: String,
            symbols: Vec<String>,
        }

        #[derive(serde::Serialize)]
        #[serde(rename_all = "camelCase")]
        struct FileInfoOut {
            file_path: String,
            relative_path: String,
            language: String,
            symbols: Vec<crate::types::Symbol>,
            imports: Vec<ImportOut>,
            dependents: Vec<DependentOut>,
        }

        let relative_path = detail
            .file_path
            .strip_prefix(&input.project)
            .unwrap_or(&detail.file_path)
            .trim_start_matches('/')
            .to_string();

        let out = FileInfoOut {
            file_path: detail.file_path,
            relative_path,
            language: detail.language,
            symbols: detail.symbols,
            imports: detail
                .imports
                .into_iter()
                .map(|(target, symbols)| ImportOut { target, symbols })
                .collect(),
            dependents: detail
                .dependents
                .into_iter()
                .map(|(source, symbols)| DependentOut { source, symbols })
                .collect(),
        };

        let json = sonic_rs::to_string_pretty(&out)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        name = "cartographer_find_cycles",
        description = "Detect circular dependencies in the import graph. Returns all dependency cycles found in the project."
    )]
    fn find_cycles_tool(
        &self,
        Parameters(input): Parameters<ProjectInput>,
    ) -> Result<CallToolResult, McpError> {
        let cycles = self
            .run_async(find_cycles(&self.db, &input.project))
            .map_err(|e| McpError::internal_error(e, None))?;

        if cycles.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(
                "No circular dependencies found.",
            )]));
        }

        #[derive(serde::Serialize)]
        struct CycleOut {
            cycle: Vec<String>,
            length: usize,
        }

        let out: Vec<CycleOut> = cycles
            .into_iter()
            .map(|c| {
                let length = c.len() - 1;
                let cycle: Vec<String> = c
                    .into_iter()
                    .map(|p| {
                        p.strip_prefix(&input.project)
                            .unwrap_or(&p)
                            .trim_start_matches('/')
                            .to_string()
                    })
                    .collect();
                CycleOut { cycle, length }
            })
            .collect();

        let json = sonic_rs::to_string_pretty(&out)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }
}
