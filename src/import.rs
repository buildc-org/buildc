use crate::create::{report, Change, Listing};
use crate::dispatch::Error;
use crate::layout;
use crate::model::remote::{At, Collection, REMOTE_DIR};
use crate::prompt::Prompt;
use crate::provenance::record;
use crate::provider::Provider;
use crate::ui::Choice;
use std::path::{Path, PathBuf};

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
                "`{}` follows a moving ref with no `sha256`, so using it is verified \
                 against nothing; `audit {{ enforce #true }}` makes that an error. Pin the \
                 collection to a tag and its hash, or drop the enforcement",
                collection.name
            ));
        }
        searched.push(&collection.name);
        let dir = tree(root, collection)?
            .join(collection.subtree().unwrap_or(""))
            .join(module);
        if dir.join(layout::MODULE_FILE).is_file() {
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

pub fn catalog(root: &Path, sources: &[Collection], fetch: bool) -> Result<Vec<Provider>, String> {
    let mut listed: Vec<Provider> = Vec::new();
    for collection in sources {
        let tree = match fetch {
            true => tree(root, collection)?,
            false => match cached(root, collection) {
                Some(dir) => dir,
                None => continue,
            },
        };
        let tree = tree.join(collection.subtree().unwrap_or(""));
        let dirs = match std::fs::read_dir(&tree) {
            Ok(dirs) => dirs,
            Err(_) if !fetch => continue,
            Err(err) => return Err(format!("`{}`: {}: {err}", collection.name, tree.display())),
        };
        for dir in dirs.flatten().map(|entry| entry.path()) {
            let manifest = dir.join(layout::MODULE_FILE);
            if !manifest.is_file() {
                continue;
            }
            listed.push(Provider {
                owner: Some(collection.name.clone()),
                name: dir
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                here: false,
                declares: crate::parse::module::summary(&manifest),
            });
        }
    }
    listed.sort_by(|a, b| (&a.name, &a.owner).cmp(&(&b.name, &b.owner)));
    Ok(listed)
}

fn offered(root: &Path, sources: &[Collection]) -> Result<(Vec<Provider>, Vec<Choice>), String> {
    let listed = catalog(root, sources, true)?;
    if listed.is_empty() {
        return Err(format!("no module in {}", names(sources)));
    }
    let options = listed
        .iter()
        .map(|module| Choice::new(module.qualified(), module.about()))
        .collect();
    Ok((listed, options))
}

fn unchosen(command: &str) -> String {
    format!("no module chosen; `buildc {command} module <name>` names one")
}

pub fn choose(
    root: &Path,
    sources: &[Collection],
    command: &str,
    prompt: &Prompt,
) -> Result<String, String> {
    let (listed, options) = offered(root, sources)?;
    match prompt.choose("which module", &options)? {
        Some(chosen) => Ok(listed[chosen].qualified()),
        None => Err(unchosen(command)),
    }
}

fn choose_several(
    root: &Path,
    sources: &[Collection],
    command: &str,
    prompt: &Prompt,
) -> Result<Vec<String>, String> {
    let (listed, options) = offered(root, sources)?;
    let chosen = match prompt.choose_many("which modules", &options, &[])? {
        crate::ui::Answer::Chosen(chosen) => chosen,
        crate::ui::Answer::Cancelled => Vec::new(),
    };
    match chosen.is_empty() {
        true => Err(unchosen(command)),
        false => Ok(chosen.iter().map(|at| listed[*at].qualified()).collect()),
    }
}

pub fn cached(root: &Path, collection: &Collection) -> Option<PathBuf> {
    let dir = match &collection.at {
        At::Dir(dir) => root.join(dir),
        At::Archive(pin) if pin.unpinned() => {
            root.join(layout::SOURCES_CACHE).join(&collection.name)
        }
        At::Archive(pin) => {
            let stamp = root
                .join(layout::SOURCES_CACHE)
                .join(format!("{}.pin", collection.name));
            match std::fs::read_to_string(&stamp).ok().as_deref() == pin.sha256.as_deref() {
                true => root.join(layout::SOURCES_CACHE).join(&collection.name),
                false => return None,
            }
        }
    };
    dir.is_dir().then_some(dir)
}

pub(crate) fn tree(root: &Path, collection: &Collection) -> Result<PathBuf, String> {
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

    let dir = root.join(layout::SOURCES_CACHE).join(&collection.name);
    let pin = root
        .join(layout::SOURCES_CACHE)
        .join(format!("{}.pin", collection.name));
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
    let dest = PathBuf::from(layout::MODULES).join(module);
    let dir = root.join(&dest);
    if !dir.exists() {
        return Ok(dest);
    }
    let mut issues = crate::diag::Issues::default();
    match record::read(&dir, &mut issues) {
        Some(record) if record.collection == found.owner => Err(format!(
            "`{module}` was already copied from `{}`",
            found.owner
        )),
        Some(record) => Err(format!(
            "`{module}` was copied from `{}`; refusing the namesake from `{}`",
            record.collection, found.owner
        )),
        None => Err(format!(
            "{} is already there; delete it to copy over it",
            dest.display()
        )),
    }
}

pub fn vendor(
    root: &Path,
    sources: &[Collection],
    found: &Found,
    dest: &Path,
) -> Result<Vec<PathBuf>, String> {
    let dir = root.join(dest);
    let mut wrote: Vec<PathBuf> = crate::init::copy_tree(&found.dir, &dir)?
        .into_iter()
        .map(|under| dest.join(under))
        .collect();
    let pin = sources
        .iter()
        .find(|c| c.name == found.owner)
        .and_then(Collection::pin);
    let content = record::hash(&dir).unwrap_or_default();
    crate::init::put(
        &dir.join(record::RECORD),
        &record::write(&found.owner, pin, &content),
    )?;
    wrote.push(dest.join(record::RECORD));
    Ok(wrote)
}

fn select(
    name: Option<String>,
    root: &Path,
    sources: &[Collection],
    enforce: bool,
    command: &str,
    prompt: &Prompt,
) -> Result<(Found, String), Error> {
    let name = match name {
        Some(name) => name,
        None => choose(root, sources, command, prompt)?,
    };
    resolve(&name, root, sources, enforce, prompt)
}

fn resolve(
    name: &str,
    root: &Path,
    sources: &[Collection],
    enforce: bool,
    prompt: &Prompt,
) -> Result<(Found, String), Error> {
    let mut found = find(root, sources, name, enforce)?;
    let module = split(name).1.to_string();
    let at = match found.as_slice() {
        [_] => 0,
        many => {
            let owners: Vec<String> = many.iter().map(|f| f.owner.clone()).collect();
            let listed = owners.join(", ");
            let options: Vec<Choice> = owners.iter().map(|owner| Choice::new(owner, "")).collect();
            prompt
                .choose(&format!("`{module}` is in {listed}; which one"), &options)?
                .ok_or_else(|| {
                    format!(
                        "`{module}` is in {listed}; name which one, as `{}/{module}`",
                        owners[0]
                    )
                })?
        }
    };
    Ok((found.swap_remove(at), module))
}

struct Member {
    from: Found,
    name: String,
}

pub struct Module {
    members: Vec<Member>,
    listing: Listing,
    workflows: Option<crate::set::Workflows>,
    conforms: Vec<crate::set::Conforms>,
}

impl Module {
    pub fn collect(
        name: Option<String>,
        root: &Path,
        sources: &[Collection],
        enforce: bool,
        images: Vec<String>,
        datastream: Option<&Path>,
        prompt: &Prompt,
    ) -> Result<Self, Error> {
        let (list, _) = crate::model::image::List::load(root);
        if list.images.is_empty() {
            return Err(
                "`import module` needs an image; run `buildc create image <name>` first"
                    .to_string()
                    .into(),
            );
        }
        let asked = match name {
            Some(name) => vec![name],
            None => choose_several(root, sources, "import", prompt)?,
        };
        let picked = asked
            .iter()
            .map(|name| resolve(name, root, sources, enforce, prompt))
            .collect::<Result<Vec<(Found, String)>, Error>>()?;
        let declares: Vec<crate::parse::module::Summary> = picked
            .iter()
            .map(|(from, _)| crate::parse::module::summary(&from.dir.join(layout::MODULE_FILE)))
            .collect();

        let listing = Listing::collect(root, images, prompt)?;
        for (from, name) in &picked {
            listing.refuse_duplicate(&list, name, Some(&from.owner))?;
        }
        let named: Vec<String> = picked.iter().map(|(_, name)| name.clone()).collect();

        let also = short(root, sources, &list, &listing, &named, &declares, prompt)?
            .into_iter()
            .filter(|qualified| {
                !picked
                    .iter()
                    .any(|(from, name)| format!("{}/{name}", from.owner) == *qualified)
            })
            .map(|qualified| {
                let mut found = find(root, sources, &qualified, enforce)?;
                let member = split(&qualified).1.to_string();
                Ok(Member {
                    from: found.swap_remove(0),
                    name: member,
                })
            })
            .collect::<Result<Vec<Member>, String>>()?;

        let unlocked = match listing.images().is_empty() {
            true => Vec::new(),
            false => {
                let args: Vec<String> =
                    declares.iter().flat_map(|held| held.args.clone()).collect();
                crate::resolve::workflow::unlocked(&list, &args)
            }
        };
        let workflows = match unlocked.as_slice() {
            [] => None,
            unlocked => {
                let named: Vec<&'static str> = unlocked.iter().map(|s| s.stem).collect();
                let rows: Vec<String> = unlocked
                    .iter()
                    .map(|s| format!("`{}` {}", s.stem, s.about))
                    .collect();
                let question = format!(
                    "{} {} CI runnable that this repo does not generate:\n{}\n\
                     Generate it now?",
                    said(&quoted(&picked)),
                    match picked.len() {
                        1 => "makes",
                        _ => "make",
                    },
                    rows.join("\n")
                );
                prompt
                    .confirm(&question, "Yes", "No")?
                    .then(|| crate::set::Workflows::adding(&list, &named))
            }
        };

        let mut members = also;
        members.extend(picked.into_iter().map(|(from, name)| Member { from, name }));
        let brought: Vec<String> = members.iter().map(|member| member.name.clone()).collect();
        let conforms = measured(
            &list, &listing, &named, &declares, datastream, &brought, prompt,
        )?;
        Ok(Self {
            members,
            listing,
            workflows,
            conforms,
        })
    }

    pub fn apply(&self, root: &Path) -> Result<(), String> {
        let wrote = self.write(root)?;
        if !wrote.is_empty() {
            report(root, &wrote);
        }
        Ok(())
    }

    pub(crate) fn write(&self, root: &Path) -> Result<Vec<(PathBuf, Change)>, String> {
        match self.listing {
            Listing::Cancelled => return Ok(Vec::new()),
            Listing::NoImage => {
                return Err(
                    "`import module` needs an image; run `buildc create image <name>` first".into(),
                )
            }
            Listing::Declined { asked: false } => {
                return Err(
                    "`import module` needs an image to list it in; name one with `--image`".into(),
                )
            }
            Listing::Declined { asked: true } => {
                return Err(
                    "an import is a reference an image lists, so listing it in none imports \
                     nothing"
                        .into(),
                )
            }
            Listing::In(_) => {}
        }
        for member in &self.members {
            let dir = layout::module(root, REMOTE_DIR)
                .join(&member.from.owner)
                .join(&member.name);
            let _ = std::fs::remove_dir_all(&dir);
            crate::init::copy_tree(&member.from.dir, &dir)?;
        }
        let listed: Vec<(&str, &str)> = self
            .members
            .iter()
            .map(|member| (member.from.owner.as_str(), member.name.as_str()))
            .collect();
        let mut wrote: Vec<(PathBuf, Change)> = self.listing.apply_source(&listed)?;
        if let Some(workflows) = &self.workflows {
            wrote.extend(workflows.apply(root)?);
        }
        for conforms in &self.conforms {
            wrote.extend(conforms.apply(root)?);
        }
        Ok(wrote)
    }

    pub(crate) fn brought(&self) -> Vec<String> {
        self.members
            .iter()
            .map(|member| member.name.clone())
            .collect()
    }
}

pub fn bring(
    root: &Path,
    sources: &[Collection],
    enforce: bool,
    named: &[String],
    image: &str,
) -> Result<Module, String> {
    let members = named
        .iter()
        .map(|qualified| {
            let mut found = find(root, sources, qualified, enforce)?;
            Ok(Member {
                from: found.swap_remove(0),
                name: split(qualified).1.to_string(),
            })
        })
        .collect::<Result<Vec<Member>, String>>()?;
    Ok(Module {
        members,
        listing: Listing::collect(root, vec![image.to_string()], &Prompt::silent())?,
        workflows: None,
        conforms: Vec::new(),
    })
}

fn short(
    root: &Path,
    sources: &[Collection],
    list: &crate::model::image::List,
    listing: &Listing,
    named: &[String],
    declares: &[crate::parse::module::Summary],
    prompt: &Prompt,
) -> Result<Vec<String>, String> {
    let images: Vec<&crate::model::image::Image> = listing
        .images()
        .iter()
        .filter_map(|named| list.images.iter().find(|image| image.name == *named))
        .collect();
    let requires: Vec<&String> = declares
        .iter()
        .flat_map(|held| held.requires.iter())
        .filter(|want| {
            !declares
                .iter()
                .any(|held| held.provides.iter().any(|has| &has == want))
        })
        .collect();
    if images.is_empty() || requires.is_empty() {
        return Ok(Vec::new());
    }
    let disk = crate::parse::disk::Disk::scan(root);
    let index = crate::provider::Index::scan(root, sources, &disk, false);

    let mut unmet: Vec<&String> = Vec::new();
    let mut bring: Vec<String> = Vec::new();
    for want in requires {
        if unmet.contains(&want) {
            continue;
        }
        let met = images.iter().all(|image| {
            image
                .base
                .iter()
                .flat_map(|base| base.provides.iter().chain(base.provides_files.iter()))
                .any(|decl| decl.name == *want)
                || image.entries.iter().any(|entry| {
                    index
                        .at(&entry.dir())
                        .is_some_and(|held| held.declares.provides.contains(want))
                })
        });
        let Some(provider) = (match met {
            true => None,
            false => index.of(want).into_iter().find(|held| held.owner.is_some()),
        }) else {
            continue;
        };
        unmet.push(want);
        let qualified = provider.qualified();
        if !bring.contains(&qualified) {
            bring.push(qualified);
        }
    }
    if bring.is_empty() {
        return Ok(Vec::new());
    }

    let question = format!(
        "{} {} {}, which nothing in {} provides.\nImport {} as well?",
        said(
            &named
                .iter()
                .map(|name| format!("`{name}`"))
                .collect::<Vec<_>>()
        ),
        match named.len() {
            1 => "requires",
            _ => "require",
        },
        said(
            &unmet
                .iter()
                .map(|want| format!("`{want}`"))
                .collect::<Vec<_>>()
        ),
        said(
            &images
                .iter()
                .map(|image| format!("`{}`", image.id))
                .collect::<Vec<_>>()
        ),
        said(&bring.iter().map(|at| format!("`{at}`")).collect::<Vec<_>>()),
    );
    match prompt.confirm(&question, "Yes", "No")? {
        true => Ok(bring),
        false => Ok(Vec::new()),
    }
}

fn measured(
    list: &crate::model::image::List,
    listing: &Listing,
    named: &[String],
    declares: &[crate::parse::module::Summary],
    datastream: Option<&Path>,
    brought: &[String],
    prompt: &Prompt,
) -> Result<Vec<crate::set::Conforms>, String> {
    let claims: Vec<&String> = declares
        .iter()
        .flat_map(|held| held.satisfies.iter())
        .collect();
    if !prompt.asks() || claims.is_empty() {
        return Ok(Vec::new());
    }
    let images: Vec<&crate::model::image::Image> = listing
        .images()
        .iter()
        .filter_map(|named| list.images.iter().find(|image| image.name == *named))
        .filter(|image| image.conforms.is_empty())
        .collect();
    let Some(first) = images.first() else {
        return Ok(Vec::new());
    };
    let family = first.base.as_ref().map_or("", |base| base.family.as_str());
    let Ok(path) = crate::scap::content_path(family, datastream) else {
        return Ok(Vec::new());
    };
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if datastream.is_some() => return Err(format!("{}: {err}", path.display())),
        Err(_) => return Ok(Vec::new()),
    };
    let content = crate::scap::Content::read(&text);
    let claimed = crate::scap::reached(&content, claims.into_iter());
    let profiles: Vec<&crate::scap::Profile> = content
        .profiles
        .iter()
        .filter(|profile| !content.selected(&profile.id).is_disjoint(&claimed))
        .collect();
    if profiles.is_empty() {
        return Ok(Vec::new());
    }

    let measured = said(
        &images
            .iter()
            .map(|i| format!("`{}`", i.id))
            .collect::<Vec<_>>(),
    );
    println!(
        "{} {} rules {} {}, and {measured} {} no `conforms`.\n{}\n",
        said(
            &named
                .iter()
                .map(|name| format!("`{name}`"))
                .collect::<Vec<_>>()
        ),
        match named.len() {
            1 => "claims",
            _ => "claim",
        },
        said(
            &profiles
                .iter()
                .map(|p| format!("`{}`", p.name()))
                .collect::<Vec<_>>()
        ),
        match profiles.len() {
            1 => "selects",
            _ => "select",
        },
        match images.len() {
            1 => "declares",
            _ => "declare",
        },
        crate::set::cost(&measured, list.audit_enforce),
    );
    let options: Vec<Choice> = profiles
        .iter()
        .map(|profile| Choice::new(profile.name(), &profile.title))
        .collect();
    let Some(chosen) = prompt.choose("which profile", &options)? else {
        return Ok(Vec::new());
    };
    Ok(images
        .iter()
        .map(|image| {
            crate::set::Conforms::declaring(image, profiles[chosen].name(), brought.to_vec())
        })
        .collect())
}

