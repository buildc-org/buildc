use crate::copy;
use crate::create::{report, Change, Listing};
use crate::dispatch::Error;
use crate::layout;
use crate::model::remote::{At, Collection, Kind as SourceKind};
use crate::provenance::lock;
use crate::provider::Provider;
use crate::said;
use common::prompt::Prompt;
use common::ui::Choice;
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct Found {
    pub owner: String,
    pub name: String,
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
        .filter(|source| source.kind == SourceKind::Modules)
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
    if name
        .split('/')
        .any(|part| part.is_empty() || part.starts_with('.'))
    {
        return Err(format!(
            "`{name}` is not a module: a module is named by a path of names, as `<path>`, or \
             `<owner>/<path>` to name one collection, and no part of it may be empty or start \
             with a dot"
        ));
    }
    no_library(sources)?;

    let (possible_owner, rest) = split(name);
    let owner = possible_owner.filter(|owner| {
        sources
            .iter()
            .any(|source| source.kind == SourceKind::Modules && source.name == *owner)
    });
    let module = owner.map_or(name, |_| rest);
    let mut searched: Vec<&str> = Vec::new();
    let mut found: Vec<Found> = Vec::new();
    for collection in sources {
        if collection.kind != SourceKind::Modules {
            continue;
        }
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
        let tree = tree(root, collection)?.join(collection.subtree().unwrap_or(""));
        if tree.join(module).join(layout::MODULE_FILE).is_file() {
            found.push(Found {
                owner: collection.name.clone(),
                name: module.to_string(),
                dir: tree.join(module),
            });
        }
    }

    if found.is_empty() {
        for collection in sources {
            if collection.kind != SourceKind::Modules {
                continue;
            }
            if owner.is_some_and(|owner| owner != collection.name) {
                continue;
            }
            let tree = tree(root, collection)?.join(collection.subtree().unwrap_or(""));
            for path in crate::resolve::name::matching(&members(&tree), module) {
                found.push(Found {
                    owner: collection.name.clone(),
                    dir: tree.join(&path),
                    name: path,
                });
            }
        }
    }

    found.sort_by(|a, b| (&a.name, &a.owner).cmp(&(&b.name, &b.owner)));

    if found.is_empty() {
        return Err(format!(
            "no module called `{module}` in {}",
            searched.join(", ")
        ));
    }
    Ok(found)
}

fn no_library(sources: &[Collection]) -> Result<(), String> {
    if sources
        .iter()
        .any(|source| source.kind == SourceKind::Modules)
    {
        return Ok(());
    }
    const NONE: &str = "repo.kdl declares no `sources`, so there is no collection to import from";
    let block = crate::init::sources_block(&crate::create::Libraries::every());
    Err(format!(
        "{NONE}.\n\nThis is the block `buildc create repo` writes:\n\n{block}"
    ))
}

fn members(tree: &Path) -> Vec<String> {
    layout::module_dirs(
        tree,
        |path| path.join(layout::MODULE_FILE).is_file(),
        |name| name.starts_with('.'),
    )
    .iter()
    .map(|path| {
        path.strip_prefix(tree)
            .unwrap_or(path)
            .display()
            .to_string()
    })
    .collect()
}

