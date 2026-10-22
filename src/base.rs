use crate::diag::{Issue, Issues, Span};
use crate::model::remote::{Collection, Kind as SourceKind};
use std::path::{Path, PathBuf};

pub const BASE_SUFFIX: &str = ".base.kdl";

pub const LIBRARY_URL: &str = "https://github.com/buildc-org/library";

pub struct Base {
    pub name: String,
    pub image: String,
    pub family: String,
    pub provides: Vec<String>,
    pub requires: Vec<String>,
    pub about: String,
    pub signed: bool,
    pub scap_content: String,
    pub bootloaders: Vec<String>,
    pub span: Span,
}

impl Base {
    pub fn differs(&self, other: &Self) -> bool {
        self.family != other.family
            || self.provides != other.provides
            || self.requires != other.requires
            || self.about != other.about
            || self.signed != other.signed
            || self.scap_content != other.scap_content
            || self.bootloaders != other.bootloaders
    }
}

pub struct Capability {
    pub name: &'static str,
    pub path: Option<&'static str>,
}

#[rustfmt::skip]
pub const CAPABILITIES: [Capability; 10] = [
    Capability { name: "ssh", path: Some("/usr/sbin/sshd") },
    Capability { name: "initramfs-generation", path: Some("/usr/bin/dracut") },
    Capability { name: "selinux-policy", path: Some("/usr/sbin/semodule") },
    Capability { name: "flatpak", path: Some("/usr/bin/flatpak") },
    Capability { name: "bootc", path: Some("/usr/bin/bootc") },
    Capability { name: "bootupctl", path: Some("/usr/bin/bootupctl") },
    Capability { name: "podman", path: Some("/usr/bin/podman") },
    Capability { name: "crun", path: Some("/usr/bin/crun") },
    Capability { name: "rechunking", path: None },
    Capability { name: "luks-initramfs", path: None },
];

pub struct DefaultLibrary {
    pub kind: SourceKind,
    pub alias: &'static str,
    pub path: &'static str,
    pub about: &'static str,
}

pub const DEFAULT_LIBRARIES: [DefaultLibrary; 2] = [
    DefaultLibrary {
        kind: SourceKind::Modules,
        alias: "buildc-modules",
        path: "modules",
        about: "Feature modules for many use cases",
    },
    DefaultLibrary {
        kind: SourceKind::BaseImages,
        alias: "core",
        path: "base-images/core",
        about: "Fedora, CentOS, RHEL, AlmaLinux, Rocky, Debian and Ubuntu",
    },
];

pub fn default_library(kind: SourceKind) -> &'static DefaultLibrary {
    DEFAULT_LIBRARIES
        .iter()
        .find(|library| library.kind == kind)
        .expect("every source kind has a default library")
}

const DEFAULT_DIRS: [&str; 2] = ["/usr/bin", "/usr/sbin"];

pub fn witness(name: &str, own: Option<&str>) -> Option<Vec<String>> {
    if let Some(own) = own {
        return Some(vec![own.to_string()]);
    }
    let row = CAPABILITIES.iter().find(|row| row.name == name)?;
    row.path.map(|path| vec![path.to_string()])
}

pub fn probe(name: &str) -> Option<Vec<String>> {
    match CAPABILITIES.iter().any(|row| row.name == name) {
        true => witness(name, None),
        false => Some(
            DEFAULT_DIRS
                .iter()
                .map(|dir| format!("{dir}/{name}"))
                .collect(),
        ),
    }
}

pub fn catalog(root: &Path, sources: &[Collection], issues: &mut Issues) -> Vec<Base> {
    let mut bases: Vec<Base> = Vec::new();
    let mut declared: Vec<(String, String)> = Vec::new();
    for collection in sources
        .iter()
        .filter(|collection| collection.kind == SourceKind::BaseImages)
    {
        let Some(dir) = crate::import::cached(root, collection) else {
            continue;
        };
        let dir = dir.join(collection.subtree().unwrap_or(""));
        for path in base_files(&dir) {
            let Some((base, src)) = crate::parse::bases::read_base(&path, issues) else {
                continue;
            };
            if let Some((_, first)) = declared.iter().find(|(image, _)| *image == base.image) {
                issues.push(
                    Issue::new(
                        format!("`{}` is described by two base files", base.image),
                        &src,
                    )
                    .at(base.span, format!("`{first}` describes it too"))
                    .help(
                        "one image reference is one base: two files would each write a \
                         different family and a different `provides` into an image \
                         scaffolded on it, and which one did would depend on the order the \
                         sources are read in",
                    ),
                );
                continue;
            }
            declared.push((base.image.clone(), path.display().to_string()));
            bases.push(base);
        }
    }
    bases
}

pub fn fetch(root: &Path, sources: &[Collection]) -> Result<(), String> {
    for collection in sources
        .iter()
        .filter(|collection| collection.kind != SourceKind::Modules)
    {
        if crate::import::cached(root, collection).is_none() {
            crate::import::tree(root, collection)?;
        }
    }
    Ok(())
}

pub(crate) fn base_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.ends_with(BASE_SUFFIX))
        })
        .collect();
    files.sort();
    files
}

fn undigested(image: &str) -> &str {
    image.split_once('@').map_or(image, |(before, _)| before)
}

pub fn find<'a>(bases: &'a [Base], image: &str) -> Option<&'a Base> {
    let image = undigested(image);
    bases
        .iter()
        .find(|base| undigested(&base.image) == image || base.name == image)
}
