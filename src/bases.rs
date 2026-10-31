use crate::model::component::BaseComponent;
use crate::model::remote::{Collection, Kind as SourceKind};
use std::path::Path;

pub fn catalog(root: &Path, sources: &[Collection]) -> Vec<BaseComponent> {
    let mut bases: Vec<BaseComponent> = Vec::new();
    let components = crate::layout::components(root);
    let mut trees: Vec<(Option<String>, std::path::PathBuf)> = vec![(None, components)];
    for collection in sources
        .iter()
        .filter(|collection| collection.kind == SourceKind::Components)
    {
        if let Some(dir) = crate::collection::cached(root, collection) {
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

fn undigested(image: &str) -> &str {
    image.split_once('@').map_or(image, |(before, _)| before)
}

pub fn find<'a>(bases: &'a [BaseComponent], image: &str) -> Option<&'a BaseComponent> {
    let image = undigested(image);
    bases
        .iter()
        .find(|base| undigested(&base.image) == image || base.name == image)
}
