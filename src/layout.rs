use std::path::{Path, PathBuf};

pub const MODULES: &str = "modules";
pub const GENERATED: &str = "generated";
pub const OUT: &str = "out";
pub const KEYS: &str = "keys";
const PUBLIC_KEYS: &str = "public";
const PRIVATE_KEYS: &str = "private";

pub const SCRIPT: &str = "module.sh";
pub const FRAGMENT: &str = "Containerfile.inc";

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

// A broad family precedes the specific families it covers, because the specific directory overrides the broad one.
pub const FAMILY_DIRS: [(&str, &[&str]); 6] = [
    ("deb", &["debian", "ubuntu"]),
    ("rpm", &["fedora", "rhel"]),
    ("fedora", &["fedora"]),
    ("rhel", &["rhel"]),
    ("debian", &["debian"]),
    ("ubuntu", &["ubuntu"]),
];

pub fn family_first(module_dir: &Path, family: &str) -> Vec<String> {
    family_dirs(module_dir, family)
        .into_iter()
        .rev()
        .map(|gated| format!("{gated}/"))
        .chain(std::iter::once(String::new()))
        .collect()
}

pub fn shipped(module_dir: &Path, family: &str, file: &str) -> Option<String> {
    family_first(module_dir, family)
        .into_iter()
        .map(|at| format!("{at}{file}"))
        .find(|at| module_dir.join(at).is_file())
}

pub fn family_dirs(module_dir: &Path, family: &str) -> Vec<&'static str> {
    FAMILY_DIRS
        .iter()
        .filter(|(_, taken_on)| taken_on.contains(&family))
        .map(|(name, _)| *name)
        .filter(|name| module_dir.join(name).is_dir())
        .collect()
}

pub fn module_owned(name: &str) -> bool {
    name == OVERLAY
        || name == SELINUX.dir
        || name == APPARMOR.dir
        || FAMILY_DIRS.iter().any(|(dir, _)| *dir == name)
}

pub fn module_dirs(
    tree: &Path,
    marks: impl Fn(&Path) -> bool,
    skip: impl Fn(&str) -> bool,
) -> Vec<PathBuf> {
    walk_modules(tree, false, marks, skip)
}

fn walk_modules(
    tree: &Path,
    in_module: bool,
    marks: impl Fn(&Path) -> bool,
    skip: impl Fn(&str) -> bool,
) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut dirs = vec![(tree.to_path_buf(), in_module)];
    while let Some((dir, in_module)) = dirs.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let path = entry.path();
            if !path.is_dir() || skip(&name) || (in_module && module_owned(&name)) {
                continue;
            }
            let module = marks(&path);
            if module {
                out.push(path.clone());
            }
            // A symlinked directory is never entered, so a cycle through a link cannot repeat the walk.
            if !entry.file_type().is_ok_and(|kind| kind.is_symlink()) {
                dirs.push((path, module));
            }
        }
    }
    out.sort();
    out
}

pub fn member_dirs(tree: &Path) -> Vec<PathBuf> {
    walk_modules(tree, false, is_member, is_dot)
}

pub fn held_modules(dir: &Path) -> Vec<PathBuf> {
    walk_modules(dir, true, is_member, is_dot)
        .into_iter()
        .filter_map(|path| path.strip_prefix(dir).ok().map(Path::to_path_buf))
        .collect()
}

fn is_member(path: &Path) -> bool {
    path.join(MODULE_FILE).is_file()
}

fn is_dot(name: &str) -> bool {
    name.starts_with('.')
}

pub const SCRIPTS: &str = "scripts";

pub fn script(root: &Path, name: &str) -> String {
    match root.join(SCRIPTS).join(name).is_file() {
        true => format!("{SCRIPTS}/{name}"),
        false => format!("{GENERATED}/{SCRIPTS}/{name}"),
    }
}

pub fn is_module(dir: &Path) -> bool {
    dir.join(MODULE_FILE).is_file()
        || dir.join(SCRIPT).is_file()
        || dir.join(FRAGMENT).is_file()
        || dir.join(OVERLAY).is_dir()
}

pub const WORKFLOW_DIR: &str = ".github/workflows";

pub const REPO_FILE: &str = "repo.kdl";
pub const IMAGE_FILE: &str = "image.kdl";
pub const IMAGE_SUFFIX: &str = ".image.kdl";
pub const MODULE_FILE: &str = "module.kdl";
pub const LOCK_FILE: &str = "modules.lock.kdl";

pub const MOUNTED: &str = "out/buildc";
pub const SOURCES_CACHE: &str = ".cache/sources";

pub fn is_image_file(name: &str) -> bool {
    name == IMAGE_FILE || name.ends_with(IMAGE_SUFFIX)
}

pub fn as_image_file(name: &str) -> String {
    format!("{}{IMAGE_SUFFIX}", name.trim_end_matches(".kdl"))
}

pub fn modules(root: &Path) -> PathBuf {
    root.join(MODULES)
}

