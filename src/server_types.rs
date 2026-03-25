use rmcp::schemars;
use serde::Deserialize;

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
pub struct ParseFileInput {
    /// Absolute path to the file to parse
    pub file_path: String,
    /// Project root directory (absolute path)
    pub project: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
pub struct ProjectInput {
    /// Project root directory (absolute path)
    pub project: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
pub struct QueryInput {
    /// Project root directory (absolute path)
    pub project: String,
    /// File paths or search terms to start the graph walk from
    pub entry_points: Vec<String>,
    /// Maximum hops to traverse (1-5, default 2)
    pub max_depth: Option<i64>,
    /// Maximum files to return (1-50, default 20)
    pub max_results: Option<i64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
pub struct SearchInput {
    /// Project root directory (absolute path)
    pub project: String,
    /// Search query — matches file paths and symbol names via FTS5
    pub query: String,
    /// Maximum results to return (default 10)
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
pub struct FileInfoInput {
    /// Absolute path to the file
    pub file_path: String,
    /// Project root directory (absolute path)
    pub project: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
pub struct EmptyInput {}
