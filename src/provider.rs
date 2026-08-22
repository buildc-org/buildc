use crate::model::remote::{Collection, REMOTE_DIR};
use crate::parse::disk::Disk;
use crate::parse::module::Summary;
use crate::{layout, parse};
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

    pub fn dir(&self) -> String {
        match &self.owner {
            Some(owner) => format!("{REMOTE_DIR}/{owner}/{}", self.name),
            None => self.name.clone(),
        }
    }

    pub fn about(&self) -> String {
        match self.declares.requires.is_empty() {
            true => self.declares.description.clone(),
            false => format!(
                "{} (requires {})",
                self.declares.description,
                self.declares.requires.join(", ")
            ),
        }
    }
}

pub struct Index(Vec<Provider>);

impl Index {
    pub fn scan(root: &Path, sources: &[Collection], disk: &Disk, fetch: bool) -> Self {
        let mut out: Vec<Provider> = Vec::new();
        for dir in disk.modules() {
            let (owner, name) = split(dir);
            out.push(Provider {
                owner,
                name,
                here: true,
                declares: parse::module::summary(&layout::manifest(root, dir)),
            });
        }
        for module in crate::import::catalog(root, sources, fetch).unwrap_or_default() {
            if !out.iter().any(|held| held.dir() == module.dir()) {
                out.push(module);
            }
        }
        Self(out)
    }

    pub fn of(&self, capability: &str) -> Vec<&Provider> {
        self.0
            .iter()
            .filter(|held| held.declares.provides.iter().any(|has| has == capability))
            .collect()
    }

    pub fn adapter(&self, capability: &str, family: &str) -> Option<&Provider> {
        self.of(capability)
            .into_iter()
            .find(|held| held.declares.supports.iter().any(|has| has == family))
    }

    pub fn declaring_key(&self, kind: &str) -> Vec<&Provider> {
        self.0
            .iter()
            .filter(|held| held.declares.keys.iter().any(|has| has == kind))
            .collect()
    }

    pub fn at(&self, dir: &str) -> Option<&Provider> {
        self.0.iter().find(|held| held.dir() == dir)
    }
}

fn split(dir: &str) -> (Option<String>, String) {
    match dir
        .strip_prefix(REMOTE_DIR)
        .and_then(|rest| rest.strip_prefix('/'))
        .and_then(|rest| rest.split_once('/'))
    {
        Some((owner, name)) => (Some(owner.to_string()), name.to_string()),
        None => (None, dir.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fetched_module_is_the_collection_member_it_came_from() {
        let (owner, name) = split(".remote/one/browser");
        assert_eq!((owner.as_deref(), name.as_str()), (Some("one"), "browser"));
        let (owner, name) = split("my/editor");
        assert_eq!((owner, name.as_str()), (None, "my/editor"));
    }
}