fn quoted(picked: &[(Found, String)]) -> Vec<String> {
    picked.iter().map(|(_, name)| format!("`{name}`")).collect()
}

pub(crate) fn said(items: &[String]) -> String {
    match items.split_last() {
        None => String::new(),
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cancelled_listing_writes_nothing() {
        let root = std::env::temp_dir().join(format!("buildc-cancel-import-{}", std::process::id()));
        let from = root.join("collection/module");
        crate::init::put(&from.join(layout::MODULE_FILE), "description \"x\"\n").unwrap();

        Module {
            members: vec![Member {
                from: Found {
                    owner: "one".into(),
                    dir: from.clone(),
                },
                name: "module".into(),
            }],
            listing: Listing::Cancelled,
            workflows: None,
            conforms: Vec::new(),
        }
        .apply(&root)
        .unwrap();
        assert!(!root.join("modules/.remote/one/module").exists());

        Copy {
            from: Found {
                owner: "one".into(),
                dir: from,
            },
            dest: PathBuf::from("modules/module"),
            name: "module".into(),
            listing: Listing::Cancelled,
        }
        .apply(&root, &[])
        .unwrap();
        assert!(!root.join("modules/module").exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn no_image_is_refused_before_a_module_is_resolved() {
        let root =
            std::env::temp_dir().join(format!("buildc-no-import-image-{}", std::process::id()));
        crate::init::put(
            &root.join(layout::REPO_FILE),
            "schema-version 1\nname \"Example\"\n",
        )
        .unwrap();
        let result = Module::collect(
            Some("nosuch".into()),
            &root,
            &[],
            false,
            Vec::new(),
            None,
            &Prompt::silent(),
        );
        let message = match result {
            Err(err) => err.message().to_string(),
            Ok(_) => panic!("an import with no image was accepted"),
        };
        assert!(message.contains("needs an image"), "{message}");
        let _ = std::fs::remove_dir_all(root);
    }
}

pub struct Copy {
    from: Found,
    dest: PathBuf,
    name: String,
    listing: Listing,
}

impl Copy {
    pub fn collect(
        name: Option<String>,
        root: &Path,
        sources: &[Collection],
        enforce: bool,
        images: Vec<String>,
        prompt: &Prompt,
    ) -> Result<Self, Error> {
        let (from, name) = select(name, root, sources, enforce, "copy", prompt)?;
        let dest = destination(root, &from, &name)?;
        let listing = Listing::collect(root, images, prompt)?;
        listing.refuse_duplicate(&crate::model::image::List::load(root).0, &name, None)?;
        Ok(Self {
            from,
            dest,
            name,
            listing,
        })
    }

    pub fn apply(&self, root: &Path, sources: &[Collection]) -> Result<(), String> {
        if self.listing.cancelled() {
            return Ok(());
        }
        let mut wrote: Vec<(PathBuf, Change)> = vendor(root, sources, &self.from, &self.dest)?
            .into_iter()
            .map(|path| (path, Change::Created))
            .collect();
        wrote.extend(self.listing.apply(&self.name)?);
        report(root, &wrote);
        Ok(())
    }
}
