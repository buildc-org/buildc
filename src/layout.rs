use std::path::{Path, PathBuf};

pub const COMPONENTS: &str = "components";
pub const GENERATED: &str = "generated";
pub const OUT: &str = "out";
pub const KEYS: &str = "keys";
const PUBLIC_KEYS: &str = "public";
const PRIVATE_KEYS: &str = "private";

pub const SCRIPT: &str = "component.sh";
pub const REPO_SCRIPT: &str = "repo.sh";
pub const PACKAGES_SCRIPT: &str = "packages.sh";
pub const FINALIZE_SCRIPT: &str = "finalize.sh";
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
    pub fn files(&self, component_dir: &Path) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(component_dir.join(self.dir)) else {
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

pub fn family_first(component_dir: &Path, family: &str) -> Vec<String> {
    family_dirs(component_dir, family)
        .into_iter()
        .rev()
        .map(|gated| format!("{gated}/"))
        .chain(std::iter::once(String::new()))
        .collect()
}

pub fn shipped(component_dir: &Path, family: &str, file: &str) -> Option<String> {
    family_first(component_dir, family)
        .into_iter()
        .map(|at| format!("{at}{file}"))
        .find(|at| component_dir.join(at).is_file())
}

pub fn family_dirs(component_dir: &Path, family: &str) -> Vec<&'static str> {
    FAMILY_DIRS
        .iter()
        .filter(|(_, taken_on)| taken_on.contains(&family))
        .map(|(name, _)| *name)
        .filter(|name| component_dir.join(name).is_dir())
        .collect()
}

pub fn layered(on_disk: &Path, family: &str, file: &str) -> Vec<String> {
    std::iter::once(String::new())
        .chain(
            family_dirs(on_disk, family)
                .into_iter()
                .map(|gated| format!("{gated}/")),
        )
        .map(|at| format!("{at}{file}"))
        .filter(|at| on_disk.join(at).is_file())
        .collect()
}

pub fn component_owned(name: &str) -> bool {
    name == OVERLAY
        || name == SELINUX.dir
        || name == APPARMOR.dir
        || FAMILY_DIRS.iter().any(|(dir, _)| *dir == name)
}

pub fn component_dirs(
    tree: &Path,
    marks: impl Fn(&Path) -> bool,
    skip: impl Fn(&str) -> bool,
) -> Vec<PathBuf> {
    walk_components(tree, false, marks, skip)
}

fn walk_components(
    tree: &Path,
    in_component: bool,
    marks: impl Fn(&Path) -> bool,
    skip: impl Fn(&str) -> bool,
) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut dirs = vec![(tree.to_path_buf(), in_component)];
    while let Some((dir, in_component)) = dirs.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let path = entry.path();
            if !path.is_dir() || skip(&name) || (in_component && component_owned(&name)) {
                continue;
            }
            let component = marks(&path);
            if component {
                out.push(path.clone());
            }
            // A symlinked directory is never entered, so a cycle through a link cannot repeat the walk.
            if !entry.file_type().is_ok_and(|kind| kind.is_symlink()) {
                dirs.push((path, component));
            }
        }
    }
    out.sort();
    out
}

pub fn member_dirs(tree: &Path) -> Vec<PathBuf> {
    walk_components(tree, false, is_member, is_dot)
}

pub fn held_components(dir: &Path) -> Vec<PathBuf> {
    walk_components(dir, true, is_member, is_dot)
        .into_iter()
        .filter_map(|path| path.strip_prefix(dir).ok().map(Path::to_path_buf))
        .collect()
}

