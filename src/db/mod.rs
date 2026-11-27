//! Storage layer: an embedded SQLite file under the Cartographer data
//! directory, shared by the server and hook processes.

pub mod client;
pub mod cycles;
pub mod graph;
pub mod queries;
mod schema;
mod search_words;

#[cfg(test)]
mod fixtures_tests;
