mod cli;
mod constants;
mod db;
mod grammars;
mod handler;
mod hooks;
mod indexer;
mod parse_cli;
mod parser;
#[cfg(test)]
mod project_fixture_tests;
mod server;
mod server_types;
mod types;

use std::sync::Arc;

use rmcp::ServiceExt;
use tracing_subscriber::EnvFilter;

use crate::db::client::connect;
use crate::server::CartographerServer;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();

    // CLI hook subcommands (hook:context, hook:prompt, etc.)
    if args.len() > 1 && !args[1].starts_with("--") {
        cli::run(&args[1..]);
        return Ok(());
    }

    let parse_only = args.iter().any(|a| a == "--parse-only");

    // MCP server mode
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let report = grammars::load_registry();
    grammars::log_report(&report);
    let registry = Arc::new(report.registry);

    if parse_only {
        // Parse-only mode: no DB connection required. Parsing tools work,
        // query/search/stats tools return errors. mimir-acp handles storage.
        tracing::info!("Cartographer MCP server starting in parse-only mode (no DB)");
        let server = CartographerServer::parse_only(registry);
        let service = server.serve(rmcp::transport::io::stdio()).await?;
        service.waiting().await?;
        return Ok(());
    }

    // Full mode: open the store for parsing + storage + queries.
    let db = connect().map_err(|e| {
        eprintln!("Failed to open the Cartographer store: {e}");
        e
    })?;

    let server = CartographerServer::new(db, registry);

    tracing::info!("Cartographer MCP server starting on stdio (full mode)");

    let service = server.serve(rmcp::transport::io::stdio()).await?;
    service.waiting().await?;

    Ok(())
}