fn is_member(path: &Path) -> bool {
    path.join(MANIFEST_FILE).is_file()
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

pub fn is_component(dir: &Path) -> bool {
    [
        MANIFEST_FILE,
        SCRIPT,
        REPO_SCRIPT,
        PACKAGES_SCRIPT,
        FINALIZE_SCRIPT,
        FRAGMENT,
    ]
    .iter()
    .any(|file| dir.join(file).is_file())
        || dir.join(OVERLAY).is_dir()
}

pub const WORKFLOW_DIR: &str = ".github/workflows";

pub const REPO_FILE: &str = "repo.kdl";
pub const IMAGE_FILE: &str = "image.kdl";
pub const IMAGE_SUFFIX: &str = ".image.kdl";
pub const MANIFEST_FILE: &str = "manifest.kdl";
pub const LOCK_FILE: &str = "components.lock.kdl";

pub const MOUNTED: &str = "out/buildc";
pub const SOURCES_CACHE: &str = ".cache/sources";

pub fn is_image_file(name: &str) -> bool {
    name == IMAGE_FILE || name.ends_with(IMAGE_SUFFIX)
}

pub fn as_image_file(name: &str) -> String {
    format!("{}{IMAGE_SUFFIX}", name.trim_end_matches(".kdl"))
}

pub fn components(root: &Path) -> PathBuf {
    root.join(COMPONENTS)
}

pub fn lock(root: &Path) -> PathBuf {
    components(root).join(LOCK_FILE)
}

pub fn component(root: &Path, dir: impl AsRef<Path>) -> PathBuf {
    components(root).join(dir)
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
    component(root, dir).join(MANIFEST_FILE)
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
        Place { path: &[COMPONENTS, "/"], writer: "the user, `buildc create component` or `buildc import component`", tracked: true,
            what: "The components of the repository, one directory each. See [Components](components.md)." },
        Place { path: &[COMPONENTS, "/", LOCK_FILE], writer: "`buildc import component`, `buildc fetch components` and `buildc set component`", tracked: true,
            what: "Where each imported component came from, and what its content hashed to. See \
                [`components.lock.kdl`](lock.md)." },
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
            what: "The libraries that `buildc import component` fetches and copies a component from. \
                `buildc check`, `buildc generate` and `buildc build` read nothing here." },
        Place { path: &["disk_config/"], writer: "`buildc create repo`, then the user", tracked: true,
            what: "The settings that bootc-image-builder reads when it builds a Fedora or RHEL disk \
                image. `disk.toml` sets the smallest size of `/`." },
        Place { path: &[".github/renovate.json5"], writer: "`buildc generate`", tracked: true,
            what: "The Renovate settings, written from the `renovate` block in `repo.kdl`. Its custom \
                managers keep the `buildc` release, the base images, each pin that declares \
                `renovate` and each image entry's tracked `version` current." },
        Place { path: &[".gitignore"], writer: "`buildc create repo`", tracked: true,
            what: "Keeps `keys/private/`, `out/` and `.cache/` out of git." },
    ],
    lists: &[],
    notes: &[],
};

