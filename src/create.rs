use crate::copy;
use crate::error::Error;
use crate::forge;
use crate::layout;
use crate::layout::{named_after_root, under};
use crate::parse::declaration::{append, append_inside, placed, wrap};
use crate::parse::local::{Listed, Place};
use crate::report::report;
use crate::setup::{Field, Libraries, Listing, Workflows};
use common::prompt::Prompt;
pub use common::ui::tree::Change;
use common::ui::{Answer, Choice};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const GH_INSTALL: &str = "install gh from https://github.com/cli/cli";

const BUILD_ENVIRONMENT: &str = "build-environment";

const OS_RELEASE: &str = "os-release";
const BASE_URL_REFERENCE: &str = "{base-url}";

#[derive(Default)]
struct Given {
    name: Option<String>,
    host: Option<String>,
    owner: Option<String>,
    image: Option<String>,
    base: Option<String>,
    root: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct NonEmpty(String);

impl NonEmpty {
    fn parse(value: impl Into<String>, what: &str) -> Result<Self, String> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(format!("{what} is empty"));
        }
        Ok(Self(value))
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for NonEmpty {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Setup {
    Defaults,
    Minimal,
}

impl Setup {
    fn label(self) -> &'static str {
        match self {
            Self::Defaults => copy::SETUP_DEFAULTS,
            Self::Minimal => copy::SETUP_MINIMAL,
        }
    }
}

fn ask_setup(prompt: &Prompt) -> Result<Setup, Error> {
    let options = [
        Choice::new(copy::SETUP_DEFAULTS, copy::SETUP_DEFAULTS_ABOUT),
        Choice::new(copy::SETUP_MINIMAL, copy::SETUP_MINIMAL_ABOUT),
    ];
    Ok(match prompt.choose(copy::REPO_SETUP, &options)? {
        Some(1) => Setup::Minimal,
        _ => Setup::Defaults,
    })
}

pub struct Repo {
    name: NonEmpty,
    id: NonEmpty,
    root: PathBuf,
    host: String,
    owner: Option<String>,
    assets: PathBuf,
    setup: Setup,
    libraries: Libraries,
    image: Option<Image>,
    workflows: Option<Workflows>,
    scripts: Scripts,
    remote: bool,
    install_gh: bool,
}

impl Repo {
    pub fn collect(
        name: Option<String>,
        host: Option<String>,
        owner: Option<String>,
        image_name: Option<String>,
        base: Option<String>,
        root_arg: Option<PathBuf>,
        prompt: &Prompt,
    ) -> Result<Option<Self>, Error> {
        let mut given = Given {
            name,
            host: host.filter(|host| !host.trim().is_empty()),
            owner: owner.filter(|owner| !owner.trim().is_empty()),
            image: image_name,
            base,
            root: root_arg,
        };
        let mut repo = Self::ask(Field::Name, &given, None, prompt)?;
        // A flag answers the first pass alone. A later pass keeps the review's answer and the flag never overrides it.
        given = Given {
            root: given.root.take(),
            ..Given::default()
        };
        while prompt.draws() {
            let rows = repo.rows();
            let drawn: Vec<(String, String)> = rows
                .iter()
                .map(|(_, label, value)| (label.to_string(), value.clone()))
                .collect();
            match common::ui::review(
                copy::REVIEW,
                &drawn,
                copy::CREATE,
                copy::REVIEW_KEYS,
                None,
                rows.len(),
            )? {
                None => return Ok(None),
                Some(at) if at == rows.len() => break,
                Some(at) => repo = Self::ask(rows[at].0, &given, Some(&repo), prompt)?,
            }
        }
        Ok(Some(repo))
    }

    fn ask(
        from: Field,
        given: &Given,
        prev: Option<&Self>,
        prompt: &Prompt,
    ) -> Result<Self, Error> {
        let name = match prev {
            Some(prev) if from > Field::Name => prev.name.clone(),
            _ => NonEmpty::parse(
                prompt.line(
                    given
                        .name
                        .clone()
                        .or_else(|| given.root.as_deref().and_then(named_after_root)),
                    copy::REPO_NAME,
                    "a name argument",
                    "",
                    prev.map(|prev| prev.name.as_str()),
                )?,
                "the repository name",
            )?,
        };
        let id = NonEmpty::parse(crate::init::id(name.as_str())?, "the repository id")?;
        let root = given
            .root
            .clone()
            .unwrap_or_else(|| PathBuf::from(id.as_str()));
        refuse_nesting(&root)?;
        let assets = crate::init::assets()?;
        if prev.is_none() {
            println!("Creating {id}...\n");
        }

        let setup = match prev {
            Some(prev) if from > Field::Setup => prev.setup,
            _ => ask_setup(prompt)?,
        };

        let configure = given.host.is_some()
            || given.owner.is_some()
            || from == Field::Provider
            || (setup == Setup::Defaults && from <= Field::Provider);
        let (host, owner) = match prev {
            Some(prev) if from > Field::Provider => (prev.host.clone(), prev.owner.clone()),
            _ => {
                let host = match (given.host.clone(), from) {
                    (Some(host), _) => host,
                    (None, Field::Provider) => {
                        choose_host(prev.map(|prev| prev.host.as_str()), prompt)?
                    }
                    (None, _) if prev.is_none() && configure => choose_host(None, prompt)?,
                    (None, _) => prev
                        .map(|prev| prev.host.clone())
                        .unwrap_or_else(|| forge::HOST.to_string()),
                };
                let owner = match configure {
                    true => Some(prompt.line(
                        given.owner.clone(),
                        &copy::username(&host),
                        "`--owner`",
                        &format!("{host}/"),
                        prev.and_then(|prev| prev.owner.as_deref()),
                    )?),
                    false => None,
                };
                (host, owner)
            }
        };
        let mut remote = false;
        let mut install_gh = false;
        if let Some(named) = &owner {
            match prev {
                Some(prev) if from > Field::Remote => {
                    remote = prev.remote;
                    install_gh = prev.install_gh;
                }
                _ if from <= Field::Remote => {
                    let offering = host == forge::HOST && prompt.asks();
                    let current = prev.is_some_and(|prev| prev.remote);
                    let asked = match offering {
                        false => false,
                        true => prompt.confirm_current(
                            copy::CREATE_REMOTE,
                            copy::YES,
                            copy::SKIP,
                            current,
                        )?,
                    };
                    if asked {
                        match (gh_installed(), gh_logged_in()) {
                            (false, _) => {
                                install_gh =
                                    prompt.confirm(copy::NO_GH, copy::YES, copy::SKIP_REMOTE)?
                            }
                            (true, false) => println!(
                                "You will need to login with user '{named}' to create the repo on Github.\n\
                                 You can log in with the following command:\n\
                                 gh auth login\n"
                            ),
                            (true, true) => remote = true,
                        }
                    }
                }
                _ => {}
            }
        }
        let url = owner.as_deref().map(|_| BASE_URL_REFERENCE.to_string());
        let libraries = match prev {
            Some(prev) if from > Field::Libraries => prev.libraries.clone(),
            Some(prev) => Libraries::collect(&assets, Some(&prev.libraries), prompt)?,
            None => Libraries {
                chosen: Libraries::every(&assets),
            },
        };
        let held = prev.and_then(|prev| prev.image.as_ref());
        let image = match prev {
            Some(prev) if from > Field::Base => prev.image.clone(),
            _ => {
                let wanted = given.image.is_some()
                    || from == Field::Base
                    || match prev {
                        None => match (setup, prompt.asks()) {
                            (Setup::Minimal, _) => false,
                            (Setup::Defaults, true) => {
                                prompt.confirm(copy::IMAGES, copy::YES, copy::NO)?
                            }
                            (Setup::Defaults, false) => true,
                        },
                        Some(prev) => prompt.confirm_current(
                            copy::IMAGES,
                            copy::YES,
                            copy::NO,
                            prev.image.is_some(),
                        )?,
                    };
                match wanted {
                    true => Some(Image::collect(
                        Home {
                            root: &root,
                            repo: name.as_str(),
                            url,
                            libraries_text: &crate::init::libraries_block(&libraries.chosen),
                        },
                        given
                            .image
                            .clone()
                            .or_else(|| prev.is_none().then(|| name.to_string())),
                        given.base.clone(),
                        "`--image`",
                        from,
                        held,
                        prompt,
                    )?),
                    false => None,
                }
            }
        };
        let workflows = match owner.as_ref() {
            None => None,
            Some(_) => match prev {
                Some(prev) if from > Field::Workflows => prev.workflows.clone(),
                Some(prev) => {
                    let family = prev
                        .image
                        .as_ref()
                        .and_then(|image| image.family.as_ref())
                        .map_or("", NonEmpty::as_str);
                    let basis = crate::resolve::workflow::Basis::scaffolding(family);
                    let mut say = |text: &str| print!("{text}");
                    match prev.workflows.as_ref() {
                        Some(held) => held.again(&basis, from, prompt, &mut say)?,
                        None => Workflows::collect(&basis, &[], from, prompt, &mut say)?,
                    }
                    .map(Workflows::scaffolded)
                }
                None => match setup {
                    Setup::Defaults => Some(Workflows::sensible()),
                    Setup::Minimal => None,
                },
            },
        };
        let scripts = match prev {
            Some(prev) if from > Field::Scripts => prev.scripts.clone(),
            Some(prev) => Scripts::collect(&root, &[], &prev.scripts.chosen, prompt)?,
            None => match setup {
                Setup::Defaults => Scripts {
                    chosen: vec![SKELETON_FILE],
                },
                Setup::Minimal => Scripts::default(),
            },
        };
        Ok(Self {
            name,
            id,
            root,
            host,
            owner,
            assets,
            setup,
            libraries,
            image,
            workflows,
            scripts,
            remote,
            install_gh,
        })
    }

    fn rows(&self) -> Vec<(Field, &'static str, String)> {
        let none = |command: &str| format!("{}; {command}", copy::NONE);
        let mut rows = vec![
            (Field::Name, copy::ROW_NAME, self.name.to_string()),
            (
                Field::Setup,
                copy::ROW_SETUP,
                self.setup.label().to_string(),
            ),
            (
                Field::Provider,
                copy::ROW_PROVIDER,
                match &self.owner {
                    Some(owner) => format!("{}/{owner}", self.host),
                    None => none(copy::SET_PROVIDER),
                },
            ),
        ];
        if self.owner.is_some() && self.host == forge::HOST {
            rows.push((
                Field::Remote,
                copy::ROW_REMOTE,
                match self.remote {
                    true => copy::REMOTE_MADE,
                    false => copy::REMOTE_NOT,
                }
                .to_string(),
            ));
        }
        rows.push(self.libraries.row());
        match &self.image {
            Some(image) => {
                rows.push((Field::Image, copy::ROW_IMAGE, image.name.to_string()));
                rows.push((Field::Base, copy::ROW_BASE, image.base.to_string()));
            }
            None => rows.push((Field::Image, copy::ROW_IMAGE, none(copy::SET_IMAGE))),
        }
        match (&self.workflows, self.owner.is_some()) {
            (Some(workflows), _) => rows.extend(workflows.rows()),
            (None, true) => rows.push((
                Field::Workflows,
                copy::ROW_WORKFLOWS,
                none(copy::SET_WORKFLOWS_COMMAND),
            )),
            (None, false) => rows.push((
                Field::Workflows,
                copy::ROW_WORKFLOWS,
                none(copy::SET_WORKFLOWS_COMMAND),
            )),
        }
        match &self.scripts.chosen.is_empty() {
            true => rows.push((
                Field::Scripts,
                copy::ROW_SCRIPTS,
                none(copy::SET_SCRIPTS_COMMAND),
            )),
            false => rows.push(self.scripts.row()),
        }
        rows
    }

    fn registry(&self) -> Option<String> {
        let owner = self.owner.as_deref().filter(|_| self.host == forge::HOST)?;
        // ghcr.io refuses capitals in an image path. A GitHub owner name may hold them.
        Some(format!("ghcr.io/{}", owner.to_lowercase()))
    }

