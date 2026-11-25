use crate::error::Error;
use crate::layout;
use crate::model::image::{Image, List};
use crate::parse::component::Summary;
use crate::parse::declaration::{append, append_inside, placed, wrap};
use crate::parse::local::{Listed, Place};
use common::prompt::Prompt;
use common::ui::tree::Change;
use common::ui::{Answer, Choice};
use std::path::{Path, PathBuf};

pub struct Request {
    pub paths: Vec<String>,
    pub inside: Vec<String>,
    pub library: Option<String>,
    pub pin: Option<String>,
    pub dependencies: bool,
    pub providers: Vec<String>,
}

struct Offer {
    /// `None` is this repository's own components folder.
    library: Option<String>,
    path: String,
    declares: Summary,
}

impl Offer {
    fn shown(&self) -> String {
        match &self.library {
            Some(library) => format!("{library}/{}", self.path),
            None => self.path.clone(),
        }
    }
}

struct Target {
    listed: Listed,
    images: Vec<usize>,
}

struct Reference {
    target: String,
    path: String,
    library: Option<String>,
    written: Option<String>,
    pin: Option<String>,
}

struct Owed {
    target: usize,
    by: usize,
    want: String,
    candidates: Vec<usize>,
}

struct Unlisted {
    file: PathBuf,
    name: String,
    outside: bool,
}

pub struct Add {
    references: Vec<Reference>,
    unlisted: Vec<Unlisted>,
    skipped: Vec<String>,
}

fn loaded(root: &Path) -> Result<crate::Loaded, String> {
    let mut loaded = crate::load(root);
    let blocking = std::mem::take(&mut loaded.issues).blocking_edits();
    match blocking.is_empty() {
        true => Ok(loaded),
        false => Err(blocking.plain()),
    }
}

impl Add {
    pub fn collect(
        request: Request,
        root: &Path,
        prompt: &Prompt,
        say: crate::report::Say<'_>,
    ) -> Result<Self, Error> {
        let loaded = loaded(root)?;
        let list = &loaded.list;
        if request.pin.is_some() && request.paths.len() > 1 {
            return Err(Error::Operation(
                "`--pin` fixes one component at one commit; name one path with it".to_string(),
            ));
        }
        let local = crate::parse::local::all(root, list);
        let targets = targets(list, &local, &request.inside, prompt)?;
        let offers = offers(root, list, &local)?;
        let picked = match request.paths.is_empty() {
            true => chosen(&offers, list, &targets, prompt)?,
            false => request
                .paths
                .iter()
                .map(|path| named(&offers, path, request.library.as_deref(), prompt))
                .collect::<Result<_, _>>()?,
        };
        let mut placed: Vec<(usize, usize)> = Vec::new();
        let mut skipped: Vec<String> = Vec::new();
        for &at in &picked {
            let offer = &offers[at];
            let refused: Vec<(&Target, String)> = targets
                .iter()
                .filter_map(|target| Some((target, unfit_in(list, target, &offer.declares)?)))
                .collect();
            if refused.len() == targets.len() {
                let (target, why) = &refused[0];
                return Err(Error::Operation(format!(
                    "`{}` cannot go into `{}`: it {why}",
                    offer.shown(),
                    target.listed.path
                )));
            }
            if let Some(pin) = &request.pin {
                pinned(list, offer, pin)?;
            }
            for (into, target) in targets.iter().enumerate() {
                if let Some((_, why)) = refused
                    .iter()
                    .find(|(held, _)| held.listed.path == target.listed.path)
                {
                    skipped.push(format!(
                        "`{}` is not added to `{}`, because it {why}",
                        offer.shown(),
                        target.listed.path
                    ));
                    continue;
                }
                refuse_listed(list, target, &offer.path)?;
                placed.push((into, at));
            }
        }
        let owed = owed(list, &targets, &offers, &placed);
        let brought = settle(
            &owed,
            &offers,
            &targets,
            (request.dependencies, &request.providers),
            prompt,
            say,
        )?;
        // Each provider goes above the component that requires it because the order of a component tree is its build order.
        let mut order: Vec<(usize, usize, bool)> = brought
            .into_iter()
            .filter(|pair| !placed.contains(pair))
            .map(|(into, from)| (into, from, false))
            .collect();
        order.extend(placed.iter().map(|&(into, from)| (into, from, true)));
        let all: Vec<(usize, usize)> = order.iter().map(|&(into, from, _)| (into, from)).collect();
        let unlisted = clashes(list, &targets, &offers, &all, prompt)?;
        let references = order
            .iter()
            .map(|&(into, from, chosen)| {
                let offer = &offers[from];
                Ok(Reference {
                    target: targets[into].listed.path.clone(),
                    path: offer.path.clone(),
                    library: offer.library.clone(),
                    written: written(list, &targets[into].listed, offer)?,
                    pin: match (chosen, &request.pin) {
                        (true, Some(pin)) => Some(pinned(list, offer, pin)?),
                        _ => None,
                    },
                })
            })
            .collect::<Result<_, Error>>()?;
        Ok(Self {
            references,
            unlisted,
            skipped,
        })
    }

