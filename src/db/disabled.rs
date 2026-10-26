//! Stand-ins for the storage API when the `surrealdb` feature is off.
//!
//! `Db` is an uninhabited enum: no value of it can ever exist. `connect()`
//! therefore always fails, the server stays in parse-only mode, and every
//! function taking `&Db` is provably unreachable (`match *db {}`). The stubs
//! exist only so callers type-check without `#[cfg]` scattered through them —
//! they carry no behaviour of their own.

use std::collections::HashMap;

use crate::types::{ImportEdge, RelevantFile, Symbol};

pub mod client {
    pub const STORAGE_DISABLED: &str = "Cartographer was built without the `surrealdb` feature; \
         storage, graph and search tools are unavailable. Rebuild with default features to enable them.";

    /// Uninhabited: a `Db` cannot be constructed in this configuration.
    #[derive(Debug, Clone)]
    pub enum Db {}

    pub async fn connect() -> Result<Db, String> {
        Err(STORAGE_DISABLED.to_string())
    }
}

pub mod queries {
    use super::client::Db;
    use super::*;

    pub async fn upsert_file(
        db: &Db,
        _project: &str,
        _file_path: &str,
        _language: &str,
        _symbols: &[Symbol],
        _content_hash: &str,
    ) -> Result<(), String> {
        match *db {}
    }

    pub async fn get_file_hash(
        db: &Db,
        _project: &str,
        _file_path: &str,
    ) -> Result<Option<String>, String> {
        match *db {}
    }

    pub async fn remove_file(db: &Db, _project: &str, _file_path: &str) -> Result<(), String> {
        match *db {}
    }

    pub async fn replace_imports(
        db: &Db,
        _project: &str,
        _source_path: &str,
        _edges: &[ImportEdge],
    ) -> Result<(), String> {
        match *db {}
    }

    pub async fn find_files_by_suffix(
        db: &Db,
        _project: &str,
        _suffix: &str,
        _limit: i64,
    ) -> Result<Vec<String>, String> {
        match *db {}
    }

    pub async fn get_file_count(db: &Db, _project: &str) -> Result<i64, String> {
        match *db {}
    }

    pub async fn get_language_counts(
        db: &Db,
        _project: &str,
    ) -> Result<HashMap<String, usize>, String> {
        match *db {}
    }

    pub async fn get_import_count(db: &Db, _project: &str) -> Result<i64, String> {
        match *db {}
    }

    pub async fn get_project_stats(db: &Db) -> Result<Vec<(String, i64)>, String> {
        match *db {}
    }

    pub async fn get_last_git_status(
        db: &Db,
        _project: &str,
    ) -> Result<HashMap<String, String>, String> {
        match *db {}
    }

    pub async fn save_git_status(
        db: &Db,
        _project: &str,
        _status: &HashMap<String, String>,
    ) -> Result<(), String> {
        match *db {}
    }
}

pub mod graph {
    use super::client::Db;
    use super::*;

    pub struct FileDetail {
        pub file_path: String,
        pub language: String,
        pub symbols: Vec<Symbol>,
        pub imports: Vec<(String, Vec<String>)>,
        pub dependents: Vec<(String, Vec<String>)>,
    }

    pub async fn walk_import_graph(
        db: &Db,
        _project: &str,
        _entry_points: &[String],
        _max_depth: Option<i64>,
        _max_results: Option<i64>,
    ) -> Result<Vec<RelevantFile>, String> {
        match *db {}
    }

    pub async fn search_files(
        db: &Db,
        _project: &str,
        _query: &str,
        _limit: i64,
    ) -> Result<Vec<(String, Vec<Symbol>)>, String> {
        match *db {}
    }

    pub async fn find_cycles(db: &Db, _project: &str) -> Result<Vec<Vec<String>>, String> {
        match *db {}
    }

    pub async fn get_file_detail(
        db: &Db,
        _project: &str,
        _file_path: &str,
    ) -> Result<Option<FileDetail>, String> {
        match *db {}
    }
}
