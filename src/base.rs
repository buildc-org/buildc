use crate::diag::{Issue, Issues, Span};
use crate::model::remote::Collection;
use std::path::Path;

pub const BASES_FILE: &str = "bases.kdl";

const BUILT_IN: &str = include_str!("../assets/bases.kdl");

pub struct Base {
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
    fn differs(&self, other: &Base) -> bool {
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
    pub span: Span,
}

const DEFAULT_DIRS: [&str; 2] = ["/usr/bin", "/usr/sbin"];

pub fn witness(name: &str, rows: &[Capability]) -> Option<Vec<String>> {
    match rows.iter().find(|row| row.name == name) {
        Some(row) => row.path.clone().map(|path| vec![path]),
        None => Some(
            DEFAULT_DIRS
                .iter()
                .map(|dir| format!("{dir}/{name}"))
                .collect(),
        ),
    }
}

pub struct Shadow {
    pub image: String,
    pub collection: String,
}

pub fn catalog(
    root: &Path,
    sources: &[Collection],
    issues: &mut Issues,
) -> (Vec<Base>, Vec<Shadow>) {
    let loaded = load(root, sources, issues);
    (loaded.bases, loaded.shadows)
}

pub fn capabilities(root: &Path, sources: &[Collection], issues: &mut Issues) -> Vec<Capability> {
    load(root, sources, issues).capabilities
}

struct Loaded {
    bases: Vec<Base>,
    shadows: Vec<Shadow>,
    capabilities: Vec<Capability>,
}

fn load(root: &Path, sources: &[Collection], issues: &mut Issues) -> Loaded {
    let runtime = crate::init::assets()
        .ok()
        .map(|assets| assets.join(BASES_FILE));
    let tool = match runtime
        .as_deref()
        .and_then(|path| crate::parse::bases::read(path, issues))
    {
        Some(read) => read,
        None => crate::parse::bases::parse("built-in bases.kdl", BUILT_IN, issues),
    };
    let mut bases = tool.bases;
    let mut capabilities = tool.capabilities;
    let mut shadows: Vec<Shadow> = Vec::new();
    let mut declared: Vec<(String, String)> = Vec::new();
    let mut named: Vec<(String, String)> = Vec::new();

    for collection in sources {
        let Some(dir) = crate::import::cached(root, collection) else {
            continue;
        };
        let Some(read) = crate::parse::bases::read(&dir.join(BASES_FILE), issues) else {
            continue;
        };
        let src = read.src;
        for base in read.bases {
            if let Some((_, first)) = declared.iter().find(|(image, _)| *image == base.image) {
                issues.push(
                    Issue::new(
                        format!("`{}` is described by two collections", base.image),
                        &src,
                    )
                    .at(base.span, format!("`{first}` describes it too"))
                    .help(
                        "one base is one entry: two of them would each write a different family \
                         and a different `provides` into an image scaffolded on it, and which one \
                         did would depend on the order repo.kdl lists the collections in",
                    ),
                );
                continue;
            }
            declared.push((base.image.clone(), collection.name.clone()));
            match bases.iter().position(|known| known.image == base.image) {
                Some(at) => {
                    if bases[at].differs(&base) {
                        shadows.push(Shadow {
                            image: base.image.clone(),
                            collection: collection.name.clone(),
                        });
                    }
                    bases[at] = base;
                }
                None => bases.push(base),
            }
        }
        for row in read.capabilities {
            let at = capabilities.iter().position(|known| known.name == row.name);
            if let Some((_, first)) = named.iter().find(|(name, _)| *name == row.name) {
                if at.is_some_and(|at| capabilities[at].path != row.path) {
                    issues.push(
                        Issue::new(
                            format!("capability `{}` is described by two collections", row.name),
                            &src,
                        )
                        .at(row.span, format!("`{first}` reads it elsewhere"))
                        .help(
                            "one name is read at one path; which row won would depend on the \
                             order repo.kdl lists the collections in",
                        ),
                    );
                }
                continue;
            }
            named.push((row.name.clone(), collection.name.clone()));
            match at {
                Some(at) => capabilities[at] = row,
                None => capabilities.push(row),
            }
        }
    }
    Loaded {
        bases,
        shadows,
        capabilities,
    }
}

fn undigested(image: &str) -> &str {
    image.split_once('@').map_or(image, |(before, _)| before)
}

pub fn find<'a>(bases: &'a [Base], image: &str) -> Option<&'a Base> {
    let image = undigested(image);
    bases.iter().find(|base| undigested(&base.image) == image)
}