    pub(crate) fn into_component(
        root: &Path,
        target: &str,
        wanted: &[(Option<String>, String, Option<String>)],
    ) -> Result<Self, Error> {
        let loaded = loaded(root)?;
        let list = &loaded.list;
        let local = crate::parse::local::all(root, list);
        let listed = crate::parse::local::named(&local, target)?.clone();
        let target = Target {
            images: reaching(list, &listed),
            listed,
        };
        let mut references: Vec<Reference> = Vec::new();
        for (library, path, pin) in wanted {
            refuse_listed(list, &target, path)?;
            let offer = Offer {
                library: library.clone(),
                path: path.clone(),
                declares: Summary::default(),
            };
            references.push(Reference {
                target: target.listed.path.clone(),
                path: path.clone(),
                library: library.clone(),
                written: written(list, &target.listed, &offer)?,
                pin: pin.clone(),
            });
        }
        Ok(Self {
            references,
            unlisted: Vec::new(),
            skipped: Vec::new(),
        })
    }

    pub fn apply(&self, root: &Path) -> Result<Vec<(PathBuf, Change)>, String> {
        self.applied(root).map(|(wrote, _)| wrote)
    }

    pub fn applied(
        &self,
        root: &Path,
    ) -> Result<(Vec<(PathBuf, Change)>, crate::init::Kept), String> {
        let mut kept: Vec<(PathBuf, String)> = Vec::new();
        let done = self
            .write(root, &mut kept)
            .and_then(|wrote| self.verify(root).map(|()| wrote));
        let kept: crate::init::Kept = kept
            .into_iter()
            .map(|(file, text)| (file, Some(text)))
            .collect();
        let done = done
            .map(|wrote| (wrote, kept.clone()))
            .map_err(|err| crate::init::undone(err, &crate::init::restore(&kept)));
        for line in &self.skipped {
            println!("{line}");
        }
        done
    }

    fn write(
        &self,
        root: &Path,
        kept: &mut Vec<(PathBuf, String)>,
    ) -> Result<Vec<(PathBuf, Change)>, String> {
        let mut wrote: Vec<(PathBuf, Change)> = Vec::new();
        for gone in &self.unlisted {
            let text = std::fs::read_to_string(&gone.file)
                .map_err(|err| format!("{}: {err}", gone.file.display()))?;
            if !kept.iter().any(|(held, _)| *held == gone.file) {
                kept.push((gone.file.clone(), text.clone()));
            }
            let out = crate::parse::declaration::unlisting(
                root,
                &gone.file,
                &text,
                &gone.name,
                gone.outside,
            )?;
            std::fs::write(&gone.file, out)
                .map_err(|err| format!("{}: {err}", gone.file.display()))?;
            wrote.push((
                layout::under(root, &gone.file),
                Change::Updated(format!("`{}` removed", gone.name)),
            ));
        }
        for reference in &self.references {
            // Each reference reloads the repository because the reference before it moved the spans in the target file.
            let list =
                crate::model::image::List::editable(root).map_err(|issues| issues.plain())?;
            let target = crate::parse::local::all(root, &list)
                .into_iter()
                .find(|listed| listed.path == reference.target)
                .ok_or_else(|| format!("`{}` is no longer a local component", reference.target))?;
            let file = target.place.file().to_path_buf();
            if !kept.iter().any(|(held, _)| *held == file) {
                let text = std::fs::read_to_string(&file)
                    .map_err(|err| format!("{}: {err}", file.display()))?;
                kept.push((file.clone(), text));
            }
            let below = relative(&target.context, &reference.path);
            let (mut chain, _) = placed(below.is_some(), below.unwrap_or(&reference.path), "");
            let (held, inherited) = match &target.place {
                Place::Directory { .. } => crate::parse::declaration::held(&file, &chain)?,
                Place::Inline { .. } => (0, None),
            };
            let parent = parent(&list, &target, reference, &chain, held)?;
            let inherited = parent
                .as_ref()
                .map(|(_, library)| library.clone())
                .or(inherited);
            let mut props = String::new();
            if let Some(library) = leaf_library(&list, reference, inherited)? {
                props.push_str(&format!(" library=\"{library}\""));
            }
            if let Some(pin) = &reference.pin {
                props.push_str(&format!(" pin=\"{pin}\""));
            }
            let (_, leaf) = placed(below.is_some(), below.unwrap_or(&reference.path), &props);
            let carried;
            if let Some((at, library)) = parent {
                carried = format!("{} library=\"{library}\"", chain[at]);
                chain[at] = &carried;
            }
            match &target.place {
                Place::Directory { .. } => append(&file, &chain, &leaf)?,
                Place::Inline { node, .. } => append_inside(&file, *node, &wrap(&chain, &leaf))?,
            }
            let shown = layout::under(root, &file);
            let said = format!("`{}` added to `{}`", reference.path, reference.target);
            match wrote.iter_mut().find(|(path, _)| *path == shown) {
                Some((_, Change::Updated(held))) => {
                    held.push_str(", and ");
                    held.push_str(&said);
                }
                _ => wrote.push((shown, Change::Updated(said))),
            }
        }
        Ok(wrote)
    }

