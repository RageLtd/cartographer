//! Language registry: maps file extensions to a grammar and an extraction strategy.
//!
//! Built-in grammars are compiled into the binary. The registry is the single
//! place that decides which files Cartographer will index and how each one is
//! parsed, so host-provided grammars can be registered alongside the builtins
//! without touching the indexer or the server.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use tree_sitter::Language;

use super::generic::GenericLanguage;

/// Grammars statically linked into the binary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinLanguage {
    TypeScript,
    Tsx,
    JavaScript,
    Rust,
    Ruby,
    Elixir,
}

impl BuiltinLanguage {
    pub const ALL: [BuiltinLanguage; 6] = [
        Self::TypeScript,
        Self::Tsx,
        Self::JavaScript,
        Self::Rust,
        Self::Ruby,
        Self::Elixir,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::TypeScript => "typescript",
            Self::Tsx => "tsx",
            Self::JavaScript => "javascript",
            Self::Rust => "rust",
            Self::Ruby => "ruby",
            Self::Elixir => "elixir",
        }
    }

    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::TypeScript => &[".ts"],
            Self::Tsx => &[".tsx"],
            Self::JavaScript => &[".js", ".jsx", ".mjs", ".cjs"],
            Self::Rust => &[".rs"],
            Self::Ruby => &[".rb"],
            Self::Elixir => &[".ex", ".exs"],
        }
    }

    pub fn grammar(self) -> Language {
        match self {
            Self::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Self::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
            Self::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            Self::Rust => tree_sitter_rust::LANGUAGE.into(),
            Self::Ruby => tree_sitter_ruby::LANGUAGE.into(),
            Self::Elixir => tree_sitter_elixir::LANGUAGE.into(),
        }
    }

    pub fn extractor(self) -> Extractor {
        match self {
            Self::TypeScript | Self::Tsx | Self::JavaScript => Extractor::TsJs,
            Self::Rust => Extractor::Rust,
            Self::Ruby => Extractor::Ruby,
            Self::Elixir => Extractor::Elixir,
        }
    }
}

/// How imports and symbols are pulled out of a parsed tree.
#[derive(Debug, Clone)]
pub enum Extractor {
    TsJs,
    Rust,
    Ruby,
    Elixir,
    /// Query-driven extraction for a host-provided grammar.
    Generic(Arc<GenericLanguage>),
}

#[derive(Debug, Clone)]
pub struct LanguageDef {
    pub name: String,
    pub extensions: Vec<String>,
    pub grammar: Language,
    pub extractor: Extractor,
}

impl LanguageDef {
    pub fn builtin(lang: BuiltinLanguage) -> Self {
        Self {
            name: lang.name().to_string(),
            extensions: lang.extensions().iter().map(|e| e.to_string()).collect(),
            grammar: lang.grammar(),
            extractor: lang.extractor(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct LanguageRegistry {
    languages: Vec<LanguageDef>,
    by_extension: HashMap<String, usize>,
}

impl LanguageRegistry {
    pub fn empty() -> Self {
        Self::default()
    }

    /// Registry containing only the compiled-in grammars.
    pub fn builtin() -> Self {
        let mut registry = Self::empty();
        for lang in BuiltinLanguage::ALL {
            registry.register(LanguageDef::builtin(lang));
        }
        registry
    }

    /// Register a language. A later registration claims any extension it shares
    /// with an earlier one, so host grammars can deliberately override builtins.
    pub fn register(&mut self, def: LanguageDef) {
        let idx = self.languages.len();
        for ext in &def.extensions {
            self.by_extension.insert(ext.clone(), idx);
        }
        self.languages.push(def);
    }

    pub fn for_path(&self, path: &str) -> Option<&LanguageDef> {
        let ext = extension_of(path)?;
        self.by_extension
            .get(ext.as_str())
            .map(|&idx| &self.languages[idx])
    }

    pub fn supports_path(&self, path: &str) -> bool {
        self.for_path(path).is_some()
    }

    pub fn languages(&self) -> &[LanguageDef] {
        &self.languages
    }

    /// Extensions currently routed to the language called `name`, sorted. A
    /// language overridden on every extension by a later registration owns none.
    pub fn owned_extensions(&self, name: &str) -> Vec<String> {
        let mut owned: Vec<String> = self
            .by_extension
            .iter()
            .filter(|(_, &idx)| self.languages[idx].name == name)
            .map(|(ext, _)| ext.clone())
            .collect();
        owned.sort();
        owned
    }
}

impl LanguageDef {
    /// `true` for grammars loaded from the host rather than compiled in.
    pub fn is_host(&self) -> bool {
        matches!(self.extractor, Extractor::Generic(_))
    }
}

/// The dot-prefixed extension of a path (`".ts"`), or `None` if it has none.
pub fn extension_of(path: &str) -> Option<String> {
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| format!(".{e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_registry_maps_every_builtin_extension() {
        let registry = LanguageRegistry::builtin();
        for lang in BuiltinLanguage::ALL {
            for ext in lang.extensions() {
                let def = registry
                    .for_path(&format!("/src/file{ext}"))
                    .unwrap_or_else(|| panic!("{ext} should map to {}", lang.name()));
                assert_eq!(def.name, lang.name());
                assert_eq!(
                    std::mem::discriminant(&def.extractor),
                    std::mem::discriminant(&lang.extractor())
                );
            }
        }
        assert_eq!(registry.languages().len(), BuiltinLanguage::ALL.len());
    }

    #[test]
    fn unknown_or_missing_extension_is_unsupported() {
        let registry = LanguageRegistry::builtin();
        assert!(registry.for_path("/src/main.py").is_none());
        assert!(registry.for_path("/src/Makefile").is_none());
        assert!(!registry.supports_path("/src/main.py"));
    }

    #[test]
    fn extension_of_uses_the_last_segment() {
        assert_eq!(extension_of("/a/b.test.ts").as_deref(), Some(".ts"));
        assert_eq!(extension_of("/a/.hidden"), None);
        assert_eq!(extension_of("/a/Makefile"), None);
    }

    #[test]
    fn later_registration_overrides_a_shared_extension() {
        let mut registry = LanguageRegistry::builtin();
        registry.register(LanguageDef {
            name: "custom-rust".to_string(),
            extensions: vec![".rs".to_string()],
            grammar: BuiltinLanguage::Rust.grammar(),
            extractor: Extractor::Rust,
        });

        let def = registry
            .for_path("/src/lib.rs")
            .expect(".rs stays supported");
        assert_eq!(def.name, "custom-rust");
        assert_eq!(registry.languages().len(), BuiltinLanguage::ALL.len() + 1);
        // Unrelated extensions are untouched.
        assert_eq!(
            registry.for_path("/src/a.ts").map(|d| d.name.as_str()),
            Some("typescript")
        );
        // Ownership follows the override.
        assert_eq!(registry.owned_extensions("custom-rust"), vec![".rs"]);
        assert!(registry.owned_extensions("rust").is_empty());
        assert_eq!(
            registry.owned_extensions("javascript"),
            vec![".cjs", ".js", ".jsx", ".mjs"]
        );
        assert!(
            !def.is_host(),
            "a builtin extractor is never reported as host"
        );
    }

    #[test]
    fn every_builtin_grammar_is_loadable_by_a_parser() {
        for lang in BuiltinLanguage::ALL {
            let mut parser = tree_sitter::Parser::new();
            parser
                .set_language(&lang.grammar())
                .unwrap_or_else(|e| panic!("{} grammar rejected: {e}", lang.name()));
        }
    }
}