    pub fn apply(&self) -> Result<(), String> {
        let base_url = self
            .owner
            .as_deref()
            .map(|owner| format!("{}/{}", forge::origin(&self.host, owner), self.id.as_str()));
        let mut wrote: Vec<(PathBuf, Change)> = crate::init::write(
            &self.root,
            self.name.as_str(),
            base_url.as_deref(),
            self.registry().as_deref(),
            &self.assets,
            &crate::init::libraries_block(&self.libraries.chosen),
        )?
        .into_iter()
        .map(|path| (path, Change::Created))
        .collect();
        git_init(&self.root)?;
        println!("initialised a git repository in {}", self.root.display());
        if let Some(image) = &self.image {
            wrote.extend(image.apply(&self.root)?);
        }
        if let Some(workflows) = &self.workflows {
            wrote.extend(workflows.apply(&self.root)?);
        }
        wrote.extend(self.scripts.apply(&self.root)?);
        if let (true, Some(owner)) = (self.remote, &self.owner) {
            create_remote(owner, self.id.as_str())?;
            println!("created {}/{} on github", owner, self.id);
        }
        report(&self.root, &wrote);

        let Self { host, id, .. } = self;
        let mut next = Vec::new();
        if std::fs::canonicalize(&self.root).ok() != std::env::current_dir().ok() {
            next.push(format!("cd {}", self.root.display()));
        }
        next.push("buildc generate".to_string());
        next.push("git add -A && git commit".to_string());
        if self.install_gh {
            next.push(GH_INSTALL.to_string());
        }
        if let Some(owner) = &self.owner {
            next.push(match self.remote || host != forge::HOST {
                true => format!(
                    "git remote add origin {}/{id} && git push -u origin main",
                    forge::origin(host, owner)
                ),
                false => format!("gh repo create {owner}/{id} --source=. --push"),
            });
        }
        let next: Vec<String> = next.iter().map(|line| format!("\x20 {line}")).collect();
        println!("\nnext:\n{}\n", next.join("\n"));
        Ok(())
    }
}
const SKELETON_FILE: &str = "Containerfile.skeleton";

fn offered() -> Vec<(&'static str, &'static str, &'static str)> {
    std::iter::once((
        SKELETON_FILE,
        "the Containerfile around the generated component layers",
        crate::init::SKELETON,
    ))
    .chain(
        crate::emit::SCRIPTS
            .iter()
            .map(|script| (script.name, script.about, script.body)),
    )
    .collect()
}

#[derive(Clone, Default)]
pub struct Scripts {
    chosen: Vec<&'static str>,
}

impl Scripts {
    pub fn collect(
        root: &Path,
        names: &[&str],
        held: &[&str],
        prompt: &Prompt,
    ) -> Result<Self, Error> {
        let open: Vec<_> = offered()
            .into_iter()
            .filter(|(name, _, _)| !root.join(layout::SCRIPTS).join(name).exists())
            .collect();
        if !names.is_empty() {
            let chosen = names
                .iter()
                .map(|name| match open.iter().find(|(open, _, _)| open == name) {
                    Some((open, _, _)) => Ok(*open),
                    None if root.join(layout::SCRIPTS).join(name).exists() => Err(
                        Error::Operation(format!("scripts/{name} is already the repository's own")),
                    ),
                    None => Err(Error::Operation(format!(
                        "`{name}` is not a file buildc supplies; it supplies {}",
                        offered()
                            .iter()
                            .map(|(name, _, _)| *name)
                            .collect::<Vec<_>>()
                            .join(", ")
                    ))),
                })
                .collect::<Result<Vec<_>, Error>>()?;
            return Ok(Self { chosen });
        }
        let options: Vec<Choice> = open
            .iter()
            .map(|(name, about, _)| Choice::new(*name, *about))
            .collect();
        let on: Vec<usize> = open
            .iter()
            .enumerate()
            .filter(|(_, (name, _, _))| held.contains(name))
            .map(|(at, _)| at)
            .collect();
        let Answer::Chosen(chosen) = prompt.choose_many(copy::SCRIPTS, &options, &on)? else {
            unreachable!("choose_many answers with a choice or fails")
        };
        Ok(Self {
            chosen: chosen.iter().map(|at| open[*at].0).collect(),
        })
    }

    fn row(&self) -> (Field, &'static str, String) {
        (
            Field::Scripts,
            copy::ROW_SCRIPTS,
            match self.chosen.is_empty() {
                true => copy::NONE.to_string(),
                false => self.chosen.join(", "),
            },
        )
    }

    pub fn apply(&self, root: &Path) -> Result<Vec<(PathBuf, Change)>, String> {
        let kept = |name: &str| root.join(layout::SCRIPTS).join(name);
        let pristine: Vec<_> = crate::emit::SCRIPTS
            .iter()
            .filter(|script| {
                !self.chosen.contains(&script.name)
                    && std::fs::read_to_string(kept(script.name))
                        .is_ok_and(|held| held == crate::emit::located(root, script.body))
            })
            .map(|script| (script.name, script.body, Change::Updated(String::new())))
            .collect();
        let chosen: Vec<_> = offered()
            .into_iter()
            .filter(|(name, _, _)| self.chosen.contains(name))
            .map(|(name, _, body)| (name, body, Change::Created))
            .collect();
        for (name, body, _) in &chosen {
            crate::init::put(&kept(name), body)?;
        }
        let mut wrote = Vec::new();
        for (name, body, change) in chosen.into_iter().chain(pristine) {
            let at = kept(name);
            crate::init::put(&at, &crate::emit::located(root, body))?;
            if name.ends_with(".sh") {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&at, std::fs::Permissions::from_mode(0o755))
                    .map_err(|err| format!("{}: {err}", at.display()))?;
            }
            wrote.push((PathBuf::from(layout::SCRIPTS).join(name), change));
        }
        Ok(wrote)
    }
}

fn choose_host(current: Option<&str>, prompt: &Prompt) -> Result<String, Error> {
    let options = host_options();
    let at = current.map(|held| usize::from(held != forge::HOST));
    match ask_one(prompt, copy::REPO_HOST, &options, at)? {
        Some(0) | None => Ok(forge::HOST.to_string()),
        _ => Ok(prompt.line(
            None,
            copy::FORGEJO_ADDRESS,
            "`--host`",
            "",
            current.filter(|held| *held != forge::HOST),
        )?),
    }
}

fn host_options() -> [Choice; 2] {
    [
        Choice::new(forge::HOST, copy::host_github()),
        Choice::new("forgejo", copy::HOST_FORGEJO),
    ]
}

fn ask_one(
    prompt: &Prompt,
    question: &str,
    options: &[Choice],
    at: Option<usize>,
) -> Result<Option<usize>, Error> {
    Ok(match at {
        Some(at) => prompt.choose_current(question, options, at)?,
        None => prompt.choose(question, options)?,
    })
}

#[derive(Clone)]
pub struct Image {
    id: String,
    file: PathBuf,
    text: String,
    name: NonEmpty,
    base: NonEmpty,
    family: Option<NonEmpty>,
    takes_components: bool,
    bootloader: Option<String>,
    names_default: Option<String>,
    kept: Vec<String>,
    own: Option<(String, String)>,
}

pub struct Home<'a> {
    pub root: &'a Path,
    pub repo: &'a str,
    pub url: Option<String>,
    pub libraries_text: &'a str,
}

impl Image {
    pub fn collect(
        home: Home,
        name: Option<String>,
        base: Option<String>,
        flag: &str,
        from: Field,
        prev: Option<&Self>,
        prompt: &Prompt,
    ) -> Result<Self, Error> {
        let Home {
            root,
            repo,
            url,
            libraries_text,
        } = home;
        let name = match prev {
            Some(prev) if from > Field::Image => prev.name.clone(),
            _ => NonEmpty::parse(
                prompt.line(
                    name,
                    copy::IMAGE_NAME,
                    flag,
                    "",
                    prev.map(|prev| prev.name.as_str())
                        .or_else(|| crate::init::id(repo).is_ok().then_some(repo)),
                )?,
                "the image name",
            )?,
        };
        let takes_components = prev.is_none_or(|prev| prev.takes_components);
        if base.as_deref().is_some_and(|base| base.trim().is_empty()) {
            return Err(Error::Operation(
                "`--base` is empty; name a base image or a base component".to_string(),
            ));
        }
        let id = crate::init::id(name.as_str())?;
        let file = layout::sibling(&root.join(&id));
        if let Some(held) = layout::manifests_at(&root.join(&id)).first() {
            return Err(Error::Operation(format!(
                "{} is already there",
                held.display()
            )));
        }
        let (list, _) = crate::model::image::List::load(root);
        if list.builds.iter().any(|build| build.name == id) {
            return Err(Error::Operation(format!(
                "`{id}` is already a build in {}",
                layout::REPO_FILE
            )));
        }
        let names_default = implicit_default(&list, &id);

        let scaffolded;
        // Until this run writes `repo.kdl`, the libraries come from the block it will put there.
        let (sources, root_library, default_library) =
            match list.libraries.is_empty() && !root.join(layout::REPO_FILE).is_file() {
                true => {
                    let (libraries, own) = crate::resolve::library::split_root(
                        root,
                        crate::parse::repo::libraries_in(libraries_text),
                    );
                    scaffolded = libraries;
                    let default = crate::parse::repo::default_library_in(libraries_text);
                    (scaffolded.as_slice(), own, default)
                }
                false => (
                    list.libraries.as_slice(),
                    list.root_library.clone(),
                    list.default_library.as_ref().map(|(name, _)| name.clone()),
                ),
            };
        if let Err(why) = crate::collection::fetch(root, sources) {
            eprintln!("buildc: {why}; the bases offered are only what is already on this machine");
        }
        let bases = crate::bases::catalog(root, sources);
        let base = match base {
            Some(given) => given,
            None => choose_base(&bases, prev.map(|prev| prev.base.as_str()), prompt)?,
        };
        let base = catalogued(&bases, base);
        if crate::bases::find(&bases, &base).is_none() && crate::model::image::is_local_path(&base)
        {
            return Err(Error::Operation(format!(
                "`{base}` names no base component in the repository or a library; give an image \
                 reference with a tag, such as `quay.io/fedora/fedora-bootc:44`"
            )));
        }
        let known = crate::bases::find(&bases, &base);
        let family = known
            .filter(|known| !known.family.trim().is_empty())
            .and_then(|known| NonEmpty::parse(known.family.clone(), "the base family").ok());
        let disk = crate::parse::disk::Disk::scan(root);
        let index = crate::provider::Index::scan(root, sources, &disk, false);
        let roles = match family {
            Some(_) => roles(known, &index),
            None => Vec::new(),
        };
        let mut wanted = match &family {
            Some(family) => wanted(&index, family.as_str(), &roles),
            None => Vec::new(),
        };
        let fetched;
        if !roles.is_empty()
            && wanted.len() < roles.len()
            && prompt.asks()
            && !index.unread().is_empty()
        {
            fetched = crate::provider::Index::scan(root, sources, &disk, true);
            if let Some(why) = fetched.unreached() {
                eprintln!(
                    "buildc: {why}; what is offered below is only what is already on this machine"
                );
            }
            if let Some(family) = &family {
                wanted = self::wanted(&fetched, family.as_str(), &roles);
            }
        }
        let take_wanted = match wanted.is_empty() {
            true => true,
            false => offer(&base, &wanted, prompt, takes_components)?,
        };
        let url = seeded_home_url(&list, url, &index);
        let holds = |provider: &crate::provider::Provider| {
            layout::is_component(&layout::component(root, &provider.name))
        };
        let resolver = crate::resolve::library::Libraries::read(
            root,
            sources,
            root_library.as_deref(),
            default_library.as_deref(),
            Vec::new(),
        );
        let path = |provider: &crate::provider::Provider| {
            let owner = match holds(provider) {
                true => None,
                false => provider.owner.as_deref(),
            };
            listed(&resolver, root_library.as_deref(), &provider.name, owner)
        };
        let seed = match take_wanted {
            true => seeded(&wanted, path, name.as_str(), url.as_deref()),
            false => Vec::new(),
        };
        let kept: Vec<String> = match take_wanted {
            true => wanted
                .iter()
                .filter(|provider| provider.owner.is_some() && holds(provider))
                .map(|provider| provider.name.clone())
                .collect(),
            false => Vec::new(),
        };
        let (component, own, version, digest) = match known {
            Some(known) => (
                listed(
                    &resolver,
                    root_library.as_deref(),
                    &known.name,
                    known.owner.as_deref(),
                ),
                None,
                known.version.clone(),
                None,
            ),
            None => {
                let OwnBase {
                    path,
                    manifest,
                    version,
                    digest,
                } = own_base(root, &base)?;
                (path.clone(), Some((path, manifest)), version, digest)
            }
        };
        let held = prev
            .filter(|prev| prev.base.as_str() == base)
            .and_then(|prev| prev.bootloader.as_deref());
        let bootloader = choose_bootloader(known, held, prompt)?;
        let text = image_kdl(
            &BaseLine {
                path: &component,
                version: version.as_deref(),
                digest: digest.as_deref(),
            },
            &seed,
        );
        Ok(Self {
            id: id.to_string(),
            text,
            file,
            name,
            base: NonEmpty::parse(base, "the base")?,
            family,
            takes_components: wanted.is_empty() || !seed.is_empty(),
            bootloader,
            names_default,
            kept,
            own,
        })
    }

    fn build_entry(&self) -> String {
        let entry = format!("{0} \"./{0}\"", self.id);
        match &self.bootloader {
            Some(bootloader) => format!(
                "{entry} {{\n    install {{\n        bootloader \"{bootloader}\"\n    }}\n}}"
            ),
            None => entry,
        }
    }

