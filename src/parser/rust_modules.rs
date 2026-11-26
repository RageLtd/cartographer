//! Where a Rust source file's child modules live on disk.

use std::path::{Path, PathBuf};

const PACKAGE_MANIFEST: &str = "Cargo.toml";

/// File names whose `mod x;` children sit beside them.
const OWNS_DIRECTORY: &[&str] = &["mod.rs", "lib.rs", "main.rs"];

const BUILD_SCRIPT: &str = "build.rs";

/// Directories, relative to the package root, whose direct `.rs` children
/// Cargo auto-discovers as crate roots.
const TARGET_DIRS: &[&str] = &["src/bin", "tests", "examples", "benches"];

/// The nearest ancestor directory of `file` holding a `Cargo.toml`.
pub fn package_root(file: &Path) -> Option<PathBuf> {
    file.ancestors()
        .skip(1)
        .find(|dir| dir.join(PACKAGE_MANIFEST).is_file())
        .map(Path::to_path_buf)
}

/// The one directory holding the files of `from_file`'s `mod x;` declarations.
///
/// Crate roots and `mod.rs` keep their children beside them; any other file
/// `a/b.rs` keeps them in `a/b/`. Targets whose path is set by hand in
/// `Cargo.toml` are not recognised as crate roots.
pub fn child_module_dir(from_file: &Path) -> Option<PathBuf> {
    let dir = from_file.parent()?;
    let name = from_file.file_name()?.to_str()?;
    if OWNS_DIRECTORY.contains(&name) || is_auto_discovered_target(from_file, dir, name) {
        return Some(dir.to_path_buf());
    }
    Some(dir.join(from_file.file_stem()?))
}

fn is_auto_discovered_target(file: &Path, dir: &Path, name: &str) -> bool {
    let Some(root) = package_root(file) else {
        return false;
    };
    (name == BUILD_SCRIPT && dir == root)
        || TARGET_DIRS
            .iter()
            .any(|target_dir| dir == root.join(target_dir))
}
