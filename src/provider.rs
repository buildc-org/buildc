use crate::model::remote::{Collection, Kind as SourceKind};
use crate::parse::component::Summary;
use crate::parse::disk::Disk;
use crate::{layout, parse};
use std::collections::BTreeSet;
use std::path::Path;

pub struct Provider {
    pub owner: Option<String>,
    pub name: String,
    pub here: bool,
    pub declares: Summary,
}

impl Provider {
    pub fn qualified(&self) -> String {
        match &self.owner {
            Some(owner) => format!("{owner}/{}", self.name),
            None => self.name.clone(),
        }
    }

    pub fn about(&self) -> String {
        let requires = format!("(requires {})", self.declares.requires.join(", "));
        match (
            self.declares.description.is_empty(),
            self.declares.requires.is_empty(),
        ) {
            (_, true) => self.declares.description.clone(),
            (true, false) => requires,
            (false, false) => format!("{} {requires}", self.declares.description),
        }
    }
}

pub struct Index {
    held: Vec<Provider>,
    unread: Vec<String>,
    sourced: bool,
    unreached: Option<String>,
}

impl Index {
    pub fn local(root: &Path, sources: &[Collection], disk: &Disk) -> Self {
        Self {
            held: own(root, disk),
            unread: Vec::new(),
            sourced: sources
                .iter()
                .any(|collection| collection.kind == SourceKind::Components),
            unreached: None,
        }
    }

    pub fn scan(root: &Path, sources: &[Collection], disk: &Disk, fetch: bool) -> Self {
        let mut out = own(root, disk);
        let fetched = catalog(root, sources, fetch);
        let unreached = fetched.as_ref().err().cloned();
        let reached = fetched.is_ok();
        let found = fetched
            .or_else(|_| catalog(root, sources, false))
            .unwrap_or_default();
        out.extend(found);
        Self {
            held: out,
            unreached,
            unread: match fetch && reached {
                true => Vec::new(),
                false => sources
                    .iter()
                    .filter(|collection| collection.kind == SourceKind::Components)
                    .filter(|collection| {
                        !crate::collection::cached(root, collection).is_some_and(|dir| {
                            dir.join(collection.subtree().unwrap_or("")).is_dir()
                        })
                    })
                    .map(|collection| collection.name.clone())
                    .collect(),
            },
            sourced: sources
                .iter()
                .any(|collection| collection.kind == SourceKind::Components),
        }
    }

    pub fn unread(&self) -> &[String] {
        &self.unread
    }

    pub fn unreached(&self) -> Option<&str> {
        self.unreached.as_deref()
    }

    pub fn sourced(&self) -> bool {
        self.sourced
    }

    pub fn unsearched(&self) -> String {
        if let Some(why) = &self.unreached {
            return format!(
                "A declared collection could not be read, so nothing searched it: {why}"
            );
        }
        let named: Vec<String> = self.unread.iter().map(|name| format!("`{name}`")).collect();
        match named.len() {
            0 => String::new(),
            1 => format!(
                "The {} collection is declared but not on this machine, so nothing read it: `buildc import component` fetches it",
                named.join(", ")
            ),
            _ => format!(
                "The {} collections are declared but not on this machine, so nothing read them: `buildc import component` fetches them",
                named.join(", ")
            ),
        }
    }

    pub fn of(&self, capability: &str) -> Vec<&Provider> {
        self.held
            .iter()
            .filter(|held| {
                crate::resolve::answers(
                    &held.name,
                    held.declares.provides.iter().map(String::as_str),
                    capability,
                )
            })
            .collect()
    }

    pub fn fitting(&self, capability: &str, family: &str) -> Vec<&Provider> {
        self.of(capability)
            .into_iter()
            .filter(|held| crate::resolve::targets(&held.declares.targets, family))
            // A provider that supplies `base-image` is a whole base, so its capabilities come with the image and it never fits as the component that adds one.
            .filter(|held| {
                !held
                    .declares
                    .provides
                    .iter()
                    .any(|name| name == crate::parse::component::BASE_IMAGE)
            })
            .collect()
    }

    pub fn adapter(&self, capability: &str, family: &str) -> Option<&Provider> {
        self.fitting(capability, family).into_iter().next()
    }

    pub fn declaring_key(&self, kind: &str) -> Vec<&Provider> {
        self.held
            .iter()
            .filter(|held| held.declares.keys.iter().any(|has| has == kind))
            .collect()
    }

    pub fn claiming(&self, numbers: &BTreeSet<String>) -> Vec<&Provider> {
        self.held
            .iter()
            .filter(|held| {
                held.declares
                    .satisfies
                    .iter()
                    .any(|number| numbers.contains(number))
            })
            .collect()
    }

    pub fn at(&self, dir: &str) -> Option<&Provider> {
        self.held
            .iter()
            .find(|held| held.owner.is_none() && held.name == dir)
    }
}

pub fn catalog(root: &Path, sources: &[Collection], fetch: bool) -> Result<Vec<Provider>, String> {
    let mut listed: Vec<Provider> = Vec::new();
    for collection in sources {
        if collection.kind != SourceKind::Components {
            continue;
        }
        let tree = match fetch {
            true => crate::collection::tree(root, collection)?,
            false => match crate::collection::cached(root, collection) {
                Some(dir) => dir,
                None => continue,
            },
        };
        let tree = tree.join(collection.subtree().unwrap_or(""));
        if let Err(err) = std::fs::read_dir(&tree) {
            if !fetch {
                continue;
            }
            return Err(format!("`{}`: {}: {err}", collection.name, tree.display()));
        }
        for name in crate::collection::members(&tree) {
            listed.push(Provider {
                owner: Some(collection.name.clone()),
                declares: crate::parse::component::summary(
                    &tree.join(&name).join(layout::MANIFEST_FILE),
                ),
                name,
                here: false,
            });
        }
    }
    listed.sort_by(|a, b| (&a.name, &a.owner).cmp(&(&b.name, &b.owner)));
    Ok(listed)
}

fn own(root: &Path, disk: &Disk) -> Vec<Provider> {
    disk.components()
        .map(|dir| Provider {
            owner: None,
            name: dir.to_string(),
            here: true,
            declares: parse::component::summary(&layout::manifest(root, dir)),
        })
        .collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn two_providers(name: &str) -> std::path::PathBuf {
        let root =
            std::env::temp_dir().join(format!("buildc-providers-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        crate::init::put(
            &root.join("components/both/manifest.kdl"),
            "schema-version 1\n\ntargets \"fedora\"\n\nprovides \"x\" \"y\"\n",
        )
        .unwrap();
        crate::init::put(
            &root.join("components/other/manifest.kdl"),
            "schema-version 1\n\ntargets \"fedora\"\n\nprovides \"y\"\n",
        )
        .unwrap();
        root
    }

    #[test]
    fn an_entry_finds_the_component_it_names() {
        let root = two_providers("at");
        let index = Index::local(&root, &[], &Disk::scan(&root));
        let found = index.at("other").map(|held| held.name.as_str());
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(found, Some("other"));
    }
}
