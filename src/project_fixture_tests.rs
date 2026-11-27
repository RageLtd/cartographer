//! A small TypeScript and JavaScript project on disk beside an index store,
//! both in one temporary directory. Shared by the indexer and hook tests.
//!
//! The import graph, with `server` and `router` forming the one cycle:
//!
//! ```text
//! main.ts   -> server.ts, util.js
//! server.ts -> router.ts
//! router.ts -> server.ts, util.js
//! ```

use std::fs;
use std::path::PathBuf;

use tempfile::TempDir;

use crate::db::client::Db;
use crate::indexer::full_index;
use crate::parser::registry::LanguageRegistry;

pub const MAIN: &str = "src/main.ts";
pub const SERVER: &str = "src/server.ts";
pub const ROUTER: &str = "src/router.ts";
pub const UTIL: &str = "src/util.js";

pub const FILE_COUNT: usize = 4;
pub const IMPORT_COUNT: i64 = 5;

const SOURCES: [(&str, &str); FILE_COUNT] = [
    (
        MAIN,
        "import { startServer } from \"./server\";\n\
         import { formatPath } from \"./util\";\n\
         \n\
         export function main() {\n  startServer(formatPath(\"/\"));\n}\n",
    ),
    (
        SERVER,
        "import { route } from \"./router\";\n\
         \n\
         export function startServer(root) {\n  route(root);\n}\n",
    ),
    (
        ROUTER,
        "import { startServer } from \"./server\";\n\
         import { formatPath } from \"./util\";\n\
         \n\
         export function route(path) {\n  if (!path) startServer(formatPath(\"/\"));\n}\n",
    ),
    (
        UTIL,
        "export function formatPath(path) {\n  return path;\n}\n",
    ),
];

/// The project and a store path beside it. Nothing is indexed and the store
/// file does not exist yet. Keep the value alive while either is used.
pub struct TestProject {
    /// The project root, which is also its project key in the store.
    pub root: String,
    pub store_path: PathBuf,
    _dir: TempDir,
}

pub fn test_project() -> TestProject {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("project");
    for (file, source) in SOURCES {
        let path = root.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, source).unwrap();
    }
    TestProject {
        root: root.to_str().unwrap().to_string(),
        store_path: dir.path().join("store").join("cartographer.db"),
        _dir: dir,
    }
}

impl TestProject {
    /// The absolute path of `file`, as the indexer stores it.
    pub fn path(&self, file: &str) -> String {
        format!("{}/{file}", self.root)
    }
}

/// Indexes the project through the indexer with the built-in grammars, the
/// way `cartographer_index_project` does: the root is the project key.
pub fn index(db: &Db, project: &TestProject) -> (usize, usize) {
    full_index(
        db,
        &LanguageRegistry::builtin(),
        &project.root,
        &project.root,
    )
    .unwrap()
}
