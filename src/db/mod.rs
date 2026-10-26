//! Storage layer.
//!
//! With the `surrealdb` feature (on by default) this talks to SurrealDB over
//! WebSocket. Without it, `disabled` provides the same `client` / `queries` /
//! `graph` API over an uninhabited `Db`, so the rest of the crate compiles
//! unchanged and the server runs in parse-only mode.

#[cfg(feature = "surrealdb")]
pub mod client;
#[cfg(feature = "surrealdb")]
pub mod graph;
#[cfg(feature = "surrealdb")]
pub mod queries;
#[cfg(feature = "surrealdb")]
pub mod schema;

#[cfg(not(feature = "surrealdb"))]
mod disabled;
#[cfg(not(feature = "surrealdb"))]
pub use disabled::{client, graph, queries};
