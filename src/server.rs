use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    AnnotateAble, CallToolResult, Content, ListResourcesResult, RawResource,
    ReadResourceRequestParams, ReadResourceResult, ResourceContents, ServerCapabilities,
    ServerInfo,
};
use rmcp::schemars;
use rmcp::{ErrorData as McpError, ServerHandler, tool, tool_handler, tool_router};
use rusqlite::Connection;
use serde::Deserialize;

use crate::constants::{DEFAULT_MAX_DEPTH, DEFAULT_MAX_RESULTS};
use crate::db::queries::{
    get_import_count, get_last_git_status, get_project_stats, get_tracked_files, replace_imports,
    save_git_status, search_files, upsert_file, walk_import_graph,
};
use crate::indexer::{
    diff_git_status, full_index, get_current_git_status, incremental_index,
};
use crate::parser::{hash_file, parse_file};

#[derive(Clone)]
pub struct CartographerServer {
    db: Arc<Mutex<Connection>>,
    tool_router: ToolRouter<Self>,
}

impl CartographerServer {
    pub fn new(db: Connection) -> Self {
        Self {
            db: Arc::new(Mutex::new(db)),
            tool_router: Self::tool_router(),
        }
    }
}

// ============================================================================
// Tool input schemas
// ============================================================================

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct ParseFileInput {
    /// Absolute path to the file to parse
    file_path: String,
    /// Project root directory (absolute path)
    project: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct ProjectInput {
    /// Project root directory (absolute path)
    project: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct QueryInput {
    /// Project root directory (absolute path)
    project: String,
    /// File paths or search terms to start the graph walk from
    entry_points: Vec<String>,
    /// Maximum hops to traverse (1-5, default 2)
    max_depth: Option<i64>,
    /// Maximum files to return (1-50, default 20)
    max_results: Option<i64>,
}

// ============================================================================
// Tool implementations
// ============================================================================

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
        let db = self.db.lock().map_err(|e| McpError::internal_error(e.to_string(), None))?;

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

        upsert_file(
            &db,
            &input.project,
            &input.file_path,
            &result.language,
            &result.symbols,
            &hash,
        )
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        replace_imports(&db, &input.project, &input.file_path, &result.imports)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

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
        let db = self.db.lock().map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let last_status = get_last_git_status(&db, &input.project)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let current_status = get_current_git_status(&input.project);
        let (modified, deleted) = diff_git_status(&last_status, &current_status);

        if modified.is_empty() && deleted.is_empty() {
            save_git_status(&db, &input.project, &current_status)
                .map_err(|e| McpError::internal_error(e.to_string(), None))?;
            return Ok(CallToolResult::success(vec![Content::text(
                "No changes detected.",
            )]));
        }

        let (indexed, removed) = incremental_index(&db, &input.project, &modified, &deleted)
            .map_err(|e| McpError::internal_error(e, None))?;

        save_git_status(&db, &input.project, &current_status)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

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
    fn query(
        &self,
        Parameters(input): Parameters<QueryInput>,
    ) -> Result<CallToolResult, McpError> {
        let db = self.db.lock().map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let mut resolved_paths: Vec<String> = Vec::new();
        for entry in &input.entry_points {
            if entry.starts_with('/') {
                resolved_paths.push(entry.clone());
            } else {
                let results = search_files(&db, &input.project, entry, 5)
                    .map_err(|e| McpError::internal_error(e.to_string(), None))?;
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

        let files = walk_import_graph(
            &db,
            &input.project,
            &resolved_paths,
            Some(max_depth),
            Some(max_results),
        )
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;

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
        let db = self.db.lock().map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let (indexed, skipped) = full_index(&db, &input.project, &input.project)
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
        let db = self.db.lock().map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let files = get_tracked_files(&db, &input.project)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let import_count = get_import_count(&db, &input.project)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let mut languages: HashMap<String, usize> = HashMap::new();
        for f in &files {
            *languages.entry(f.language.clone()).or_insert(0) += 1;
        }

        #[derive(serde::Serialize)]
        #[serde(rename_all = "camelCase")]
        struct StatsOut {
            total_files: usize,
            total_import_edges: i64,
            languages: HashMap<String, usize>,
        }

        let json = sonic_rs::to_string_pretty(&StatsOut {
            total_files: files.len(),
            total_import_edges: import_count,
            languages,
        })
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }
}

// ============================================================================
// ServerHandler — resources + server info
// ============================================================================

#[tool_handler]
impl ServerHandler for CartographerServer {
    fn get_info(&self) -> ServerInfo {
        let capabilities = ServerCapabilities::builder()
            .enable_tools()
            .enable_resources()
            .build();

        ServerInfo::new(capabilities)
            .with_instructions("Cartographer: codebase structure mapping via Tree-sitter AST parsing")
    }

    async fn list_resources(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParams>,
        _context: rmcp::service::RequestContext<rmcp::service::RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        let resource = RawResource::new("cartographer://project", "Project Index Overview")
            .with_description("List all indexed projects and their file counts")
            .with_mime_type("application/json")
            .no_annotation();

        Ok(ListResourcesResult::with_all_items(vec![resource]))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: rmcp::service::RequestContext<rmcp::service::RoleServer>,
    ) -> Result<ReadResourceResult, McpError> {
        if request.uri == "cartographer://project" {
            let db = self
                .db
                .lock()
                .map_err(|e| McpError::internal_error(e.to_string(), None))?;

            let stats = get_project_stats(&db)
                .map_err(|e| McpError::internal_error(e.to_string(), None))?;

            #[derive(serde::Serialize)]
            struct ProjectRow {
                project: String,
                count: i64,
            }

            let rows: Vec<ProjectRow> = stats
                .into_iter()
                .map(|(project, count)| ProjectRow { project, count })
                .collect();

            let json = sonic_rs::to_string_pretty(&rows)
                .map_err(|e| McpError::internal_error(e.to_string(), None))?;

            Ok(ReadResourceResult::new(vec![ResourceContents::text(
                json,
                "cartographer://project",
            )]))
        } else {
            Err(McpError::resource_not_found(
                "resource_not_found",
                Some(format!("Unknown resource: {}", request.uri).into()),
            ))
        }
    }
}
