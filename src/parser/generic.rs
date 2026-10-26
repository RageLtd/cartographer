//! Query-driven import and symbol extraction for host-provided grammars.
//!
//! Symbols come from a `tags.scm` query following tree-sitter's code-navigation
//! convention: a `@definition.<kind>` capture on the defining node, an inner
//! `@name`, and optionally `@doc` comment nodes refined by the `#strip!` and
//! `#select-adjacent!` directives. Cartographer adds one optional capture,
//! `@visibility`, for the node that marks a definition public.
//!
//! Imports come from an `imports.scm` query using Cartographer's own captures:
//! `@import.specifier` on the module/path node and zero or more `@import.symbol`
//! captures for the names imported. Specifiers are resolved to files with the
//! manifest's [`ResolverStrategy`] list.

use std::fmt;

use regex::Regex;
use tree_sitter::{Language, Node, Query, QueryCursor, QueryPredicateArg, StreamingIterator};

use crate::types::{ImportEdge, Symbol, SymbolKind, Visibility};

use super::extractor::{get_signature, strip_quotes};
use super::resolver::{resolve_with_strategies, ResolverStrategy};

pub const CAPTURE_DEFINITION_PREFIX: &str = "definition.";
pub const CAPTURE_NAME: &str = "name";
pub const CAPTURE_DOC: &str = "doc";
pub const CAPTURE_VISIBILITY: &str = "visibility";
pub const CAPTURE_IMPORT_SPECIFIER: &str = "import.specifier";
pub const CAPTURE_IMPORT_SYMBOL: &str = "import.symbol";

const DIRECTIVE_STRIP: &str = "strip!";
const DIRECTIVE_SELECT_ADJACENT: &str = "select-adjacent!";

/// Everything needed to extract from one host-provided language.
pub struct GenericLanguage {
    tags: Option<TagsQuery>,
    imports: Option<Query>,
    resolvers: Vec<ResolverStrategy>,
    default_visibility: Visibility,
}

struct TagsQuery {
    query: Query,
    /// Directives per pattern index.
    directives: Vec<PatternDirectives>,
    has_visibility_capture: bool,
}

#[derive(Default)]
struct PatternDirectives {
    /// `(#strip! @capture "regex")` — text matched by the regex is removed.
    strip: Vec<(u32, Regex)>,
    /// `(#select-adjacent! @doc @definition)` — capture indexes whose nodes
    /// must sit directly above the definition to count.
    adjacent_to_definition: Vec<u32>,
}

impl fmt::Debug for GenericLanguage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GenericLanguage")
            .field("tags", &self.tags.is_some())
            .field("imports", &self.imports.is_some())
            .field("resolvers", &self.resolvers)
            .field("default_visibility", &self.default_visibility)
            .finish()
    }
}

impl GenericLanguage {
    pub fn new(
        language: &Language,
        tags_source: Option<&str>,
        imports_source: Option<&str>,
        resolvers: Vec<ResolverStrategy>,
        default_visibility: Visibility,
    ) -> Result<Self, String> {
        let tags = tags_source
            .map(|src| compile_tags_query(language, src))
            .transpose()?;
        let imports = imports_source
            .map(|src| Query::new(language, src).map_err(|e| format!("imports query: {e}")))
            .transpose()?;
        Ok(Self {
            tags,
            imports,
            resolvers,
            default_visibility,
        })
    }
}