#[rustfmt::skip]
pub const COMPONENT_DIR: Tree = Tree {
    root: "components/<component-name>/",
    intro: "A component is a directory under `components/`, at whatever depth groups it, and its path \
        names the component. The build reads the files below by convention, so a component can be as \
        small as one `component.sh`. Any one of `manifest.kdl`, `component.sh`, `repo.sh`, `packages.sh`, \
        `finalize.sh`, `Containerfile.inc` and `files/` marks a directory as a component. A component \
        can also hold other components. An image that lists a nested component also gets the nearest \
        component that holds it, and an image that lists the outer component gets none of the components \
        inside it. The walk that finds \
        components passes over the directories that the build of a component reads.",
    places: &[
        Place { path: &[MANIFEST_FILE], writer: "", tracked: true,
            what: "What the component declares about itself. A component with only a script and \
                `files/` can leave it out. A component fetched from another repository needs one. \
                See [`manifest.kdl`](component.md)." },
        Place { path: &[SCRIPT], writer: "", tracked: true,
            what: "The install logic. The build sources it in the layer of the component, after the \
                keys and before the policies and `files/`." },
        Place { path: &[PACKAGES_SCRIPT], writer: "", tracked: true,
            what: "Package work that the declarations cannot express. The package step sources it \
                after the declared packages, with the network that `packages` in `repo.kdl` \
                allows. It can call the family's `install_packages`." },
        Place { path: &[FINALIZE_SCRIPT], writer: "", tracked: true,
            what: "The finalize phase sources it, in resolved order." },
        Place { path: &[OVERLAY, "/"], writer: "", tracked: true,
            what: "An overlay that the build copies over `/`." },
        Place { path: &[REPO_SCRIPT], writer: "", tracked: true,
            what: "A package repository file. The build sources it first. On Fedora, if the \
                `REPO_ID` of `repo.sh` is already configured, then the build skips it." },
        Place { path: &[FRAGMENT], writer: "", tracked: true,
            what: "Containerfile lines that the build adds verbatim, for a need that the \
                fields cannot express. `fragment` in `manifest.kdl` places them." },
        Place { path: &[SELINUX.dir, "/*.te"], writer: "", tracked: true,
            what: "SELinux policy. If the image provides `selinux-policy`, then the build \
                compiles and installs it." },
        Place { path: &[APPARMOR.dir, "/"], writer: "", tracked: true,
            what: "AppArmor profiles. If the image provides `apparmor-policy`, then the build \
                validates them and places them in `/etc/apparmor.d`." },
        Place { path: &["<family>/"], writer: "", tracked: true,
            what: "The files of the component that apply to one base family." },
        Place { path: &["<collected file>"], writer: "", tracked: true,
            what: "A part of a file that another component collects. The build stages it for that \
                component." },
    ],
    lists: &[
        ("A policy directory follows these rules:", &[
            "A component that ships `selinux/` or `apparmor/` builds after the component that provides \
             that MAC, as if it declared `after`.",
            "A component can ship both policy directories. The build takes each directory only where \
             the image has that MAC. An image with no MAC installs no policy and needs no provider.",
            "A policy directory declares no requirement. A component that needs a MAC declares \
             `requires \"selinux-policy\"` or `requires \"apparmor-policy\"`, and `buildc` refuses \
             it on an image without that MAC.",
        ]),
        ("A `<family>/` directory gates `component.sh`, `packages.sh`, `finalize.sh`, `files/`, \
          `repo.sh` and a collected file:", &[
            "The six directory names are `fedora/`, `rhel/`, `debian/`, `ubuntu/`, `rpm/` and \
             `deb/`. The build reads no other directory in a component.",
            "The build takes `deb/` on Debian and Ubuntu, and `rpm/` on Fedora and RHEL. It takes \
             `debian/` on Debian only, after `deb/`.",
            "A gated half runs after the ungated half. The family `files/` is copied over the \
             shared `files/`. The family `component.sh`, `packages.sh` and `finalize.sh` are each \
             sourced after the shared copy, from `deb/` or `rpm/` to the single family.",
            "A family `repo.sh` replaces the other copies. `debian/repo.sh` wins over `deb/repo.sh`, which \
             wins over the `repo.sh` at the component root, and the build sources only the winner.",
            "If `targets` is declared and the directory names no targeted family, then `buildc` \
             refuses it. With no `targets` declaration, every family is targeted.",
        ]),
    ],
    notes: &[
        "A component with no `<family>/` directory gates nothing. Its `component.sh` and `files/` run on \
         every family it targets; with no `targets` declaration, that is every family.",
    ],
};

pub fn named_after_root(root: &Path) -> Option<String> {
    std::fs::canonicalize(root)
        .ok()
        .as_deref()
        .and_then(Path::file_name)
        .map(|name| name.to_string_lossy().into_owned())
}

pub(crate) fn under(root: &Path, path: &Path) -> PathBuf {
    path.strip_prefix(root).unwrap_or(path).to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_held_component_is_what_the_library_lists_inside_the_component() {
        let root = std::env::temp_dir().join(format!("buildc-held-dirs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for dir in ["a", "a/b", "a/files/c", "a/group/files/d", "a/.x/e"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
            std::fs::write(root.join(dir).join(MANIFEST_FILE), "").unwrap();
        }
        let listed = member_dirs(&root);
        let held = held_components(&root.join("a"));
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(
            listed,
            ["a", "a/b", "a/group/files/d"].map(|dir| root.join(dir))
        );
        assert_eq!(held, [PathBuf::from("b"), PathBuf::from("group/files/d")]);
    }

    #[test]
    fn the_walk_finds_nested_components_and_passes_over_component_content() {
        let root = std::env::temp_dir().join(format!("buildc-component-dirs-{}", std::process::id()));
        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }
        for dir in ["a/b", "a/files/c", "a/fedora/d", ".git/e", "group/f"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
            std::fs::write(root.join(dir).join(MANIFEST_FILE), "").unwrap();
        }
        std::fs::write(root.join("a").join(MANIFEST_FILE), "").unwrap();
        std::os::unix::fs::symlink(&root, root.join("a/b/loop")).unwrap();
        std::os::unix::fs::symlink(root.join("group/f"), root.join("linked")).unwrap();

        let found: Vec<String> = component_dirs(
            &root,
            |path| path.join(MANIFEST_FILE).is_file(),
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
