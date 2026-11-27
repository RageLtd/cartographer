use rmcp::model::{
    AnnotateAble, ListResourcesResult, RawResource, ReadResourceRequestParams, ReadResourceResult,
    Resource, ResourceContents, ServerCapabilities, ServerInfo,
};
use rmcp::{tool_handler, ErrorData as McpError, ServerHandler};

use crate::db::queries::get_project_stats;
use crate::server::CartographerServer;

const PROJECT_RESOURCE: &str = "cartographer://project";

impl CartographerServer {
    /// The resources this server can serve. They all read the index, so a
    /// parse-only server has none.
    pub(crate) fn resources(&self) -> Vec<Resource> {
        if self.db.is_none() {
            return Vec::new();
        }
        vec![RawResource::new(PROJECT_RESOURCE, "Project Index Overview")
            .with_description("List all indexed projects and their file counts")
            .with_mime_type("application/json")
            .no_annotation()]
    }
}

#[tool_handler]
impl ServerHandler for CartographerServer {
    fn get_info(&self) -> ServerInfo {
        let capabilities = ServerCapabilities::builder()
            .enable_tools()
            .enable_resources()
            .build();

        ServerInfo::new(capabilities).with_instructions(
            "Cartographer: codebase structure mapping via Tree-sitter AST parsing",
        )
    }

    async fn list_resources(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParams>,
        _context: rmcp::service::RequestContext<rmcp::service::RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        Ok(ListResourcesResult::with_all_items(self.resources()))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: rmcp::service::RequestContext<rmcp::service::RoleServer>,
    ) -> Result<ReadResourceResult, McpError> {
        if request.uri == PROJECT_RESOURCE {
            let db = self.db.as_ref().ok_or_else(|| {
                McpError::internal_error("Resource not available in parse-only mode.", None)
            })?;
            let stats = get_project_stats(db).map_err(|e| McpError::internal_error(e, None))?;

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
                PROJECT_RESOURCE,
            )]))
        } else {
            Err(McpError::resource_not_found(
                "resource_not_found",
                Some(format!("Unknown resource: {}", request.uri).into()),
            ))
        }
    }
}