    pub fn apply(&self, root: &Path) -> Result<Vec<(PathBuf, Change)>, String> {
        let mut wrote: Vec<(PathBuf, Change)> = self
            .kept
            .iter()
            .map(|name| {
                (
                    layout::under(root, &layout::component(root, name)),
                    Change::Updated("already here, so the image lists it as it is".to_string()),
                )
            })
            .collect();
        let own = self
            .own
            .as_ref()
            .map(|(path, text)| (layout::manifest(root, path), text));
        let repo = root.join(layout::REPO_FILE);
        let files: Vec<PathBuf> = own
            .iter()
            .map(|(file, _)| file.clone())
            .chain([self.file.clone(), repo.clone()])
            .collect();
        crate::init::undoable(root, &files, || {
            self.write(root, own, &repo, &mut wrote)?;
            Ok(wrote)
        })
    }

    fn write(
        &self,
        root: &Path,
        own: Option<(PathBuf, &String)>,
        repo: &Path,
        wrote: &mut Vec<(PathBuf, Change)>,
    ) -> Result<(), String> {
        if let Some((file, text)) = own {
            crate::init::put(&file, text)?;
            wrote.push((under(root, &file), Change::Created));
        }
        crate::init::put(&self.file, &self.text)?;
        wrote.push((under(root, &self.file), Change::Created));
        crate::parse::declaration::append(repo, &["builds"], &self.build_entry())?;
        // Without `default=`, a repository with one build has that build as the default. Writing that name keeps the build the default when another build joins.
        if let Some(was) = &self.names_default {
            crate::parse::declaration::property(repo, "builds", "default", was)?;
        }
        wrote.push((
            under(root, Path::new(layout::REPO_FILE)),
            Change::Updated(format!("`{}` added as a build", self.id)),
        ));
        if let Some(was) = &self.names_default {
            wrote.push((
                under(root, Path::new(layout::REPO_FILE)),
                Change::Updated(format!("{was} set as the default build")),
            ));
        }
        Ok(())
    }
}

fn implicit_default(list: &crate::model::image::List, id: &str) -> Option<String> {
    match (&list.default_image_id, list.builds.as_slice()) {
        (None, []) => Some(id.to_string()),
        (None, [only]) => Some(only.name.clone()),
        _ => None,
    }
}

fn catalogued(bases: &[crate::model::component::BaseComponent], base: String) -> String {
    crate::bases::find(bases, &base).map_or(base, |known| known.name.clone())
}

struct OwnBase {
    path: String,
    manifest: String,
    version: Option<String>,
    digest: Option<String>,
}

fn own_base(root: &Path, image: &str) -> Result<OwnBase, String> {
    let (undigested, digest) = match image.split_once('@') {
        Some((before, digest)) => (before, Some(digest.to_string())),
        None => (image, None),
    };
    // A tag holds no slash because a registry port's colon comes before the first slash.
    let (repository, version) = match undigested.rsplit_once(':') {
        Some((repository, tag)) if !tag.contains('/') => (repository, Some(tag.to_string())),
        _ => (undigested, None),
    };
    let last = repository.rsplit('/').next().unwrap_or(repository);
    let stem: String = last
        .to_lowercase()
        .chars()
        .map(|c| match c.is_ascii_alphanumeric() {
            true => c,
            false => '-',
        })
        .collect();
    let stem = stem.trim_matches('-');
    // A component name starts with a letter. A last segment that starts with a digit takes the `base-` prefix.
    let stem = match stem.starts_with(|c: char| c.is_ascii_lowercase()) {
        true => stem.to_string(),
        false => format!("base-{stem}"),
    };
    let path = format!("base-image/{stem}");
    if layout::component(root, &path).exists() {
        return Err(format!(
            "components/{path} is already there and names another base; `--base {path}` builds on it"
        ));
    }
    let reference = match &version {
        Some(tag) => format!(
            "base \"{repository}:{{version}}\"\n\noption \"version\" type=\"string\" {{\n    default \"{tag}\"\n}}\n"
        ),
        None => format!("base \"{repository}\"\n"),
    };
    let manifest = format!(
        "schema-version {}\n\n{reference}",
        crate::model::image::SCHEMA_VERSION
    );
    Ok(OwnBase {
        path,
        manifest,
        version,
        digest,
    })
}

fn choose_base(
    bases: &[crate::model::component::BaseComponent],
    current: Option<&str>,
    prompt: &Prompt,
) -> Result<String, Error> {
    let order = base_order(bases);
    let mut options = base_options(bases);
    let at = current.and_then(|held| order.iter().position(|&at| bases[at].name == held));
    if prompt.draws() && !options.is_empty() {
        options.push(Choice::new(copy::OTHER_BASE, copy::OTHER_BASE_ABOUT));
        return match ask_one(prompt, copy::IMAGE_BASE, &options, at)? {
            Some(chosen) if chosen < order.len() => Ok(bases[order[chosen]].name.clone()),
            Some(_) => Ok(prompt.text(
                None,
                copy::BASE_IMAGE,
                "`--base`",
                order
                    .first()
                    .and_then(|at| bases.get(*at))
                    .map(|base| base.image.as_str()),
            )?),
            None => unreachable!("a drawn picker either chooses or cancels"),
        };
    }
    match ask_one(prompt, copy::IMAGE_BASE, &options, at)? {
        Some(chosen) => Ok(bases[order[chosen]].name.clone()),
        None => Ok(prompt.text(
            None,
            copy::BASE_IMAGE,
            "`--base`",
            order
                .first()
                .and_then(|at| bases.get(*at))
                .map(|base| base.image.as_str()),
        )?),
    }
}

fn base_order(bases: &[crate::model::component::BaseComponent]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..bases.len()).collect();
    order.sort_by_key(|&at| {
        let base = &bases[at];
        let fedora = base.name.contains("fedora") || base.image.contains("fedora");
        (u8::from(!fedora), u8::from(base.family != "fedora"))
    });
    order
}

fn base_options(bases: &[crate::model::component::BaseComponent]) -> Vec<Choice> {
    base_order(bases)
        .into_iter()
        .map(|at| Choice::new(base_label(&bases[at]), &bases[at].about))
        .collect()
}

fn base_label(base: &crate::model::component::BaseComponent) -> String {
    match &base.owner {
        Some(owner) => format!("{} ({owner}/{})", base.image, base.name),
        None => format!("{} ({})", base.image, base.name),
    }
}

// The base lists its default bootloader first.
fn choose_bootloader(
    known: Option<&crate::model::component::BaseComponent>,
    held: Option<&str>,
    prompt: &Prompt,
) -> Result<Option<String>, Error> {
    let offered = known
        .map(|base| base.bootloaders.as_slice())
        .unwrap_or_default();
    if offered.len() < 2 {
        return Ok(offered.first().cloned());
    }
    let options = bootloader_options(offered);
    let at = held.and_then(|held| offered.iter().position(|name| name == held));
    Ok(prompt
        .choose_current(copy::IMAGE_BOOTLOADER, &options, at.unwrap_or(0))?
        .map(|chosen| offered[chosen].clone()))
}

fn bootloader_options(offered: &[String]) -> Vec<Choice> {
    offered
        .iter()
        .map(|name| match name.as_str() {
            "grub2" => Choice::new(name, copy::BOOTLOADER_GRUB2),
            _ => Choice::new(name, copy::BOOTLOADER_SYSTEMD),
        })
        .collect()
}

pub struct Component {
    path: String,
    written: Written,
}

enum Written {
    Directory {
        file: PathBuf,
        text: String,
        listing: Listing,
    },
    InlineBuild {
        block: String,
        listing: Listing,
    },
    InlineComponent {
        block: String,
        place: Place,
        parent: String,
        name: String,
    },
}

pub(crate) fn refuse_vocabulary(path: &str, inline: bool) -> Result<(), Error> {
    let top = path.split_once('/').map_or(path, |(top, _)| top);
    if crate::parse::image::is_manifest_node(top) {
        return Err(Error::Operation(format!(
            "`{top}` is a manifest node, so a component file reads a bare `{top}` as that \
             node; give `{path}` another name"
        )));
    }
    if inline {
        if let Some(segment) = path
            .split('/')
            .find(|segment| crate::parse::image::is_manifest_node(segment))
        {
            return Err(Error::Operation(format!(
                "an inline component cannot hold the segment `{segment}` because a component file \
                 reads it as a manifest line; give `{path}` another name"
            )));
        }
    }
    Ok(())
}

fn inline_target(
    list: &crate::model::image::List,
    builds: &[String],
    prompt: &Prompt,
) -> Result<Listing, Error> {
    let build = match builds {
        [] => None,
        [one] => Some(one.as_str()),
        many => {
            return Err(Error::Operation(format!(
                "an inline component belongs to one build and takes one `--build`; the command \
                 line names {}",
                many.join(", ")
            )))
        }
    };
    match Listing::target_from(list, build, copy::TARGET_BUILD, prompt)? {
        Listing::NoImage => Err(Error::Operation(
            "there is no build to hold an inline component; `buildc create image <name>` writes one"
                .to_string(),
        )),
        Listing::Declined { asked: true } => Err(Error::Operation(
            "an inline component lives in a build's component file and needs one; choose a build"
                .to_string(),
        )),
        Listing::Declined { asked: false } | Listing::Cancelled => Err(Error::Operation(
            "an inline component lives in a build's component file; name the build with `--build`"
                .to_string(),
        )),
        listing @ Listing::In(_) => Ok(listing),
    }
}

fn local_choice(root: &Path, listed: &Listed) -> Choice {
    let about = layout::under(root, listed.place.file())
        .display()
        .to_string();
    let choice = Choice::new(listed.path.as_str(), &about);
    match listed.hidden {
        true => choice.hidden(),
        false => choice,
    }
}

impl Component {
    pub fn collect(
        root: &Path,
        name: Option<String>,
        with: Vec<(String, String)>,
        builds: Vec<String>,
        inside: Option<String>,
        inline: bool,
        prompt: &Prompt,
    ) -> Result<Self, Error> {
        let list = crate::model::image::List::editable(root).map_err(|issues| issues.plain())?;
        if inside.is_some() && !builds.is_empty() {
            return Err(Error::Operation(
                "`--build` and `--in` name different holders for the new component; give one"
                    .to_string(),
            ));
        }
        if inside.is_some() && !inline {
            return Err(Error::Operation(
                "`--in` writes the component inside another component, so it takes `--inline`"
                    .to_string(),
            ));
        }
        let name = prompt.text(name, copy::COMPONENT_NAME, "a name argument", None)?;
        let path = name
            .split('/')
            .map(crate::init::id)
            .collect::<Result<Vec<_>, _>>()?
            .join("/");
        refuse_vocabulary(&path, inline)?;
        if with.iter().any(|(verb, _)| verb == "packages") {
            return Err(Error::Operation(format!(
                "`--with packages=...` writes no package list; run `buildc add package \
                 <names...> --component {path}` after"
            )));
        }
        let components = layout::components(root);
        let home = list.resolver.home();
        let children_read_home = list
            .builds
            .iter()
            .all(|build| list.scope(&build.name).is_none());
        if !inline && children_read_home {
            home.clone().map_err(Error::Operation)?;
        }
        let local = crate::parse::local::all(root, &list);
        let vacant = |at: &str, inline: bool| -> Result<(), Error> {
            let Some(held) = local.iter().find(|listed| listed.path == at) else {
                return Ok(());
            };
            let file = held.place.file().display().to_string();
            let folder = match home.as_ref() {
                Ok(home) if held.place.file().starts_with(home) => layout::under(root, home),
                _ => layout::under(root, &components),
            }
            .display()
            .to_string();
            Err(Error::Operation(match (&held.place, inline) {
                (Place::Directory { .. }, true) => format!(
                    "{folder}/{at} is already a component; an inline `{at}` would define it a \
                     second time"
                ),
                (Place::Directory { .. }, false) => format!("{folder}/{at} is already there"),
                (Place::Inline { .. }, true) => format!(
                    "{file} defines `{at}` inline; an inline `{at}` would define it a second time"
                ),
                (Place::Inline { .. }, false) => format!(
                    "{file} defines `{at}` inline; {folder}/{at} would define it a second time"
                ),
            }))
        };
        let relative = |top: &str| match top {
            "" => path.clone(),
            top => crate::model::image::child(Some(top), &path),
        };
        let mut target: Option<(Place, String, String)> = None;
        let early = match inside.as_deref() {
            Some(inside) => {
                let held = crate::parse::local::named(&local, inside)?.clone();
                let full = relative(&held.context);
                vacant(&full, true)?;
                target = Some((held.place, inside.to_string(), full));
                None
            }
            None if inline && !builds.is_empty() => Some(inline_target(&list, &builds, prompt)?),
            None if inline => {
                if !prompt.asks() {
                    return Err(Error::Operation(
                        "an inline component lives inside another component; name it with \
                         `--in <component>` or a build with `--build`"
                            .to_string(),
                    ));
                }
                let options: Vec<Choice> = local
                    .iter()
                    .map(|listed| local_choice(root, listed))
                    .collect();
                let Some(at) = prompt.choose(copy::COMPONENT_INSIDE, &options)? else {
                    return Err(Error::Operation(
                        "an inline component lives inside another component; choose one"
                            .to_string(),
                    ));
                };
                let held = local[at].clone();
                let full = relative(&held.context);
                vacant(&full, true)?;
                target = Some((held.place, held.path.clone(), full));
                None
            }
            None if !builds.is_empty() => Some(Listing::collect_from(&list, builds, prompt)?),
            None => None,
        };
        // A taken path is refused here when its folder is already known. The check runs before the description question.
        match &early {
            Some(listing) => vacant(&listing.child(&path)?, inline)?,
            None if target.is_some() => {}
            None if children_read_home => {
                vacant(&path, false)?;
            }
            None => {}
        }

        let description = match with.iter().any(|(verb, _)| verb == "description") {
            true => String::new(),
            false => prompt.text(
                None,
                copy::COMPONENT_DESCRIPTION,
                "`--with description=...`",
                Some(""),
            )?,
        };
        let targets: Vec<String> = match with.iter().any(|(verb, _)| verb == "targets") {
            true => Vec::new(),
            false => prompt
                .text(
                    None,
                    copy::COMPONENT_SUPPORTS,
                    "`--with targets=...`",
                    Some(""),
                )?
                .split_whitespace()
                .map(str::to_string)
                .collect(),
        };

        let sections = manifest_sections(&description, &targets, &with)?;
        if inline && sections.is_empty() {
            return Err(Error::Operation(
                "an inline component needs a manifest line; give it a description, \
                 supported base families or a `--with` line"
                    .to_string(),
            ));
        }
        let block = {
            let body: String = sections
                .join("\n")
                .lines()
                .map(|line| format!("    {line}\n"))
                .collect();
            format!(" {{\n{body}}}")
        };
        let (path, written) = match (target, early) {
            (Some((place, parent, full)), _) => (
                full,
                Written::InlineComponent {
                    block,
                    place,
                    parent,
                    name: path,
                },
            ),
            (None, Some(listing)) if inline => {
                let resolved = listing.child(&path)?;
                vacant(&resolved, true)?;
                listing.refuse_duplicate(&list, &resolved)?;
                (path, Written::InlineBuild { block, listing })
            }
            (None, early) => {
                let listing = match early {
                    Some(listing) => listing,
                    None => Listing::pick_from(&list, prompt)?,
                };
                let resolved = listing.child(&path)?;
                vacant(&resolved, false)?;
                listing.refuse_duplicate(&list, &resolved)?;
                let mut text = vec![format!(
                    "schema-version {}",
                    crate::model::image::SCHEMA_VERSION
                )];
                text.extend(sections);
                // A new rooted path goes to the default library. A path below a build's component goes under that component.
                let folder = match resolved == path {
                    true => home.clone().map_err(Error::Operation)?,
                    false => components.clone(),
                };
                (
                    path,
                    Written::Directory {
                        file: layout::manifest_at(&folder.join(&resolved)),
                        text: format!("{}\n", text.join("\n\n")),
                        listing,
                    },
                )
            }
        };
        Ok(Self { path, written })
    }

