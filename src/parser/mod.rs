pub mod dynamic;
pub mod elixir;
pub mod extractor;
#[cfg(test)]
pub mod fixtures;
pub mod generic;
pub mod manifest;
pub mod registry;
pub mod resolver;
pub mod ruby;
pub mod rust_ext;
pub mod ts_js;

use std::fs;

use md5::{Digest, Md5};

use crate::types::FileParseResult;

use self::elixir::extract_elixir;
use self::generic::extract_generic;
use self::registry::{extension_of, Extractor, LanguageRegistry};
use self::ruby::extract_ruby;
use self::rust_ext::extract_rust;
use self::ts_js::extract_ts_js;

pub fn parse_file(
    registry: &LanguageRegistry,
    file_path: &str,
    crate_root: Option<&str>,
) -> Result<FileParseResult, String> {
    let lang = registry
        .for_path(file_path)
        .ok_or_else(|| match extension_of(file_path) {
            Some(ext) => format!("Unsupported file extension: {ext}"),
            None => format!("No extension: {file_path}"),
        })?;

    let content =
        fs::read_to_string(file_path).map_err(|e| format!("Failed to read {file_path}: {e}"))?;

    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&lang.grammar)
        .map_err(|e| format!("Failed to set language {}: {e}", lang.name))?;

    let tree = parser
        .parse(&content, None)
        .ok_or_else(|| format!("Failed to parse {file_path}"))?;

    let root = tree.root_node();
    let (imports, symbols) = match &lang.extractor {
        Extractor::TsJs => extract_ts_js(file_path, &content, root, &lang.name),
        Extractor::Rust => extract_rust(file_path, &content, root, crate_root.unwrap_or(file_path)),
        Extractor::Ruby => extract_ruby(file_path, &content, root),
        Extractor::Elixir => extract_elixir(file_path, &content, root),
        Extractor::Generic(generic) => extract_generic(file_path, &content, root, generic),
    };

    Ok(FileParseResult {
        imports,
        symbols,
        language: lang.name.clone(),
    })
}

pub fn hash_content(content: &str) -> String {
    let mut hasher = Md5::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub fn hash_file(file_path: &str) -> Result<String, String> {
    let content =
        fs::read_to_string(file_path).map_err(|e| format!("Failed to read {file_path}: {e}"))?;
    Ok(hash_content(&content))
}
