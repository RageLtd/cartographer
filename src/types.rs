use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportEdge {
    pub source: String,
    pub target: String,
    pub specifier: String,
    pub symbols: Vec<String>,
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
