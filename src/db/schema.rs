use super::client::Db;

pub async fn init_schema(db: &Db) -> Result<(), String> {
    db.query(
        "
        DEFINE ANALYZER IF NOT EXISTS cart_analyzer
            TOKENIZERS blank, class, punct
            FILTERS lowercase;

        DEFINE TABLE IF NOT EXISTS cart_file SCHEMAFULL;
        DEFINE FIELD IF NOT EXISTS project ON cart_file TYPE string;
        DEFINE FIELD IF NOT EXISTS file_path ON cart_file TYPE string;
        DEFINE FIELD IF NOT EXISTS language ON cart_file TYPE string;
        DEFINE FIELD IF NOT EXISTS symbols ON cart_file TYPE string;
        DEFINE FIELD IF NOT EXISTS symbol_names ON cart_file TYPE string;
        DEFINE FIELD IF NOT EXISTS content_hash ON cart_file TYPE string;
        DEFINE FIELD IF NOT EXISTS last_parsed_epoch ON cart_file TYPE int;
        DEFINE FIELD IF NOT EXISTS searchable ON cart_file TYPE string;
        DEFINE INDEX IF NOT EXISTS cart_file_unique ON cart_file FIELDS project, file_path UNIQUE;
        DEFINE INDEX IF NOT EXISTS cart_file_search ON cart_file FIELDS searchable
            FULLTEXT ANALYZER cart_analyzer BM25;

        DEFINE TABLE IF NOT EXISTS cart_import SCHEMAFULL;
        DEFINE FIELD IF NOT EXISTS project ON cart_import TYPE string;
        DEFINE FIELD IF NOT EXISTS source_path ON cart_import TYPE string;
        DEFINE FIELD IF NOT EXISTS target_path ON cart_import TYPE string;
        DEFINE FIELD IF NOT EXISTS specifier ON cart_import TYPE string;
        DEFINE FIELD IF NOT EXISTS symbols ON cart_import TYPE string;
        DEFINE FIELD IF NOT EXISTS updated_at_epoch ON cart_import TYPE int;
        DEFINE INDEX IF NOT EXISTS cart_import_source ON cart_import FIELDS project, source_path;
        DEFINE INDEX IF NOT EXISTS cart_import_target ON cart_import FIELDS project, target_path;
        DEFINE INDEX IF NOT EXISTS cart_import_edge ON cart_import FIELDS project, source_path, target_path, specifier UNIQUE;

        DEFINE TABLE IF NOT EXISTS cart_git_state SCHEMAFULL;
        DEFINE FIELD IF NOT EXISTS project ON cart_git_state TYPE string;
        DEFINE FIELD IF NOT EXISTS last_status ON cart_git_state TYPE string;
        DEFINE FIELD IF NOT EXISTS updated_at_epoch ON cart_git_state TYPE int;
        DEFINE INDEX IF NOT EXISTS cart_git_project ON cart_git_state FIELDS project UNIQUE;
        ",
    )
    .await
    .map_err(|e| format!("Failed to initialize Cartographer schema: {e}"))?;

    tracing::info!("Cartographer schema initialized");
    Ok(())
}
