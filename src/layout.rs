use std::path::{Path, PathBuf};

pub const MODULES: &str = "modules";
pub const GENERATED: &str = "generated";
pub const OUT: &str = "out";

pub const OVERLAY: &str = "files";

pub const REPO_FILE: &str = "repo.kdl";
pub const MODULE_FILE: &str = "module.kdl";
pub const RECORD_FILE: &str = "provenance.kdl";

pub const MOUNTED: &str = "out/buildc";
pub const SOURCES_CACHE: &str = "out/sources";
pub const STAMPS: &str = "out/remote-modules";

pub fn modules(root: &Path) -> PathBuf {
    root.join(MODULES)
}

pub fn module(root: &Path, dir: impl AsRef<Path>) -> PathBuf {
    modules(root).join(dir)
}

pub fn manifest(root: &Path, dir: impl AsRef<Path>) -> PathBuf {
    module(root, dir).join(MODULE_FILE)
}

pub fn generated(root: &Path) -> PathBuf {
    root.join(GENERATED)
}

pub fn out(root: &Path) -> PathBuf {
    root.join(OUT)
}
