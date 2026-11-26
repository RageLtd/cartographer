//! Where a Rust source file's child modules live on disk, and which module
//! declares a file's own module.

use std::fs;
use std::path::{Path, PathBuf};

const PACKAGE_MANIFEST: &str = "Cargo.toml";

const MOD_RS: &str = "mod.rs";
const LIB_RS: &str = "lib.rs";
const MAIN_RS: &str = "main.rs";

/// File names whose `mod x;` children sit beside them.
const OWNS_DIRECTORY: &[&str] = &[MOD_RS, LIB_RS, MAIN_RS];

const SOURCE_DIR: &str = "src";

/// The library and binary crate roots in a package's `src/`.
const CRATE_ROOT_FILES: [&str; 2] = [LIB_RS, MAIN_RS];

const BUILD_SCRIPT: &str = "build.rs";

/// Directories, relative to the package root, whose direct `.rs` children and
/// `<name>/main.rs` files Cargo auto-discovers as crate roots.
const TARGET_DIRS: &[&str] = &["src/bin", "tests", "examples", "benches"];

/// A module: its file, and the directory holding its children's files. The
/// file is `None` only for a package's `src/` root when it cannot be told
/// whether `lib.rs` or `main.rs` holds the module path being walked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Module {
    pub file: Option<PathBuf>,
    pub children_dir: PathBuf,
}

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

/// The file of child module `name` in `children_dir`: `name.rs` or `name/mod.rs`.
pub fn child_module_file(children_dir: &Path, name: &str) -> Option<PathBuf> {
    [
        children_dir.join(format!("{name}.rs")),
        children_dir.join(name).join(MOD_RS),
    ]
    .into_iter()
    .find(|path| path.exists())
}

/// The module whose file is `file`.
pub fn module_of_file(file: &Path) -> Option<Module> {
    Some(Module {
        file: Some(file.to_path_buf()),
        children_dir: child_module_dir(file)?,
    })
}

/// The root module of the crate `file` belongs to, found by following parent
/// modules up to a crate root.
pub fn crate_root_module(file: &Path, root_files: &mut RootFileModules) -> Option<Module> {
    let mut module = module_of_file(file)?;
    loop {
        let Some(current) = &module.file else {
            return Some(module);
        };
        if is_crate_root(current) {
            return Some(module);
        }
        module = parent_module(current, root_files)?;
    }
}

/// The module that declares `file`'s module with `mod x;`: the one whose
/// children live in the directory `file` sits in. A crate root has none, and
/// neither has a file whose declaring module cannot be singled out.
pub fn parent_module(file: &Path, root_files: &mut RootFileModules) -> Option<Module> {
    if is_crate_root(file) {
        return None;
    }
    let dir = file.parent()?;
    let (children_dir, module_name) = match file.file_name()?.to_str()? {
        MOD_RS => (dir.parent()?, dir.file_name()?.to_str()?),
        _ => (dir, file.file_stem()?.to_str()?),
    };
    if children_dir == package_root(file)?.join(SOURCE_DIR) {
        return package_source_root(children_dir, module_name, root_files);
    }

    let dir_name = children_dir.file_name()?.to_str()?;
    let directory_target = children_dir.join(MAIN_RS);
    let sibling = children_dir.parent()?.join(format!("{dir_name}.rs"));
    let candidates = [
        Some(children_dir.join(MOD_RS)),
        is_directory_target(&directory_target).then_some(directory_target),
        (!is_crate_root(&sibling)).then_some(sibling),
    ];
    let parent = exactly_one(candidates.into_iter().flatten().filter(|p| p.is_file()))?;
    module_of_file(&parent)
}

/// The `src/` root of top-level module `module_name`: whichever of `lib.rs`
/// and `main.rs` declares it. Both keep their children in `src/`, so the
/// directory is known even when the file is not.
fn package_source_root(
    src: &Path,
    module_name: &str,
    root_files: &mut RootFileModules,
) -> Option<Module> {
    let mut declaring = Vec::new();
    for root_file in CRATE_ROOT_FILES.map(|name| src.join(name)) {
        if root_file.is_file()
            && root_files
                .declared(&root_file)?
                .iter()
                .any(|name| name == module_name)
        {
            declaring.push(root_file);
        }
    }
    Some(Module {
        file: exactly_one(declaring.into_iter()),
        children_dir: src.to_path_buf(),
    })
}

/// The top-level `mod <name>;` items of `src/lib.rs` and `src/main.rs`, each
/// file read and parsed at most once. Meant to live for the parse of one
/// source file, so an edit to a root file is seen by the next parse.
#[derive(Debug, Default)]
pub struct RootFileModules {
    parsed: Vec<(PathBuf, Option<Vec<String>>)>,
}

impl RootFileModules {
    /// `None` when `file` cannot be read or parsed.
    fn declared(&mut self, file: &Path) -> Option<&[String]> {
        let index = match self.parsed.iter().position(|(path, _)| path == file) {
            Some(index) => index,
            None => {
                self.parsed
                    .push((file.to_path_buf(), top_level_file_modules(file)));
                self.parsed.len() - 1
            }
        };
        self.parsed[index].1.as_deref()
    }
}

/// The names of `file`'s top-level `mod <name>;` items (not inline modules).
fn top_level_file_modules(file: &Path) -> Option<Vec<String>> {
    let source = fs::read_to_string(file).ok()?;
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .ok()?;
    let tree = parser.parse(&source, None)?;
    let root = tree.root_node();
    let mut cursor = root.walk();
    let names = root
        .children(&mut cursor)
        .filter(|item| item.kind() == "mod_item" && item.child_by_field_name("body").is_none())
        .filter_map(|item| item.child_by_field_name("name"))
        .filter_map(|name| name.utf8_text(source.as_bytes()).ok())
        .map(str::to_string)
        .collect();
    Some(names)
}

fn exactly_one<T>(mut items: impl Iterator<Item = T>) -> Option<T> {
    let first = items.next()?;
    items.next().is_none().then_some(first)
}

fn is_crate_root(file: &Path) -> bool {
    let (Some(dir), Some(name)) = (file.parent(), file.file_name().and_then(|n| n.to_str())) else {
        return false;
    };
    let is_package_source_root = CRATE_ROOT_FILES.contains(&name)
        && package_root(file).is_some_and(|root| dir == root.join(SOURCE_DIR));
    is_package_source_root
        || is_auto_discovered_target(file, dir, name)
        || is_directory_target(file)
}

/// `<target dir>/<name>/main.rs`, such as `src/bin/tool/main.rs`.
fn is_directory_target(file: &Path) -> bool {
    let (Some(dir), Some(root)) = (file.parent(), package_root(file)) else {
        return false;
    };
    file.file_name().is_some_and(|name| name == MAIN_RS)
        && dir
            .parent()
            .is_some_and(|parent| is_target_dir(&root, parent))
}

fn is_auto_discovered_target(file: &Path, dir: &Path, name: &str) -> bool {
    let Some(root) = package_root(file) else {
        return false;
    };
    (name == BUILD_SCRIPT && dir == root) || is_target_dir(&root, dir)
}

fn is_target_dir(package_root: &Path, dir: &Path) -> bool {
    TARGET_DIRS
        .iter()
        .any(|target_dir| dir == package_root.join(target_dir))
}
