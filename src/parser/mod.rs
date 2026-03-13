pub mod extractor;
pub mod resolver;

use std::fs;
use std::path::Path;

use md5::{Digest, Md5};

use crate::constants::LANGUAGE_CONFIG;
use crate::types::FileParseResult;

use self::extractor::{extract_rust, extract_ts_js};

pub fn parse_file(file_path: &str, crate_root: Option<&str>) -> Result<FileParseResult, String> {
    let ext = Path::new(file_path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| format!(".{e}"))
        .ok_or_else(|| format!("No extension: {file_path}"))?;

    let config = LANGUAGE_CONFIG
        .get(ext.as_str())
        .ok_or_else(|| format!("Unsupported file extension: {ext}"))?;

    let content = fs::read_to_string(file_path)
        .map_err(|e| format!("Failed to read {file_path}: {e}"))?;

    let language_fn: tree_sitter::Language = match config.language {
        "typescript" => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        "tsx" => tree_sitter_typescript::LANGUAGE_TSX.into(),
        "javascript" => tree_sitter_javascript::LANGUAGE.into(),
        "rust" => tree_sitter_rust::LANGUAGE.into(),
        other => return Err(format!("Unknown language: {other}")),
    };

    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&language_fn)
        .map_err(|e| format!("Failed to set language: {e}"))?;

    let tree = parser
        .parse(&content, None)
        .ok_or_else(|| format!("Failed to parse {file_path}"))?;

    let (imports, symbols) = if config.language == "rust" {
        extract_rust(file_path, &content, tree.root_node(), crate_root.unwrap_or(file_path))
    } else {
        extract_ts_js(file_path, &content, tree.root_node(), config.language)
    };

    Ok(FileParseResult {
        imports,
        symbols,
        language: config.language.to_string(),
    })
}

pub fn hash_content(content: &str) -> String {
    let mut hasher = Md5::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub fn hash_file(file_path: &str) -> Result<String, String> {
    let content = fs::read_to_string(file_path)
        .map_err(|e| format!("Failed to read {file_path}: {e}"))?;
    Ok(hash_content(&content))
}
