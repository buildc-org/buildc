use crate::model::remote::{Collection, REMOTE_DIR};
use crate::parse::disk::Disk;
use crate::parse::module::Summary;
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

pub struct Index {
    held: Vec<Provider>,
    unread: Vec<String>,
    sourced: bool,
    hidden: Vec<String>,
}

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
        let (found, hidden) = crate::import::catalog(root, sources, fetch).unwrap_or_default();
        for module in found {
            if !out.iter().any(|held| held.dir() == module.dir()) {
                out.push(module);
            }
        }
        Self {
            held: out,
            hidden,
            unread: match fetch {
                true => Vec::new(),
                false => sources
                    .iter()
                    .filter(|collection| crate::import::cached(root, collection).is_none())
                    .map(|collection| collection.name.clone())
                    .collect(),
            },
            sourced: !sources.is_empty(),
        }
    }

    pub fn unread(&self) -> &[String] {
        &self.unread
    }

    pub fn sourced(&self) -> bool {
        self.sourced
    }

    pub fn hidden(&self) -> &[String] {
        &self.hidden
    }

    pub fn unsearched(&self) -> String {
        let named: Vec<String> = self.unread.iter().map(|name| format!("`{name}`")).collect();
        match named.len() {
            0 => String::new(),
            1 => format!(
                "The {} collection is declared but not on this machine, so nothing read it: `buildc fetch modules` downloads it",
                named.join(", ")
            ),
            _ => format!(
                "The {} collections are declared but not on this machine, so nothing read them: `buildc fetch modules` downloads them",
                named.join(", ")
            ),
        }
    }

    pub fn of(&self, capability: &str) -> Vec<&Provider> {
        self.held
            .iter()
            .filter(|held| held.declares.provides.iter().any(|has| has == capability))
            .collect()
    }

    pub fn fitting(&self, capability: &str, family: &str) -> Vec<&Provider> {
        self.of(capability)
            .into_iter()
            .filter(|held| held.declares.supports.iter().any(|has| has == family))
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
        self.held.iter().find(|held| held.dir() == dir)
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