pub fn lock(root: &Path) -> PathBuf {
    modules(root).join(LOCK_FILE)
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

pub struct Place {
    pub path: &'static [&'static str],
    pub writer: &'static str,
    pub tracked: bool,
    pub what: &'static str,
}

pub struct Tree {
    pub root: &'static str,
    pub intro: &'static str,
    pub places: &'static [Place],
    pub lists: &'static [(&'static str, &'static [&'static str])],
    pub notes: &'static [&'static str],
}

#[rustfmt::skip]
pub const REPOSITORY: Tree = Tree {
    root: "<repository>/",
    intro: "A repository is a git repository with `repo.kdl` at its root, which is how `buildc` \
        finds the root. `buildc create repo` scaffolds the layout below. Every path that `buildc` \
        reads or writes has a fixed place, so every repository looks alike.",
    places: &[
        Place { path: &[REPO_FILE], writer: "`buildc create repo`, then the user", tracked: true,
            what: "The settings of the whole repository. See [`repo.kdl`](repo.md)." },
        Place { path: &[IMAGE_FILE], writer: "`buildc create image`, then the user", tracked: true,
            what: "The images of the repository. See [`image.kdl`](image.md)." },
        Place { path: &["<name>", IMAGE_SUFFIX], writer: "`buildc create image`, then the user", tracked: true,
            what: "More images. The name in front of the suffix is decorative." },
        Place { path: &[MODULES, "/"], writer: "the user, `buildc create module` or `buildc import module`", tracked: true,
            what: "The modules of the repository, one directory each. See [Modules](modules.md)." },
        Place { path: &[MODULES, "/", LOCK_FILE], writer: "`buildc import module`", tracked: true,
            what: "Where each imported module came from, and what its content hashed to. See \
                [`modules.lock.kdl`](lock.md)." },
        Place { path: &[KEYS, "/", PUBLIC_KEYS, "/"], writer: "`buildc create key` or `buildc set key`", tracked: true,
            what: "The public half of each key, at the path it has in the image." },
        Place { path: &[KEYS, "/", PRIVATE_KEYS, "/"], writer: "`buildc create key`", tracked: false,
            what: "The private half of each key. The build never copies it into the image." },
        Place { path: &[GENERATED, "/"], writer: "`buildc generate`", tracked: true,
            what: "Everything that `buildc generate` writes. `buildc verify` compares it byte for \
                byte, so the user does not edit it." },
        Place { path: &[SCRIPTS, "/"], writer: "`buildc create scripts`, then the user", tracked: true,
            what: "The scripts that the repository keeps in place of the ones `buildc generate` \
                writes into `generated/scripts/`." },
        Place { path: &[WORKFLOW_DIR, "/"], writer: "`buildc generate`", tracked: true,
            what: "The CI workflows that `workflows` in `repo.kdl` names." },
        Place { path: &[OUT, "/"], writer: "`buildc`", tracked: false,
            what: "Local exports, caches and scratch. `buildc` reads nothing here as a declaration." },
        Place { path: &[SOURCES_CACHE, "/"], writer: "`buildc`", tracked: false,
            what: "The libraries that `buildc import module` fetches and copies a module from. \
                `buildc check`, `buildc generate` and `buildc build` read nothing here." },
        Place { path: &["disk_config/"], writer: "`buildc create repo`, then the user", tracked: true,
            what: "The settings that bootc-image-builder reads when it builds a Fedora or RHEL disk \
                image. `disk.toml` sets the smallest size of `/`." },
        Place { path: &[".github/renovate.json5"], writer: "`buildc create repo`, then the user", tracked: true,
            what: "The Renovate settings. Its custom managers keep the `buildc` release in `repo.kdl` \
                and each pin that declares `renovate` current." },
        Place { path: &[".gitignore"], writer: "`buildc create repo`", tracked: true,
            what: "Keeps `keys/private/`, `out/` and `.cache/` out of git." },
    ],
    lists: &[],
    notes: &[],
};