pub fn catalog(root: &Path, sources: &[Collection], fetch: bool) -> Result<Vec<Provider>, String> {
    let mut listed: Vec<Provider> = Vec::new();
    for collection in sources {
        if collection.kind != SourceKind::Modules {
            continue;
        }
        let tree = match fetch {
            true => tree(root, collection)?,
            false => match cached(root, collection) {
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
        for name in members(&tree) {
            listed.push(Provider {
                owner: Some(collection.name.clone()),
                declares: crate::parse::module::summary(
                    &tree.join(&name).join(layout::MODULE_FILE),
                ),
                name,
                here: false,
            });
        }
    }
    listed.sort_by(|a, b| (&a.name, &a.owner).cmp(&(&b.name, &b.owner)));
    Ok(listed)
}

fn offered(
    root: &Path,
    sources: &[Collection],
    fit: Option<&Fit>,
) -> Result<(Vec<Provider>, Vec<Choice>), String> {
    no_library(sources)?;
    let listed = catalog(root, sources, true)?;
    if listed.is_empty() {
        return Err(format!("no module in {}", names(sources)));
    }
    let options = listed
        .iter()
        .map(|module| {
            let group = module.name.split_once('/').map_or("", |(group, _)| group);
            match fit.and_then(|fit| fit.unavailable(&module.declares)) {
                Some(why) => Choice::new(module.qualified(), why).unavailable(),
                None => Choice::new(module.qualified(), module.about()),
            }
            .within(group)
        })
        .collect();
    Ok((listed, options))
}

struct Fit<'a> {
    image: &'a crate::model::image::Image,
    index: crate::provider::Index,
}

impl<'a> Fit<'a> {
    fn of(
        root: &Path,
        sources: &[Collection],
        list: &'a crate::model::image::List,
        listing: &'a Listing,
    ) -> Option<Self> {
        let (named, _) = listing.targets().into_iter().next()?;
        let image = list.images.iter().find(|image| image.name == named)?;
        let disk = crate::parse::disk::Disk::scan(root);
        Some(Self {
            image,
            index: crate::provider::Index::scan(root, sources, &disk, false),
        })
    }

    fn unavailable(&self, declares: &crate::parse::module::Summary) -> Option<String> {
        self.unsupported(declares).or_else(|| {
            self.clash(declares)
                .map(|(capability, held)| copy::exclusive_to(&capability, &held[0].path))
        })
    }

    fn unsupported(&self, declares: &crate::parse::module::Summary) -> Option<String> {
        let family = self
            .image
            .base
            .as_ref()
            .map(|base| base.family.as_str())
            .filter(|family| !family.is_empty())?;
        (!crate::resolve::supports(&declares.supports, family))
            .then(|| copy::unsupported(&declares.supports, family))
    }

    fn clash(
        &self,
        declares: &crate::parse::module::Summary,
    ) -> Option<(String, Vec<&'a crate::model::image::Entry>)> {
        let base: Vec<&str> = self
            .image
            .base
            .iter()
            .flat_map(|base| base.provides.iter())
            .map(|decl| decl.name.as_str())
            .collect();
        if declares
            .provides
            .iter()
            .all(|capability| base.contains(&capability.as_str()))
        {
            return None;
        }
        for capability in &declares.provides {
            let held: Vec<(&'a crate::model::image::Entry, bool)> = self
                .image
                .entries
                .iter()
                .filter_map(|entry| {
                    let provider = self.index.at(&entry.dir())?;
                    provider
                        .declares
                        .provides
                        .contains(capability)
                        .then(|| (entry, provider.declares.exclusive.contains(capability)))
                })
                .collect();
            let mut flags: Vec<bool> = held.iter().map(|(_, exclusive)| *exclusive).collect();
            flags.push(declares.exclusive.contains(capability));
            if crate::resolve::exclusive(&flags) {
                return Some((
                    capability.clone(),
                    held.into_iter().map(|(entry, _)| entry).collect(),
                ));
            }
        }
        None
    }

    fn settle(
        &self,
        capability: &str,
        held: &[&crate::model::image::Entry],
        prompt: &Prompt,
    ) -> Result<Vec<Unlisted>, String> {
        let named: Vec<String> = held.iter().map(|entry| entry.path.clone()).collect();
        let holder = said(&named);
        let options = [
            Choice::new(copy::replace(&holder), ""),
            Choice::new(copy::CANCEL, ""),
        ];
        let answer = match prompt.asks() {
            true => prompt.choose(
                &copy::exclusive_clash(capability, &holder, &self.image.name),
                &options,
            )?,
            false => None,
        };
        match answer {
            Some(0) => Ok(held
                .iter()
                .map(|entry| Unlisted {
                    file: PathBuf::from(self.image.src.name()),
                    image: self.image.name.clone(),
                    flavour: entry.flavour.clone(),
                    source: entry.source.clone(),
                    name: entry.name().to_string(),
                })
                .collect()),
            Some(_) => Err(format!(
                "nothing imported: `{capability}` is exclusive to `{holder}` in `{}`",
                self.image.name
            )),
            None => Err(format!(
                "`{capability}` is exclusive, and `{holder}` already provides it in `{}`; run \
                 `buildc import module` in a terminal to replace it",
                self.image.name
            )),
        }
    }
}

struct Unlisted {
    file: PathBuf,
    image: String,
    flavour: Option<String>,
    source: Option<String>,
    name: String,
}

fn unchosen() -> String {
    "no module chosen; `buildc import module <name>` names one".to_string()
}

fn choose_several(
    root: &Path,
    sources: &[Collection],
    fit: Option<&Fit>,
    prompt: &Prompt,
) -> Result<Vec<String>, String> {
    let (listed, options) = offered(root, sources, fit)?;
    let chosen = match prompt.choose_many(copy::WHICH_MODULES, &options, &[])? {
        common::ui::Answer::Chosen(chosen) => chosen,
        common::ui::Answer::Cancelled => Vec::new(),
    };
    match chosen.is_empty() {
        true => Err(unchosen()),
        false => Ok(chosen.iter().map(|at| listed[*at].qualified()).collect()),
    }
}

pub fn cached(root: &Path, collection: &Collection) -> Option<PathBuf> {
    let dir = match &collection.at {
        At::Dir(dir) => root.join(dir),
        At::Git(pin) => {
            let key = source_key(pin).ok()?;
            let dir = root.join(layout::SOURCES_CACHE).join(&key);
            if pin.sha256.is_none() {
                return dir.is_dir().then_some(dir);
            }
            let stamp = root.join(layout::SOURCES_CACHE).join(format!("{key}.pin"));
            match std::fs::read_to_string(&stamp).ok().as_deref() == pin.sha256.as_deref() {
                true => dir,
                false => return None,
            }
        }
    };
    dir.is_dir().then_some(dir)
}

pub(crate) fn tree(root: &Path, collection: &Collection) -> Result<PathBuf, String> {
    let remote = match &collection.at {
        At::Dir(dir) => {
            let dir = root.join(dir);
            if dir.is_dir() {
                return Ok(dir);
            }
            return Err(format!(
                "`{}` is {}, which is not a directory on this machine",
                collection.name,
                dir.display()
            ));
        }
        At::Git(remote) => remote,
    };

    let key = source_key(remote)?;
    let cache = root.join(layout::SOURCES_CACHE);
    let dir = cache.join(&key);
    let pin = cache.join(format!("{key}.pin"));
    let run = cache.join(format!("{key}.run"));
    let this_run = std::process::id().to_string();
    if remote.sha256.is_some() {
        if let Some(current) = cached(root, collection) {
            return Ok(current);
        }
    } else if dir.is_dir()
        && std::fs::read_to_string(&run).is_ok_and(|found| found.trim() == this_run)
    {
        return Ok(dir);
    }

    std::fs::create_dir_all(&cache).map_err(|err| format!("{}: {err}", cache.display()))?;
    let work = cache.join(format!(".{key}.fetching.{}", std::process::id()));
    crate::init::remove(&work)?;
    let work = crate::init::Scratch(work);
    fetch_git(remote, &work.0).map_err(|err| format!("`{}`: {err}", collection.name))?;
    crate::init::remove(&dir)?;
    let fetched = work.0.join("tree");
    std::fs::rename(&fetched, &dir).map_err(|err| format!("{}: {err}", dir.display()))?;
    match &remote.sha256 {
        Some(sha256) => {
            crate::init::put(&pin, &format!("{sha256}\n"))?;
            crate::init::remove(&run)?;
        }
        None => {
            crate::init::put(&run, &format!("{this_run}\n"))?;
            crate::init::remove(&pin)?;
        }
    }
    Ok(dir)
}

fn source_key(remote: &crate::provenance::Evidence) -> Result<String, String> {
    let url = remote.url.as_deref().ok_or("source has no Git URL")?;
    let selector = remote.selector();
    cache_key(url, selector)
}

pub fn cache_key(url: &str, selector: &str) -> Result<String, String> {
    crate::runtime::sha256_stdin(format!("{url}\0{selector}").as_bytes())
}

pub fn cache_path(root: &Path, url: &str, selector: &str) -> Result<PathBuf, String> {
    Ok(root
        .join(layout::SOURCES_CACHE)
        .join(cache_key(url, selector)?))
}

fn git(repo: &Path, args: &[&str]) -> Result<std::process::Output, String> {
    std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ASKPASS", "/bin/false")
        .env("SSH_ASKPASS", "/bin/false")
        .output()
        .map_err(|err| format!("git {}: {err}", args.join(" ")))
}

fn git_ok(repo: &Path, args: &[&str]) -> Result<std::process::Output, String> {
    let out = git(repo, args)?;
    if out.status.success() {
        return Ok(out);
    }
    Err(format!(
        "git {}: {}\n{}",
        args.join(" "),
        out.status,
        String::from_utf8_lossy(&out.stderr).trim_end()
    ))
}

fn fetch_git(remote: &crate::provenance::Evidence, work: &Path) -> Result<(), String> {
    let url = remote.url.as_deref().ok_or("source has no Git URL")?;
    let selector = remote.selector();
    let work = {
        let repo = work.join("repo.git");
        std::fs::create_dir_all(&repo).map_err(|err| format!("{}: {err}", repo.display()))?;
        std::fs::canonicalize(work).map_err(|err| format!("{}: {err}", work.display()))?
    };
    let repo = work.join("repo.git");
    let tree = work.join("tree");
    let archive = work.join("source.tar");
    git_ok(&repo, &["init", "--bare", "--quiet"])?;
    git_ok(
        &repo,
        &[
            "-c",
            "protocol.version=2",
            "fetch",
            "--quiet",
            "--depth=1",
            "--no-tags",
            "--",
            url,
            selector,
        ],
    )?;
    let commit = git_ok(&repo, &["rev-parse", "FETCH_HEAD^{commit}"])?;
    let commit = String::from_utf8_lossy(&commit.stdout).trim().to_string();
    let archive_arg = archive.to_string_lossy();
    git_ok(
        &repo,
        &[
            "archive",
            "--format=tar",
            &format!("--output={archive_arg}"),
            &commit,
        ],
    )?;
    if let Some(expected) = &remote.sha256 {
        crate::runtime::match_sha256(&archive, expected, &format!("{url} at {selector}"))?;
    }
    std::fs::create_dir_all(&tree).map_err(|err| format!("{}: {err}", tree.display()))?;
    let out = std::process::Command::new("tar")
        .args([
            "-xf",
            &archive.to_string_lossy(),
            "-C",
            &tree.to_string_lossy(),
        ])
        .output()
        .map_err(|err| format!("tar: {err}"))?;
    if !out.status.success() {
        return Err(format!(
            "tar: {}{}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(())
}

fn vendor(
    root: &Path,
    sources: &[Collection],
    member: &Member,
    tracked: bool,
) -> Result<Vec<PathBuf>, String> {
    let from = &member.from;
    let content = lock::hash(&from.dir).ok_or_else(|| {
        format!(
            "{} cannot be hashed, so nothing can record what was copied",
            from.dir.display()
        )
    })?;
    let mut pin = crate::provenance::Evidence::new(crate::diag::Span::default());
    if let Some(library) = sources
        .iter()
        .find(|c| c.kind == SourceKind::Modules && c.name == from.owner)
        .and_then(Collection::pin)
    {
        pin.url.clone_from(&library.url);
        pin.version.clone_from(&library.version);
    }
    let record = lock::Record {
        path: member.name.clone(),
        library: from.owner.clone(),
        upstream: from.name.clone(),
        pin,
        content,
        tracked,
    };
    let locked = lock::with(root, record)?;
    let dest = PathBuf::from(layout::MODULES).join(&member.name);
    let wrote = crate::init::copy_tree(&from.dir, &root.join(&dest))?
        .into_iter()
        .map(|under| dest.join(under))
        .collect();
    crate::init::put(&layout::lock(root), &locked)?;
    Ok(wrote)
}

fn resolve(
    name: &str,
    root: &Path,
    sources: &[Collection],
    enforce: bool,
    prompt: &Prompt,
) -> Result<Found, Error> {
    let mut found = find(root, sources, name, enforce)?;
    let module = split(name).1.to_string();
    let at = match found.as_slice() {
        [_] => 0,
        many => {
            let options: Vec<Choice> = many
                .iter()
                .map(|f| Choice::new(format!("{}/{}", f.owner, f.name), ""))
                .collect();
            prompt
                .choose(
                    &format!("`{module}` names more than one module; which one"),
                    &options,
                )?
                .ok_or_else(|| {
                    format!(
                        "`{module}` names more than one module; name which one, as `{}/{}`",
                        many[0].owner, many[0].name
                    )
                })?
        }
    };
    Ok(found.swap_remove(at))
}

struct Member {
    from: Found,
    name: String,
    vendor: bool,
}

impl Member {
    fn declares(&self, root: &Path) -> crate::parse::module::Summary {
        let dir = match self.vendor {
            true => self.from.dir.clone(),
            false => layout::module(root, &self.name),
        };
        crate::parse::module::summary(&dir.join(layout::MODULE_FILE))
    }
}

pub struct Request<'a> {
    pub name: Option<String>,
    pub image: Option<String>,
    pub datastream: Option<&'a Path>,
    pub dependencies: bool,
}

struct Listed<'a> {
    listing: &'a Listing,
    names: &'a [String],
    declares: &'a [crate::parse::module::Summary],
}

pub struct Module {
    members: Vec<Member>,
    listing: Listing,
    tracked: bool,
    unlisted: Vec<Unlisted>,
    workflows: Option<crate::set::Workflows>,
    conforms: Vec<crate::set::Conforms>,
}

impl Module {
    pub fn collect(
        request: Request,
        root: &Path,
        sources: &[Collection],
        enforce: bool,
        prompt: &Prompt,
    ) -> Result<Self, Error> {
        let Request {
            name,
            image,
            datastream,
            dependencies,
        } = request;
        let (list, _) = crate::model::image::List::load(root);
        let listing = Listing::target(root, image, prompt)?;
        let fit = Fit::of(root, sources, &list, &listing);
        let asked = match name {
            Some(name) => vec![name],
            None => choose_several(root, sources, fit.as_ref(), prompt)?,
        };
        let picked = asked
            .iter()
            .map(|name| resolve(name, root, sources, enforce, prompt))
            .collect::<Result<Vec<Found>, Error>>()?;
        for from in &picked {
            let issues = crate::parse::module::validate_manifest(
                &from.dir.join(layout::MODULE_FILE),
                list.schema_version,
                &list.repo_src,
            );
            if !issues.is_empty() {
                return Err(issues.plain().into());
            }
        }
        let chosen = picked
            .iter()
            .map(|from| member(root, from, &listing, prompt))
            .collect::<Result<Vec<Member>, String>>()?;
        let declares: Vec<crate::parse::module::Summary> =
            chosen.iter().map(|member| member.declares(root)).collect();
        if let Some(fit) = &fit {
            for (member, declares) in chosen.iter().zip(&declares) {
                if let Some(why) = fit.unsupported(declares) {
                    return Err(format!("`{}` {why}", member.name).into());
                }
            }
        }
        for member in &chosen {
            listing.refuse_duplicate(&list, &member.name, None)?;
        }
        let named: Vec<String> = chosen.iter().map(|member| member.name.clone()).collect();

        let also = short(
            root,
            sources,
            &list,
            &Listed {
                listing: &listing,
                names: &named,
                declares: &declares,
            },
            dependencies,
            prompt,
        )?
        .into_iter()
        .filter(|qualified| {
            !picked
                .iter()
                .any(|from| format!("{}/{}", from.owner, from.name) == *qualified)
        })
        .map(|qualified| brought(root, sources, enforce, &qualified))
        .collect::<Result<Vec<Member>, String>>()?;
        let mut unlisted: Vec<Unlisted> = Vec::new();
        if let Some(fit) = &fit {
            for member in also.iter().chain(&chosen) {
                let Some((capability, held)) = fit.clash(&member.declares(root)) else {
                    continue;
                };
                let held: Vec<&crate::model::image::Entry> = held
                    .into_iter()
                    .filter(|entry| !unlisted.iter().any(|gone| gone.name == entry.name()))
                    .collect();
                if !held.is_empty() {
                    unlisted.extend(fit.settle(&capability, &held, prompt)?);
                }
            }
        }

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
                println!("{}\n", rows.join("\n"));
                prompt
                    .confirm(copy::GENERATE_WORKFLOWS, copy::YES, copy::NO)?
                    .then(|| crate::set::Workflows::adding(&list, &named))
            }
        };

        let members: Vec<Member> = also.into_iter().chain(chosen).collect();
        let brought: Vec<String> = members.iter().map(|member| member.name.clone()).collect();
        let conforms = measured(
            &list, &listing, &named, &declares, datastream, &brought, prompt,
        )?;
        let copied: Vec<String> = members
            .iter()
            .filter(|member| member.vendor)
            .map(|member| format!("`{}`", member.name))
            .collect();
        let tracked = match copied.is_empty() {
            true => true,
            false => {
                prompt.confirm_current(&copy::track(&said(&copied)), copy::YES, copy::NO, true)?
            }
        };
        Ok(Self {
            members,
            listing,
            tracked,
            unlisted,
            workflows,
            conforms,
        })
    }

    pub fn apply(&self, root: &Path, sources: &[Collection]) -> Result<(), String> {
        let wrote = self.write(root, sources)?;
        if !wrote.is_empty() {
            report(root, &wrote);
        }
        Ok(())
    }

    pub(crate) fn write(
        &self,
        root: &Path,
        sources: &[Collection],
    ) -> Result<Vec<(PathBuf, Change)>, String> {
        let mut wrote: Vec<(PathBuf, Change)> = Vec::new();
        let existed = layout::lock(root).exists();
        let mut recorded: Vec<String> = Vec::new();
        for member in self.members.iter().filter(|member| member.vendor) {
            wrote.extend(
                vendor(root, sources, member, self.tracked)?
                    .into_iter()
                    .map(|path| (path, Change::Created)),
            );
            recorded.push(member.name.clone());
        }
        if !recorded.is_empty() {
            wrote.push((
                PathBuf::from(layout::MODULES).join(layout::LOCK_FILE),
                match existed {
                    true => Change::Updated(format!("{} recorded", said(&recorded))),
                    false => Change::Created,
                },
            ));
        }
        let listed: Vec<(&str, Option<&str>)> = self
            .members
            .iter()
            .map(|member| (member.name.as_str(), None))
            .collect();
        for gone in &self.unlisted {
            crate::create::unlist(
                &root.join(&gone.file),
                &gone.image,
                gone.flavour.as_deref(),
                gone.source.as_deref(),
                &gone.name,
            )?;
        }
        let (list, _) = crate::model::image::List::load(root);
        wrote.extend(self.listing.apply_declaration(&list, &listed)?);
        for gone in &self.unlisted {
            let removed = format!("{} removed", gone.name);
            match wrote.iter_mut().find(|(path, _)| *path == gone.file) {
                Some((_, Change::Updated(said))) => {
                    said.push_str(", and ");
                    said.push_str(&removed);
                }
                _ => wrote.push((gone.file.clone(), Change::Updated(removed))),
            }
        }
        if let Some(workflows) = &self.workflows {
            wrote.extend(workflows.apply(root)?);
        }
        for conforms in &self.conforms {
            wrote.extend(conforms.apply(root)?);
        }
        collisions(root, &self.brought());
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
        .map(|qualified| brought(root, sources, enforce, qualified))
        .collect::<Result<Vec<Member>, String>>()?;
    Ok(Module {
        members,
        listing: Listing::collect(root, vec![image.to_string()], &Prompt::silent())?,
        tracked: true,
        unlisted: Vec::new(),
        workflows: None,
        conforms: Vec::new(),
    })
}

fn brought(
    root: &Path,
    sources: &[Collection],
    enforce: bool,
    qualified: &str,
) -> Result<Member, String> {
    let mut found = find(root, sources, qualified, enforce)?;
    let from = found.swap_remove(0);
    let name = from.name.clone();
    Ok(Member {
        vendor: !layout::module(root, &name).exists(),
        from,
        name,
    })
}

fn member(root: &Path, from: &Found, listing: &Listing, prompt: &Prompt) -> Result<Member, String> {
    let name = from.name.clone();
    if !layout::module(root, &name).exists() {
        return Ok(Member {
            from: from.clone(),
            name,
            vendor: true,
        });
    }
    let image = listing.images().first().map(ToString::to_string);
    let mut options = Vec::new();
    if let Some(image) = &image {
        options.push(Choice::new(copy::add_local(&name, image), ""));
    }
    options.push(Choice::new(copy::ANOTHER_NAME, ""));
    let answer = match prompt.asks() {
        true => prompt.choose(&copy::present(&name), &options)?,
        false => None,
    };
    match (answer, &image) {
        (Some(0), Some(_)) => Ok(Member {
            from: from.clone(),
            name,
            vendor: false,
        }),
        (Some(_), _) => {
            let suggested = free_name(root, &name);
            let chosen =
                prompt.line(None, copy::LOCAL_NAME, "a local name", "", Some(&suggested))?;
            if !crate::model::image::is_local_path(&chosen) {
                return Err(format!(
                    "`{chosen}` is not a module path: each part is lowercase letters, digits and \
                     dashes, starting with a letter, and the last part may end in one `@<tag>`"
                ));
            }
            if layout::module(root, &chosen).exists() {
                return Err(format!("`{chosen}` is already in this repository"));
            }
            Ok(Member {
                from: from.clone(),
                name: chosen,
                vendor: true,
            })
        }
        (None, _) => Err(format!(
            "`{name}` is already in this repository; run `buildc import module` in a terminal to \
             list the local `{name}` or import the library's under another name"
        )),
    }
}

fn free_name(root: &Path, name: &str) -> String {
    (1..)
        .map(|at| format!("{name}@{at}"))
        .find(|candidate| !layout::module(root, candidate).exists())
        .expect("an unbounded range holds a free name")
}

fn short(
    root: &Path,
    sources: &[Collection],
    list: &crate::model::image::List,
    listed: &Listed,
    dependencies: bool,
    prompt: &Prompt,
) -> Result<Vec<String>, String> {
    let &Listed {
        listing,
        names,
        declares,
    } = listed;
    let targets: Vec<(&crate::model::image::Image, Option<&str>)> = listing
        .targets()
        .iter()
        .filter_map(|(named, flavour)| {
            list.images
                .iter()
                .find(|image| image.name == *named)
                .map(|image| (image, *flavour))
        })
        .collect();
    let requires: Vec<(&String, &String)> = names
        .iter()
        .zip(declares)
        .flat_map(|(name, held)| held.requires.iter().map(move |want| (name, want)))
        .filter(|(_, want)| {
            !declares
                .iter()
                .any(|held| held.provides.iter().any(|has| &has == want))
        })
        .collect();
    if targets.is_empty() || requires.is_empty() {
        return Ok(Vec::new());
    }
    let disk = crate::parse::disk::Disk::scan(root);
    let index = crate::provider::Index::scan(root, sources, &disk, false);

    let mut mappings: Vec<(String, String, String, String)> = Vec::new();
    let mut bring: Vec<String> = Vec::new();
    for (module, want) in requires {
        for (image, flavour) in &targets {
            if image_has(image, *flavour, want, &index) {
                continue;
            }
            let family = image.base.as_ref().map_or("", |base| base.family.as_str());
            let Some(provider) = index
                .fitting(want, family)
                .into_iter()
                .find(|held| held.owner.is_some())
            else {
                continue;
            };
            let base = image
                .base
                .as_ref()
                .map(|base| base.image.rsplit('/').next().unwrap_or(&base.image))
                .unwrap_or_default();
            let mapping = (
                module.to_string(),
                base.to_string(),
                want.to_string(),
                provider.name.clone(),
            );
            if !mappings.contains(&mapping) {
                mappings.push(mapping);
            }
            let qualified = provider.qualified();
            if !bring.contains(&qualified) {
                bring.push(qualified);
            }
        }
    }
    if bring.is_empty() {
        return Ok(Vec::new());
    }
    if dependencies {
        return Ok(bring);
    }
    if !prompt.asks() {
        return Ok(Vec::new());
    }

    println!("{}", copy::REQUIRED);
    for (module, base, want, provider) in mappings {
        println!("\n  {module} > {base}\n    requires: {want}\n    provider module: {provider}");
    }
    println!();

    match prompt.confirm(copy::BRING_REQUIRED, copy::YES, copy::NO)? {
        true => Ok(bring),
        false => Ok(Vec::new()),
    }
}

fn image_has(
    image: &crate::model::image::Image,
    flavour: Option<&str>,
    want: &str,
    index: &crate::provider::Index,
) -> bool {
    image
        .base
        .iter()
        .flat_map(|base| base.provides.iter())
        .any(|decl| decl.name == want)
        || image.entries.iter().any(|entry| {
            (entry.flavour.is_none() || entry.flavour.as_deref() == flavour)
                && index
                    .at(&entry.dir())
                    .is_some_and(|held| held.declares.provides.iter().any(|has| has == want))
        })
}

fn collisions(root: &Path, brought: &[String]) {
    let brought: std::collections::BTreeSet<String> = brought.iter().cloned().collect();
    let loaded = crate::load(root);
    for (image, resolved) in loaded.list.images.iter().zip(&loaded.resolved) {
        for line in crate::resolve::overlay::collisions(image, &resolved.shipped, &brought) {
            println!("{line}");
        }
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
    let content = match crate::scap::content_of(&path) {
        Ok(content) => content,
        Err(err) if datastream.is_some() => return Err(err),
        Err(_) => return Ok(Vec::new()),
    };
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
    let Some(chosen) = prompt.choose(copy::WHICH_PROFILE, &options)? else {
        return Ok(Vec::new());
    };
    Ok(images
        .iter()
        .map(|image| {
            crate::set::Conforms::declaring(image, profiles[chosen].name(), brought.to_vec())
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collection(name: &str, path: &str) -> Collection {
        Collection {
            kind: SourceKind::Modules,
            name: name.to_string(),
            at: At::Dir(path.to_string()),
            span: crate::diag::Span::default(),
        }
    }

    #[test]
    fn a_complete_nested_path_wins_before_an_owner_prefix() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let sources = [collection("grouped", "tests/collections/grouped")];
        let found = find(root, &sources, "hardening/coredumps", false).unwrap();

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].owner, "grouped");
        assert_eq!(found[0].name, "hardening/coredumps");
    }

    #[test]
    fn exact_matches_are_sorted_like_suffix_matches() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let sources = [
            collection("community", "tests/collections/community"),
            collection("upstream", "tests/collections/upstream"),
        ];
        let found = find(root, &sources, "browser", false).unwrap();
        let owners: Vec<&str> = found.iter().map(|found| found.owner.as_str()).collect();

        assert_eq!(owners, ["community", "upstream"]);
    }

    #[test]
    fn the_module_picker_groups_nested_paths_by_their_first_directory() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let sources = [
            collection("community", "tests/collections/community"),
            collection("grouped", "tests/collections/grouped"),
        ];
        let (_, options) = offered(root, &sources, None).unwrap();
        let nested = options
            .iter()
            .find(|option| option.label == "grouped/hardening/coredumps")
            .unwrap();
        let root_level = options
            .iter()
            .find(|option| option.label == "community/browser")
            .unwrap();

        assert_eq!(nested.group, "hardening");
        assert!(root_level.group.is_empty());
    }

    fn targeted(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("buildc-{name}-{}", std::process::id()));
        crate::init::put(
            &root.join(layout::REPO_FILE),
            "schema-version 1\nname \"Example\"\n",
        )
        .unwrap();
        crate::init::put(
            &root.join(layout::IMAGE_FILE),
            "schema-version 1\n\nimage {\n    name \"example\"\n    base \"example\" { family \"fedora\" }\n    modules {\n        module \"flathub\"\n    }\n}\n",
        )
        .unwrap();
        crate::init::put(
            &root.join("modules/flathub/module.kdl"),
            "schema-version 1\n\nprovides \"flatpak\" exclusive=#true\n",
        )
        .unwrap();
        root
    }

    #[test]
    fn a_member_that_would_clash_is_unavailable_with_its_reason() {
        let root = targeted("clash-row");
        let sources = [collection(
            "upstream",
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/collections/upstream"),
        )];
        let (list, _) = crate::model::image::List::load(&root);
        let listing = Listing::target(&root, Some("example".into()), &Prompt::silent()).unwrap();
        let fit = Fit::of(&root, &sources, &list, &listing).expect("the image is the target");
        let (_, options) = offered(&root, &sources, Some(&fit)).unwrap();
        let row = |label: &str| {
            options
                .iter()
                .find(|option| option.label == label)
                .unwrap_or_else(|| panic!("no row {label}"))
        };

        assert!(!row("upstream/flatpak").available);
        assert_eq!(
            row("upstream/flatpak").detail,
            "`flatpak` is exclusive to `flathub`"
        );
        assert!(!row("upstream/debian-family").available);
        assert!(row("upstream/browser").available);
        std::fs::remove_dir_all(root).unwrap();
    }

    fn flavoured(name: &str, base: &str) -> PathBuf {
        let root = targeted(name);
        crate::init::put(
            &root.join(layout::IMAGE_FILE),
            &format!("schema-version 1\n\nimage {{\n    name \"example\"\n    {base}\n    flavours {{\n        dev\n        prod\n    }}\n    modules {{\n        flavour \"dev\" {{ module \"flathub\" }}\n    }}\n}}\n"),
        )
        .unwrap();
        root
    }

    fn upstream_flatpak() -> crate::parse::module::Summary {
        crate::parse::module::summary(Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/collections/upstream/flatpak/module.kdl"
        )))
    }

    #[test]
    fn a_holder_in_another_flavour_clashes_as_check_counts_it() {
        let root = flavoured("clash-flavour", "base \"example\" { family \"fedora\" }");
        let (list, _) = crate::model::image::List::load(&root);
        let listing =
            Listing::target(&root, Some("example/prod".into()), &Prompt::silent()).unwrap();
        let fit = Fit::of(&root, &[], &list, &listing).expect("the flavour is the target");
        let (capability, held) = fit.clash(&upstream_flatpak()).expect("a clash");

        assert_eq!(capability, "flatpak");
        assert_eq!(held.len(), 1);
        assert_eq!(held[0].path, "flathub");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_member_the_base_covers_never_clashes() {
        let root = flavoured(
            "clash-base",
            "base \"example\" {\n        family \"fedora\"\n        provides \"flatpak\"\n    }",
        );
        let (list, _) = crate::model::image::List::load(&root);
        let listing = Listing::target(&root, Some("example".into()), &Prompt::silent()).unwrap();
        let fit = Fit::of(&root, &[], &list, &listing).expect("the image is the target");

        assert!(fit.clash(&upstream_flatpak()).is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn no_library_is_refused_with_the_block_that_declares_one() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let named = find(root, &[], "browser", false)
            .err()
            .expect("no library is refused");
        let picked = offered(root, &[], None)
            .err()
            .expect("no library is refused");

        assert!(named.contains("declares no `sources`"), "{named}");
        assert_eq!(picked, named);
    }

    #[test]
    fn choosing_no_module_is_refused() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let sources = [collection("upstream", "tests/collections/upstream")];
        let refused = choose_several(root, &sources, None, &Prompt::silent()).unwrap_err();

        assert!(refused.starts_with("no module chosen"), "{refused}");
    }

    #[test]
    fn a_copy_from_a_fetched_library_records_its_url_and_revision() {
        let root = std::env::temp_dir().join(format!("buildc-vendor-pin-{}", std::process::id()));
        crate::init::put(&root.join("library/hello/module.sh"), "true\n").unwrap();
        let mut pin = crate::provenance::Evidence::new(crate::diag::Span::default());
        pin.url = Some("https://host/owner/library.git".into());
        pin.version = Some("v2.0.0".into());
        let sources = [
            collection("other", "elsewhere"),
            Collection {
                kind: SourceKind::Modules,
                name: "one".into(),
                at: At::Git(pin),
                span: crate::diag::Span::default(),
            },
        ];
        let member = Member {
            from: Found {
                owner: "one".into(),
                name: "hello".into(),
                dir: root.join("library/hello"),
            },
            name: "hello".into(),
            vendor: true,
        };
        vendor(&root, &sources, &member, false).unwrap();
        let entry = lock::entry(&root, "hello").expect("the copy is recorded");

        assert_eq!(
            entry.pin.url.as_deref(),
            Some("https://host/owner/library.git")
        );
        assert_eq!(entry.pin.version.as_deref(), Some("v2.0.0"));
        assert!(!entry.tracked);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn no_target_leaves_every_member_available() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let sources = [collection("upstream", "tests/collections/upstream")];
        let (_, options) = offered(root, &sources, None).unwrap();

        assert!(options.iter().all(|option| option.available));
    }

    #[test]
    fn a_provider_in_one_flavour_does_not_cover_its_sibling() {
        let root =
            std::env::temp_dir().join(format!("buildc-flavour-provider-{}", std::process::id()));
        crate::init::put(
            &root.join("image.kdl"),
            r#"schema-version 1

image {
    name "Example"
    base "example" { family "fedora" }
    flavours {
        dev
        server
    }
    modules { flavour "dev" { module "provider" } }
}
"#,
        )
        .unwrap();
        crate::init::put(
            &root.join("modules/provider/module.kdl"),
            "schema-version 1\n\ndescription \"Provider\"\nsupports \"fedora\"\nprovides \"tool\"\n",
        )
        .unwrap();
        let (list, _) = crate::model::image::List::load(&root);
        let disk = crate::parse::disk::Disk::scan(&root);
        let index = crate::provider::Index::scan(&root, &[], &disk, false);
        let image = &list.images[0];

        assert!(image_has(image, Some("dev"), "tool", &index));
        assert!(!image_has(image, Some("server"), "tool", &index));
        assert!(!image_has(image, None, "tool", &index));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn an_unhashable_copy_writes_nothing() {
        let root =
            std::env::temp_dir().join(format!("buildc-unhashable-copy-{}", std::process::id()));
        let member = Member {
            from: Found {
                owner: "one".into(),
                name: "missing".into(),
                dir: root.join("missing"),
            },
            name: "missing".into(),
            vendor: true,
        };

        assert!(vendor(&root, &[], &member, true).is_err());
        assert!(!root.join("modules/missing").exists());
        assert!(!layout::lock(&root).exists());
    }

    #[test]
    fn a_renamed_copy_takes_the_first_free_tag() {
        let root = std::env::temp_dir().join(format!("buildc-free-name-{}", std::process::id()));
        crate::init::put(&root.join("modules/browser/module.sh"), "true\n").unwrap();
        crate::init::put(&root.join("modules/browser@1/module.sh"), "true\n").unwrap();

        assert_eq!(free_name(&root, "browser"), "browser@2");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn an_import_refuses_a_manifest_newer_than_the_repository() {
        let root = std::env::temp_dir().join(format!("buildc-import-schema-{}", std::process::id()));
        crate::init::put(
            &root.join(layout::REPO_FILE),
            "schema-version 1\nname \"Example\"\n",
        )
        .unwrap();
        crate::init::put(
            &root.join(layout::IMAGE_FILE),
            "schema-version 1\n\nimage { name \"Example\"; base \"example\"; modules { } }\n",
        )
        .unwrap();
        crate::init::put(
            &root.join("collection/new/module.kdl"),
            "schema-version 2\n\npackages \"new\"\n",
        )
        .unwrap();
        let sources = [collection("one", "collection")];

        let result = Module::collect(
            Request {
                name: Some("new".into()),
                image: Some("example".into()),
                datastream: None,
                dependencies: false,
            },
            &root,
            &sources,
            false,
            &Prompt::silent(),
        );
        let message = match result {
            Err(err) => err.message().to_string(),
            Ok(_) => panic!("an incompatible import was accepted"),
        };
        assert!(message.contains("module.kdl declares schema"), "{message}");
        assert!(message.contains("version 2, but"), "{message}");
        assert!(message.contains("repo.kdl declares 1"), "{message}");
        assert!(!root.join("modules/new").exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}
