use crate::layout;
use crate::model::image::{Entry, Held, Revision};
use crate::model::remote::{At, Collection};
use crate::parse::disk::Disk;
use crate::provenance::repo_lock::Lock;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub struct Libraries {
    root: PathBuf,
    local: Option<PathBuf>,
    root_library: Option<String>,
    lock: Lock,
    /// If `follow` is true, then each tag and each unpinned library is read past its lock entry.
    pub(crate) follow: bool,
    declared: Vec<Shelf>,
    default: Option<Option<String>>,
    pub collectors: BTreeMap<String, Collector>,
}

pub struct Collector {
    pub path: String,
    pub home: PathBuf,
}

struct Shelf {
    name: String,
    tree: Option<PathBuf>,
    components: Option<PathBuf>,
    pinned: bool,
    remote: Option<Remote>,
}

struct Remote {
    url: String,
    selector: String,
}

pub enum Unresolved {
    Undeclared(String),
    Absent {
        library: Option<String>,
        /// The libraries that were not searched because this machine holds no folder or cached tree of them.
        unread: Vec<String>,
    },
    Ambiguous(Vec<Option<String>>),
}

impl Libraries {
    pub fn read(
        root: &Path,
        libraries: &[Collection],
        root_library: Option<&str>,
        default: Option<&str>,
    ) -> Self {
        let root_library = root_library.map(str::to_string);
        let local = reads_own(libraries, root_library.as_deref()).then(|| layout::components(root));
        let locals: Vec<&str> = root_library
            .as_deref()
            .into_iter()
            .chain(
                libraries
                    .iter()
                    .filter(|library| library.pin().is_none())
                    .map(|library| library.name.as_str()),
            )
            .collect();
        let default = match (default, locals.as_slice()) {
            (Some(name), _) => Some(name),
            (None, [only]) => Some(*only),
            (None, _) => None,
        };
        let default = match default {
            Some(name) if Some(name) == root_library.as_deref() => Some(None),
            Some(name) => Some(Some(name.to_string())),
            None if locals.is_empty() => Some(None),
            None => None,
        };
        let declared: Vec<Shelf> = libraries
            .iter()
            .map(|library| {
                let tree = tree(root, library);
                Shelf {
                    name: library.name.clone(),
                    components: tree.as_deref().map(layout::library_components),
                    tree,
                    pinned: library.pin().is_some(),
                    remote: library.pin().and_then(|pin| {
                        Some(Remote {
                            url: pin.url.clone()?,
                            selector: pin.selector().to_string(),
                        })
                    }),
                }
            })
            .collect();
        let mut collectors = BTreeMap::new();
        for components in local.iter().chain(
            declared
                .iter()
                .filter_map(|shelf| shelf.components.as_ref()),
        ) {
            for (file, collector) in Disk::scan_at(components, &[]).collectors {
                collectors.entry(file).or_insert_with(|| Collector {
                    home: components.join(&collector),
                    path: collector,
                });
            }
        }
        Self {
            root: root.to_path_buf(),
            local,
            lock: Lock::read(root),
            follow: false,
            declared,
            default,
            root_library,
            collectors,
        }
    }

    pub fn collector_names(&self) -> Vec<String> {
        self.collectors.keys().cloned().collect()
    }

    pub fn home(&self) -> Result<PathBuf, String> {
        let name = match &self.default {
            Some(Some(name)) => name,
            Some(None) => return Ok(layout::components(&self.root)),
            None if self.root_library.is_some() => return Ok(layout::components(&self.root)),
            None => {
                return Err(
                    "`libraries` declares several local libraries and names no `default=`; \
                     set `default=` to the library that takes new components"
                        .to_string(),
                )
            }
        };
        let shelf = self
            .declared
            .iter()
            .find(|shelf| &shelf.name == name)
            .ok_or_else(|| format!("`libraries default=\"{name}\"` names no declared library"))?;
        if shelf.pinned {
            return Err(format!(
                "`libraries default=\"{name}\"` names a remote library, and buildc writes no \
                 component into a remote library; set `default=` to a local library"
            ));
        }
        shelf
            .components
            .clone()
            .ok_or_else(|| format!("the default library `{name}` has no folder on this machine"))
    }

    pub fn reads_own(&self) -> bool {
        self.local.is_some()
    }

    pub fn dir(&self, entry: &Entry) -> PathBuf {
        match &entry.held {
            Some(held) => held.components.join(&entry.path),
            None => layout::component(&self.root, &entry.path),
        }
    }

