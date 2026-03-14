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

use std::fs;

use rmcp::ServiceExt;
use tracing_subscriber::EnvFilter;

use crate::constants::{data_dir, default_db_path};
use crate::db::setup::create_database;
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

    fs::create_dir_all(data_dir()?)?;

    let db_path = default_db_path()?;
    let db_path_str = db_path
        .to_str()
        .ok_or("Database path contains non-UTF-8 characters")?;
    let db = create_database(db_path_str)?;

    tracing::info!("Cartographer MCP server starting on stdio");

    let server = CartographerServer::new(db);
    let service = server.serve(rmcp::transport::io::stdio()).await?;
    service.waiting().await?;

    Ok(())
}
