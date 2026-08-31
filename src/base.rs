use crate::diag::{Issue, Issues, Span};
use crate::model::remote::Collection;
use std::path::Path;

pub const BASES_FILE: &str = "bases.kdl";

const BUILT_IN: &str = include_str!("../assets/bases.kdl");

pub struct Base {
    pub image: String,
    pub family: String,
    pub provides: Vec<String>,
    pub provides_files: Vec<String>,
    pub requires: Vec<String>,
    pub about: String,
    pub signed: bool,
    pub span: Span,
}

impl Base {
    fn differs(&self, other: &Base) -> bool {
        self.family != other.family
            || self.provides != other.provides
            || self.provides_files != other.provides_files
            || self.requires != other.requires
            || self.about != other.about
            || self.signed != other.signed
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
    let runtime = crate::init::assets()
        .ok()
        .map(|assets| assets.join(BASES_FILE));
    let mut bases = match runtime
        .as_deref()
        .and_then(|path| crate::parse::bases::read(path, issues))
    {
        Some((bases, _)) => bases,
        None => crate::parse::bases::parse("built-in bases.kdl", BUILT_IN, issues).0,
    };
    let mut shadows: Vec<Shadow> = Vec::new();
    let mut declared: Vec<(String, String)> = Vec::new();

    for collection in sources {
        let Some(dir) = crate::import::cached(root, collection) else {
            continue;
        };
        let Some((found, src)) = crate::parse::bases::read(&dir.join(BASES_FILE), issues) else {
            continue;
        };
        for base in found {
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
    }
    (bases, shadows)
}

pub fn find<'a>(bases: &'a [Base], image: &str) -> Option<&'a Base> {
    bases.iter().find(|base| base.image == image)
}
