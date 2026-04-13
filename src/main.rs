mod cli;
mod constants;
mod db;
mod handler;
mod hooks;
mod indexer;
mod parser;
mod server;
mod server_types;
mod types;

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
        cli::run(&args[1]);
        return Ok(());
    }

    let parse_only = args.iter().any(|a| a == "--parse-only");

    // MCP server mode
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    if parse_only {
        // Parse-only mode: no DB connection required. Parsing tools work,
        // query/search/stats tools return errors. mimir-acp handles storage.
        tracing::info!("Cartographer MCP server starting in parse-only mode (no DB)");
        let server = CartographerServer::parse_only();
        let service = server.serve(rmcp::transport::io::stdio()).await?;
        service.waiting().await?;
        return Ok(());
    }

    // Full mode: connect to SurrealDB for parsing + storage + queries.
    let db = connect().await.map_err(|e| {
        eprintln!("Failed to connect to SurrealDB: {e}");
        e
    })?;

    // Auto-index CWD if it looks like a project directory.
    // Zed launches MCP servers with the project root as CWD.
    if let Ok(cwd) = std::env::current_dir() {
        let cwd_str = cwd.to_string_lossy().to_string();
        let home = std::env::var("HOME").unwrap_or_default();

        let is_project =
            cwd_str != "/" && cwd_str != home && !cwd_str.starts_with("/tmp") && cwd_str.len() > 1;

        if is_project {
            tracing::info!("Auto-indexing CWD: {cwd_str}");
            let db_clone = db.clone();
            let project = cwd_str.clone();
            tokio::spawn(async move {
                match full_index(&db_clone, &project, &project).await {
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

    let server = CartographerServer::new(db);
    let service = server.serve(rmcp::transport::io::stdio()).await?;
    service.waiting().await?;

    Ok(())
}
