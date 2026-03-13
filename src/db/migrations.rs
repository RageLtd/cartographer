use rusqlite::Connection;

struct Migration {
    version: i64,
    description: &'static str,
    up: fn(&Connection) -> rusqlite::Result<()>,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        description: "Create files and imports tables",
        up: |db| {
            db.execute_batch(
                "CREATE TABLE IF NOT EXISTS files (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    project TEXT NOT NULL,
                    file_path TEXT NOT NULL,
                    language TEXT NOT NULL,
                    symbols TEXT NOT NULL DEFAULT '[]',
                    last_parsed_epoch INTEGER NOT NULL,
                    content_hash TEXT NOT NULL,
                    UNIQUE(project, file_path)
                );

                CREATE TABLE IF NOT EXISTS imports (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    project TEXT NOT NULL,
                    source_path TEXT NOT NULL,
                    target_path TEXT NOT NULL,
                    specifier TEXT NOT NULL,
                    symbols TEXT NOT NULL DEFAULT '[]',
                    updated_at_epoch INTEGER NOT NULL
                );

                CREATE TABLE IF NOT EXISTS git_state (
                    project TEXT PRIMARY KEY,
                    last_status TEXT NOT NULL DEFAULT '{}',
                    updated_at_epoch INTEGER NOT NULL
                );",
            )
        },
    },
    Migration {
        version: 2,
        description: "Create indexes for graph queries",
        up: |db| {
            db.execute_batch(
                "CREATE INDEX IF NOT EXISTS idx_files_project_path ON files(project, file_path);
                 CREATE INDEX IF NOT EXISTS idx_files_project ON files(project);
                 CREATE INDEX IF NOT EXISTS idx_imports_source ON imports(project, source_path);
                 CREATE INDEX IF NOT EXISTS idx_imports_target ON imports(project, target_path);
                 CREATE UNIQUE INDEX IF NOT EXISTS idx_imports_edge ON imports(project, source_path, target_path, specifier);",
            )
        },
    },
    Migration {
        version: 3,
        description: "Create FTS5 for file path and export search",
        up: |db| {
            db.execute_batch(
                "CREATE VIRTUAL TABLE IF NOT EXISTS files_fts USING fts5(
                    file_path,
                    symbols,
                    content='files',
                    content_rowid='id'
                );

                CREATE TRIGGER IF NOT EXISTS files_fts_ai AFTER INSERT ON files BEGIN
                    INSERT INTO files_fts(rowid, file_path, symbols)
                    VALUES (new.id, new.file_path, new.symbols);
                END;

                CREATE TRIGGER IF NOT EXISTS files_fts_ad AFTER DELETE ON files BEGIN
                    INSERT INTO files_fts(files_fts, rowid, file_path, symbols)
                    VALUES ('delete', old.id, old.file_path, old.symbols);
                END;

                CREATE TRIGGER IF NOT EXISTS files_fts_au AFTER UPDATE ON files BEGIN
                    INSERT INTO files_fts(files_fts, rowid, file_path, symbols)
                    VALUES ('delete', old.id, old.file_path, old.symbols);
                    INSERT INTO files_fts(rowid, file_path, symbols)
                    VALUES (new.id, new.file_path, new.symbols);
                END;",
            )
        },
    },
    Migration {
        version: 4,
        description: "Add symbol_names column for clean FTS indexing",
        up: |db| {
            // Add a plain-text column with space-separated symbol names
            db.execute_batch(
                "ALTER TABLE files ADD COLUMN symbol_names TEXT NOT NULL DEFAULT '';",
            )?;

            // Backfill existing rows: extract symbol names from JSON
            // json_each requires the json1 extension which is bundled with rusqlite
            db.execute_batch(
                "UPDATE files SET symbol_names = COALESCE(
                    (SELECT GROUP_CONCAT(json_extract(value, '$.name'), ' ')
                     FROM json_each(files.symbols)),
                    ''
                );",
            )?;

            // Drop old FTS table and triggers, recreate with symbol_names
            db.execute_batch(
                "DROP TRIGGER IF EXISTS files_fts_ai;
                 DROP TRIGGER IF EXISTS files_fts_ad;
                 DROP TRIGGER IF EXISTS files_fts_au;
                 DROP TABLE IF EXISTS files_fts;

                 CREATE VIRTUAL TABLE files_fts USING fts5(
                     file_path,
                     symbol_names,
                     content='files',
                     content_rowid='id'
                 );

                 -- Rebuild FTS from existing data
                 INSERT INTO files_fts(rowid, file_path, symbol_names)
                 SELECT id, file_path, symbol_names FROM files;

                 CREATE TRIGGER files_fts_ai AFTER INSERT ON files BEGIN
                     INSERT INTO files_fts(rowid, file_path, symbol_names)
                     VALUES (new.id, new.file_path, new.symbol_names);
                 END;

                 CREATE TRIGGER files_fts_ad AFTER DELETE ON files BEGIN
                     INSERT INTO files_fts(files_fts, rowid, file_path, symbol_names)
                     VALUES ('delete', old.id, old.file_path, old.symbol_names);
                 END;

                 CREATE TRIGGER files_fts_au AFTER UPDATE ON files BEGIN
                     INSERT INTO files_fts(files_fts, rowid, file_path, symbol_names)
                     VALUES ('delete', old.id, old.file_path, old.symbol_names);
                     INSERT INTO files_fts(rowid, file_path, symbol_names)
                     VALUES (new.id, new.file_path, new.symbol_names);
                 END;",
            )
        },
    },
];

pub fn run_migrations(db: &Connection) -> rusqlite::Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS migrations (
            version INTEGER PRIMARY KEY,
            applied_at TEXT NOT NULL
        )",
    )?;

    let current_version: i64 = db
        .query_row("SELECT COALESCE(MAX(version), 0) FROM migrations", [], |row| {
            row.get(0)
        })?;

    for migration in MIGRATIONS {
        if migration.version > current_version {
            tracing::info!("Running migration {}: {}", migration.version, migration.description);
            let tx = db.unchecked_transaction()?;
            (migration.up)(&tx)?;
            tx.execute(
                "INSERT INTO migrations (version, applied_at) VALUES (?1, datetime('now'))",
                [migration.version],
            )?;
            tx.commit()?;
        }
    }

    Ok(())
}
