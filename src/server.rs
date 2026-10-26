//! MCP server state and tool router assembly.
//!
//! Tool handlers live in the `server/` submodules, grouped by concern; each
//! contributes a named router and they are summed here into the one the
//! `ServerHandler` impl (handler.rs) serves.

mod graph_tools;
mod parse_tools;
mod project_tools;

use std::sync::Arc;

use rmcp::handler::server::tool::ToolRouter;
use rmcp::ErrorData as McpError;
use tokio::runtime::Handle;

use crate::db::client::Db;
use crate::parser::registry::LanguageRegistry;

#[derive(Clone)]
pub struct CartographerServer {
    pub(crate) db: Option<Db>,
    pub(crate) registry: Arc<LanguageRegistry>,
    pub(crate) tool_router: ToolRouter<Self>,
}

impl CartographerServer {
    /// Full mode: DB connected, all tools available.
    pub fn new(db: Db, registry: Arc<LanguageRegistry>) -> Self {
        Self {
            db: Some(db),
            registry,
            tool_router: Self::tool_router(),
        }
    }

    /// Parse-only mode: no DB, only parsing tools work.
    /// Query/search/stats tools return errors guiding the caller
    /// to use the server-side equivalents.
    pub fn parse_only(registry: Arc<LanguageRegistry>) -> Self {
        Self {
            db: None,
            registry,
            tool_router: Self::tool_router(),
        }
    }

    /// Every tool the server exposes, assembled from the per-concern routers.
    pub fn tool_router() -> ToolRouter<Self> {
        Self::parse_tools_router() + Self::graph_tools_router() + Self::project_tools_router()
    }

    /// Get the DB reference, or return an MCP error if in parse-only mode.
    fn require_db(&self) -> Result<&Db, McpError> {
        self.db.as_ref().ok_or_else(|| {
            McpError::internal_error(
                "This tool requires a database connection. In parse-only mode, \
                 use the server-side equivalent (cartographer_query, \
                 cartographer_search, etc. on mimir-server).",
                None,
            )
        })
    }

    /// Bridge sync tool handlers to async DB calls
    fn run_async<F: std::future::Future>(&self, fut: F) -> F::Output {
        tokio::task::block_in_place(|| Handle::current().block_on(fut))
    }
}
