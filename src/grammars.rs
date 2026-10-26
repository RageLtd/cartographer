//! Assembles the language registry (built-in plus host grammars) for the
//! server, and implements the `cartographer grammars` CLI subcommand that lists
//! and validates them.

use std::path::PathBuf;

use crate::constants::{grammars_dir, DATA_DIR_ENV};
use crate::parser::manifest::{register_host_grammars, GrammarLoadError};
use crate::parser::registry::LanguageRegistry;

pub struct RegistryReport {
    pub registry: LanguageRegistry,
    /// `None` when neither `CARTOGRAPHER_HOME` nor `HOME` is set.
    pub grammars_dir: Option<PathBuf>,
    pub failures: Vec<GrammarLoadError>,
}

/// Built-in grammars plus whatever is installed under the grammars directory.
/// A grammar that fails to load is recorded in `failures`, never fatal.
pub fn load_registry() -> RegistryReport {
    let mut registry = LanguageRegistry::builtin();
    let grammars_dir = grammars_dir();
    let failures = match &grammars_dir {
        Some(dir) => register_host_grammars(&mut registry, dir),
        None => Vec::new(),
    };
    RegistryReport {
        registry,
        grammars_dir,
        failures,
    }
}

/// Report registry assembly through tracing (for the MCP server, where stdout
/// is the protocol channel).
pub fn log_report(report: &RegistryReport) {
    let Some(dir) = &report.grammars_dir else {
        tracing::warn!("Neither {DATA_DIR_ENV} nor HOME is set; host grammars are unavailable");
        return;
    };
    for failure in &report.failures {
        tracing::warn!(
            "Skipping grammar {}: {}",
            failure.dir.display(),
            failure.message
        );
    }
    let host = report
        .registry
        .languages()
        .iter()
        .filter(|def| def.is_host())
        .count();
    if host > 0 {
        tracing::info!("Loaded {host} host grammar(s) from {}", dir.display());
    }
}

/// `cartographer grammars`: print every known language with the extensions it
/// owns, then any grammar directory that failed to load. Exits non-zero on
/// failures so it doubles as a validation step after installing a grammar.
pub fn run_cli(report: &RegistryReport) {
    match &report.grammars_dir {
        Some(dir) => println!("Grammars directory: {}", dir.display()),
        None => println!("Grammars directory: (unset — set {DATA_DIR_ENV} or HOME)"),
    }
    println!();
    println!("{:<16} {:<8} EXTENSIONS", "LANGUAGE", "SOURCE");
    for def in report.registry.languages() {
        let source = if def.is_host() { "host" } else { "builtin" };
        let owned = report.registry.owned_extensions(&def.name);
        let extensions = if owned.is_empty() {
            "(overridden by another grammar)".to_string()
        } else {
            owned.join(" ")
        };
        println!("{:<16} {:<8} {extensions}", def.name, source);
    }

    if report.failures.is_empty() {
        return;
    }
    eprintln!();
    eprintln!("{} grammar(s) failed to load:", report.failures.len());
    for failure in &report.failures {
        eprintln!("  {}", failure.dir.display());
        eprintln!("    {}", failure.message);
    }
    std::process::exit(1);
}
