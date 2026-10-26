//! Tools that parse source files: single-file parse, change detection, full index.

use std::collections::HashMap;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content};
use rmcp::{tool, tool_router, ErrorData as McpError};

use crate::db::queries::{get_last_git_status, replace_imports, save_git_status, upsert_file};
use crate::indexer::{
    diff_git_status, full_index, get_current_git_status, incremental_index, walk_project_files,
};
use crate::parser::{hash_file, parse_file};
use crate::server_types::{ParseFileInput, ProjectInput};

use super::CartographerServer;

#[tool_router(router = parse_tools_router, vis = "pub(crate)")]
impl CartographerServer {
    #[tool(
        name = "cartographer_parse_file",
        description = "Parse a file's AST using Tree-sitter to extract its imports and symbols. In full mode, stores results in the database. In parse-only mode, returns results without storage. Built-in languages: TypeScript, JavaScript, TSX, JSX, Rust, Ruby, Elixir; plus any grammar installed under ~/.cartographer/grammars (cartographer_stats lists them)."
    )]
    fn parse_file_tool(
        &self,
        Parameters(input): Parameters<ParseFileInput>,
    ) -> Result<CallToolResult, McpError> {
        let result = match parse_file(&self.registry, &input.file_path, None) {
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

        // Store in DB if available (full mode); skip in parse-only mode.
        if let Some(db) = &self.db {
            self.run_async(async {
                upsert_file(
                    db,
                    &input.project,
                    &input.file_path,
                    &result.language,
                    &result.symbols,
                    &hash,
                )
                .await
                .map_err(|e| McpError::internal_error(e, None))?;

                replace_imports(db, &input.project, &input.file_path, &result.imports)
                    .await
                    .map_err(|e| McpError::internal_error(e, None))?;

                Ok::<_, McpError>(())
            })?;
        }

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
        // Get last known status from DB (or empty if parse-only)
        let last_status = match &self.db {
            Some(db) => self
                .run_async(get_last_git_status(db, &input.project))
                .map_err(|e| McpError::internal_error(e, None))?,
            None => HashMap::new(),
        };

        let current_status = get_current_git_status(&self.registry, &input.project);
        let (modified, deleted) = diff_git_status(&last_status, &current_status);

        if modified.is_empty() && deleted.is_empty() {
            if let Some(db) = &self.db {
                self.run_async(save_git_status(db, &input.project, &current_status))
                    .map_err(|e| McpError::internal_error(e, None))?;
            }
            return Ok(CallToolResult::success(vec![Content::text(
                "No changes detected.",
            )]));
        }

        // In full mode, do incremental index (parse + store).
        // In parse-only mode, just parse modified files and report.
        let (indexed, removed) = match &self.db {
            Some(db) => {
                let result = self
                    .run_async(incremental_index(
                        db,
                        &self.registry,
                        &input.project,
                        &modified,
                        &deleted,
                    ))
                    .map_err(|e| McpError::internal_error(e, None))?;
                self.run_async(save_git_status(db, &input.project, &current_status))
                    .map_err(|e| McpError::internal_error(e, None))?;
                result
            }
            None => {
                // Parse-only: parse each modified file to validate, count results
                let mut idx = 0usize;
                for file_path in &modified {
                    if std::path::Path::new(file_path).exists()
                        && parse_file(&self.registry, file_path, None).is_ok()
                    {
                        idx += 1;
                    }
                }
                (idx, deleted.len())
            }
        };

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
        name = "cartographer_index_project",
        description = "Perform a full index of all supported files in a project directory. Walks the file tree, parses each file with Tree-sitter. In full mode, stores the import graph. In parse-only mode, parses without storage."
    )]
    fn index_project(
        &self,
        Parameters(input): Parameters<ProjectInput>,
    ) -> Result<CallToolResult, McpError> {
        match &self.db {
            Some(db) => {
                let (indexed, skipped) = self
                    .run_async(full_index(
                        db,
                        &self.registry,
                        &input.project,
                        &input.project,
                    ))
                    .map_err(|e| McpError::internal_error(e, None))?;
                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Indexed {indexed} files ({skipped} skipped)."
                ))]))
            }
            None => {
                // Parse-only: walk and parse files, report counts
                let files = walk_project_files(&self.registry, &input.project);
                let mut indexed = 0usize;
                let mut skipped = 0usize;
                for file_path in &files {
                    match parse_file(&self.registry, file_path, None) {
                        Ok(_) => indexed += 1,
                        Err(_) => skipped += 1,
                    }
                }
                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Parsed {indexed} files ({skipped} skipped). (parse-only mode, no storage)"
                ))]))
            }
        }
    }
}
