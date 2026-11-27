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
    /// Maximum hops to traverse (default 2)
    pub max_depth: Option<i64>,
    /// Maximum files to return (default 20)
    pub max_results: Option<i64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
pub struct SearchInput {
    /// Project root directory (absolute path)
    pub project: String,
    /// Search query, matched against file paths and symbol names. Every word must match.
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
pub struct FindUsesInput {
    /// Project root directory (absolute path)
    pub project: String,
    /// Absolute path to the file that defines the symbol
    pub file_path: String,
    /// The symbol's name as that file exports it; `default` for its default export
    pub symbol: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
pub struct UsesInFileInput {
    /// Absolute path to the file to look in
    pub file_path: String,
    /// Absolute path to the file that exports the symbol
    pub target_file: String,
    /// The symbol's name as the target file exports it; `default` for its default export
    pub symbol: String,
}
