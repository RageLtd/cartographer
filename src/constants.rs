use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::LazyLock;

pub const DEFAULT_MAX_DEPTH: i64 = 2;
pub const DEFAULT_MAX_RESULTS: i64 = 20;

pub fn data_dir() -> Result<PathBuf, std::env::VarError> {
    let home = std::env::var("HOME")?;
    Ok(PathBuf::from(home).join(".cartographer"))
}

pub fn default_db_path() -> Result<PathBuf, std::env::VarError> {
    Ok(data_dir()?.join("map.db"))
}

#[derive(Debug, Clone)]
pub struct LanguageConfig {
    pub language: &'static str,
}

pub static LANGUAGE_CONFIG: LazyLock<HashMap<&'static str, LanguageConfig>> = LazyLock::new(|| {
    let mut m = HashMap::new();
    m.insert(
        ".ts",
        LanguageConfig {
            language: "typescript",
        },
    );
    m.insert(".tsx", LanguageConfig { language: "tsx" });
    m.insert(
        ".js",
        LanguageConfig {
            language: "javascript",
        },
    );
    m.insert(
        ".jsx",
        LanguageConfig {
            language: "javascript",
        },
    );
    m.insert(
        ".mjs",
        LanguageConfig {
            language: "javascript",
        },
    );
    m.insert(
        ".cjs",
        LanguageConfig {
            language: "javascript",
        },
    );
    m.insert(".rs", LanguageConfig { language: "rust" });
    m.insert(".rb", LanguageConfig { language: "ruby" });
    m.insert(".ex", LanguageConfig { language: "elixir" });
    m.insert(".exs", LanguageConfig { language: "elixir" });
    m
});

pub static SUPPORTED_EXTENSIONS: LazyLock<HashSet<&'static str>> =
    LazyLock::new(|| LANGUAGE_CONFIG.keys().copied().collect());

pub static SKIP_DIRS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    [
        "node_modules",
        ".git",
        "dist",
        "build",
        ".next",
        ".nuxt",
        "coverage",
        ".turbo",
        ".cache",
        ".output",
        "target",
        "_build",
        "deps",
        ".elixir_ls",
        "vendor",
        ".bundle",
    ]
    .into_iter()
    .collect()
});
