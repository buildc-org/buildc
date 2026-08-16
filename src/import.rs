use crate::model::remote::{At, Collection};
use crate::prompt::Prompt;
use crate::provenance::record;
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

pub fn find(
    root: &Path,
    sources: &[Collection],
    name: &str,
    enforce: bool,
) -> Result<Vec<Found>, String> {
    let (owner, module) = split(name);
    if module.is_empty() || module.contains('/') || module.starts_with('.') {
        return Err(format!(
            "`{name}` is not a module: `<name>`, or `<owner>/<name>` when two collections have it"
        ));
    }
    if sources.is_empty() {
        const NONE: &str = "repo.kdl declares no `sources`, so there is no collection to import \
                            from";
        let block = crate::init::assets().map(|dir| crate::init::sources(&dir));
        return Err(match block.as_deref().unwrap_or("") {
            "" => format!("{NONE}, and no collection is scaffolded to name one"),
            block => format!("{NONE}.\n\nThis is the block `buildc create repo` writes:\n\n{block}"),
        });
    }

    let mut searched: Vec<&str> = Vec::new();
    let mut found: Vec<Found> = Vec::new();
    for collection in sources {
        if owner.is_some_and(|owner| owner != collection.name) {
            continue;
        }
        if enforce && collection.unpinned() {
            return Err(format!(
                "`{}` follows a moving ref with no `sha256`, so an import of it is verified \
                 against nothing; `audit {{ enforce #true }}` makes that an error. Pin the \
                 collection to a tag and its hash, or drop the enforcement",
                collection.name
            ));
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
    pub keys: Vec<String>,
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
            let (description, requires, keys) =
                crate::parse::module::summary(&dir.join("module.kdl"));
            listed.push(Listed {
                qualified: format!("{}/{name}", collection.name),
                name: name.into_owned(),
                owner: collection.name.clone(),
                description,
                requires,
                keys,
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

pub fn cached(root: &Path, collection: &Collection) -> Option<PathBuf> {
    let dir = match &collection.at {
        At::Dir(dir) => root.join(dir),
        At::Archive(pin) if pin.unpinned() => root.join(CACHE).join(&collection.name),
        At::Archive(pin) => {
            let stamp = root.join(CACHE).join(format!("{}.pin", collection.name));
            match std::fs::read_to_string(&stamp).ok().as_deref() == pin.sha256.as_deref() {
                true => root.join(CACHE).join(&collection.name),
                false => return None,
            }
        }
    };
    dir.is_dir().then_some(dir)
}

fn tree(root: &Path, collection: &Collection) -> Result<PathBuf, String> {
    if !collection.unpinned() {
        if let Some(dir) = cached(root, collection) {
            return Ok(dir);
        }
    }
    let remote = match &collection.at {
        At::Dir(dir) => {
            return Err(format!(
                "`{}` is {}, which is not a directory on this machine",
                collection.name,
                root.join(dir).display()
            ))
        }
        At::Archive(remote) => remote,
    };

    let dir = root.join(CACHE).join(&collection.name);
    let pin = root.join(CACHE).join(format!("{}.pin", collection.name));
    let _ = std::fs::remove_dir_all(&dir);
    let url = remote.url_resolved().unwrap_or_default();
    let sha256 = (!remote.unpinned())
        .then(|| remote.sha256.as_deref())
        .flatten();
    crate::runtime::extract(&url, sha256, &dir, &["--strip-components=1"])
        .map_err(|err| format!("`{}`: {err}", collection.name))?;
    match sha256 {
        Some(sha256) => {
            std::fs::write(&pin, sha256).map_err(|err| format!("{}: {err}", pin.display()))?
        }
        None => drop(std::fs::remove_file(&pin)),
    }
    Ok(dir)
}

pub fn destination(root: &Path, found: &Found, module: &str) -> Result<PathBuf, String> {
    let dest = PathBuf::from("modules").join(&found.owner).join(module);
    match root.join(&dest).exists() {
        true => Err(format!(
            "{} is already there; delete it to import over it",
            dest.display()
        )),
        false => Ok(dest),
    }
}

pub fn vendor(
    root: &Path,
    sources: &[Collection],
    found: &Found,
    dest: &Path,
) -> Result<(), String> {
    let dir = root.join(dest);
    crate::init::copy_tree(&found.dir, &dir)?;
    let pin = sources
        .iter()
        .find(|c| c.name == found.owner)
        .and_then(Collection::pin);
    let content = record::hash(&dir).unwrap_or_default();
    crate::init::put(
        &dir.join(record::RECORD),
        &record::write(&found.owner, pin, &content),
    )
}