    pub fn apply(&self, root: &Path) -> Result<Vec<(PathBuf, Change)>, String> {
        match &self.written {
            Written::Directory {
                file,
                text,
                listing,
            } => {
                if listing.cancelled() {
                    return Ok(Vec::new());
                }
                listing.validate()?;
                crate::init::put(file, text)?;
                let list =
                    crate::model::image::List::editable(root).map_err(|issues| issues.plain())?;
                let mut wrote = vec![(under(root, file), Change::Created)];
                wrote.extend(listing.apply_children(&list, &[(self.path.as_str(), "")])?);
                Ok(wrote)
            }
            Written::InlineBuild { block, listing } => {
                if listing.cancelled() {
                    return Ok(Vec::new());
                }
                listing.validate()?;
                let list =
                    crate::model::image::List::editable(root).map_err(|issues| issues.plain())?;
                listing.apply_children(&list, &[(self.path.as_str(), block.as_str())])
            }
            Written::InlineComponent {
                block,
                place,
                parent,
                name,
            } => {
                let (chain, leaf) = placed(true, name, block);
                let file = place.file().to_path_buf();
                match place {
                    Place::Directory { .. } => append(&file, &chain, &leaf)?,
                    Place::Inline { node: at, .. } => {
                        append_inside(&file, *at, &wrap(&chain, &leaf))?
                    }
                }
                Ok(vec![(
                    under(root, &file),
                    Change::Updated(format!("`{}` added inside `{parent}`", self.path)),
                )])
            }
        }
    }
}

fn manifest_sections(
    description: &str,
    targets: &[String],
    with: &[(String, String)],
) -> Result<Vec<String>, String> {
    let mut sections: Vec<String> = Vec::new();
    if !description.is_empty() {
        sections.push(format!(
            "description \"{}\"",
            crate::parse::declaration::quotable(description)?
        ));
    }
    let mut declarations: Vec<String> = Vec::new();
    if !targets.is_empty() {
        let families = targets
            .iter()
            .map(|family| {
                Ok(format!(
                    "\"{}\"",
                    crate::parse::declaration::quotable(family)?
                ))
            })
            .collect::<Result<Vec<_>, String>>()?
            .join(" ");
        declarations.push(format!("targets {families}"));
    }
    for (verb, value) in with {
        declarations.push(format!(
            "{} \"{}\"",
            crate::parse::declaration::quotable(verb)?,
            crate::parse::declaration::quotable(value)?
        ));
    }
    if !declarations.is_empty() {
        sections.push(declarations.join("\n"));
    }
    Ok(sections)
}

#[cfg(feature = "fuzzing")]
pub fn fuzz_rewrite(file: &Path) {
    let _ = crate::parse::declaration::append(file, &[], "fuzz");
}

fn wanted<'a>(
    index: &'a crate::provider::Index,
    family: &str,
    roles: &[&str],
) -> Vec<&'a crate::provider::Provider> {
    let mut out: Vec<&crate::provider::Provider> = Vec::new();
    for capability in roles {
        let Some(provider) = index.adapter(capability, family) else {
            continue;
        };
        if !out
            .iter()
            .any(|held| held.qualified() == provider.qualified())
        {
            out.push(provider);
        }
    }
    out
}

fn roles<'a>(
    base: Option<&'a crate::model::component::BaseComponent>,
    index: &crate::provider::Index,
) -> Vec<&'a str> {
    let lacks = |role: &str| {
        base.is_none_or(|base| {
            !index
                .fitting(role, &base.family)
                .iter()
                .any(|provider| base.imports.contains(&provider.name))
        })
    };
    [BUILD_ENVIRONMENT, OS_RELEASE]
        .into_iter()
        .filter(|role| lacks(role))
        .chain(
            base.into_iter()
                .flat_map(|base| base.requires.iter().map(String::as_str)),
        )
        .collect()
}

fn offer(
    base: &str,
    wanted: &[&crate::provider::Provider],
    prompt: &Prompt,
    current: bool,
) -> Result<bool, Error> {
    if prompt.asks() {
        println!("{base} brings none of these components:");
        for provider in wanted {
            println!("\x20   {}", provider.qualified());
        }
        println!();
    }
    let [yes, no] = yes_no_options();
    Ok(prompt.confirm_current(copy::BRING_FOR_BASE, &yes.label, &no.label, current)?)
}

fn yes_no_options() -> [Choice; 2] {
    [Choice::new(copy::YES, ""), Choice::new(copy::NO, "")]
}

fn seeded(
    wanted: &[&crate::provider::Provider],
    path: impl Fn(&crate::provider::Provider) -> String,
    name: &str,
    url: Option<&str>,
) -> Vec<(String, Vec<String>)> {
    let issues = url.map(|url| format!("{url}/issues"));
    let fields = [
        ("name", Some(name)),
        ("home-url", url),
        ("bug-report-url", issues.as_deref()),
    ];
    wanted
        .iter()
        .map(|provider| {
            // The image name and the URLs belong to the `os-release` component alone.
            let lines = match provider
                .declares
                .provides
                .iter()
                .any(|has| has == OS_RELEASE)
            {
                true => fields
                    .iter()
                    .filter(|(option, _)| provider.declares.options.iter().any(|has| has == option))
                    .filter_map(|(option, value)| Some(format!("{option} \"{}\"", (*value)?)))
                    .collect(),
                false => Vec::new(),
            };
            (path(provider), lines)
        })
        .collect()
}

/// The image line names `library=` only when the bare path would read another library or fail to resolve.
fn listed(
    resolver: &crate::resolve::library::Libraries,
    root_library: Option<&str>,
    name: &str,
    owner: Option<&str>,
) -> String {
    let reads = resolver
        .resolve(name, None, true, None)
        .ok()
        .map(|held| held.map(|held| held.library));
    match (
        reads == Some(owner.map(str::to_string)),
        owner.or(root_library),
    ) {
        (false, Some(library)) => format!("{name} library=\"{library}\""),
        _ => name.to_string(),
    }
}

fn tree(entries: &[(&str, Vec<String>)]) -> String {
    let mut out = String::new();
    let mut at = 0;
    while at < entries.len() {
        let head = entries[at].0.split('/').next().unwrap_or_default();
        let run = entries[at..]
            .iter()
            .take_while(|(path, _)| path.split('/').next() == Some(head))
            .count();
        let mut body: Vec<String> = Vec::new();
        let mut below: Vec<(&str, Vec<String>)> = Vec::new();
        for (path, lines) in &entries[at..at + run] {
            match path.split_once('/') {
                Some((_, rest)) => below.push((rest, lines.clone())),
                None => body.extend(lines.iter().cloned()),
            }
        }
        let inner: String = body
            .iter()
            .map(|line| format!("{line}\n"))
            .chain(std::iter::once(tree(&below)))
            .collect::<String>()
            .lines()
            .map(|line| format!("    {line}\n"))
            .collect();
        out.push_str(&match inner.is_empty() {
            true => format!("{head}\n"),
            false => format!("{head} {{\n{inner}}}\n"),
        });
        at += run;
    }
    out
}

struct BaseLine<'a> {
    path: &'a str,
    version: Option<&'a str>,
    digest: Option<&'a str>,
}

fn image_kdl(base: &BaseLine, components: &[(String, Vec<String>)]) -> String {
    let pins: Vec<String> = base
        .version
        .map(|version| format!("version \"{version}\""))
        .into_iter()
        .collect();
    let digest = match base.digest {
        Some(digest) => format!(" digest=\"{digest}\""),
        None => String::new(),
    };
    // The base is written as unsigned because its published signature is not checked yet.
    let base_line = format!("{}{digest} signed=#false", base.path);
    let entries: Vec<(&str, Vec<String>)> = std::iter::once((base_line.as_str(), pins))
        .chain(
            components
                .iter()
                .map(|(path, lines)| (path.as_str(), lines.clone())),
        )
        .collect();
    let members = tree(&entries);
    format!(
        "schema-version {}\n\
         \n\
         {members}",
        crate::model::image::SCHEMA_VERSION
    )
}

fn seeded_home_url(
    list: &crate::model::image::List,
    url: Option<String>,
    index: &crate::provider::Index,
) -> Option<String> {
    match list.base_url.is_empty() {
        true => url.or_else(|| home_url(list, index)),
        false => Some(BASE_URL_REFERENCE.to_string()),
    }
}

fn home_url(list: &crate::model::image::List, index: &crate::provider::Index) -> Option<String> {
    list.images
        .iter()
        .flat_map(|image| &image.entries)
        .filter(|entry| {
            index
                .of(OS_RELEASE)
                .iter()
                .any(|provider| provider.name == entry.path)
        })
        .flat_map(|entry| entry.settings())
        .find(|setting| setting.name == "home-url")
        .and_then(|setting| setting.values.first()?.as_string().map(str::to_string))
}

fn refuse_nesting(root: &Path) -> Result<(), String> {
    let full = match root.is_absolute() {
        true => root.to_path_buf(),
        false => std::env::current_dir().unwrap_or_default().join(root),
    };
    let from = full.ancestors().find(|dir| dir.exists()).unwrap_or(&full);
    match crate::find_root(from) {
        Some(outer) => Err(format!(
            "{} is inside the repository at {}; a repository does not nest",
            root.display(),
            outer.display()
        )),
        None => Ok(()),
    }
}

pub fn git_control(root: &Path) -> Result<(), String> {
    let full = match root.is_absolute() {
        true => root.to_path_buf(),
        false => std::env::current_dir().unwrap_or_default().join(root),
    };
    if let Some(outer) = enclosing_git(&full) {
        return Err(match outer == full {
            true => format!("{} is already a git repository", full.display()),
            false => format!(
                "{} is inside the git repository at {}; a repository does not nest",
                full.display(),
                outer.display()
            ),
        });
    }
    let mut wrote: Vec<(PathBuf, Change)> = Vec::new();
    let ignore = full.join(".gitignore");
    if !ignore.exists() {
        crate::init::put(&ignore, crate::init::GITIGNORE)?;
        wrote.push((PathBuf::from(".gitignore"), Change::Created));
    }
    git_init(&full)?;
    report(&full, &wrote);
    println!("initialised a git repository in {}", full.display());
    Ok(())
}

