use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportEdge {
    pub source: String,
    pub target: String,
    pub specifier: String,
    pub symbols: Vec<String>,
    pub kind: EdgeKind,
}

/// What an edge from one file to another says about them.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    /// The source uses something the target defines.
    Import,
    /// The source says where the target lives without using it: Rust's
    /// `mod x;`, which a parent must write for its child module to exist.
    ModuleDeclaration,
}

impl EdgeKind {
    /// Whether the source depends on the target's code. A cycle of edges that
    /// are not dependencies is not a dependency cycle.
    pub fn is_dependency(self) -> bool {
        match self {
            EdgeKind::Import => true,
            EdgeKind::ModuleDeclaration => false,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SymbolKind {
    Function,
    Class,
    Interface,
    Type,
    Enum,
    Const,
    Let,
    Var,
    Struct,
    Trait,
    Impl,
    Macro,
    Module,
    Unknown,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    Exported,
    DefaultExport,
    Public,
    PubCrate,
    PubSuper,
    Private,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    pub signature: String,
    pub doc_comment: Option<String>,
    pub visibility: Visibility,
    pub line: usize,
}

#[derive(Debug, Clone)]
pub struct FileParseResult {
    pub imports: Vec<ImportEdge>,
    pub symbols: Vec<Symbol>,
    pub language: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelevantFile {
    pub file_path: String,
    pub relative_path: String,
    pub reason: String,
    pub depth: i64,
    pub symbols: Vec<Symbol>,
}
