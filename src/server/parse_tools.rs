//! Tools that parse source files: single-file parse, change detection, full index.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content};
use rmcp::{tool, tool_router, ErrorData as McpError};

use crate::db::queries::{replace_imports, upsert_file};
use crate::indexer::{self, full_index, walk_project_files};
use crate::parser::ts_uses::uses_in_file;
use crate::parser::{hash_file, parse_file};
use crate::server_types::{ParseFileInput, ProjectInput, UsesInFileInput};

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
        let result = match parse_file(&self.registry, &input.file_path) {
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
            upsert_file(
                db,
                &input.project,
                &input.file_path,
                &result.language,
                &result.symbols,
                &hash,
            )
            .map_err(|e| McpError::internal_error(e, None))?;

            replace_imports(db, &input.project, &input.file_path, &result.imports)
                .map_err(|e| McpError::internal_error(e, None))?;
        }

        #[derive(serde::Serialize)]
        #[serde(rename_all = "camelCase")]
        struct ImportOut {
            target: String,
            specifier: String,
            symbols: Vec<String>,
            kind: crate::types::EdgeKind,
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
                    kind: i.kind,
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
        description = "Bring the index in line with the project's files: re-parses files that are new or whose content changed, and removes files that are no longer on disk. It compares content, not git status, so it also sees a pull, a checkout or a revert. Call this after file modifications. Needs the index, so it is unavailable in parse-only mode."
    )]
    fn detect_changes(
        &self,
        Parameters(input): Parameters<ProjectInput>,
    ) -> Result<CallToolResult, McpError> {
        let db = self.require_db()?;
        let changes = indexer::detect_changes(db, &self.registry, &input.project, &input.project)
            .map_err(|e| McpError::internal_error(e, None))?;

        if changes.reparsed.is_empty() && changes.removed.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(
                "No changes detected.",
            )]));
        }

        #[derive(serde::Serialize)]
        struct ChangesOut {
            indexed: usize,
            removed: usize,
            modified: Vec<String>,
            deleted: Vec<String>,
        }

        let json = sonic_rs::to_string_pretty(&ChangesOut {
            indexed: changes.reparsed.len(),
            removed: changes.removed.len(),
            modified: changes.reparsed,
            deleted: changes.removed,
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
                let (indexed, skipped) =
                    full_index(db, &self.registry, &input.project, &input.project)
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
                    match parse_file(&self.registry, file_path) {
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

    #[tool(
        name = "cartographer_uses_in_file",
        description = "Find where one file uses a symbol that another file exports. Returns the lines of the file that name the symbol, and the names the file re-exports it under, if any, for following the symbol to the files that import from this one. Give the file to look in, the file that exports the symbol, and the symbol's name as that file exports it (`default` for a default export). Understands renamed imports, namespace imports and imports by workspace package name. TypeScript and JavaScript only. This parses the one file and needs no index, so it works in parse-only mode; cartographer_find_uses answers for a whole project when there is an index."
    )]
    fn uses_in_file_tool(
        &self,
        Parameters(input): Parameters<UsesInFileInput>,
    ) -> Result<CallToolResult, McpError> {
        let uses = match uses_in_file(
            &self.registry,
            &input.file_path,
            &input.target_file,
            &input.symbol,
        ) {
            Ok(uses) => uses,
            Err(e) => return Ok(CallToolResult::error(vec![Content::text(e)])),
        };

        #[derive(serde::Serialize)]
        #[serde(rename_all = "camelCase")]
        struct UsesOut {
            file_path: String,
            lines: Vec<usize>,
            re_exported_as: Vec<String>,
        }

        let json = sonic_rs::to_string_pretty(&UsesOut {
            file_path: input.file_path,
            lines: uses.lines,
            re_exported_as: uses.re_exported_as,
        })
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }
}
