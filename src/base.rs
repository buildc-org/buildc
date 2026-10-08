use crate::diag::{Issue, Issues, Span};
use crate::model::remote::{Collection, Kind as SourceKind};
use std::path::{Path, PathBuf};

pub const BASE_SUFFIX: &str = ".base.kdl";

pub const CAPABILITIES_FILE: &str = "capabilities.kdl";

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
    pub fn differs(&self, other: &Base) -> bool {
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
    pub name: String,
    pub path: Option<String>,
    pub families: Vec<(Vec<String>, String)>,
    pub span: Span,
}

impl Capability {
    fn path_for(&self, family: &str) -> Option<&str> {
        self.families
            .iter()
            .find(|(families, _)| families.iter().any(|named| named == family))
            .map(|(_, path)| path.as_str())
            .or(self.path.as_deref())
    }

    fn same_as(&self, other: &Capability) -> bool {
        self.path == other.path && self.families == other.families
    }
}

pub struct DefaultLibrary {
    pub kind: SourceKind,
    pub alias: &'static str,
    pub path: &'static str,
    pub about: &'static str,
}

pub const DEFAULT_LIBRARIES: [DefaultLibrary; 3] = [
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
    DefaultLibrary {
        kind: SourceKind::Capabilities,
        alias: "core",
        path: "capabilities",
        about: "Where each capability's witness is read",
    },
];

pub fn default_library(kind: SourceKind) -> &'static DefaultLibrary {
    DEFAULT_LIBRARIES
        .iter()
        .find(|library| library.kind == kind)
        .expect("every source kind has a default library")
}

const DEFAULT_DIRS: [&str; 2] = ["/usr/bin", "/usr/sbin"];

pub fn witness(
    name: &str,
    family: &str,
    own: Option<&str>,
    rows: &[Capability],
) -> Option<Vec<String>> {
    if let Some(own) = own {
        return Some(vec![own.to_string()]);
    }
    let row = rows.iter().find(|row| row.name == name)?;
    row.path_for(family).map(|path| vec![path.to_string()])
}

pub fn probe(name: &str, family: &str, rows: &[Capability]) -> Option<Vec<String>> {
    match rows.iter().any(|row| row.name == name) {
        true => witness(name, family, None, rows),
        false => Some(
            DEFAULT_DIRS
                .iter()
                .map(|dir| format!("{dir}/{name}"))
                .collect(),
        ),
    }
}

pub fn catalog(root: &Path, sources: &[Collection], issues: &mut Issues) -> Vec<Base> {
    load(root, sources, issues).bases
}

pub fn capabilities(root: &Path, sources: &[Collection], issues: &mut Issues) -> Vec<Capability> {
    load(root, sources, issues).capabilities
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

struct Loaded {
    bases: Vec<Base>,
    capabilities: Vec<Capability>,
}

fn load(root: &Path, sources: &[Collection], issues: &mut Issues) -> Loaded {
    let mut bases: Vec<Base> = Vec::new();
    let mut capabilities: Vec<Capability> = Vec::new();
    let mut declared: Vec<(String, String)> = Vec::new();
    let mut named: Vec<(String, String)> = Vec::new();

    for collection in sources {
        if collection.kind == SourceKind::Modules {
            continue;
        }
        let Some(dir) = crate::import::cached(root, collection) else {
            continue;
        };
        let dir = dir.join(collection.subtree().unwrap_or(""));
        match collection.kind {
            SourceKind::Modules => {}
            SourceKind::BaseImages => {
                for path in base_files(&dir) {
                    let Some((base, src)) = crate::parse::bases::read_base(&path, issues) else {
                        continue;
                    };
                    if let Some((_, first)) =
                        declared.iter().find(|(image, _)| *image == base.image)
                    {
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
            SourceKind::Capabilities => {
                let path = dir.join(CAPABILITIES_FILE);
                let Some((read, src)) = crate::parse::bases::read_capabilities(&path, issues)
                else {
                    continue;
                };
                for row in read {
                    if let Some((_, first)) = named.iter().find(|(name, _)| *name == row.name) {
                        if let Some(at) =
                            capabilities.iter().position(|known| known.name == row.name)
                        {
                            if !capabilities[at].same_as(&row) {
                                issues.push(
                                    Issue::new(
                                        format!(
                                            "capability `{}` is described by two libraries",
                                            row.name
                                        ),
                                        &src,
                                    )
                                    .at(row.span, format!("`{first}` reads it elsewhere"))
                                    .help(
                                        "one name is read at one path; which row won would depend \
                                         on the order repo.kdl lists the sources in",
                                    ),
                                );
                            }
                        }
                        continue;
                    }
                    named.push((row.name.clone(), path.display().to_string()));
                    capabilities.push(row);
                }
            }
        }
    }
    Loaded {
        bases,
        capabilities,
    }
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