fn compile_tags_query(language: &Language, source: &str) -> Result<TagsQuery, String> {
    let query = Query::new(language, source).map_err(|e| format!("tags query: {e}"))?;
    let has_visibility_capture = query.capture_names().contains(&CAPTURE_VISIBILITY);

    let mut directives = Vec::with_capacity(query.pattern_count());
    for pattern in 0..query.pattern_count() {
        let mut parsed = PatternDirectives::default();
        for predicate in query.general_predicates(pattern) {
            match (predicate.operator.as_ref(), predicate.args.as_ref()) {
                (
                    DIRECTIVE_STRIP,
                    [QueryPredicateArg::Capture(capture), QueryPredicateArg::String(pattern_src)],
                ) => {
                    let regex = Regex::new(pattern_src).map_err(|e| {
                        format!("tags query: invalid #strip! pattern {pattern_src:?}: {e}")
                    })?;
                    parsed.strip.push((*capture, regex));
                }
                (
                    DIRECTIVE_SELECT_ADJACENT,
                    [QueryPredicateArg::Capture(doc), QueryPredicateArg::Capture(_)],
                ) => parsed.adjacent_to_definition.push(*doc),
                (other, _) => tracing::warn!(
                    "tags query pattern {pattern}: ignoring unsupported directive #{other}"
                ),
            }
        }
        directives.push(parsed);
    }

    Ok(TagsQuery {
        query,
        directives,
        has_visibility_capture,
    })
}

pub fn extract_generic(
    file_path: &str,
    source: &str,
    root: Node,
    lang: &GenericLanguage,
) -> (Vec<ImportEdge>, Vec<Symbol>) {
    let symbols = lang
        .tags
        .as_ref()
        .map(|tags| extract_symbols(source, root, tags, lang.default_visibility))
        .unwrap_or_default();
    let imports = lang
        .imports
        .as_ref()
        .map(|query| extract_imports(file_path, source, root, query, &lang.resolvers))
        .unwrap_or_default();
    (imports, symbols)
}

/// The captures of one tags-query match, sorted into their roles.
#[derive(Default)]
struct TagMatch<'tree> {
    definition: Option<(Node<'tree>, SymbolKind)>,
    name: Option<Node<'tree>>,
    visibility: Option<Node<'tree>>,
    docs: Vec<(u32, Node<'tree>)>,
}

fn extract_symbols(
    source: &str,
    root: Node,
    tags: &TagsQuery,
    default_visibility: Visibility,
) -> Vec<Symbol> {
    let bytes = source.as_bytes();
    let names = tags.query.capture_names();
    let mut symbols: Vec<Symbol> = Vec::new();

    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&tags.query, root, bytes);
    while let Some(m) = matches.next() {
        let directives = &tags.directives[m.pattern_index];
        let mut tag = TagMatch::default();

        for capture in m.captures() {
            match names[capture.index as usize] {
                CAPTURE_NAME => tag.name = Some(capture.node),
                CAPTURE_VISIBILITY => tag.visibility = Some(capture.node),
                CAPTURE_DOC => tag.docs.push((capture.index, capture.node)),
                other => {
                    if let Some(kind) = other.strip_prefix(CAPTURE_DEFINITION_PREFIX) {
                        tag.definition = Some((capture.node, kind_from_tag(kind)));
                    }
                }
            }
        }

        // `@reference.*` patterns and malformed matches carry no definition.
        let (Some((def_node, kind)), Some(name_node)) = (tag.definition, tag.name) else {
            continue;
        };
        let Ok(name) = name_node.utf8_text(bytes) else {
            continue;
        };
        let line = def_node.start_position().row + 1;
        // `@definition.method` and `@definition.function` patterns routinely
        // both match the same node; the first match wins.
        if symbols.iter().any(|s| s.line == line && s.name == name) {
            continue;
        }

        let visibility = match (tags.has_visibility_capture, tag.visibility) {
            (true, Some(node)) => visibility_from_text(node.utf8_text(bytes).unwrap_or("")),
            (true, None) => Visibility::Private,
            (false, _) => default_visibility,
        };

        symbols.push(Symbol {
            name: name.to_string(),
            kind,
            signature: get_signature(&def_node, bytes),
            doc_comment: build_doc(bytes, tag.docs, directives, &def_node),
            visibility,
            line,
        });
    }

    symbols
}

