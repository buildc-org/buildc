use crate::model::remote::{At, Collection};
use crate::prompt::Prompt;
use crate::ui::Choice;
use std::path::{Path, PathBuf};

const CACHE: &str = "out/sources";

pub struct Found {
    pub owner: String,
    pub dir: PathBuf,
}

pub fn split(name: &str) -> (Option<&str>, &str) {
    match name.split_once('/') {
        Some((owner, module)) => (Some(owner), module),
        None => (None, name),
    }
}

fn names(sources: &[Collection]) -> String {
    sources
        .iter()
        .map(|c| c.name.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn find(root: &Path, sources: &[Collection], name: &str) -> Result<Vec<Found>, String> {
    let (owner, module) = split(name);
    if module.is_empty() || module.contains('/') || module.starts_with('.') {
        return Err(format!(
            "`{name}` is not a module: `<name>`, or `<owner>/<name>` when two collections have it"
        ));
    }
    if sources.is_empty() {
        return Err(
            "repo.kdl declares no `sources`, so there is no collection to import from".to_string(),
        );
    }

    let mut searched: Vec<&str> = Vec::new();
    let mut found: Vec<Found> = Vec::new();
    for collection in sources {
        if owner.is_some_and(|owner| owner != collection.name) {
            continue;
        }
        searched.push(&collection.name);
        let dir = tree(root, collection)?
            .join(collection.subtree().unwrap_or(""))
            .join(module);
        if dir.join("module.kdl").is_file() {
            found.push(Found {
                owner: collection.name.clone(),
                dir,
            });
        }
    }

    if searched.is_empty() {
        return Err(format!(
            "no collection called `{}`; repo.kdl declares {}",
            owner.unwrap_or_default(),
            names(sources)
        ));
    }
    if found.is_empty() {
        return Err(format!(
            "no module called `{module}` in {}",
            searched.join(", ")
        ));
    }
    Ok(found)
}

pub struct Listed {
    pub owner: String,
    pub name: String,
    pub qualified: String,
    pub description: String,
    pub requires: Vec<String>,
}

impl Listed {
    pub fn about(&self) -> String {
        match self.requires.is_empty() {
            true => self.description.clone(),
            false => format!(
                "{} (requires {})",
                self.description,
                self.requires.join(", ")
            ),
        }
    }
}

pub fn catalog(root: &Path, sources: &[Collection]) -> Result<Vec<Listed>, String> {
    let mut listed: Vec<Listed> = Vec::new();
    for collection in sources {
        let tree = tree(root, collection)?.join(collection.subtree().unwrap_or(""));
        let dirs = std::fs::read_dir(&tree)
            .map_err(|err| format!("`{}`: {}: {err}", collection.name, tree.display()))?;
        for dir in dirs.flatten().map(|entry| entry.path()) {
            if !dir.join("module.kdl").is_file() {
                continue;
            }
            let name = dir.file_name().unwrap_or_default().to_string_lossy();
            let (description, requires) = crate::parse::module::summary(&dir.join("module.kdl"));
            listed.push(Listed {
                qualified: format!("{}/{name}", collection.name),
                name: name.into_owned(),
                owner: collection.name.clone(),
                description,
                requires,
            });
        }
    }
    listed.sort_by(|a, b| (&a.name, &a.owner).cmp(&(&b.name, &b.owner)));
    Ok(listed)
}

pub fn choose(root: &Path, sources: &[Collection], prompt: &Prompt) -> Result<String, String> {
    let listed = catalog(root, sources)?;
    if listed.is_empty() {
        return Err(format!("no module in {}", names(sources)));
    }
    let options: Vec<Choice> = listed
        .iter()
        .map(|module| Choice::new(&module.qualified, module.about()))
        .collect();
    match prompt.choose("which module", &options)? {
        Some(chosen) => Ok(listed[chosen].qualified.clone()),
        None => Err("no module chosen; `buildc import module <name>` names one".to_string()),
    }
}

fn tree(root: &Path, collection: &Collection) -> Result<PathBuf, String> {
    let remote = match &collection.at {
        At::Dir(dir) => {
            let path = root.join(dir);
            return match path.is_dir() {
                true => Ok(path),
                false => Err(format!(
                    "`{}` is {}, which is not a directory on this machine",
                    collection.name,
                    path.display()
                )),
            };
        }
        At::Archive(remote) => remote,
    };

    let dir = root.join(CACHE).join(&collection.name);
    let pin = root.join(CACHE).join(format!("{}.pin", collection.name));
    if std::fs::read_to_string(&pin).ok().as_deref() == Some(remote.sha256.as_str()) {
        return Ok(dir);
    }

    let _ = std::fs::remove_dir_all(&dir);
    let url = remote.url_resolved();
    let target = dir.to_string_lossy().into_owned();
    crate::runtime::fetch(&[
        "tree",
        &url,
        &remote.sha256,
        &target,
        "--strip-components=1",
    ])
    .map_err(|err| format!("`{}`: {err}", collection.name))?;
    std::fs::write(&pin, &remote.sha256).map_err(|err| format!("{}: {err}", pin.display()))?;
    Ok(dir)
}

pub fn vendor(root: &Path, found: &Found, module: &str) -> Result<PathBuf, String> {
    let dest = PathBuf::from("modules").join(&found.owner).join(module);
    let full = root.join(&dest);
    if full.exists() {
        return Err(format!(
            "{} is already there; delete it to import over it",
            dest.display()
        ));
    }
    crate::init::copy_tree(&found.dir, &full)?;
    Ok(dest)
}