fn enclosing_git(from: &Path) -> Option<PathBuf> {
    from.ancestors()
        .find(|dir| dir.join(".git").exists())
        .map(Path::to_path_buf)
}

pub(crate) fn git_init(root: &Path) -> Result<(), String> {
    match quietly(Command::new("git").args(["init", "-b", "main"]).arg(root)).status() {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!("`git init` failed: {status}")),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Err(
            "`git` is not installed, and it is what initialises the repository: \
             install it from your platform's git package"
                .to_string(),
        ),
        Err(err) => Err(format!("git: {err}")),
    }
}

fn quietly(command: &mut Command) -> &mut Command {
    command.stdout(Stdio::null()).stderr(Stdio::null())
}

fn gh_installed() -> bool {
    quietly(Command::new("gh").arg("--version"))
        .status()
        .is_ok_and(|status| status.success())
}

fn gh_logged_in() -> bool {
    quietly(Command::new("gh").args(["auth", "status"]))
        .status()
        .is_ok_and(|status| status.success())
}

fn create_remote(owner: &str, id: &str) -> Result<(), String> {
    match Command::new("gh")
        .args(["repo", "create", &format!("{owner}/{id}"), "--public"])
        .status()
    {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!("`gh repo create` failed: {status}")),
        Err(err) => Err(format!("gh: {err}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::declaration::{append, placed, unlisting};

    #[test]
    fn a_component_filling_two_roles_is_offered_once() {
        let root = crate::provider::tests::two_providers("wanted");
        let index =
            crate::provider::Index::local(&root, &[], &crate::parse::disk::Disk::scan(&root));
        let offered: Vec<String> = wanted(&index, "fedora", &["x", "y"])
            .iter()
            .map(|provider| provider.name.clone())
            .collect();
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(offered, ["both"]);
    }

    #[test]
    fn a_second_kept_script_moves_the_calls_of_an_unchanged_one() {
        let root = std::env::temp_dir().join(format!("buildc-kept-{}", std::process::id()));
        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }
        let keep = |names: &[&'static str]| {
            Scripts {
                chosen: names.to_vec(),
            }
            .apply(&root)
            .unwrap()
        };
        keep(&["lint.sh", "smoke.sh"]);
        let smoke = root.join("scripts/smoke.sh");
        let edited = std::fs::read_to_string(&smoke).unwrap() + "# reviewed\n";
        std::fs::write(&smoke, &edited).unwrap();

        keep(&["buildc.sh"]);
        let lint = std::fs::read_to_string(root.join("scripts/lint.sh")).unwrap();
        assert!(lint.contains("\n./scripts/buildc.sh check\n"), "{lint}");
        assert!(!lint.contains("generated/scripts/buildc.sh"), "{lint}");
        assert_eq!(std::fs::read_to_string(&smoke).unwrap(), edited);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn repeated_targets_write_once_and_one_file_names_every_addition() {
        let root = std::env::temp_dir().join(format!("buildc-listing-{}", std::process::id()));
        crate::testing::images(
            &root,
            &[(
                "example",
                "example { base \"example\" { family \"fedora\"; }; }\n",
            )],
        );
        let listing = Listing::collect(
            &root,
            vec!["example".into(), "example".into()],
            &Prompt::silent(),
        )
        .unwrap();
        assert_eq!(listing.images().len(), 1);

        let (list, _) = crate::model::image::List::load(&root);
        let wrote = listing
            .apply_children(&list, &[("one", ""), ("two", "")])
            .unwrap();
        assert_eq!(wrote.len(), 1);
        let Change::Updated(description) = &wrote[0].1 else {
            panic!("an existing image file is updated")
        };
        assert!(
            description.contains("one") && description.contains("two"),
            "{description}"
        );

        let image = crate::testing::image_text(&root, "example");
        for name in ["one", "two"] {
            assert_eq!(crate::parse::image::listed(&image, name), 1, "{image}");
        }
        let _ = std::fs::remove_dir_all(root);
    }

    fn base(
        name: &str,
        image: &str,
        version: Option<&str>,
        bootloaders: &[&str],
    ) -> crate::model::component::BaseComponent {
        crate::model::component::BaseComponent {
            name: name.to_string(),
            owner: Some("ublue".to_string()),
            image: image.to_string(),
            family: "fedora".to_string(),
            provides: vec!["rechunking".to_string(), "bootc".to_string()],
            requires: Vec::new(),
            imports: Vec::new(),
            about: format!("what {name} ships"),
            version: version.map(str::to_string),
            scap_content: String::new(),
            bootloaders: bootloaders.iter().map(|b| b.to_string()).collect(),
        }
    }

    #[test]
    fn a_path_that_two_libraries_hold_names_its_library() {
        let root = std::env::temp_dir().join(format!("buildc-listed-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for path in [
            "one/tool",
            "two/tool",
            "one/only",
            "one/own",
            "components/own",
        ] {
            crate::init::put(&root.join(path).join("component.kdl"), "schema-version 1\n").unwrap();
        }
        let sources: Vec<crate::model::remote::Collection> = ["one", "two"]
            .into_iter()
            .map(|name| crate::model::remote::Collection {
                name: name.to_string(),
                at: crate::model::remote::At::Dir(name.to_string()),
                span: Default::default(),
            })
            .collect();
        let own = Some(layout::ROOT_LIBRARY);
        let written = |default: Option<&str>, name: &str, owner: Option<&str>| {
            let resolver =
                crate::resolve::library::Libraries::read(&root, &sources, own, default, Vec::new());
            listed(&resolver, own, name, owner)
        };
        let lines = [
            written(None, "tool", Some("two")),
            written(None, "only", Some("one")),
            written(None, "own", Some("one")),
            written(None, "own", None),
            written(Some("one"), "own", Some("one")),
            written(Some("one"), "own", None),
            written(own, "own", None),
        ];
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(
            lines,
            [
                "tool library=\"two\"",
                "only",
                "own library=\"one\"",
                "own library=\"local-library\"",
                "own",
                "own library=\"local-library\"",
                "own",
            ]
        );
    }

    #[test]
    fn a_base_that_imports_no_role_provider_offers_one() {
        let root = std::env::temp_dir().join(format!("buildc-roles-{}", std::process::id()));
        for (path, provides) in [
            ("fedora/build-environment", "build-environment"),
            ("identity", "os-release"),
        ] {
            crate::init::put(
                &root.join("components").join(path).join("component.kdl"),
                &format!("schema-version 1\n\nprovides \"{provides}\"\n"),
            )
            .unwrap();
        }
        let index =
            crate::provider::Index::local(&root, &[], &crate::parse::disk::Disk::scan(&root));
        let own = base("base-image/own", "quay.io/own/own:1", None, &[]);
        let mut identity_only = base("base-image/named", "quay.io/named/named:1", None, &[]);
        identity_only.imports = vec!["identity".to_string()];
        let mut library = base("base-image/library", "quay.io/lib/lib:1", None, &[]);
        library.imports = vec!["fedora/build-environment".to_string()];
        let offered = [
            roles(Some(&own), &index),
            roles(Some(&identity_only), &index),
            roles(Some(&library), &index),
            roles(None, &index),
        ];
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(offered[0], [BUILD_ENVIRONMENT, OS_RELEASE]);
        assert_eq!(offered[1], [BUILD_ENVIRONMENT]);
        assert_eq!(offered[2], [OS_RELEASE]);
        assert_eq!(offered[3], [BUILD_ENVIRONMENT, OS_RELEASE]);
    }

    #[test]
    fn a_new_image_without_a_repository_url_uses_the_flag_or_another_image() {
        let root = std::env::temp_dir().join(format!("buildc-home-url-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        crate::testing::images(
            &root,
            &[(
                "first",
                "os-release {\n    name \"First\"\n    home-url \"https://example.org/first\"\n}\n",
            )],
        );
        crate::init::put(
            &root.join("components/os-release/component.kdl"),
            "schema-version 1\n\nprovides \"os-release\"\n\noption \"name\" type=\"string\" {\n    default \"\"\n}\n\noption \"home-url\" type=\"string\" {\n    default \"\"\n}\n",
        )
        .unwrap();
        let (list, _) = crate::model::image::List::load(&root);
        let index =
            crate::provider::Index::local(&root, &[], &crate::parse::disk::Disk::scan(&root));
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(
            seeded_home_url(&list, None, &index).as_deref(),
            Some("https://example.org/first")
        );
        assert_eq!(
            seeded_home_url(&list, Some("https://example.org/named".to_string()), &index)
                .as_deref(),
            Some("https://example.org/named")
        );
    }

    #[test]
    fn a_new_image_takes_the_home_url_from_os_release_alone() {
        let root =
            std::env::temp_dir().join(format!("buildc-home-url-filter-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        crate::testing::images(
            &root,
            &[(
                "first",
                "browser {\n    home-url \"https://browser.example\"\n}\nos-release {\n    home-url \"https://example.org/first\"\n}\n",
            )],
        );
        crate::init::put(
            &root.join("components/browser/component.kdl"),
            "schema-version 1\n\noption \"home-url\" type=\"string\" {\n    default \"\"\n}\n",
        )
        .unwrap();
        crate::init::put(
            &root.join("components/os-release/component.kdl"),
            "schema-version 1\n\nprovides \"os-release\"\n\noption \"home-url\" type=\"string\" {\n    default \"\"\n}\n",
        )
        .unwrap();
        let (list, _) = crate::model::image::List::load(&root);
        let index =
            crate::provider::Index::local(&root, &[], &crate::parse::disk::Disk::scan(&root));
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(
            seeded_home_url(&list, None, &index).as_deref(),
            Some("https://example.org/first")
        );
    }

    #[test]
    fn a_new_image_seeds_the_repository_url() {
        let root = std::env::temp_dir().join(format!("buildc-base-url-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        crate::init::put(
            &root.join("repo.kdl"),
            "schema-version 1\nbase-url \"https://example.org/example\"\n",
        )
        .unwrap();
        let (list, _) = crate::model::image::List::load(&root);
        let index =
            crate::provider::Index::local(&root, &[], &crate::parse::disk::Disk::scan(&root));
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(
            seeded_home_url(&list, Some("https://example.org/named".to_string()), &index)
                .as_deref(),
            Some("{base-url}")
        );
    }

    #[test]
    fn a_new_image_writes_the_repository_url_into_os_release() {
        let root = std::env::temp_dir().join(format!("buildc-seed-url-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        crate::init::put(
            &root.join("repo.kdl"),
            "schema-version 1\nname \"Example\"\nbase-url \"https://example.org/example\"\n",
        )
        .unwrap();
        crate::init::put(
            &root.join("components/base-image/own/component.kdl"),
            "schema-version 1\n\ndescription \"A base\"\n\nbase \"example.invalid/base:1\" {\n    family \"fedora\"\n}\n",
        )
        .unwrap();
        crate::init::put(
            &root.join("components/os-release/component.kdl"),
            "schema-version 1\n\ndescription \"Image name and URLs\"\n\nprovides \"os-release\" exclusive=#true\n\noption \"name\" type=\"string\" {\n    default \"\"\n}\n\noption \"home-url\" type=\"string\" {\n    default \"\"\n}\n\noption \"bug-report-url\" type=\"string\" {\n    default \"\"\n}\n",
        )
        .unwrap();
        let image = Image::collect(
            Home {
                root: &root,
                repo: "example",
                url: Some("https://example.org/named".to_string()),
                libraries_text: "",
            },
            Some("Second".into()),
            Some("base-image/own".into()),
            "a name argument",
            Field::Image,
            None,
            &Prompt::silent(),
        )
        .expect("the image is collected");
        std::fs::remove_dir_all(root).unwrap();
        assert!(
            image.text.contains("home-url \"{base-url}\""),
            "{}",
            image.text
        );
        assert!(
            image.text.contains("bug-report-url \"{base-url}/issues\""),
            "{}",
            image.text
        );
    }

    #[test]
    fn an_image_names_its_base_component_and_sets_its_version() {
        let known = image_kdl(
            &BaseLine {
                path: "base-image/bazzite",
                version: Some("stable"),
                digest: None,
            },
            &[],
        );
        assert!(
            known.contains(
                "\nbase-image {\n    bazzite signed=#false {\n        version \"stable\"\n    }\n}\n"
            ),
            "{known}"
        );

        let os_release = crate::provider::Provider {
            owner: Some("buildc".to_string()),
            name: "os-release".to_string(),
            here: true,
            declares: crate::parse::component::Summary {
                provides: vec![OS_RELEASE.to_string()],
                options: vec!["name".to_string(), "home-url".to_string()],
                ..Default::default()
            },
        };
        let shared = image_kdl(
            &BaseLine {
                path: "base-image/own",
                version: None,
                digest: None,
            },
            &seeded(
                &[&os_release],
                |provider| provider.name.clone(),
                "Server",
                Some("https://github.com/someone/example"),
            ),
        );
        assert!(
            shared.contains(
                "\nos-release {\n    name \"Server\"\n    home-url \"https://github.com/someone/example\"\n}\n"
            ),
            "{shared}"
        );
        assert!(!shared.contains("bug-report-url"), "{shared}");
        assert!(!shared.contains("    version"), "{shared}");

        assert!(manifest_sections("", &[], &[]).unwrap().is_empty());

        let one = base("base-image/one", "example.invalid/one:1", None, &["grub2"]);
        let described = base(
            "base-image/two",
            "example.invalid/two:1",
            None,
            &["grub2", "systemd"],
        );
        assert_eq!(
            choose_bootloader(Some(&one), None, &Prompt::scripted(Vec::new())).unwrap(),
            Some("grub2".to_string())
        );
        assert_eq!(
            choose_bootloader(Some(&described), None, &Prompt::silent()).unwrap(),
            Some("grub2".to_string())
        );
        assert_eq!(
            choose_bootloader(Some(&described), None, &Prompt::scripted(vec!["2".into()])).unwrap(),
            Some("systemd".to_string())
        );
        assert_eq!(
            choose_bootloader(Some(&described), Some("systemd"), &Prompt::silent()).unwrap(),
            Some("systemd".to_string()),
            "asking again opens on the answer held"
        );
        assert_eq!(
            choose_bootloader(None, None, &Prompt::silent()).unwrap(),
            None
        );
    }

    #[test]
    fn a_base_is_canonicalised_to_its_component() {
        let bases = [base(
            "base-image/bazzite",
            "ghcr.io/ublue-os/bazzite:stable",
            None,
            &["grub2"],
        )];
        for typed in ["base-image/bazzite", "ghcr.io/ublue-os/bazzite:stable"] {
            assert_eq!(catalogued(&bases, typed.to_string()), "base-image/bazzite");
        }
        assert_eq!(
            catalogued(&bases, "ghcr.io/other/unknown:1".to_string()),
            "ghcr.io/other/unknown:1"
        );
    }

    #[test]
    fn the_base_picker_opens_on_the_base_held() {
        let bases = [
            base("base-image/one", "example.invalid/one:1", None, &["grub2"]),
            base("base-image/two", "example.invalid/two:1", None, &["grub2"]),
        ];
        let chosen = choose_base(
            &bases,
            Some("base-image/two"),
            &Prompt::scripted(vec![String::new()]),
        )
        .unwrap();
        assert_eq!(chosen, "base-image/two");
    }

    #[test]
    fn an_unknown_reference_gets_a_base_component() {
        let root = std::env::temp_dir().join(format!("buildc-own-base-{}", std::process::id()));
        let own = own_base(&root, "ghcr.io/other/Unknown_Base:1.2@sha256:abc")
            .expect("nothing is in the way");
        assert_eq!(own.path, "base-image/unknown-base");
        assert_eq!(own.version.as_deref(), Some("1.2"));
        assert_eq!(own.digest.as_deref(), Some("sha256:abc"));
        assert_eq!(
            own.manifest,
            "schema-version 1\n\nbase \"ghcr.io/other/Unknown_Base:{version}\"\n\noption \"version\" type=\"string\" {\n    default \"1.2\"\n}\n"
        );
        let untagged = own_base(&root, "localhost:5000/own").expect("nothing is in the way");
        assert_eq!(untagged.version, None);
        assert_eq!(
            untagged.manifest,
            "schema-version 1\n\nbase \"localhost:5000/own\"\n"
        );
    }

    #[test]
    fn an_empty_base_is_refused_before_a_file_is_written() {
        let root = std::env::temp_dir().join(format!("buildc-empty-base-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let said = match Image::collect(
            Home {
                root: &root,
                repo: "example",
                url: None,
                libraries_text: "",
            },
            Some("Example".into()),
            Some("  ".into()),
            "`--image`",
            Field::Base,
            None,
            &Prompt::silent(),
        ) {
            Ok(_) => panic!("an empty base was accepted"),
            Err(said) => said,
        };
        assert!(said.message().contains("`--base` is empty"), "{said}");
        assert!(!root.join("example.kdl").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_name_that_builds_declares_elsewhere_is_refused_before_a_file_is_written() {
        let root = std::env::temp_dir().join(format!("buildc-held-build-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let repo = "schema-version 1\nbuilds {\n    example \"./images/example\"\n}\n";
        std::fs::write(root.join(layout::REPO_FILE), repo).unwrap();
        let said = match Image::collect(
            Home {
                root: &root,
                repo: "example",
                url: None,
                libraries_text: "",
            },
            Some("Example".into()),
            Some("quay.io/fedora/fedora-bootc:44".into()),
            "`--image`",
            Field::Base,
            None,
            &Prompt::silent(),
        ) {
            Ok(_) => panic!("a second build of one name was accepted"),
            Err(said) => said,
        };
        assert!(
            said.message().contains("`example` is already a build"),
            "{said}"
        );
        assert!(!root.join("example.kdl").exists());
        assert_eq!(
            std::fs::read_to_string(root.join(layout::REPO_FILE)).unwrap(),
            repo
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn every_library_name_is_a_name() {
        use crate::model::image::is_name;
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/collections");
        let sources: Vec<crate::model::remote::Collection> = ["upstream", "ublue"]
            .into_iter()
            .map(|name| crate::model::remote::Collection {
                name: name.to_string(),
                at: crate::model::remote::At::Dir(dir.join(name).display().to_string()),
                span: crate::diag::Span::default(),
            })
            .collect();
        let bases = crate::bases::catalog(Path::new("/nonexistent"), &sources);
        assert_eq!(bases.len(), 3);
        for base in bases {
            assert!(is_name(&base.family), "{}", base.image);
            for name in &base.provides {
                assert!(is_name(name), "{} provides {name}", base.image);
            }
        }
    }

    #[test]
    fn a_missing_parent_block_is_written_around_the_declaration() {
        let root = std::env::temp_dir().join(format!("buildc-nested-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("example.kdl");
        std::fs::write(
            &file,
            image_kdl(
                &BaseLine {
                    path: "base-image/fedora",
                    version: None,
                    digest: None,
                },
                &[],
            ),
        )
        .unwrap();

        let (chain, line) = placed(true, "tools/dev-tools", "");
        append(&file, &chain, &line).unwrap();
        let written = std::fs::read_to_string(&file).unwrap();
        assert!(
            written.ends_with("}\n\ntools {\n    dev-tools\n}\n"),
            "{written}"
        );

        let (chain, line) = placed(true, "tools/editor", " { theme \"dark\"; }");
        append(&file, &chain, &line).unwrap();
        let written = std::fs::read_to_string(&file).unwrap();
        assert_eq!(written.matches("tools {").count(), 1);
        assert!(
            written.contains("dev-tools\n    editor { theme \"dark\"; }\n"),
            "{written}"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn unlisting_removes_only_the_named_entry() {
        let root = std::env::temp_dir().join(format!("buildc-unlist-{}", std::process::id()));
        let file = root.join("components/one.kdl");
        crate::init::put(
            &file,
            "component \"/\" {\n    kept; trailing\n    apps {\n        gone\n    }\n}\n",
        )
        .unwrap();

        for name in ["apps/gone", "trailing"] {
            let text = std::fs::read_to_string(&file).unwrap();
            let text = unlisting(&root, &file, &text, name, false).unwrap();
            crate::init::put(&file, &text).unwrap();
        }
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "component \"/\" {\n    kept;\n}\n"
        );
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(unlisting(&root, &file, &text, "absent", false).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    fn unlisted(case: &str, text: &str, name: &str) -> Result<String, String> {
        let root =
            std::env::temp_dir().join(format!("buildc-unlist-{case}-{}", std::process::id()));
        let file = root.join("components/one.kdl");
        let text = format!("component \"/\" {{\n{text}}}\n");
        crate::init::put(&file, &text).unwrap();
        crate::init::put(&root.join("components/hardware/nvidia/component.kdl"), "").unwrap();
        let out = unlisting(&root, &file, &text, name, false);
        std::fs::remove_dir_all(root).unwrap();
        out.map(|out| {
            out.strip_prefix("component \"/\" {\n")
                .and_then(|out| out.strip_suffix("}\n"))
                .map_or(out.clone(), str::to_string)
        })
    }

    #[test]
    fn unlisting_keeps_a_parent_that_holds_another_segment() {
        let image = "    apps {\n        gone\n        kept\n    }\n";
        assert_eq!(
            unlisted("kept", image, "apps/gone").unwrap(),
            "    apps {\n        kept\n    }\n"
        );
    }

    #[test]
    fn unlisting_removes_every_parent_it_empties() {
        let image = "    a {\n        b {\n            c\n        }\n    }\n    kept\n";
        assert_eq!(unlisted("deep", image, "a/b/c").unwrap(), "    kept\n");
    }

    #[test]
    fn unlisting_finds_a_path_through_a_repeated_parent() {
        let image = "    hardware { audio-a }\n    hardware { audio-b }\n";
        assert_eq!(
            unlisted("repeated", image, "hardware/audio-b").unwrap(),
            "    hardware { audio-a }\n"
        );
    }

    #[test]
    fn unlisting_keeps_a_parent_that_is_a_component() {
        let image = "    hardware {\n        nvidia {\n            cuda\n        }\n    }\n";
        assert_eq!(
            unlisted("component", image, "hardware/nvidia/cuda").unwrap(),
            "    hardware {\n        nvidia\n    }\n"
        );
    }

    #[test]
    fn unlisting_keeps_the_properties_of_a_component_parent() {
        let image = "    hardware {\n        nvidia variant=\"lean\" {\n            cuda\n        }\n    }\n";
        assert_eq!(
            unlisted("variant", image, "hardware/nvidia/cuda").unwrap(),
            "    hardware {\n        nvidia variant=\"lean\"\n    }\n"
        );
    }

    #[test]
    fn unlisting_an_inline_definition_removes_it() {
        let image = "    tools {\n        fragment position=\"after\"\n    }\n    kept\n";
        assert_eq!(unlisted("inline", image, "tools").unwrap(), "    kept\n");
    }

    #[test]
    fn unlisting_a_segment_that_holds_segments_is_refused() {
        let image = "    hardware {\n        nvidia {\n            cuda\n        }\n    }\n";
        let refused = unlisted("holds", image, "hardware/nvidia").unwrap_err();
        assert!(refused.contains("with cuda below it"), "{refused}");
    }

    #[test]
    fn a_cancelled_listing_does_not_create_a_component() {
        let root =
            std::env::temp_dir().join(format!("buildc-cancel-component-{}", std::process::id()));
        let file = root.join("components/hello/component.kdl");
        let component = Component {
            path: "hello".into(),
            written: Written::Directory {
                file: file.clone(),
                text: "description \"hello\"\n".into(),
                listing: Listing::Cancelled,
            },
        };
        assert!(component.apply(&root).unwrap().is_empty());
        assert!(!file.exists());
        let _ = std::fs::remove_dir_all(root);
    }

    fn inline_repo(case: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("buildc-inline-{case}-{}", std::process::id()));
        crate::init::put(
            &root.join("repo.kdl"),
            "schema-version 1\nbuilds {\n    one\n    two\n}\n",
        )
        .unwrap();
        for image in ["one", "two"] {
            crate::init::put(
                &root.join(format!("components/{image}.kdl")),
                "schema-version 1\n\ncomponent \"/\" {\n    kept\n}\n",
            )
            .unwrap();
        }
        root
    }

    fn inline(
        root: &Path,
        name: &str,
        with: &[(&str, &str)],
        images: &[&str],
        prompt: &Prompt,
    ) -> Result<(), Error> {
        Component::collect(
            root,
            Some(name.into()),
            with.iter()
                .map(|(verb, value)| ((*verb).to_string(), (*value).to_string()))
                .collect(),
            images.iter().map(|image| (*image).to_string()).collect(),
            None,
            true,
            prompt,
        )?
        .apply(root)?;
        Ok(())
    }

    #[test]
    fn the_inline_flag_lands_bare_in_the_chosen_build() {
        let root = inline_repo("asked");
        // The script answers the components picker with row one, then the description question and the families question. Row zero declines the picker.
        let prompt = Prompt::scripted(["1", "Apps", ""].map(str::to_string).to_vec());
        let wrote = Component::collect(
            &root,
            Some("apps".into()),
            Vec::new(),
            Vec::new(),
            None,
            true,
            &prompt,
        )
        .unwrap()
        .apply(&root)
        .unwrap();
        let image = std::fs::read_to_string(root.join("components/one.kdl")).unwrap();
        let directory = root.join("components/apps").exists();
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(wrote.len(), 1);
        assert_eq!(
            image,
            "schema-version 1\n\ncomponent \"/\" {\n    kept\n}\napps {\n    description \"Apps\"\n}\n"
        );
        assert!(!directory);
    }

    #[test]
    fn an_inline_component_that_meets_a_directory_component_is_refused_naming_both() {
        let root = inline_repo("clash");
        crate::init::put(
            &root.join("components/one/apps/component.kdl"),
            "schema-version 1\n",
        )
        .unwrap();
        let refused = inline(
            &root,
            "apps",
            &[("description", "Apps")],
            &["one"],
            &Prompt::silent(),
        );
        std::fs::remove_dir_all(&root).unwrap();
        let refused = refused.unwrap_err();
        assert!(refused.message().contains("inline `one/apps`"), "{refused}");
        assert!(
            refused.message().contains("components/one/apps"),
            "{refused}"
        );
    }

    #[test]
    fn an_inline_component_listed_in_two_builds_is_refused() {
        let root = inline_repo("two-images");
        let refused = inline(
            &root,
            "apps",
            &[("description", "Apps")],
            &["one", "two"],
            &Prompt::silent(),
        );
        let image =
            crate::testing::image_text(&root, "one") + &crate::testing::image_text(&root, "two");
        std::fs::remove_dir_all(&root).unwrap();
        assert!(refused.unwrap_err().message().contains("one `--build`"));
        assert!(!image.contains("apps"), "{image}");
    }

    #[test]
    fn an_inline_path_holding_a_manifest_node_name_is_refused() {
        let root = inline_repo("manifest-segment");
        let refused = inline(
            &root,
            "tools/packages",
            &[("description", "Tools")],
            &["one"],
            &Prompt::silent(),
        );
        let image = crate::testing::image_text(&root, "one");
        std::fs::remove_dir_all(&root).unwrap();
        assert!(refused
            .unwrap_err()
            .message()
            .contains("the segment `packages`"));
        assert!(!image.contains("tools"), "{image}");
    }

    #[test]
    fn a_second_build_writes_the_first_as_the_default() {
        let root =
            std::env::temp_dir().join(format!("buildc-create-implicit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        crate::init::put(&root.join(layout::REPO_FILE), "schema-version 1\n").unwrap();
        crate::testing::images(&root, &[("alpha", "")]);
        let list = crate::model::image::List::editable(&root)
            .map_err(|issues| issues.plain())
            .unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(implicit_default(&list, "beta").as_deref(), Some("alpha"));
    }

    #[test]
    fn a_new_component_lands_in_the_local_default_library() {
        let root = std::env::temp_dir().join(format!("buildc-create-shelf-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        crate::init::put(
            &root.join("repo.kdl"),
            "schema-version 1\n\nlibraries default=\"shelf\" {\n    local {\n        local-library { source \".\" }\n        shelf { source \"shelf\" }\n    }\n}\n",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("shelf")).unwrap();
        Component::collect(
            &root,
            Some("tools".into()),
            vec![("description".into(), "Tools".into())],
            Vec::new(),
            None,
            false,
            &Prompt::silent(),
        )
        .unwrap()
        .apply(&root)
        .unwrap();
        let written = root.join("shelf/tools/component.kdl").is_file();
        let stray = root.join("components/tools").exists();
        let list = crate::model::image::List::editable(&root)
            .map_err(|issues| issues.plain())
            .unwrap();
        let listed = crate::parse::local::all(&root, &list)
            .iter()
            .any(|listed| listed.path == "tools");
        std::fs::remove_dir_all(&root).unwrap();
        assert!(written);
        assert!(!stray);
        assert!(listed, "the other commands find the new component");
    }

    #[test]
    fn a_remote_default_library_is_refused_before_any_question() {
        let root =
            std::env::temp_dir().join(format!("buildc-create-remote-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        crate::init::put(
            &root.join("repo.kdl"),
            "schema-version 1\n\nlibraries default=\"upstream\" {\n    remote {\n        upstream { source \"https://example.invalid/library\" }\n    }\n}\n",
        )
        .unwrap();
        let refused = Component::collect(
            &root,
            Some("tools".into()),
            Vec::new(),
            Vec::new(),
            None,
            false,
            &Prompt::scripted(Vec::new()),
        );
        std::fs::remove_dir_all(&root).unwrap();
        assert!(refused
            .err()
            .is_some_and(|err| err.message().contains("names a remote library")));
    }

    #[test]
    fn an_inline_component_goes_into_the_default_library_copy_of_a_path_both_folders_hold() {
        let root = std::env::temp_dir().join(format!("buildc-create-both-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        crate::init::put(
            &root.join("repo.kdl"),
            "schema-version 1\n\nlibraries default=\"shelf\" {\n    local {\n        local-library { source \".\" }\n        shelf { source \"shelf\" }\n    }\n}\n",
        )
        .unwrap();
        crate::init::put(
            &root.join("shelf/tools/component.kdl"),
            "schema-version 1\n",
        )
        .unwrap();
        crate::init::put(
            &root.join("components/tools/component.kdl"),
            "schema-version 1\n",
        )
        .unwrap();
        Component::collect(
            &root,
            Some("editors".into()),
            vec![("description".into(), "Editors".into())],
            Vec::new(),
            Some("tools".into()),
            true,
            &Prompt::silent(),
        )
        .unwrap()
        .apply(&root)
        .unwrap();
        let shelf = std::fs::read_to_string(root.join("shelf/tools/component.kdl")).unwrap();
        let own = std::fs::read_to_string(root.join("components/tools/component.kdl")).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert!(shelf.contains("editors {"), "{shelf}");
        assert_eq!(own, "schema-version 1\n");
    }

    #[test]
    fn a_child_below_a_build_component_stays_in_the_root_library() {
        let root =
            std::env::temp_dir().join(format!("buildc-create-scoped-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        crate::init::put(
            &root.join("repo.kdl"),
            "schema-version 1\n\nlibraries default=\"shelf\" {\n    local {\n        local-library { source \".\" }\n        shelf { source \"shelf\" }\n    }\n}\n",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("shelf")).unwrap();
        crate::testing::images(&root, &[("one", "")]);
        Component::collect(
            &root,
            Some("tools".into()),
            vec![("description".into(), "Tools".into())],
            vec!["one".into()],
            None,
            false,
            &Prompt::silent(),
        )
        .unwrap()
        .apply(&root)
        .unwrap();
        let written = root.join("components/one/tools/component.kdl").is_file();
        let stray = root.join("shelf/tools").exists() || root.join("shelf/one").exists();
        std::fs::remove_dir_all(&root).unwrap();
        assert!(written);
        assert!(!stray);
    }

    #[test]
    fn an_inline_path_inside_a_component_holding_a_manifest_node_name_is_refused() {
        let root = inline_repo("in-manifest-segment");
        crate::init::put(
            &root.join("components/desktop/component.kdl"),
            "schema-version 1\n",
        )
        .unwrap();
        let refused = Component::collect(
            &root,
            Some("tools/packages".into()),
            vec![("description".into(), "Tools".into())],
            Vec::new(),
            Some("desktop".into()),
            true,
            &Prompt::silent(),
        );
        let manifest =
            std::fs::read_to_string(root.join("components/desktop/component.kdl")).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert!(refused
            .err()
            .expect("the manifest segment is refused")
            .message()
            .contains("the segment `packages`"));
        assert_eq!(manifest, "schema-version 1\n");
    }

    #[test]
    fn an_inline_component_named_after_a_manifest_node_is_refused() {
        let root = inline_repo("keyword-name");
        let refused = inline(
            &root,
            "conforms",
            &[("description", "Conforms")],
            &["one"],
            &Prompt::silent(),
        );
        let image = crate::testing::image_text(&root, "one");
        std::fs::remove_dir_all(&root).unwrap();
        assert!(refused
            .unwrap_err()
            .message()
            .contains("`conforms` is a manifest node"));
        assert!(!image.contains("conforms"), "{image}");
    }

    #[test]
    fn the_in_flag_writes_inside_the_named_component() {
        let root = inline_repo("in-directory");
        crate::init::put(
            &root.join("components/desktop/component.kdl"),
            "schema-version 1\n",
        )
        .unwrap();
        Component::collect(
            &root,
            Some("tools".into()),
            vec![("description".into(), "Tools".into())],
            Vec::new(),
            Some("desktop".into()),
            true,
            &Prompt::silent(),
        )
        .unwrap()
        .apply(&root)
        .unwrap();
        let text = std::fs::read_to_string(root.join("components/desktop/component.kdl")).unwrap();
        let directory = root.join("components/desktop/tools").exists();
        std::fs::remove_dir_all(&root).unwrap();
        assert!(
            text.ends_with("tools {\n    description \"Tools\"\n}\n"),
            "{text}"
        );
        assert!(!directory);
    }

    #[test]
    fn the_in_flag_writes_inside_an_inline_definition() {
        let root = inline_repo("in-inline");
        crate::init::put(
            &root.join("components/desktop/component.kdl"),
            "schema-version 1\n\napps {\n    description \"Apps\"\n}\n",
        )
        .unwrap();
        Component::collect(
            &root,
            Some("tools".into()),
            vec![("description".into(), "Tools".into())],
            Vec::new(),
            Some("desktop/apps".into()),
            true,
            &Prompt::silent(),
        )
        .unwrap()
        .apply(&root)
        .unwrap();
        let text = std::fs::read_to_string(root.join("components/desktop/component.kdl")).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert!(
            text.ends_with(
                "apps {\n    description \"Apps\"\n    tools {\n        description \"Tools\"\n    }\n}\n"
            ),
            "{text}"
        );
    }

    #[test]
    fn the_in_flag_keeps_a_top_level_namesake_of_the_new_path() {
        let root =
            std::env::temp_dir().join(format!("buildc-inline-namesake-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        crate::init::put(&root.join("repo.kdl"), "schema-version 1\n").unwrap();
        for component in ["desktop", "tools"] {
            crate::init::put(
                &root.join(format!("components/{component}/component.kdl")),
                "schema-version 1\n",
            )
            .unwrap();
        }
        Component::collect(
            &root,
            Some("tools".into()),
            vec![("description".into(), "Tools".into())],
            Vec::new(),
            Some("desktop".into()),
            true,
            &Prompt::silent(),
        )
        .unwrap()
        .apply(&root)
        .unwrap();
        let desktop =
            std::fs::read_to_string(root.join("components/desktop/component.kdl")).unwrap();
        let top = std::fs::read_to_string(root.join("components/tools/component.kdl")).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert!(
            desktop.ends_with("tools {\n    description \"Tools\"\n}\n"),
            "{desktop}"
        );
        assert_eq!(top, "schema-version 1\n");
    }

    #[test]
    fn the_in_flag_refuses_a_component_this_repository_does_not_hold() {
        let root = inline_repo("in-missing");
        let refused = Component::collect(
            &root,
            Some("apps".into()),
            Vec::new(),
            Vec::new(),
            Some("elsewhere/desktop".into()),
            true,
            &Prompt::silent(),
        );
        std::fs::remove_dir_all(&root).unwrap();
        let refused = refused.err().expect("the missing holder is refused");
        assert!(
            refused
                .message()
                .contains("is not a component this repository holds"),
            "{refused}"
        );
    }

    #[test]
    fn the_in_flag_and_a_build_are_refused_together() {
        let root = inline_repo("in-and-build");
        let refused = Component::collect(
            &root,
            Some("apps".into()),
            Vec::new(),
            vec!["one".into()],
            Some("two".into()),
            true,
            &Prompt::silent(),
        );
        std::fs::remove_dir_all(&root).unwrap();
        let refused = refused.err().expect("two holders are refused");
        assert!(refused.message().contains("give one"), "{refused}");
    }

    #[test]
    fn the_in_flag_needs_the_inline_flag() {
        let root = inline_repo("in-without-inline");
        let refused = Component::collect(
            &root,
            Some("apps".into()),
            Vec::new(),
            Vec::new(),
            Some("one".into()),
            false,
            &Prompt::silent(),
        );
        std::fs::remove_dir_all(&root).unwrap();
        let refused = refused.err().expect("the missing inline flag is refused");
        assert!(refused.message().contains("takes `--inline`"), "{refused}");
    }

    #[test]
    fn the_in_flag_refuses_a_path_that_is_already_defined() {
        let root = inline_repo("in-taken");
        crate::init::put(
            &root.join("components/desktop/component.kdl"),
            "schema-version 1\n\napps {\n    description \"Apps\"\n}\n",
        )
        .unwrap();
        let refused = Component::collect(
            &root,
            Some("apps".into()),
            Vec::new(),
            Vec::new(),
            Some("desktop".into()),
            true,
            &Prompt::silent(),
        );
        std::fs::remove_dir_all(&root).unwrap();
        let refused = refused.err().expect("the taken path is refused");
        assert!(
            refused.message().contains("would define it a second time"),
            "{refused}"
        );
    }

    #[test]
    fn the_inline_picker_offers_the_builds_components_first() {
        let root = inline_repo("in-picker");
        let prompt = Prompt::scripted(["1", "Apps", ""].map(str::to_string).to_vec());
        Component::collect(
            &root,
            Some("apps".into()),
            Vec::new(),
            Vec::new(),
            None,
            true,
            &prompt,
        )
        .unwrap()
        .apply(&root)
        .unwrap();
        let image = crate::testing::image_text(&root, "one");
        std::fs::remove_dir_all(&root).unwrap();
        assert!(
            image
                .ends_with("component \"/\" {\n    kept\n}\napps {\n    description \"Apps\"\n}\n"),
            "{image}"
        );
    }

    #[test]
    fn a_directory_component_lands_where_the_build_reads_its_bare_child() {
        let root = inline_repo("scoped");
        Component::collect(
            &root,
            Some("tools".into()),
            Vec::new(),
            vec!["one".into()],
            None,
            false,
            &Prompt::silent(),
        )
        .unwrap()
        .apply(&root)
        .unwrap();
        let written = root.join("components/one/tools/component.kdl").exists();
        let image = crate::testing::image_text(&root, "one");
        std::fs::remove_dir_all(&root).unwrap();
        assert!(written);
        assert_eq!(
            image,
            "schema-version 1\n\ncomponent \"/\" {\n    kept\n}\ntools\n"
        );
    }

    #[test]
    fn a_library_root_namesake_does_not_refuse_a_component_for_an_in_library_build() {
        let root = inline_repo("namesake");
        crate::init::put(
            &root.join("components/tools/component.kdl"),
            "schema-version 1\n",
        )
        .unwrap();
        // The script answers the description, the families and the builds in that order.
        let prompt = Prompt::scripted(["", "", "1"].map(str::to_string).to_vec());
        Component::collect(
            &root,
            Some("tools".into()),
            Vec::new(),
            Vec::new(),
            None,
            false,
            &prompt,
        )
        .unwrap()
        .apply(&root)
        .unwrap();
        let written = root.join("components/one/tools/component.kdl").exists();
        std::fs::remove_dir_all(&root).unwrap();
        assert!(written);
    }

    #[test]
    fn choosing_no_build_declines_the_listing() {
        let root = inline_repo("none-chosen");
        let list = crate::model::image::List::editable(&root)
            .unwrap_or_else(|issues| panic!("{}", issues.plain()));
        let listing = Listing::pick_from(&list, &Prompt::scripted(vec!["0".to_string()]));
        std::fs::remove_dir_all(&root).unwrap();
        assert!(matches!(listing, Ok(Listing::Declined { asked: true })));
    }

    #[test]
    fn builds_that_read_a_bare_child_from_two_folders_are_refused() {
        let root = inline_repo("two-folders");
        crate::init::put(
            &root.join("repo.kdl"),
            "schema-version 1\nbuilds {\n    one\n    two \"./two\"\n}\n",
        )
        .unwrap();
        crate::init::put(&root.join("two.kdl"), "schema-version 1\n").unwrap();
        let refused = Component::collect(
            &root,
            Some("tools".into()),
            Vec::new(),
            vec!["one".into(), "two".into()],
            None,
            false,
            &Prompt::silent(),
        );
        let written =
            root.join("components/tools").exists() || root.join("components/one/tools").exists();
        std::fs::remove_dir_all(&root).unwrap();
        let refused = refused.err().expect("two folders are refused");
        assert!(
            refused.message().contains("`one/tools` in `one`")
                && refused.message().contains("`tools` in `two`"),
            "{refused}"
        );
        assert!(!written);
    }

    #[test]
    fn a_directory_component_whose_top_segment_is_a_manifest_node_is_refused() {
        let root = inline_repo("vocabulary");
        let refused = Component::collect(
            &root,
            Some("permissions/tools".into()),
            Vec::new(),
            Vec::new(),
            None,
            false,
            &Prompt::silent(),
        );
        let directory = root.join("components/permissions").exists();
        std::fs::remove_dir_all(&root).unwrap();
        let refused = refused.err().expect("the manifest node name is refused");
        assert!(
            refused
                .message()
                .contains("`permissions` is a manifest node"),
            "{refused}"
        );
        assert!(!directory);
    }

    #[test]
    fn a_directory_component_that_a_build_defines_inline_is_refused_naming_both() {
        let root = inline_repo("defined-inline");
        inline(
            &root,
            "apps",
            &[("description", "Apps")],
            &["one"],
            &Prompt::silent(),
        )
        .unwrap();
        let refused = Component::collect(
            &root,
            Some("one/apps".into()),
            Vec::new(),
            Vec::new(),
            None,
            false,
            &Prompt::silent(),
        );
        let directory = root.join("components/one/apps").exists();
        std::fs::remove_dir_all(&root).unwrap();
        let refused = refused.err().expect("the directory component is refused");
        assert!(
            refused
                .message()
                .contains("one.kdl defines `one/apps` inline"),
            "{refused}"
        );
        assert!(
            refused.message().contains("components/one/apps"),
            "{refused}"
        );
        assert!(!directory);
    }

    #[test]
    fn a_directory_component_beside_another_inline_definition_is_written() {
        let root = inline_repo("beside-inline");
        inline(
            &root,
            "tools",
            &[("description", "Tools")],
            &["one"],
            &Prompt::silent(),
        )
        .unwrap();
        Component::collect(
            &root,
            Some("apps".into()),
            Vec::new(),
            Vec::new(),
            None,
            false,
            &Prompt::silent(),
        )
        .unwrap()
        .apply(&root)
        .unwrap();
        let written = root.join("components/apps/component.kdl").exists();
        std::fs::remove_dir_all(&root).unwrap();
        assert!(written);
    }

    #[test]
    fn declining_the_holder_of_an_inline_component_is_refused() {
        let root = inline_repo("declined");
        let refused = Component::collect(
            &root,
            Some("apps".into()),
            Vec::new(),
            Vec::new(),
            None,
            true,
            &Prompt::scripted(["0"].map(str::to_string).to_vec()),
        );
        std::fs::remove_dir_all(&root).unwrap();
        let refused = refused.err().expect("the declined holder is refused");
        assert!(
            refused.message().contains("lives inside another component"),
            "{refused}"
        );
    }

    #[test]
    fn a_packages_line_through_with_is_refused() {
        let root = inline_repo("with-packages");
        let refused = Component::collect(
            &root,
            Some("apps".into()),
            vec![("packages".into(), "firefox".into())],
            vec!["one".into()],
            None,
            false,
            &Prompt::silent(),
        );
        std::fs::remove_dir_all(&root).unwrap();
        let refused = refused.err().expect("the packages line is refused");
        assert!(
            refused.message().contains("buildc add package"),
            "{refused}"
        );
    }

    #[test]
    fn an_inline_component_with_no_manifest_line_is_refused() {
        let root = inline_repo("empty");
        let refused = inline(&root, "apps", &[], &["one"], &Prompt::silent());
        std::fs::remove_dir_all(&root).unwrap();
        assert!(refused
            .unwrap_err()
            .message()
            .contains("needs a manifest line"));
    }

    #[test]
    fn create_repo_and_image_screens_snapshot_production_copy() {
        use crate::screen_tests::{assert_screen, screen};

        assert_screen(
            "create-repo-name",
            screen().line(copy::REPO_NAME, "", "Example", Some("example")),
        );
        assert_screen(
            "create-repo-owner",
            screen().line(&copy::username(forge::HOST), "github.com/", "someone", None),
        );
        let yes_no = yes_no_options();
        assert_screen(
            "create-repo-provider",
            screen().picker(copy::SCHEDULED, &yes_no, None, "enter confirms", 0),
        );
        assert_screen(
            "create-repo-host",
            screen().picker(copy::REPO_HOST, &host_options(), None, "enter confirms", 0),
        );
        assert_screen(
            "create-repo-image",
            screen().picker(copy::IMAGES, &yes_no, None, "enter confirms", 1),
        );
        assert_screen(
            "create-repo-remote",
            screen().picker(copy::CREATE_REMOTE, &yes_no, None, "enter confirms", 1),
        );
        let scripts: Vec<Choice> = offered()
            .into_iter()
            .map(|(name, about, _)| Choice::new(name, about))
            .collect();
        assert_screen(
            "create-repo-scripts",
            screen().picker(copy::SCRIPTS, &scripts, Some(&[0]), "space toggles", 0),
        );

        assert_screen(
            "create-image-name",
            screen().line(copy::IMAGE_NAME, "", "Workstation", Some("Example")),
        );
        let libraries: Vec<Choice> =
            Libraries::every(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/assets")))
                .iter()
                .map(|library| Choice::new(library.name.clone(), library.about.clone()))
                .collect();
        assert_screen(
            "create-repo-libraries",
            screen().picker(
                copy::LIBRARIES,
                &libraries,
                Some(&[0, 1, 2]),
                "space toggles",
                0,
            ),
        );
        let bases = vec![
            base(
                "base-image/fedora-bootc",
                "quay.io/fedora/fedora-bootc:44",
                Some("44"),
                &["grub2"],
            ),
            base(
                "base-image/ubuntu",
                "docker.io/library/ubuntu:26.04",
                Some("26.04"),
                &["grub2", "systemd"],
            ),
        ];
        let mut options = base_options(&bases);
        options.push(Choice::new(copy::OTHER_BASE, copy::OTHER_BASE_ABOUT));
        assert_screen(
            "create-image-base",
            screen().picker(copy::IMAGE_BASE, &options, None, "enter confirms", 0),
        );
        assert_screen(
            "create-image-add-components",
            screen().picker(
                copy::BRING_FOR_BASE,
                &yes_no_options(),
                None,
                "enter confirms",
                0,
            ),
        );
        let dual = bases
            .iter()
            .find(|base| base.bootloaders.len() == 2)
            .expect("the production catalog carries a two-bootloader base");
        assert_screen(
            "create-image-bootloader",
            screen().picker(
                copy::IMAGE_BOOTLOADER,
                &bootloader_options(&dual.bootloaders),
                None,
                "enter confirms",
                0,
            ),
        );
    }

    #[test]
    fn repository_review_screen_snapshots_collected_rows() {
        use crate::screen_tests::{assert_screen, assert_style_at, screen};

        let repo = Repo {
            name: NonEmpty("Example".to_string()),
            id: NonEmpty("example".to_string()),
            root: PathBuf::from("example"),
            host: forge::HOST.to_string(),
            owner: Some("someone".to_string()),
            assets: PathBuf::from("assets"),
            setup: Setup::Defaults,
            libraries: Libraries {
                chosen: Libraries::every(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/assets"))),
            },
            image: None,
            workflows: None,
            scripts: Scripts {
                chosen: vec![SKELETON_FILE],
            },
            remote: false,
            install_gh: false,
        };
        let mut rows: Vec<Choice> = repo
            .rows()
            .into_iter()
            .map(|(_, label, value)| Choice::new(format!("{label}  {value}"), ""))
            .collect();
        rows.push(Choice::new(copy::CREATE, ""));
        let rendered = assert_screen(
            "create-repo-review",
            screen().picker(copy::REVIEW, &rows, None, copy::REVIEW_KEYS, rows.len() - 1),
        );
        assert_style_at(
            &rendered,
            0,
            9,
            "fg: Rgb(238, 111, 248), bg: Reset, underline: Reset, modifier: BOLD",
            "the selected Create action",
        );
    }

    #[test]
    fn a_minimal_repository_names_the_command_that_sets_each_empty_row() {
        let repo = Repo {
            name: NonEmpty("Example".to_string()),
            id: NonEmpty("example".to_string()),
            root: PathBuf::from("example"),
            host: forge::HOST.to_string(),
            owner: None,
            assets: PathBuf::from("assets"),
            setup: Setup::Minimal,
            libraries: Libraries {
                chosen: Libraries::every(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/assets"))),
            },
            image: None,
            workflows: None,
            scripts: Scripts::default(),
            remote: false,
            install_gh: false,
        };
        let rows = repo.rows();
        let value = |field: Field| {
            rows.iter()
                .find(|(at, ..)| *at == field)
                .map(|(_, _, value)| value.as_str())
        };
        assert_eq!(value(Field::Setup), Some(copy::SETUP_MINIMAL));
        assert!(value(Field::Provider)
            .unwrap()
            .contains("buildc create image"));
        assert!(value(Field::Image).unwrap().contains("buildc create image"));
        assert!(value(Field::Workflows)
            .unwrap()
            .contains("buildc set workflows"));
        assert!(value(Field::Scripts)
            .unwrap()
            .contains("buildc create scripts"));
    }

    #[test]
    fn git_control_initialises_once_and_keeps_a_kept_ignore_file() {
        let fresh = std::env::temp_dir().join(format!("buildc-git-{}", std::process::id()));
        let kept = std::env::temp_dir().join(format!("buildc-git-kept-{}", std::process::id()));
        for dir in [&fresh, &kept] {
            let _ = std::fs::remove_dir_all(dir);
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::write(kept.join(".gitignore"), "mine\n").unwrap();

        git_control(&fresh).unwrap();
        assert!(fresh.join(".git").is_dir());
        assert_eq!(
            std::fs::read_to_string(fresh.join(".gitignore")).unwrap(),
            crate::init::GITIGNORE
        );
        let refused = git_control(&fresh).unwrap_err();
        assert!(refused.contains("already a git repository"), "{refused}");

        git_control(&kept).unwrap();
        assert_eq!(
            std::fs::read_to_string(kept.join(".gitignore")).unwrap(),
            "mine\n"
        );

        let _ = std::fs::remove_dir_all(&fresh);
        let _ = std::fs::remove_dir_all(&kept);
    }
}