#[rustfmt::skip]
pub const MODULE_DIR: Tree = Tree {
    root: "modules/<module-name>/",
    intro: "A module is a directory under `modules/`, at whatever depth groups it, and its path \
        names the module. The build reads the files below by convention, so a module can be as \
        small as one `module.sh`. A module can also hold other modules. The walk that finds \
        modules passes over the directories that the build of a module reads.",
    places: &[
        Place { path: &[MODULE_FILE], writer: "", tracked: true,
            what: "What the module declares about itself. A module with only a script and \
                `files/` can leave it out. A module fetched from another repository needs one. \
                See [`module.kdl`](module.md)." },
        Place { path: &[SCRIPT], writer: "", tracked: true,
            what: "The install logic. The build sources it in the layer of the module." },
        Place { path: &["finalize.sh"], writer: "", tracked: true,
            what: "The finalize phase sources it, in resolved order." },
        Place { path: &[OVERLAY, "/"], writer: "", tracked: true,
            what: "An overlay that the build copies over `/`." },
        Place { path: &["repo"], writer: "", tracked: true,
            what: "A package repository file. The build sources it first. On Fedora, if the \
                `REPO_ID` of `repo` is already configured, then the build skips it." },
        Place { path: &[FRAGMENT], writer: "", tracked: true,
            what: "Containerfile lines that the build adds verbatim, for a need that the \
                fields cannot express. `fragment` in `module.kdl` places them." },
        Place { path: &[SELINUX.dir, "/*.te"], writer: "", tracked: true,
            what: "SELinux policy. If the image provides `selinux-policy`, then the build \
                compiles and installs it." },
        Place { path: &[APPARMOR.dir, "/"], writer: "", tracked: true,
            what: "AppArmor profiles. If the image provides `apparmor-policy`, then the build \
                validates them and places them in `/etc/apparmor.d`." },
        Place { path: &["<family>/"], writer: "", tracked: true,
            what: "The files of the module that apply to one base family." },
        Place { path: &["<collected file>"], writer: "", tracked: true,
            what: "A part of a file that another module collects. The build stages it for that \
                module." },
    ],
    lists: &[
        ("A policy directory follows these rules:", &[
            "A module that ships `selinux/` or `apparmor/` builds after the module that provides \
             that MAC, as if it declared `after`.",
            "A module can ship both policy directories. The build takes each directory only where \
             the image has that MAC. An image with no MAC installs no policy and needs no provider.",
            "A policy directory declares no requirement. A module that needs a MAC declares \
             `requires \"selinux-policy\"` or `requires \"apparmor-policy\"`, and `buildc` refuses \
             it on an image without that MAC.",
        ]),
        ("A `<family>/` directory gates `module.sh`, `finalize.sh`, `files/`, `repo` and a \
          collected file:", &[
            "The six directory names are `fedora/`, `rhel/`, `debian/`, `ubuntu/`, `rpm/` and \
             `deb/`. The build reads no other directory in a module.",
            "The build takes `deb/` on Debian and Ubuntu, and `rpm/` on Fedora and RHEL. It takes \
             `debian/` on Debian only, after `deb/`.",
            "A gated half runs after the ungated half. The family `files/` is copied over the \
             shared `files/`, and the family `module.sh` is sourced after the shared `module.sh`.",
            "A family `repo` replaces the other copies. `debian/repo` wins over `deb/repo`, which \
             wins over the `repo` at the module root, and the build sources only the winner.",
            "If `supports` is declared and the directory names no supported family, then `buildc` \
             refuses it. With no `supports` declaration, every family is supported.",
        ]),
    ],
    notes: &[
        "A module with no `<family>/` directory gates nothing. Its `module.sh` and `files/` run on \
         every family it supports; with no `supports` declaration, that is every family.",
    ],
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_held_module_is_what_the_library_lists_inside_the_module() {
        let root = std::env::temp_dir().join(format!("buildc-held-dirs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for dir in ["a", "a/b", "a/files/c", "a/group/files/d", "a/.x/e"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
            std::fs::write(root.join(dir).join(MODULE_FILE), "").unwrap();
        }
        let listed = member_dirs(&root);
        let held = held_modules(&root.join("a"));
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(
            listed,
            ["a", "a/b", "a/group/files/d"].map(|dir| root.join(dir))
        );
        assert_eq!(held, [PathBuf::from("b"), PathBuf::from("group/files/d")]);
    }

    #[test]
    fn the_walk_finds_nested_modules_and_passes_over_module_content() {
        let root = std::env::temp_dir().join(format!("buildc-module-dirs-{}", std::process::id()));
        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }
        for dir in ["a/b", "a/files/c", "a/fedora/d", ".git/e", "group/f"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
            std::fs::write(root.join(dir).join(MODULE_FILE), "").unwrap();
        }
        std::fs::write(root.join("a").join(MODULE_FILE), "").unwrap();
        std::os::unix::fs::symlink(&root, root.join("a/b/loop")).unwrap();
        std::os::unix::fs::symlink(root.join("group/f"), root.join("linked")).unwrap();

        let found: Vec<String> = module_dirs(
            &root,
            |path| path.join(MODULE_FILE).is_file(),
            |name| name.starts_with('.'),
        )
        .iter()
        .map(|path| path.strip_prefix(&root).unwrap().display().to_string())
        .collect();
        assert_eq!(found, ["a", "a/b", "group/f", "linked"]);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_family_reads_the_shared_directory_first() {
        let root = std::env::temp_dir().join(format!("buildc-family-dirs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for name in ["rpm", "rhel", "fedora", "deb"] {
            std::fs::create_dir_all(root.join(name)).unwrap();
        }

        assert_eq!(family_dirs(&root, "rhel"), ["rpm", "rhel"]);
        assert_eq!(family_dirs(&root, "fedora"), ["rpm", "fedora"]);
        assert_eq!(family_first(&root, "rhel"), ["rhel/", "rpm/", ""]);
        std::fs::remove_dir_all(root).unwrap();
    }
}