    /// Library resolution can read a written reference from another library than the request named.
    /// An enclosing `pin=` also reaches the new reference.
    fn verify(&self, root: &Path) -> Result<(), String> {
        let loaded = loaded(root)
            .map_err(|blocking| format!("nothing is added, because the references leave a file that buildc cannot edit:\n\n{blocking}"))?;
        let list = &loaded.list;
        let local = crate::parse::local::all(root, list);
        for reference in &self.references {
            let Some(target) = local.iter().find(|listed| listed.path == reference.target) else {
                continue;
            };
            let file = target.place.file();
            for image in &list.images {
                let read = image.entries.iter().find(|entry| {
                    entry.path == reference.path
                        && entry
                            .setters
                            .iter()
                            .any(|setter| Path::new(setter.src.name()) == file)
                });
                let Some(entry) = read else {
                    let reaches = image.component == reference.target
                        || image
                            .entries
                            .iter()
                            .any(|entry| entry.path == reference.target);
                    if reaches {
                        return Err(format!(
                            "nothing is added, because `{}` reads `{}` from no library; `buildc check` names the problem",
                            image.id, reference.path
                        ));
                    }
                    continue;
                };
                let library = entry
                    .library()
                    .filter(|name| Some(*name) != list.root_library.as_deref());
                if library != reference.library.as_deref() {
                    return Err(format!(
                        "nothing is added, because `{}` reads `{}` from {} and not from {}",
                        image.id,
                        reference.path,
                        crate::resolve::library::shown(library),
                        crate::resolve::library::shown(reference.library.as_deref())
                    ));
                }
                if entry.pin != reference.pin {
                    return Err(format!(
                        "nothing is added because `{}` would read `{}` at {} and not at {}",
                        image.id,
                        reference.path,
                        at_pin(entry.pin.as_deref()),
                        at_pin(reference.pin.as_deref())
                    ));
                }
            }
        }
        Ok(())
    }
}

fn is_build_component(list: &List, listed: &Listed) -> bool {
    matches!(listed.place, Place::Directory { .. })
        && list.builds.iter().any(|build| {
            build
                .component
                .strip_prefix("./")
                .unwrap_or(&build.component)
                == listed.path
        })
}

fn targets(
    list: &List,
    local: &[Listed],
    inside: &[String],
    prompt: &Prompt,
) -> Result<Vec<Target>, Error> {
    let chosen: Vec<&Listed> = match inside {
        [] => {
            // The build components are the default targets because a user adds a feature to an image.
            let builds: Vec<&Listed> = local
                .iter()
                .filter(|listed| is_build_component(list, listed))
                .collect();
            match builds.as_slice() {
                [only] => vec![*only],
                _ if !prompt.asks() => {
                    return Err(Error::Operation(
                        "no single build component takes the components; `--in <component>` names each component that lists them"
                            .to_string(),
                    ))
                }
                _ => {
                    let options: Vec<Choice> = local
                        .iter()
                        .map(|listed| {
                            let choice = Choice::new(listed.path.as_str(), "");
                            match is_build_component(list, listed) {
                                true => choice,
                                false => choice.hidden_as(crate::copy::SHOW_ALL_COMPONENTS),
                            }
                        })
                        .collect();
                    match prompt.choose_many(crate::copy::ADD_TO, &options, &[])? {
                        Answer::Chosen(at) if !at.is_empty() => {
                            at.into_iter().map(|at| &local[at]).collect()
                        }
                        _ => {
                            return Err(Error::Operation(
                                "no component chosen to take the reference; `--in <component>` names one"
                                    .to_string(),
                            ))
                        }
                    }
                }
            }
        }
        named => named
            .iter()
            .map(|path| crate::parse::local::named(local, path).map_err(Error::Operation))
            .collect::<Result<_, _>>()?,
    };
    Ok(chosen
        .into_iter()
        .map(|listed| Target {
            images: reaching(list, listed),
            listed: listed.clone(),
        })
        .collect())
}

fn reaching(list: &List, listed: &Listed) -> Vec<usize> {
    list.images
        .iter()
        .enumerate()
        .filter(|(_, image)| {
            image
                .component
                .strip_prefix("./")
                .unwrap_or(&image.component)
                == listed.path
                || image
                    .entries
                    .iter()
                    .any(|entry| entry.path == listed.path && entry.held.is_none())
        })
        .map(|(at, _)| at)
        .collect()
}

