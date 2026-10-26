//! Tools that read the stored import graph: walk, search, file detail, cycles, stats.

use std::collections::HashMap;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content};
use rmcp::{tool, tool_router, ErrorData as McpError};

use crate::constants::{DEFAULT_MAX_DEPTH, DEFAULT_MAX_RESULTS};
use crate::db::graph::{find_cycles, get_file_detail, search_files, walk_import_graph};
use crate::db::queries::{get_file_count, get_import_count, get_language_counts};
use crate::server_types::{FileInfoInput, ProjectInput, QueryInput, SearchInput};

use super::CartographerServer;

#[tool_router(router = graph_tools_router, vis = "pub(crate)")]
impl CartographerServer {
    #[tool(
        name = "cartographer_query",
        description = "Walk the import graph outward from entry point files. Returns dependencies and dependents up to a configurable depth. Entry points can be absolute paths or search terms."
    )]
    fn query(&self, Parameters(input): Parameters<QueryInput>) -> Result<CallToolResult, McpError> {
        let db = self.require_db()?;
        let mut resolved_paths: Vec<String> = Vec::new();
        for entry in &input.entry_points {
            if entry.starts_with('/') {
                resolved_paths.push(entry.clone());
            } else {
                let results = self
                    .run_async(search_files(db, &input.project, entry, 5))
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
                db,
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
        name = "cartographer_stats",
        description = "Show statistics about the indexed codebase: file count, import edges, languages, and the grammars currently loaded (built-in and host-provided)."
    )]
    fn stats(
        &self,
        Parameters(input): Parameters<ProjectInput>,
    ) -> Result<CallToolResult, McpError> {
        let db = self.require_db()?;
        let total_files = self
            .run_async(get_file_count(db, &input.project))
            .map_err(|e| McpError::internal_error(e, None))?;

        let import_count = self
            .run_async(get_import_count(db, &input.project))
            .map_err(|e| McpError::internal_error(e, None))?;

        let languages = self
            .run_async(get_language_counts(db, &input.project))
            .map_err(|e| McpError::internal_error(e, None))?;

        #[derive(serde::Serialize)]
        #[serde(rename_all = "camelCase")]
        struct GrammarOut {
            name: String,
            /// `builtin` (compiled in) or `host` (loaded from the grammars directory).
            source: &'static str,
            extensions: Vec<String>,
        }

        #[derive(serde::Serialize)]
        #[serde(rename_all = "camelCase")]
        struct StatsOut {
            total_files: i64,
            total_import_edges: i64,
            languages: HashMap<String, usize>,
            grammars: Vec<GrammarOut>,
        }

        let grammars = self
            .registry
            .languages()
            .iter()
            .map(|def| GrammarOut {
                name: def.name.clone(),
                source: if def.is_host() { "host" } else { "builtin" },
                extensions: self.registry.owned_extensions(&def.name),
            })
            .collect();

        let json = sonic_rs::to_string_pretty(&StatsOut {
            total_files,
            total_import_edges: import_count,
            languages,
            grammars,
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
        let db = self.require_db()?;
        let limit = input.limit.unwrap_or(10);
        let results = self
            .run_async(search_files(db, &input.project, &input.query, limit))
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
        let db = self.require_db()?;
        let detail = self
            .run_async(get_file_detail(db, &input.project, &input.file_path))
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
        let db = self.require_db()?;
        let cycles = self
            .run_async(find_cycles(db, &input.project))
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
