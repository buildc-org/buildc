use std::path::{Path, PathBuf};

pub const MODULES: &str = "modules";
pub const GENERATED: &str = "generated";
pub const OUT: &str = "out";
pub const KEYS: &str = "keys";
const PUBLIC_KEYS: &str = "public";
const PRIVATE_KEYS: &str = "private";

pub const OVERLAY: &str = "files";

pub struct Policy {
    pub dir: &'static str,
    pub ext: Option<&'static str>,
    pub capability: &'static str,
}

pub const SELINUX: Policy = Policy {
    dir: "selinux",
    ext: Some("te"),
    capability: "selinux-policy",
};

pub const APPARMOR: Policy = Policy {
    dir: "apparmor",
    ext: None,
    capability: "apparmor-policy",
};

impl Policy {
    pub fn files(&self, module_dir: &Path) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(module_dir.join(self.dir)) else {
            return Vec::new();
        };
        let mut out: Vec<String> = entries
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|ty| ty.is_file()))
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|name| match self.ext {
                Some(ext) => name.ends_with(&format!(".{ext}")),
                None => true,
            })
            .collect();
        out.sort();
        out
    }
}

pub fn family_dir(module_dir: &Path, family: &str) -> Option<PathBuf> {
    let dir = module_dir.join(family);
    (!family.is_empty() && dir.is_dir()).then_some(dir)
}

pub const WORKFLOW_DIR: &str = ".github/workflows";

pub const REPO_FILE: &str = "repo.kdl";
pub const IMAGE_FILE: &str = "image.kdl";
pub const IMAGE_SUFFIX: &str = ".image.kdl";
pub const MODULE_FILE: &str = "module.kdl";
pub const RECORD_FILE: &str = "provenance.kdl";

pub const MOUNTED: &str = "out/buildc";
pub const SOURCES_CACHE: &str = "out/sources";
pub const STAMPS: &str = "out/remote-modules";

pub fn is_image_file(name: &str) -> bool {
    name == IMAGE_FILE || name.ends_with(IMAGE_SUFFIX)
}

pub fn as_image_file(name: &str) -> String {
    format!("{}{IMAGE_SUFFIX}", name.trim_end_matches(".kdl"))
}

pub fn modules(root: &Path) -> PathBuf {
    root.join(MODULES)
}

pub fn module(root: &Path, dir: impl AsRef<Path>) -> PathBuf {
    modules(root).join(dir)
}

pub fn public_key(root: &Path, path: &str) -> PathBuf {
    root.join(KEYS)
        .join(PUBLIC_KEYS)
        .join(path.trim_start_matches('/'))
}

pub fn private_key(root: &Path, name: &str) -> PathBuf {
    root.join(KEYS).join(PRIVATE_KEYS).join(name)
}

pub fn nonempty(path: &Path) -> bool {
    path.metadata().is_ok_and(|meta| meta.len() > 0)
}

pub fn manifest(root: &Path, dir: impl AsRef<Path>) -> PathBuf {
    module(root, dir).join(MODULE_FILE)
}

pub fn generated(root: &Path) -> PathBuf {
    root.join(GENERATED)
}

pub fn generated_image(id: &str) -> PathBuf {
    PathBuf::from(GENERATED).join(id)
}

pub const CONTAINERFILE: &str = "Containerfile";

pub const LIB: &str = "lib";

pub fn out(root: &Path) -> PathBuf {
    root.join(OUT)
}