fn offers(root: &Path, list: &List, local: &[Listed]) -> Result<Vec<Offer>, Error> {
    let home = list
        .resolver
        .home()
        .ok()
        .filter(|home| *home != layout::components(root));
    let mut out: Vec<Offer> = local
        .iter()
        .filter(|listed| {
            matches!(listed.place, Place::Directory { .. }) && !is_build_component(list, listed)
        })
        .map(|listed| Offer {
            library: home
                .as_ref()
                .filter(|home| listed.place.file().starts_with(home))
                .and(list.default_library.as_ref().map(|(name, _)| name.clone())),
            path: listed.path.clone(),
            declares: crate::parse::component::summary(listed.place.file(), &listed.path),
        })
        .collect();
    for library in &list.libraries {
        let tree = match crate::collection::cached(root, library) {
            Some(tree) => tree,
            // This command moves no cached library to a newer commit.
            // It fetches a library only when the cache holds no tree of it.
            None => crate::collection::tree(root, library)?,
        };
        let components = layout::library_components(&tree);
        for relative in layout::held_components(&components) {
            let path = relative.to_string_lossy().replace('\\', "/");
            if !crate::model::image::is_local_path(&path) {
                continue;
            }
            out.push(Offer {
                declares: crate::parse::component::summary(
                    &layout::manifest_at(&components.join(&path)),
                    &path,
                ),
                library: Some(library.name.clone()),
                path,
            });
        }
    }
    if out.is_empty() {
        return Err(Error::Operation(
            "no library offers a component; `buildc set library` declares one".to_string(),
        ));
    }
    out.sort_by(|a, b| (&a.path, &a.library).cmp(&(&b.path, &b.library)));
    out.dedup_by(|a, b| a.path == b.path && a.library == b.library);
    Ok(out)
}

fn at_pin(pin: Option<&str>) -> String {
    pin.map_or("the library version".to_string(), |pin| {
        format!("pin `{pin}`")
    })
}

fn unfit(image: &Image, declares: &Summary) -> Option<String> {
    if declares
        .provides
        .iter()
        .any(|name| name == crate::parse::component::BASE_IMAGE)
    {
        return Some(crate::copy::IS_BASE.to_string());
    }
    let family = image
        .base
        .as_ref()
        .map(|base| base.family.as_str())
        .filter(|family| !family.is_empty())?;
    (!crate::resolve::targets(&declares.targets, family))
        .then(|| crate::copy::unsupported(&declares.targets, family))
}

fn unfit_in(list: &List, target: &Target, declares: &Summary) -> Option<String> {
    target
        .images
        .iter()
        .find_map(|at| unfit(&list.images[*at], declares))
}

fn chosen(
    offers: &[Offer],
    list: &List,
    targets: &[Target],
    prompt: &Prompt,
) -> Result<Vec<usize>, Error> {
    if !prompt.asks() {
        return Err(Error::Operation(
            "no component named; `buildc add component <path>` names one".to_string(),
        ));
    }
    let mut kept: Vec<usize> = Vec::new();
    let mut options: Vec<Choice> = Vec::new();
    for (at, offer) in offers.iter().enumerate() {
        let refused: Vec<&str> = targets
            .iter()
            .filter(|target| unfit_in(list, target, &offer.declares).is_some())
            .map(|target| target.listed.path.as_str())
            .collect();
        let listed = targets
            .iter()
            .all(|target| refuse_listed(list, target, &offer.path).is_err());
        if refused.len() == targets.len() || listed {
            continue;
        }
        let mut about = vec![offer.declares.description.clone()];
        if !offer.declares.requires.is_empty() {
            about.push(format!("(requires {})", offer.declares.requires.join(", ")));
        }
        if !refused.is_empty() {
            about.push(format!("(does not support: {})", refused.join(", ")));
        }
        let about = about.join(" ").trim_start().to_string();
        let group = offer.path.split_once('/').map_or("", |(group, _)| group);
        let choice = Choice::new(offer.shown(), about).within(group);
        options.push(match offer.declares.hidden {
            true => choice.hidden(),
            false => choice,
        });
        kept.push(at);
    }
    match prompt.choose_many(crate::copy::WHICH_COMPONENTS, &options, &[])? {
        Answer::Chosen(at) if !at.is_empty() => Ok(at.into_iter().map(|at| kept[at]).collect()),
        _ => Err(Error::Operation(
            "no component chosen; `buildc add component <path>` names one".to_string(),
        )),
    }
}

fn named(
    offers: &[Offer],
    path: &str,
    library: Option<&str>,
    prompt: &Prompt,
) -> Result<usize, Error> {
    let holders: Vec<usize> = offers
        .iter()
        .enumerate()
        .filter(|(_, offer)| offer.path == path)
        .filter(|(_, offer)| library.is_none_or(|name| offer.library.as_deref() == Some(name)))
        .map(|(at, _)| at)
        .collect();
    let shown = |at: &usize| crate::resolve::library::shown(offers[*at].library.as_deref());
    match holders.as_slice() {
        [only] => Ok(*only),
        [] => Err(Error::Operation(match library {
            Some(name) => format!("library `{name}` holds no component `{path}`"),
            None => format!("no library holds `{path}`"),
        })),
        many => {
            let options: Vec<Choice> = many
                .iter()
                .map(|at| Choice::new(offers[*at].shown(), ""))
                .collect();
            let names: Vec<String> = many.iter().map(shown).collect();
            let at = prompt
                .choose(&crate::copy::which_library(path), &options)?
                .ok_or_else(|| {
                    format!(
                        "{} each hold `{path}`; `--library <name>` names one",
                        crate::said(&names)
                    )
                })?;
            Ok(many[at])
        }
    }
}

