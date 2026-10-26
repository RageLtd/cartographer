use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::LazyLock;

pub const DEFAULT_MAX_DEPTH: i64 = 2;
pub const DEFAULT_MAX_RESULTS: i64 = 20;

pub const DATA_DIR_ENV: &str = "CARTOGRAPHER_HOME";
pub const DATA_DIR_NAME: &str = ".cartographer";
pub const GRAMMARS_DIR_NAME: &str = "grammars";

/// Cartographer's per-user data directory: `$CARTOGRAPHER_HOME`, else `~/.cartographer`.
/// `None` only when neither variable is set.
pub fn data_dir() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var(DATA_DIR_ENV) {
        if !dir.is_empty() {
            return Some(PathBuf::from(dir));
        }
    }
    std::env::var("HOME")
        .ok()
        .filter(|home| !home.is_empty())
        .map(|home| PathBuf::from(home).join(DATA_DIR_NAME))
}

/// Where host-provided tree-sitter grammars are discovered.
pub fn grammars_dir() -> Option<PathBuf> {
    data_dir().map(|dir| dir.join(GRAMMARS_DIR_NAME))
}

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
