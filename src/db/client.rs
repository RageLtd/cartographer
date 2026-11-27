use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use rusqlite::{Connection, TransactionBehavior};

use super::schema::init_schema;
use crate::constants::{data_dir, DATA_DIR_ENV};

/// The store's file name inside the Cartographer data directory.
const STORE_FILE_NAME: &str = "cartographer.db";

/// How long a statement waits for a lock another process holds. The server
/// and short-lived hook processes open the same file.
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

const WAL: &str = "wal";

/// A handle on the store file. Clones share one connection.
#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

impl Db {
    /// Runs `f` against the connection; a failure is reported as
    /// `"{what}: {cause}"`.
    pub(super) fn read<T>(
        &self,
        what: &str,
        f: impl FnOnce(&Connection) -> rusqlite::Result<T>,
    ) -> Result<T, String> {
        let conn = self.lock(what)?;
        f(&conn).map_err(|e| format!("{what}: {e}"))
    }

    /// Runs `f` in one transaction that takes the write lock up front, so
    /// waiting on another process goes through the busy timeout instead of
    /// failing when a read transaction tries to upgrade.
    pub(super) fn write<T>(
        &self,
        what: &str,
        f: impl FnOnce(&Connection) -> rusqlite::Result<T>,
    ) -> Result<T, String> {
        let mut conn = self.lock(what)?;
        let run = || {
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let value = f(&tx)?;
            tx.commit()?;
            Ok(value)
        };
        run().map_err(|e: rusqlite::Error| format!("{what}: {e}"))
    }

    fn lock(&self, what: &str) -> Result<MutexGuard<'_, Connection>, String> {
        self.conn.lock().map_err(|_| {
            format!("{what}: the store connection is unusable after a panic while it was in use")
        })
    }
}

/// The store file's path under the Cartographer data directory.
pub fn store_path() -> Result<PathBuf, String> {
    let dir = data_dir().ok_or_else(|| {
        format!(
            "Cannot locate the Cartographer data directory: neither {DATA_DIR_ENV} nor HOME is set"
        )
    })?;
    Ok(dir.join(STORE_FILE_NAME))
}

/// Opens the store file under the Cartographer data directory.
pub fn connect() -> Result<Db, String> {
    let path = store_path()?;
    let db = open(&path)?;
    tracing::info!("Opened the Cartographer store at {}", path.display());
    Ok(db)
}

/// Opens (creating if needed) the store file at `path` and its directory.
pub fn open(path: &Path) -> Result<Db, String> {
    let shown = path.display();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| {
            format!(
                "Failed to create the store directory {}: {e}",
                dir.display()
            )
        })?;
    }

    let mut conn =
        Connection::open(path).map_err(|e| format!("Failed to open the store {shown}: {e}"))?;
    conn.busy_timeout(BUSY_TIMEOUT)
        .map_err(|e| format!("Failed to set a busy timeout on the store {shown}: {e}"))?;
    // WAL lets a hook process read while the server writes.
    let mode: String = conn
        .pragma_update_and_check(None, "journal_mode", WAL, |row| row.get(0))
        .map_err(|e| format!("Failed to switch the store {shown} to WAL mode: {e}"))?;
    if !mode.eq_ignore_ascii_case(WAL) {
        tracing::warn!("The store {shown} stayed in {mode} journal mode instead of WAL");
    }
    // The store is a cache of the index: NORMAL risks only the last commits
    // on power loss, and saves a sync on every one.
    conn.pragma_update(None, "synchronous", "NORMAL")
        .map_err(|e| format!("Failed to set synchronous mode on the store {shown}: {e}"))?;

    init_schema(&mut conn)?;
    Ok(Db {
        conn: Arc::new(Mutex::new(conn)),
    })
}

#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;
