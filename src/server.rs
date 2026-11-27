//! MCP server state and tool router assembly.
//!
//! Tool handlers live in the `server/` submodules, grouped by concern; each
//! contributes a named router and they are summed here into the one the
//! `ServerHandler` impl (handler.rs) serves.

mod graph_tools;
mod index_tools;
mod parse_tools;

use std::sync::Arc;

use rmcp::handler::server::tool::ToolRouter;
use rmcp::ErrorData as McpError;

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

    /// Parse-only mode: no index. The server lists only the tools that work
    /// without one, so a caller can tell from the list what it can use.
    pub fn parse_only(registry: Arc<LanguageRegistry>) -> Self {
        Self {
            db: None,
            registry,
            tool_router: Self::parse_tools_router(),
        }
    }

    /// Every tool a server with an index exposes, assembled from the
    /// per-concern routers.
    pub fn tool_router() -> ToolRouter<Self> {
        Self::parse_tools_router() + Self::index_tools_router() + Self::graph_tools_router()
    }

    /// The index, or an MCP error on a parse-only server. A parse-only server
    /// does not list the tools that call this, so the error is for a caller
    /// that named one anyway.
    fn require_db(&self) -> Result<&Db, McpError> {
        self.db.as_ref().ok_or_else(|| {
            McpError::internal_error(
                "This tool needs the index, and this server was started with \
                 --parse-only, which does not open one.",
                None,
            )
        })
    }
}

#[cfg(test)]
#[path = "server_tests.rs"]
mod tests;
