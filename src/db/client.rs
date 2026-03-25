use surrealdb::engine::remote::ws::{Client, Ws};
use surrealdb::opt::auth::Root;
use surrealdb::Surreal;

use super::schema::init_schema;

pub type Db = Surreal<Client>;

pub async fn connect() -> Result<Db, String> {
    let url = std::env::var("SURREAL_URL").unwrap_or_else(|_| "localhost:8001".to_string());
    let user = std::env::var("SURREAL_USER").unwrap_or_else(|_| "root".to_string());
    let pass = std::env::var("SURREAL_PASS").unwrap_or_else(|_| "changeme".to_string());
    let ns = std::env::var("SURREAL_NS").unwrap_or_else(|_| "mimir".to_string());
    let database = std::env::var("SURREAL_DB").unwrap_or_else(|_| "mimir".to_string());

    let db = Surreal::new::<Ws>(&url)
        .await
        .map_err(|e| format!("Failed to connect to SurrealDB at {url}: {e}"))?;

    db.signin(Root {
        username: &user,
        password: &pass,
    })
    .await
    .map_err(|e| format!("Failed to authenticate with SurrealDB: {e}"))?;

    db.use_ns(&ns)
        .use_db(&database)
        .await
        .map_err(|e| format!("Failed to select namespace/database: {e}"))?;

    init_schema(&db).await?;

    tracing::info!("Connected to SurrealDB at {url} ({ns}/{database})");
    Ok(db)
}
