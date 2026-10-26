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
use crate::indexer::full_index;
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

    // Auto-index CWD if it looks like a project directory.
    // Zed launches MCP servers with the project root as CWD.
    if let Ok(cwd) = std::env::current_dir() {
        let cwd_str = cwd.to_string_lossy().to_string();
        let home = std::env::var("HOME").unwrap_or_default();

        let is_project =
            cwd_str != "/" && cwd_str != home && !cwd_str.starts_with("/tmp") && cwd_str.len() > 1;

        if is_project {
            tracing::info!("Auto-indexing CWD: {cwd_str}");
            // The worker clones the whole server rather than `db` alone: without
            // the `surrealdb` feature `Db` is uninhabited, and a bare `db.clone()`
            // would make rustc treat everything after it as unreachable.
            let worker = server.clone();
            tokio::spawn(async move {
                let Some(db) = &worker.db else {
                    tracing::error!("Auto-index skipped: server started in full mode without a DB");
                    return;
                };
                match full_index(db, &worker.registry, &cwd_str, &cwd_str).await {
                    Ok((indexed, skipped)) => {
                        tracing::info!(
                            "Auto-index complete: {indexed} files indexed, {skipped} skipped"
                        );
                    }
                    Err(e) => {
                        tracing::error!("Auto-index failed: {e}");
                    }
                }
            });
        }
    }

    tracing::info!("Cartographer MCP server starting on stdio (full mode)");

    let service = server.serve(rmcp::transport::io::stdio()).await?;
    service.waiting().await?;

    Ok(())
}
