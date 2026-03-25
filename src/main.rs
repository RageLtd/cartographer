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
use crate::server::CartographerServer;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();

    if args.len() > 1 {
        cli::run(&args[1]);
        return Ok(());
    }

    // Default: run MCP server
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let db = connect().await.map_err(|e| {
        eprintln!("Failed to connect to SurrealDB: {e}");
        e
    })?;

    tracing::info!("Cartographer MCP server starting on stdio");

    let server = CartographerServer::new(db);
    let service = server.serve(rmcp::transport::io::stdio()).await?;
    service.waiting().await?;

    Ok(())
}