    pub fn resolve(
        &self,
        path: &str,
        written: Option<&str>,
        rooted: bool,
        referrer: Option<&str>,
    ) -> Result<Option<Held>, Unresolved> {
        if let Some(name) = written {
            // This repository's own library has no shelf.
            // Its name reads the local components folder.
            let own = Some(name) == self.root_library.as_deref();
            return self.within((!own).then_some(name), path);
        }
        if !rooted {
            return self.within(referrer, path);
        }
        let holders: Vec<Option<&str>> = self
            .local
            .iter()
            .map(|_| None)
            .chain(self.declared.iter().map(|shelf| Some(shelf.name.as_str())))
            .filter(|library| self.within(*library, path).is_ok())
            .collect();
        let default = self.default.as_ref().map(Option::as_deref);
        let chosen = match holders.as_slice() {
            [] => {
                return Err(Unresolved::Absent {
                    library: None,
                    unread: self
                        .declared
                        .iter()
                        .filter(|shelf| shelf.components.is_none())
                        .map(|shelf| shelf.name.clone())
                        .collect(),
                });
            }
            [only] => *only,
            // A library component reads its own library first so that the library keeps its own siblings.
            _ if referrer.is_some() && holders.contains(&referrer) => referrer,
            _ if default.is_some_and(|default| holders.contains(&default)) => default.flatten(),
            many => {
                return Err(Unresolved::Ambiguous(
                    many.iter()
                        .map(|library| library.map(str::to_string))
                        .collect(),
                ));
            }
        };
        self.within(chosen, path)
    }

    fn within(&self, library: Option<&str>, path: &str) -> Result<Option<Held>, Unresolved> {
        let Some(name) = library else {
            return match self
                .local
                .as_ref()
                .is_some_and(|local| self.holds(local, path))
            {
                true => Ok(None),
                false => Err(Unresolved::Absent {
                    library: None,
                    unread: Vec::new(),
                }),
            };
        };
        let shelf = self
            .declared
            .iter()
            .find(|shelf| shelf.name == name)
            .ok_or_else(|| Unresolved::Undeclared(name.to_string()))?;
        match (&shelf.tree, &shelf.components) {
            (Some(tree), Some(components)) if self.holds(components, path) => Ok(Some(Held {
                library: shelf.name.clone(),
                tree: tree.clone(),
                components: components.clone(),
                remote: None,
            })),
            (Some(_), Some(_)) => Err(Unresolved::Absent {
                library: Some(shelf.name.clone()),
                unread: Vec::new(),
            }),
            _ => Err(Unresolved::Absent {
                library: Some(shelf.name.clone()),
                unread: vec![shelf.name.clone()],
            }),
        }
    }

    pub fn revise(&self, entry: &mut Entry, build: &str) -> Result<(), String> {
        let path = entry.path.clone();
        let pin = entry.pin.clone();
        let remote = entry.held.as_ref().and_then(|held| {
            self.declared
                .iter()
                .find(|shelf| shelf.name == held.library)
                .and_then(|shelf| {
                    shelf
                        .remote
                        .as_ref()
                        .zip(shelf.tree.as_ref().zip(shelf.components.as_ref()))
                })
        });
        let Some(((remote, (cached, components)), held)) = remote.zip(entry.held.as_mut()) else {
            return match pin {
                Some(pin) => Err(format!(
                    "`{path}` is in {}, which the user edits in place, so `pin=\"{pin}\"` has no \
                     revision to read",
                    shown(entry.held.as_ref().map(|held| held.library.as_str()))
                )),
                None => Ok(()),
            };
        };
        let locked = self.lock.find(build, &held.library, &path, pin.as_deref());
        let url = remote.url.as_str();
        let (commit, tree) = match (&pin, locked.filter(|_| !self.follow)) {
            (Some(pin), _) if crate::collection::is_commit(pin) => (
                pin.clone(),
                crate::collection::at_commit(&self.root, url, pin)
                    .map_err(|err| format!("`{path}` pins {pin}: {err}"))?,
            ),
            (_, Some(locked)) => {
                let commit = locked.commit.clone();
                let tree = crate::collection::at_commit(&self.root, url, &commit)
                    .map_err(|err| format!("`{path}` is locked at {commit}: {err}"))?;
                (commit, tree)
            }
            (Some(pin), None) => {
                crate::collection::at_tag(&self.root, url, pin, self.follow).map_err(|err| {
                    format!(
                        "`{path}` pins `{pin}`. buildc could not read `{pin}` as a tag of {url}; a pin is a tag or a \
                     full 40-character commit: {err}"
                    )
                })?
            }
            (None, None) => {
                let commit = crate::collection::cached_commit(&self.root, url, &remote.selector)
                    .ok_or_else(|| format!("the cache records no commit for {url}; `buildc generate` fetches it"))?;
                held.tree = cached.clone();
                held.components = components.clone();
                held.remote = Some(Revision {
                    source: url.to_string(),
                    commit,
                });
                return Ok(());
            }
        };
        held.components = layout::library_components(&tree);
        held.tree = tree;
        held.remote = Some(Revision {
            source: url.to_string(),
            commit: commit.clone(),
        });
        let Some(locked) = locked.filter(|locked| locked.commit == commit) else {
            return Ok(());
        };
        let found = crate::provenance::repo_lock::content(&held.components.join(&path));
        if found.as_deref() == Some(locked.content.as_str()) {
            return Ok(());
        }
        Err(crate::provenance::repo_lock::mismatch(
            build,
            locked,
            found.as_deref(),
        ))
    }

