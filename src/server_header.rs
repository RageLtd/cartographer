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
use crate::server_types::{EmptyInput, FileInfoInput, ParseFileInput, ProjectInput, QueryInput, SearchInput};