fn pinned(list: &List, offer: &Offer, pin: &str) -> Result<String, Error> {
    let remote = offer.library.as_deref().is_some_and(|name| {
        list.libraries
            .iter()
            .any(|library| library.name == name && library.pin().is_some())
    });
    if !remote {
        return Err(Error::Operation(format!(
            "`{}` is in a local library, which is read in place and takes no pin",
            offer.shown()
        )));
    }
    Ok(crate::parse::declaration::quotable(pin)?.to_string())
}

fn refuse_listed(list: &List, target: &Target, path: &str) -> Result<(), Error> {
    let file = target.listed.place.file();
    let listed = list.images.iter().any(|image| {
        image.entries.iter().any(|entry| {
            entry.path == path
                && entry
                    .setters
                    .iter()
                    .any(|setter| Path::new(setter.src.name()) == file)
        })
    });
    match listed {
        true => Err(Error::Operation(format!(
            "`{}` already lists `{path}`",
            target.listed.path
        ))),
        false => Ok(()),
    }
}

fn relative<'a>(top: &str, path: &'a str) -> Option<&'a str> {
    match top {
        "" => Some(path),
        top => path.strip_prefix(top)?.strip_prefix('/'),
    }
}

fn written(list: &List, target: &Listed, offer: &Offer) -> Result<Option<String>, Error> {
    let rooted = relative(&target.context, &offer.path).is_none() || target.context.is_empty();
    let read = list
        .resolver
        .resolve(&offer.path, None, rooted, None)
        .ok()
        .map(|held| held.map(|held| held.library));
    if read.as_ref() == Some(&offer.library) {
        return Ok(None);
    }
    match &offer.library {
        Some(library) => Ok(Some(library.clone())),
        None => list.root_library.clone().map(Some).ok_or_else(|| {
            Error::Operation(format!(
                "another library also holds `{}`, and this repository's own library has no name to choose it by; `buildc set library` declares it under `local`",
                offer.path
            ))
        }),
    }
}

fn parent(
    list: &List,
    target: &Listed,
    reference: &Reference,
    chain: &[&str],
    held: usize,
) -> Result<Option<(usize, String)>, String> {
    let rooted = relative(&target.context, &reference.path).is_none() || target.context.is_empty();
    let mut path = match rooted {
        true => String::new(),
        false => target.context.clone(),
    };
    for (at, link) in chain.iter().enumerate() {
        if *link == crate::parse::image::ROOT {
            continue;
        }
        path = crate::parse::tree::join(&path, link);
        if at < held {
            continue;
        }
        let settled = match list.resolver.resolve(&path, None, rooted, None) {
            Ok(held) => held.map(|held| held.library) == reference.library,
            Err(crate::resolve::library::Unresolved::Ambiguous(_)) => false,
            // A segment that no library holds is a context and needs no `library=`.
            Err(
                crate::resolve::library::Unresolved::Absent { .. }
                | crate::resolve::library::Unresolved::Undeclared(_),
            ) => true,
        };
        if settled {
            continue;
        }
        let library = reference.library.clone().or_else(|| list.root_library.clone()).ok_or_else(|| {
            format!(
                "another library also holds `{path}`, and this repository's own library has no name to choose it by; `buildc set library` declares it under `local`"
            )
        })?;
        return Ok(Some((at, library)));
    }
    Ok(None)
}

fn leaf_library(
    list: &List,
    reference: &Reference,
    inherited: Option<String>,
) -> Result<Option<String>, String> {
    let Some(inherited) = inherited else {
        return Ok(reference.written.clone());
    };
    let reads = Some(inherited).filter(|name| Some(name) != list.root_library.as_ref());
    if reads == reference.library {
        return Ok(None);
    }
    reference
        .library
        .clone()
        .or_else(|| list.root_library.clone())
        .map(Some)
        .ok_or_else(|| {
            format!(
                "`{}` sits below a `library=` that names another library, and this repository's own library has no name to choose it by; `buildc set library` declares it under `local`",
                reference.path
            )
        })
}

fn provides(offer: &Offer, want: &str) -> bool {
    crate::resolve::answers(
        &offer.path,
        offer.declares.provides.iter().map(String::as_str),
        want,
    )
}

fn owed(list: &List, targets: &[Target], offers: &[Offer], placed: &[(usize, usize)]) -> Vec<Owed> {
    let mut out: Vec<Owed> = Vec::new();
    for &(into, by) in placed {
        let images = &targets[into].images;
        for want in &offers[by].declares.requires {
            let met = placed
                .iter()
                .any(|&(held, from)| held == into && provides(&offers[from], want));
            if met
                || images.is_empty()
                || images.iter().all(|at| list.images[*at].answers(want))
                || out
                    .iter()
                    .any(|owed| owed.target == into && owed.want == *want)
            {
                continue;
            }
            let fits: Vec<usize> = offers
                .iter()
                .enumerate()
                .filter(|(_, offer)| {
                    provides(offer, want)
                        && images
                            .iter()
                            .all(|at| unfit(&list.images[*at], &offer.declares).is_none())
                })
                .map(|(at, _)| at)
                .collect();
            // A provider that this repository holds wins because the user wrote it for this repository.
            let own: Vec<usize> = fits
                .iter()
                .copied()
                .filter(|at| offers[*at].library.is_none())
                .collect();
            let candidates = match own.is_empty() {
                true => fits,
                false => own,
            };
            if !candidates.is_empty() {
                out.push(Owed {
                    target: into,
                    by,
                    want: want.clone(),
                    candidates,
                });
            }
        }
    }
    out
}

