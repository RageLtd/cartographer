mod cli;
mod constants;
mod db;
mod grammars;
mod handler;
mod hooks;
mod indexer;
mod parse_cli;
mod parser;
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

    let parse_only_flag = args.iter().any(|a| a == "--parse-only");
    // Without the `surrealdb` feature there is no storage backend to connect to.
    let parse_only = parse_only_flag || !cfg!(feature = "surrealdb");

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
        if !parse_only_flag {
            tracing::info!(
                "Built without the `surrealdb` feature; parse-only mode is the only mode"
            );
        }
        tracing::info!("Cartographer MCP server starting in parse-only mode (no DB)");
        let server = CartographerServer::parse_only(registry);
        let service = server.serve(rmcp::transport::io::stdio()).await?;
        service.waiting().await?;
        return Ok(());
    }

    // Full mode: connect to SurrealDB for parsing + storage + queries.
    let db = connect().await.map_err(|e| {
        eprintln!("Failed to connect to SurrealDB: {e}");
        e
    })?;

    let server = CartographerServer::new(db, registry);

    tracing::info!("Cartographer MCP server starting on stdio (full mode)");

    let service = server.serve(rmcp::transport::io::stdio()).await?;
    service.waiting().await?;

    Ok(())
}