    fn holds(&self, components: &Path, path: &str) -> bool {
        if !crate::model::image::is_local_path(path) {
            return false;
        }
        let dir = components.join(path);
        layout::is_component(&dir) || self.collectors.keys().any(|file| dir.join(file).is_file())
    }
}

pub fn split_root(
    root: &Path,
    mut libraries: Vec<Collection>,
) -> (Vec<Collection>, Option<String>) {
    let canonical = std::fs::canonicalize(root).ok();
    let is_root = |dir: &str| {
        Path::new(dir)
            .components()
            .all(|part| part == std::path::Component::CurDir)
            || canonical.is_some() && std::fs::canonicalize(root.join(dir)).ok() == canonical
    };
    let own = libraries.iter().position(|library| match &library.at {
        At::Dir(dir) => is_root(dir),
        At::Git(_) => false,
    });
    let name = own.map(|at| libraries.remove(at).name);
    (libraries, name)
}

pub fn unread_folders(
    root: &Path,
    libraries: &[Collection],
    root_library: Option<&str>,
) -> Option<String> {
    if reads_own(libraries, root_library) {
        return None;
    }
    let folders: Vec<String> = [
        layout::components(root),
        root.join("workflows"),
        root.join("plugins"),
    ]
    .iter()
    .filter(|folder| folder.is_dir())
    .map(|folder| format!("`{}/`", layout::under(root, folder).display()))
    .collect();
    if folders.is_empty() {
        return None;
    }
    Some(format!(
        "{} {} in no library, because `libraries` declares a `local` library and none has the \
         source `.`; add `{} {{ source \".\" }}` under `local` to read {}",
        crate::said(&folders),
        if folders.len() == 1 { "is" } else { "are" },
        layout::ROOT_LIBRARY,
        if folders.len() == 1 { "it" } else { "them" },
    ))
}

/// A repository that declares no local library reads its own root as its local library.
pub fn reads_own(libraries: &[Collection], root_library: Option<&str>) -> bool {
    root_library.is_some() || libraries.iter().all(|library| library.pin().is_some())
}

fn tree(root: &Path, library: &Collection) -> Option<PathBuf> {
    match &library.at {
        At::Dir(dir) => Some(root.join(dir)).filter(|dir| dir.is_dir()),
        At::Git(_) => crate::collection::cached(root, library),
    }
}

pub fn shown(library: Option<&str>) -> String {
    match library {
        Some(name) => format!("`{name}`"),
        None => "this repository".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local(name: &str, dir: &str) -> Collection {
        Collection {
            name: name.to_string(),
            at: At::Dir(dir.to_string()),
            span: crate::diag::Span::default(),
        }
    }

    #[test]
    fn a_default_folder_outside_every_library_is_named_with_the_entry_to_add() {
        let root = std::env::temp_dir().join(format!("buildc-unread-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        crate::init::put(&root.join(layout::REPO_FILE), "name \"Example\"\n").unwrap();
        std::fs::create_dir_all(root.join("components")).unwrap();
        let tools = [local("tools", "tools")];
        assert_eq!(unread_folders(&root, &[], None), None);
        assert_eq!(unread_folders(&root, &[], Some("example-library")), None);
        let said = unread_folders(&root, &tools, None).expect("the root folder is unread");
        assert!(said.starts_with("`components/` is in no library"), "{said}");
        assert!(
            said.ends_with("add `local-library { source \".\" }` under `local` to read it"),
            "{said}"
        );
        std::fs::create_dir_all(root.join("plugins")).unwrap();
        let said = unread_folders(&root, &tools, None).unwrap();
        assert!(
            said.starts_with("`components/` and `plugins/` are in no library"),
            "{said}"
        );
        std::fs::remove_dir_all(root.join("components")).unwrap();
        std::fs::remove_dir_all(root.join("plugins")).unwrap();
        assert_eq!(unread_folders(&root, &tools, None), None);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn the_root_library_is_found_by_its_text_or_by_its_folder() {
        let absent =
            std::env::temp_dir().join(format!("buildc-split-absent-{}", std::process::id()));
        // The absent root has no canonical path.
        // Only the text `.` can match it.
        let (rest, own) = split_root(&absent, vec![local("mine", "."), local("tools", "tools")]);
        assert_eq!(own.as_deref(), Some("mine"));
        assert_eq!(rest.len(), 1);

        let root = std::env::temp_dir().join(format!("buildc-split-root-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let (rest, own) = split_root(&root, vec![local("mine", &root.display().to_string())]);
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(own.as_deref(), Some("mine"));
        assert!(rest.is_empty());
    }
}