fn settle(
    owed: &[Owed],
    offers: &[Offer],
    targets: &[Target],
    (dependencies, preferred): (bool, &[String]),
    prompt: &Prompt,
    say: crate::report::Say<'_>,
) -> Result<Vec<(usize, usize)>, Error> {
    let mut used = vec![false; preferred.len()];
    let mut chosen: Vec<Option<usize>> = owed
        .iter()
        .map(|owed| {
            let named = preferred.iter().enumerate().find_map(|(at, name)| {
                let found = owed.candidates.iter().copied().find(|candidate| {
                    offers[*candidate].shown() == *name || offers[*candidate].path == *name
                })?;
                Some((at, found))
            });
            match (named, owed.candidates.as_slice()) {
                (Some((at, found)), _) => {
                    used[at] = true;
                    Some(found)
                }
                (None, [only]) => Some(*only),
                (None, _) => None,
            }
        })
        .collect();
    if let Some(at) = used.iter().position(|used| !used) {
        return Err(Error::Operation(format!(
            "`{}` provides no requirement of the chosen components that a build lacks",
            preferred[at]
        )));
    }
    if owed.is_empty() || !(dependencies || prompt.asks()) {
        return Ok(Vec::new());
    }
    if !dependencies {
        say(&format!("{}\n", crate::copy::REQUIRED));
        for (owed, chosen) in owed.iter().zip(&chosen) {
            let provider = match chosen {
                Some(at) => offers[*at].shown(),
                None => {
                    let many: Vec<String> = owed
                        .candidates
                        .iter()
                        .map(|at| offers[*at].shown())
                        .collect();
                    format!("one of {}", many.join(", "))
                }
            };
            say(&format!(
                "\n  {} > {}\n    requires: {}\n    provider component: {provider}\n",
                offers[owed.by].shown(),
                targets[owed.target].listed.path,
                owed.want
            ));
        }
        say("\n");
        if !prompt.confirm(
            crate::copy::BRING_REQUIRED,
            crate::copy::YES,
            crate::copy::NO,
        )? {
            return Ok(Vec::new());
        }
    }
    for (owed, chosen) in owed.iter().zip(chosen.iter_mut()) {
        if chosen.is_some() {
            continue;
        }
        let names: Vec<String> = owed
            .candidates
            .iter()
            .map(|at| format!("`{}`", offers[*at].shown()))
            .collect();
        if !prompt.asks() {
            return Err(Error::Operation(format!(
                "`{}` requires `{}`, and {} each provide it; `--dependencies <provider>` names one",
                offers[owed.by].shown(),
                owed.want,
                crate::said(&names)
            )));
        }
        let options: Vec<Choice> = owed
            .candidates
            .iter()
            .map(|at| {
                let choice = Choice::new(
                    offers[*at].shown(),
                    offers[*at].declares.description.as_str(),
                );
                match offers[*at].declares.hidden {
                    true => choice.hidden(),
                    false => choice,
                }
            })
            .collect();
        *chosen = prompt
            .choose(
                &crate::copy::which_provider(&offers[owed.by].shown(), &owed.want),
                &options,
            )?
            .map(|at| owed.candidates[at]);
    }
    Ok(owed
        .iter()
        .zip(chosen)
        .filter_map(|(owed, chosen)| Some((owed.target, chosen?)))
        .collect())
}

