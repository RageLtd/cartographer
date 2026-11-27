//! Tools that keep the stored index in line with a project's files. They need
//! the index, so a parse-only server does not list them.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content};
use rmcp::{tool, tool_router, ErrorData as McpError};

use crate::indexer::{self, full_index};
use crate::server_types::ProjectInput;

use super::CartographerServer;

#[tool_router(router = index_tools_router, vis = "pub(crate)")]
impl CartographerServer {
    #[tool(
        name = "cartographer_detect_changes",
        description = "Bring the index in line with the project's files: re-parses files that are new or whose content changed, and removes files that are no longer on disk. It compares content, not git status, so it also sees a pull, a checkout or a revert. Call this after file modifications."
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
        description = "Index every supported file in a project directory: walks the file tree, parses each file with Tree-sitter, and stores the import graph. Files whose content has not changed since the last index are not parsed again, and files no longer on disk are removed."
    )]
    fn index_project(
        &self,
        Parameters(input): Parameters<ProjectInput>,
    ) -> Result<CallToolResult, McpError> {
        let db = self.require_db()?;
        let (indexed, skipped) = full_index(db, &self.registry, &input.project, &input.project)
            .map_err(|e| McpError::internal_error(e, None))?;

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Indexed {indexed} files ({skipped} skipped)."
        ))]))
    }
}