fn build_doc(
    bytes: &[u8],
    mut docs: Vec<(u32, Node)>,
    directives: &PatternDirectives,
    definition: &Node,
) -> Option<String> {
    if docs.is_empty() {
        return None;
    }
    docs.sort_by_key(|(_, node)| node.start_byte());

    let requires_adjacency = docs
        .iter()
        .any(|(index, _)| directives.adjacent_to_definition.contains(index));
    if requires_adjacency {
        // Walk upward from the definition; a comment counts only while there
        // is no blank line between it and whatever sits below it.
        let mut kept = Vec::new();
        let mut next_start_row = definition.start_position().row;
        for entry in docs.iter().rev() {
            if entry.1.end_position().row + 1 < next_start_row {
                break;
            }
            next_start_row = entry.1.start_position().row;
            kept.push(*entry);
        }
        kept.reverse();
        docs = kept;
    }

    let lines: Vec<String> = docs
        .iter()
        .filter_map(|(index, node)| {
            let raw = node.utf8_text(bytes).ok()?;
            let stripped = directives
                .strip
                .iter()
                .filter(|(capture, _)| capture == index)
                .fold(raw.to_string(), |text, (_, regex)| {
                    regex.replace_all(&text, "").into_owned()
                });
            let trimmed = stripped.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        })
        .collect();

    (!lines.is_empty()).then(|| lines.join("\n"))
}

fn extract_imports(
    file_path: &str,
    source: &str,
    root: Node,
    query: &Query,
    resolvers: &[ResolverStrategy],
) -> Vec<ImportEdge> {
    let bytes = source.as_bytes();
    let names = query.capture_names();
    // Keyed by the specifier node's byte offset: an unquantified
    // `@import.symbol` capture makes tree-sitter report one match per symbol
    // for the same statement, and those must fold into one edge.
    let mut imports: Vec<(usize, ImportEdge)> = Vec::new();

    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(query, root, bytes);
    while let Some(m) = matches.next() {
        let mut specifier_node: Option<Node> = None;
        let mut symbols: Vec<String> = Vec::new();
        for capture in m.captures() {
            match names[capture.index as usize] {
                CAPTURE_IMPORT_SPECIFIER => specifier_node = Some(capture.node),
                CAPTURE_IMPORT_SYMBOL => {
                    if let Ok(text) = capture.node.utf8_text(bytes) {
                        symbols.push(text.to_string());
                    }
                }
                _ => {}
            }
        }

        let Some(specifier_node) = specifier_node else {
            continue;
        };
        let key = specifier_node.start_byte();
        if let Some((_, existing)) = imports.iter_mut().find(|(k, _)| *k == key) {
            for symbol in symbols {
                if !existing.symbols.contains(&symbol) {
                    existing.symbols.push(symbol);
                }
            }
            continue;
        }

        let Ok(raw_specifier) = specifier_node.utf8_text(bytes) else {
            continue;
        };
        let specifier = strip_quotes(raw_specifier).to_string();
        let Some(resolved) = resolve_with_strategies(resolvers, &specifier, file_path) else {
            continue;
        };
        if symbols.is_empty() {
            symbols.extend(resolved.trailing_item);
        }

        imports.push((
            key,
            ImportEdge {
                source: file_path.to_string(),
                target: resolved.path,
                specifier,
                symbols,
            },
        ));
    }

    imports.into_iter().map(|(_, edge)| edge).collect()
}

/// Map a `@definition.<tag>` suffix onto Cartographer's symbol kinds. Tags are
/// an open vocabulary, so anything unrecognised is `Unknown` rather than dropped.
fn kind_from_tag(tag: &str) -> SymbolKind {
    match tag {
        "function" | "method" => SymbolKind::Function,
        "class" => SymbolKind::Class,
        "interface" => SymbolKind::Interface,
        "module" => SymbolKind::Module,
        "type" => SymbolKind::Type,
        "enum" => SymbolKind::Enum,
        "struct" => SymbolKind::Struct,
        "trait" => SymbolKind::Trait,
        "macro" => SymbolKind::Macro,
        "constant" => SymbolKind::Const,
        _ => SymbolKind::Unknown,
    }
}

fn visibility_from_text(text: &str) -> Visibility {
    match text.trim() {
        t if t.contains("crate") => Visibility::PubCrate,
        t if t.contains("super") => Visibility::PubSuper,
        t if t.starts_with("export") => Visibility::Exported,
        _ => Visibility::Public,
    }
}

#[cfg(test)]
#[path = "generic_tests.rs"]
mod tests;