fn clashes(
    list: &List,
    targets: &[Target],
    offers: &[Offer],
    placed: &[(usize, usize)],
    prompt: &Prompt,
) -> Result<Vec<Unlisted>, Error> {
    let mut out: Vec<Unlisted> = Vec::new();
    for &(into, from) in placed {
        let offer = &offers[from];
        for image in targets[into].images.iter().map(|at| &list.images[*at]) {
            let covered = image.base.as_ref().is_some_and(|base| {
                base.covers(offer.declares.provides.iter().map(String::as_str))
            });
            if covered {
                continue;
            }
            for capability in &offer.declares.provides {
                let exclusive = offer.declares.exclusive.contains(capability);
                let held: Vec<&str> = image
                    .entries
                    .iter()
                    .filter(|entry| {
                        entry.path != offer.path
                            && !out.iter().any(|gone| gone.name == entry.path)
                            && entry.component.as_ref().is_some_and(|component| {
                                component
                                    .provides
                                    .iter()
                                    .any(|decl| decl.name == *capability)
                                    && (exclusive || component.exclusive.contains(capability))
                            })
                    })
                    .map(|entry| entry.path.as_str())
                    .collect();
                if held.is_empty() {
                    continue;
                }
                let named: Vec<String> = held.iter().map(ToString::to_string).collect();
                let holder = crate::said(&named);
                let options = [
                    Choice::new(crate::copy::replace(&holder), ""),
                    Choice::new(crate::copy::CANCEL, ""),
                ];
                let answer = match prompt.asks() {
                    true => prompt.choose(
                        &crate::copy::exclusive_clash(capability, &holder, &image.id),
                        &options,
                    )?,
                    false => None,
                };
                match answer {
                    Some(0) => out.extend(held.into_iter().map(|name| Unlisted {
                        file: PathBuf::from(image.src.name()),
                        name: name.to_string(),
                        outside: image.outside,
                    })),
                    Some(_) => {
                        return Err(Error::Operation(format!(
                            "nothing added: `{capability}` is exclusive to `{holder}` in `{}`",
                            image.id
                        )))
                    }
                    None => {
                        return Err(Error::Operation(format!(
                            "`{capability}` is exclusive, and `{holder}` already provides it in `{}`; run `buildc add component` in a terminal to replace it",
                            image.id
                        )))
                    }
                }
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo(case: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("buildc-add-{case}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let files = [
            (
                "repo.kdl",
                "schema-version 1\n\nlibraries default=\"me\" {\n    local {\n        me { source \".\" }\n        shelf { source \"shelf\" }\n    }\n}\n\nbuilds default=\"ws\" {\n    ws\n}\n",
            ),
            (
                "components/ws.kdl",
                "schema-version 1\nbase \"quay.io/fedora/fedora-bootc:44\"\n\ncomponent \"/\" {\n    desktop\n}\n",
            ),
            ("components/desktop/manifest.kdl", "schema-version 1\n"),
            ("shelf/desktop/extras/manifest.kdl", "schema-version 1\n"),
            ("shelf/hardware/nvidia/manifest.kdl", "schema-version 1\n"),
            ("shelf/hardware/nvidia/cuda/manifest.kdl", "schema-version 1\n"),
            (
                "shelf/apps/store/manifest.kdl",
                "schema-version 1\nrequires \"flatpak\"\n",
            ),
            ("shelf/flat/a/manifest.kdl", "schema-version 1\nprovides \"flatpak\"\n"),
            ("shelf/flat/b/manifest.kdl", "schema-version 1\nprovides \"flatpak\"\n"),
        ];
        for (path, text) in files {
            crate::init::put(&root.join(path), text).unwrap();
        }
        root
    }

    fn add(root: &Path, path: &str, inside: &[&str], pin: Option<&str>) -> Result<(), Error> {
        let request = Request {
            paths: vec![path.to_string()],
            inside: inside.iter().map(ToString::to_string).collect(),
            library: None,
            pin: pin.map(str::to_string),
            dependencies: false,
            providers: Vec::new(),
        };
        Add::collect(request, root, &Prompt::silent(), &mut |_| {})?.apply(root)?;
        Ok(())
    }

    #[test]
    fn a_component_of_a_local_default_library_is_offered_once() {
        let root = repo("default-shelf");
        crate::init::put(
            &root.join("repo.kdl"),
            "schema-version 1\n\nlibraries default=\"shelf\" {\n    local {\n        me { source \".\" }\n        shelf { source \"shelf\" }\n    }\n}\n\nbuilds default=\"ws\" {\n    ws\n}\n",
        )
        .unwrap();
        add(&root, "hardware/nvidia", &[], None).unwrap();
        let text = std::fs::read_to_string(root.join("components/ws.kdl")).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(
            text,
            "schema-version 1\nbase \"quay.io/fedora/fedora-bootc:44\"\n\ncomponent \"/\" {\n    desktop\n    hardware {\n        nvidia\n    }\n}\n"
        );
    }

    #[test]
    fn the_only_build_takes_a_nested_path_inside_its_rooted_block() {
        let root = repo("nested");
        add(&root, "hardware/nvidia/cuda", &[], None).unwrap();
        let text = std::fs::read_to_string(root.join("components/ws.kdl")).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(
            text,
            "schema-version 1\nbase \"quay.io/fedora/fedora-bootc:44\"\n\ncomponent \"/\" {\n    desktop\n    hardware {\n        nvidia {\n            cuda\n        }\n    }\n}\n"
        );
    }

    fn two_tools(case: &str, ws: &str) -> PathBuf {
        let root = repo(case);
        let files = [
            (
                "repo.kdl",
                "schema-version 1\n\nlibraries default=\"me\" {\n    local {\n        me { source \".\" }\n        shelf { source \"shelf\" }\n        up { source \"up\" }\n    }\n}\n\nbuilds default=\"ws\" {\n    ws\n}\n",
            ),
            ("components/ws.kdl", ws),
            ("shelf/tools/manifest.kdl", "schema-version 1\n"),
            ("shelf/tools/podman/manifest.kdl", "schema-version 1\n"),
            ("up/tools/manifest.kdl", "schema-version 1\n"),
            ("up/tools/podman/manifest.kdl", "schema-version 1\n"),
        ];
        for (path, text) in files {
            crate::init::put(&root.join(path), text).unwrap();
        }
        root
    }

    fn add_from(root: &Path, path: &str, library: &str) -> Result<(), Error> {
        let request = Request {
            paths: vec![path.to_string()],
            inside: Vec::new(),
            library: Some(library.to_string()),
            pin: None,
            dependencies: false,
            providers: Vec::new(),
        };
        Add::collect(request, root, &Prompt::silent(), &mut |_| {})?.apply(root)?;
        Ok(())
    }

    #[test]
    fn a_new_parent_segment_takes_the_library_of_the_chosen_component() {
        let root = two_tools(
            "parent-new",
            "schema-version 1\nbase \"quay.io/fedora/fedora-bootc:44\"\n\ncomponent \"/\" {\n    desktop\n}\n",
        );
        add_from(&root, "tools/podman", "up").unwrap();
        let text = std::fs::read_to_string(root.join("components/ws.kdl")).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(
            text,
            "schema-version 1\nbase \"quay.io/fedora/fedora-bootc:44\"\n\ncomponent \"/\" {\n    desktop\n    tools library=\"up\" {\n        podman\n    }\n}\n"
        );
    }

    #[test]
    fn a_listed_parent_segment_keeps_its_library_and_takes_the_child() {
        let root = two_tools(
            "parent-listed",
            "schema-version 1\nbase \"quay.io/fedora/fedora-bootc:44\"\n\ncomponent \"/\" {\n    desktop\n    tools library=\"shelf\"\n}\n",
        );
        add_from(&root, "tools/podman", "up").unwrap();
        let text = std::fs::read_to_string(root.join("components/ws.kdl")).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(
            text,
            "schema-version 1\nbase \"quay.io/fedora/fedora-bootc:44\"\n\ncomponent \"/\" {\n    desktop\n    tools library=\"shelf\" {\n        podman library=\"up\"\n    }\n}\n"
        );
    }

    #[test]
    fn a_child_below_a_listed_parent_names_the_library_the_parent_does_not() {
        let root = two_tools(
            "parent-inherited",
            "schema-version 1\nbase \"quay.io/fedora/fedora-bootc:44\"\n\ncomponent \"/\" {\n    desktop\n    tools library=\"up\"\n}\n",
        );
        // Only `shelf` now holds `tools/podman` and the parent segment still names `up`.
        std::fs::remove_dir_all(root.join("up/tools/podman")).unwrap();
        add(&root, "tools/podman", &[], None).unwrap();
        let text = std::fs::read_to_string(root.join("components/ws.kdl")).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(
            text,
            "schema-version 1\nbase \"quay.io/fedora/fedora-bootc:44\"\n\ncomponent \"/\" {\n    desktop\n    tools library=\"up\" {\n        podman library=\"shelf\"\n    }\n}\n"
        );
    }

    #[test]
    fn a_library_directory_that_is_no_component_path_is_not_offered() {
        let root = repo("unpathed");
        // The folder name holds KDL property syntax.
        // A path made from it would write `library="y"` into the target file.
        crate::init::put(
            &root.join("shelf/x library=\"y\"/manifest.kdl"),
            "schema-version 1\n",
        )
        .unwrap();
        let loaded = loaded(&root).unwrap();
        let local = crate::parse::local::all(&root, &loaded.list);
        let offered: Vec<String> = offers(&root, &loaded.list, &local)
            .unwrap()
            .into_iter()
            .map(|offer| offer.path)
            .collect();
        std::fs::remove_dir_all(&root).unwrap();
        assert!(
            offered.contains(&"hardware/nvidia".to_string()),
            "{offered:?}"
        );
        assert!(
            !offered.iter().any(|path| path.contains("library")),
            "{offered:?}"
        );
    }

    #[test]
    fn a_path_below_the_target_is_a_bare_child_that_names_its_library() {
        let root = repo("below");
        add(&root, "desktop/extras", &["desktop"], None).unwrap();
        let text = std::fs::read_to_string(root.join("components/desktop/manifest.kdl")).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(text, "schema-version 1\nextras library=\"shelf\"\n");
    }

    #[test]
    fn a_pin_on_a_local_library_is_refused_and_nothing_is_written() {
        let root = repo("pin");
        let refused = add(&root, "hardware/nvidia", &[], Some("v1"));
        let text = std::fs::read_to_string(root.join("components/ws.kdl")).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert!(refused.is_err());
        assert!(!text.contains("nvidia"));
    }

    #[test]
    fn a_preferred_provider_settles_two_candidates_without_a_terminal() {
        let root = repo("prefer");
        let request = |providers: &[&str]| Request {
            paths: vec!["apps/store".to_string()],
            inside: Vec::new(),
            library: None,
            pin: None,
            dependencies: true,
            providers: providers.iter().map(ToString::to_string).collect(),
        };
        let silent = Prompt::silent();
        let refused = Add::collect(request(&[]), &root, &silent, &mut |_| {}).is_err();
        Add::collect(request(&["flat/b"]), &root, &silent, &mut |_| {})
            .unwrap()
            .apply(&root)
            .unwrap();
        let text = std::fs::read_to_string(root.join("components/ws.kdl")).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert!(refused);
        assert!(text.contains("    flat {\n        b\n    }\n"), "{text}");
        assert!(!text.contains("        a\n"), "{text}");
    }
}
