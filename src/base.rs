use crate::model::remote::{Collection, Kind as SourceKind};
use std::path::Path;

pub const LIBRARY_URL: &str = "https://github.com/buildc-org/library";

pub struct Base {
    pub name: String,
    pub owner: Option<String>,
    pub image: String,
    pub family: String,
    pub provides: Vec<String>,
    pub requires: Vec<String>,
    pub imports: Vec<String>,
    pub about: String,
    pub version: Option<String>,
    pub scap_content: String,
    pub bootloaders: Vec<String>,
}

impl Base {
    pub fn label(&self) -> String {
        match &self.owner {
            Some(owner) => format!("{} ({owner}/{})", self.image, self.name),
            None => format!("{} ({})", self.image, self.name),
        }
    }
}

pub struct DefaultLibrary {
    pub kind: SourceKind,
    pub alias: &'static str,
    pub path: &'static str,
    pub about: &'static str,
}

pub const DEFAULT_LIBRARIES: [DefaultLibrary; 1] = [DefaultLibrary {
    kind: SourceKind::Components,
    alias: "buildc-components",
    path: "components",
    about: "Feature components and base images for many use cases",
}];

pub fn default_library(kind: SourceKind) -> &'static DefaultLibrary {
    DEFAULT_LIBRARIES
        .iter()
        .find(|library| library.kind == kind)
        .expect("every source kind has a default library")
}

pub fn catalog(root: &Path, sources: &[Collection]) -> Vec<Base> {
    let mut bases: Vec<Base> = Vec::new();
    let components = crate::layout::components(root);
    let mut trees: Vec<(Option<String>, std::path::PathBuf)> = vec![(None, components)];
    for collection in sources
        .iter()
        .filter(|collection| collection.kind == SourceKind::Components)
    {
        if let Some(dir) = crate::import::cached(root, collection) {
            trees.push((
                Some(collection.name.clone()),
                dir.join(collection.subtree().unwrap_or("")),
            ));
        }
    }
    for (owner, tree) in trees {
        for dir in crate::layout::member_dirs(&tree) {
            let Ok(name) = dir.strip_prefix(&tree) else {
                continue;
            };
            let name = name.display().to_string();
            if bases.iter().any(|base| base.name == name) {
                continue;
            }
            if let Some(base) = crate::parse::component::offered_base(&dir, name, owner.clone()) {
                bases.push(base);
            }
        }
    }
    bases
}

pub fn fetch(root: &Path, sources: &[Collection]) -> Result<(), String> {
    for collection in sources {
        if crate::import::cached(root, collection).is_none() {
            crate::import::tree(root, collection)?;
        }
    }
    Ok(())
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
