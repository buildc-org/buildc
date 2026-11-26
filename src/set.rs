use crate::conformance::profile_options;
use crate::copy;
use crate::error::Error;
use crate::model::remote::DefaultLibrary;
use crate::parse::declaration::splice;
use crate::parse::remote::Kind;
use crate::scap::{ordinal, rule_name, Content};
use crate::{layout, parse};
use common::prompt::Prompt;
use common::ui::tree::Change;
use common::ui::{Answer, Choice};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub struct Library {
    name: String,
    source: String,
    kind: Kind,
    root_library: Option<String>,
    creates_components: bool,
}

impl Library {
    pub fn adding(
        root: &Path,
        defaults: Vec<DefaultLibrary>,
        name: Option<String>,
        source: Option<String>,
        prompt: &Prompt,
    ) -> Result<Option<Self>, Error> {
        let asks = name.is_none() && source.is_none();
        let Some(mut library) = Self::chosen(root, defaults, name, source, prompt)? else {
            return Ok(None);
        };
        let text = std::fs::read_to_string(root.join(layout::REPO_FILE)).unwrap_or_default();
        let components = layout::components(root);
        if asks
            && library.root_library.is_none()
            && parse::repo::libraries_in(&text).is_empty()
            && !components.exists()
        {
            let folder = format!("{}/", layout::under(root, &components).display());
            let question = copy::create_local_library(&folder);
            if prompt.confirm(&question, copy::YES, copy::NO)? {
                library.root_library = Some(layout::ROOT_LIBRARY.to_string());
                library.creates_components = true;
            }
        }
        Ok(Some(library))
    }

    fn chosen(
        root: &Path,
        defaults: Vec<DefaultLibrary>,
        name: Option<String>,
        source: Option<String>,
        prompt: &Prompt,
    ) -> Result<Option<Self>, Error> {
        let text = std::fs::read_to_string(root.join(layout::REPO_FILE)).unwrap_or_default();
        let declared = parse::repo::libraries_in(&text);
        let root_library = inferred_root(root, &declared);
        let taken = |name: &str| {
            declared.iter().any(|held| held.name == name) || root_library.as_deref() == Some(name)
        };
        let offered: Vec<DefaultLibrary> = defaults
            .into_iter()
            .filter(|library| !declared.iter().any(|held| held.name == library.name))
            .collect();
        let picked = |library: &DefaultLibrary| Self {
            name: library.name.clone(),
            source: library.source.clone(),
            kind: Kind::Remote,
            root_library: root_library.clone(),
            creates_components: false,
        };
        if let Some(source) = source {
            https_only(&source)?;
            let name = match name.or_else(|| derived_name(root, &source)) {
                Some(name) => name,
                None => {
                    return Err(Error::Invocation(format!(
                        "buildc derives no library name from `{source}`; give the name as \
                         `buildc set library <name> --source {source}`"
                    )))
                }
            };
            return Self::at(root, source, name, &taken, root_library.clone()).map(Some);
        }
        if let Some(name) = name {
            if taken(&name) {
                return Err(Error::Operation(format!(
                    "`{name}` is already a library in {}",
                    layout::REPO_FILE
                )));
            }
            let Some(library) = offered.iter().find(|library| library.name == name) else {
                let there: Vec<String> = offered
                    .iter()
                    .map(|library| format!("`{}`", library.name))
                    .collect();
                let there = match there.is_empty() {
                    true => String::new(),
                    false => format!("; the libraries buildc offers are {}", there.join(", ")),
                };
                return Err(Error::Invocation(format!(
                    "`{name}` is not a library buildc offers{there}; `--source <url or path>` \
                     declares another library"
                )));
            };
            return Ok(Some(picked(library)));
        }
        if !prompt.asks() {
            return Err(Error::Invocation(
                "nothing can be asked here; name a library buildc offers as `buildc set library \
                 <name>`, or give `--source <url or path>`"
                    .to_string(),
            ));
        }
        if !offered.is_empty() {
            let options: Vec<Choice> = offered
                .iter()
                .map(|library| {
                    Choice::new(
                        library.name.clone(),
                        format!("{} ({})", library.about, library.source),
                    )
                })
                .chain([Choice::new(copy::LIBRARY_ELSEWHERE, "")])
                .collect();
            let Some(chosen) = prompt.choose(copy::WHICH_LIBRARY, &options)? else {
                return Ok(None);
            };
            if let Some(library) = offered.get(chosen) {
                return Ok(Some(picked(library)));
            }
        }
        let source = prompt.text_with_placeholder(
            None,
            copy::LIBRARY_LOCATION,
            copy::LIBRARY_LOCATION_HINT,
            "`--source <url or path>`",
            None,
        )?;
        https_only(&source)?;
        let derived = derived_name(root, &source);
        let placeholder = derived.as_ref().map(|name| format!("{name} (default)"));
        let name = prompt.text_with_placeholder(
            None,
            copy::LIBRARY_NAME,
            placeholder.as_deref().unwrap_or(copy::LIBRARY_NAME_HINT),
            "a `name` argument",
            derived.as_deref(),
        )?;
        let library = Self::at(root, source, name, &taken, root_library.clone())?;
        println!("{}", library.summary());
        match prompt.confirm_current(copy::ADD_LIBRARY, copy::YES, copy::CANCEL, true)? {
            true => Ok(Some(library)),
            false => Ok(None),
        }
    }

    fn at(
        root: &Path,
        source: String,
        name: String,
        taken: &dyn Fn(&str) -> bool,
        root_library: Option<String>,
    ) -> Result<Self, Error> {
        if !crate::model::image::is_name(&name) {
            return Err(Error::Operation(format!(
                "`{name}` is not a library name: lowercase, digits and dashes, starting with a \
                 letter"
            )));
        }
        if taken(&name) {
            return Err(Error::Operation(format!(
                "`{name}` is already a library in {}",
                layout::REPO_FILE
            )));
        }
        let kind = kind_of(root, &source);
        if kind == Kind::Remote && !source.contains("://") {
            crate::collection::git_source(root, &source).map_err(|why| {
                Error::Operation(format!(
                    "{why}; a folder of this repository reads as a `local` library once git \
                     tracks it"
                ))
            })?;
        }
        Ok(Self {
            name,
            kind,
            source,
            root_library,
            creates_components: false,
        })
    }

    fn summary(&self) -> String {
        let mut out = format!(
            "  location  {}\n  name      {}\n  kind      {}\n",
            self.source,
            self.name,
            self.kind.block()
        );
        if let Some(own) = &self.root_library {
            out.push_str(&format!(
                "  also      `{own}` declares this repository under `local`\n"
            ));
        }
        out
    }

    pub fn apply(&self, root: &Path) -> Result<Vec<(PathBuf, Change)>, String> {
        let file = root.join(layout::REPO_FILE);
        let keep = layout::components(root).join(".gitkeep");
        crate::init::undoable(root, &[file.clone(), keep.clone()], || {
            self.write(root, &file, &keep)
        })
    }

    fn write(
        &self,
        root: &Path,
        file: &Path,
        keep: &Path,
    ) -> Result<Vec<(PathBuf, Change)>, String> {
        let mut wrote = Vec::new();
        if self.creates_components {
            crate::init::put(keep, "")?;
            wrote.push((layout::under(root, keep), Change::Created));
        }
        let block = self.kind.block();
        let mut said = format!("`{}` added to libraries under `{block}`", self.name);
        if let Some(own) = &self.root_library {
            crate::parse::declaration::append(
                file,
                &["libraries", Kind::Local.block()],
                &format!("{own} {{ source \".\" }}"),
            )?;
            crate::parse::declaration::property(file, "libraries", "default", own)?;
            said.push_str(&format!(", and this repository as `{own}`"));
        }
        crate::parse::declaration::append(
            file,
            &["libraries", block],
            &format!("{} {{ source {:?} }}", self.name, self.source),
        )?;
        wrote.push((PathBuf::from(layout::REPO_FILE), Change::Updated(said)));
        Ok(wrote)
    }
}

fn https_only(source: &str) -> Result<(), Error> {
    match source.contains("://") && !source.starts_with("https://") {
        true => Err(Error::Operation(format!(
            "`{source}` is not an HTTPS Git URL; credentials and host policy stay with Git"
        ))),
        false => Ok(()),
    }
}

/// The repository root counts as a library while `libraries` declares no `local` library. A root that holds components, workflows or plugins keeps them as that library once a remote library is declared.
fn inferred_root(root: &Path, declared: &[crate::model::remote::Collection]) -> Option<String> {
    if declared.iter().any(|library| library.pin().is_none()) {
        return None;
    }
    let holds = layout::components(root).is_dir()
        || ["workflows", "plugins"]
            .iter()
            .any(|dir| root.join(dir).is_dir());
    holds.then(|| layout::ROOT_LIBRARY.to_string())
}

fn kind_of(root: &Path, source: &str) -> Kind {
    if source.contains("://") {
        return Kind::Remote;
    }
    // A checkout's own config can name the program that `core.fsmonitor` runs. The override stops git from running it.
    let tracked = std::process::Command::new("git")
        .args([
            "--no-optional-locks",
            "-c",
            "core.fsmonitor=false",
            "ls-files",
            "--",
            source,
        ])
        .current_dir(root)
        .output()
        .is_ok_and(|out| out.status.success() && !out.stdout.is_empty());
    match tracked {
        true => Kind::Local,
        false => Kind::Remote,
    }
}

/// The library's own `repo.kdl` names it. A URL with no such name takes its owner segment.
fn derived_name(root: &Path, source: &str) -> Option<String> {
    let tree = match source.contains("://") {
        true => crate::collection::resolve(root, source, "HEAD")
            .ok()
            .map(|(_, tree)| tree),
        false => Some(root.join(source)),
    };
    tree.and_then(|tree| std::fs::read_to_string(tree.join(layout::REPO_FILE)).ok())
        .and_then(|text| parse::repo::derived_id(&text))
        .or_else(|| publisher(source))
}

fn publisher(source: &str) -> Option<String> {
    let name = match source.split_once("://") {
        Some((_, rest)) => rest.split('/').nth(1)?,
        None => Path::new(source).file_name()?.to_str()?,
    }
    .to_lowercase();
    crate::model::image::is_name(&name).then_some(name)
}

fn group(number: &str) -> &str {
    number.rsplit_once('.').map_or("other", |(head, _)| head)
}

fn claimable_rules<'a>(
    content: &'a Content,
    selected: &'a BTreeSet<String>,
) -> Vec<(&'a str, &'a BTreeSet<String>)> {
    let mut rules: Vec<_> = selected
        .iter()
        .filter_map(|rule| {
            content
                .numbers
                .get(rule)
                .map(|numbers| (rule.as_str(), numbers))
        })
        .collect();
    rules.sort_by_key(|(rule, numbers)| {
        (
            numbers
                .iter()
                .next()
                .map(|number| ordinal(number))
                .unwrap_or_default(),
            rule.to_string(),
        )
    });
    rules
}

fn claim_options(content: &Content, rules: &[(&str, &BTreeSet<String>)]) -> Vec<Choice> {
    rules
        .iter()
        .map(|(rule, numbers)| {
            let first = numbers.iter().next().expect("a number reaches the rule");
            Choice::new(
                format!(
                    "{}  {}  {}",
                    first,
                    content
                        .titles
                        .get(*rule)
                        .map(String::as_str)
                        .unwrap_or(*rule),
                    rule_name(rule)
                ),
                content.descriptions.get(*rule).cloned().unwrap_or_default(),
            )
            .within(group(first))
        })
        .collect()
}

pub struct Conforms {
    declaration: crate::conformance::Conforms,
    bring: Option<Bring>,
}

struct Bring {
    add: crate::add_component::Add,
    copy: Vec<String>,
    build: String,
}

impl Conforms {
    pub fn collect(
        root: &Path,
        named: Option<String>,
        datastream: Option<PathBuf>,
        prompt: &Prompt,
        say: crate::report::Say<'_>,
    ) -> Result<Option<Self>, Error> {
        let crate::Loaded { list, .. } = crate::load(root);
        let disk = crate::parse::disk::Disk::scan(root);
        let index = crate::provider::Index::scan(root, &list.libraries, &disk, false);
        if list.images.is_empty() {
            return Err(
                "`set conforms` needs an image; run `buildc create image <name>` first"
                    .to_string()
                    .into(),
            );
        }
        let ids: Vec<&str> = list.images.iter().map(|image| image.id.as_str()).collect();
        let at = match named {
            Some(name) => list
                .images
                .iter()
                .position(|image| image.id == name)
                .ok_or_else(|| {
                    Error::Invocation(format!(
                        "`{name}` is not a declared image; there is {}",
                        ids.join(", ")
                    ))
                })?,
            None if list.images.len() == 1 => 0,
            None => {
                let options: Vec<Choice> = list
                    .images
                    .iter()
                    .map(|image| Choice::new(&image.id, ""))
                    .collect();
                match prompt.choose(copy::MEASURED_IMAGE, &options)? {
                    Some(at) => at,
                    None => return Ok(None),
                }
            }
        };
        let image = &list.images[at];

        let (content_name, family) = crate::scap::named_by(image.base.as_ref());
        let path = crate::scap::content_path(content_name, family, datastream.as_deref())?;
        let content = crate::scap::content_of(&path)?;
        if content.profiles.is_empty() {
            return Err(format!(
                "{} carries no profile to be measured against",
                path.display()
            )
            .into());
        }

        say(&format!(
            "{}\n\n",
            crate::conformance::cost(&format!("`{}`", image.id), list.audit_enforce)
        ));
        let options = profile_options(&content);
        let Some(chosen) = prompt.choose(copy::WHICH_PROFILE, &options)? else {
            return Ok(None);
        };
        let profile = &content.profiles[chosen];

        let claiming = claiming(image, &content, profile, &index);
        let bring = match claiming.is_empty() {
            true => None,
            false => bring(root, &list, image, &claiming, prompt)?,
        };
        let brought: Vec<String> = match &bring {
            Some(_) => claiming.iter().map(|(_, path)| path.clone()).collect(),
            None => Vec::new(),
        };
        Ok(Some(Self {
            declaration: crate::conformance::Conforms::declaring(image, profile.name(), brought),
            bring,
        }))
    }

    pub fn apply(
        &self,
        root: &Path,
        prompt: &Prompt,
        say: crate::report::Say<'_>,
    ) -> Result<Vec<(PathBuf, Change)>, String> {
        let mut wrote = Vec::new();
        if let Some(bring) = &self.bring {
            let (added, kept) = bring.add.applied(root)?;
            wrote.extend(added);
            if !bring.copy.is_empty() {
                let request = crate::copy_component::Request {
                    paths: bring.copy.clone(),
                    from: None,
                    build: Some(bring.build.clone()),
                };
                let copied = crate::copy_component::Copy::collect(&request, root, prompt, say)
                    .map_err(|err| err.to_string())
                    .and_then(|copy| copy.map(|copy| copy.apply(root)).transpose());
                let failed = match &copied {
                    Ok(Some(_)) => Vec::new(),
                    Ok(None) | Err(_) => crate::init::restore(&kept),
                };
                match copied {
                    Ok(Some(copied)) => wrote.extend(copied),
                    Ok(None) if failed.is_empty() => return Ok(Vec::new()),
                    Ok(None) => {
                        return Err(crate::init::undone(
                            "the copy is cancelled, so the references it needed are undone"
                                .to_string(),
                            &failed,
                        ))
                    }
                    Err(err) => return Err(crate::init::undone(err, &failed)),
                }
            }
        }
        wrote.extend(self.declaration.apply(root)?);
        Ok(wrote)
    }
}

fn claiming(
    image: &crate::model::image::Image,
    content: &Content,
    profile: &crate::scap::Profile,
    index: &crate::provider::Index,
) -> Vec<(String, String)> {
    crate::scap::owed(image, content, profile, index)
        .helping
        .iter()
        .filter(|provider| {
            !image
                .entries
                .iter()
                .any(|entry| entry.path == provider.name)
        })
        .filter_map(|provider| Some((provider.owner.clone()?, provider.name.clone())))
        .collect()
}

fn bring(
    root: &Path,
    list: &crate::model::image::List,
    image: &crate::model::image::Image,
    claiming: &[(String, String)],
    prompt: &Prompt,
) -> Result<Option<Bring>, Error> {
    let options = [
        Choice::new(copy::ADD_REFERENCE, ""),
        Choice::new(copy::COPY_INTO_REPOSITORY, ""),
    ];
    let Some(how) = prompt.choose(copy::BRING_CLAIMING, &options)? else {
        return Ok(None);
    };
    let how = match how {
        0 => Bringing::Reference,
        _ => Bringing::Copy,
    };
    let remote = |name: &str| {
        list.libraries
            .iter()
            .find(|library| library.name == name)
            .and_then(|library| library.pin())
    };
    // Only a reference takes a pin. A copy reads the commit that the build already reads.
    let pin = match matches!(how, Bringing::Reference)
        && claiming
            .iter()
            .any(|(library, _)| remote(library).is_some())
    {
        true => Pin::ask(prompt)?,
        false => Pin::Follow,
    };
    let mut wanted: Vec<(Option<String>, String, Option<String>)> = Vec::new();
    for (library, path) in claiming {
        let pinned = match (remote(library), &pin) {
            (None, _) | (_, Pin::Follow) => None,
            (Some(_), Pin::Named(named)) => Some(named.clone()),
            (Some(evidence), Pin::Current) => {
                let url = evidence.url.as_deref().unwrap_or_default();
                Some(
                    crate::collection::cached_commit(root, url, evidence.selector()).ok_or_else(
                        || {
                            format!(
                                "the cache holds no commit of library `{library}`; `buildc generate` fetches it"
                            )
                        },
                    )?,
                )
            }
        };
        wanted.push((Some(library.clone()), path.clone(), pinned));
    }
    let target = image
        .component
        .strip_prefix("./")
        .unwrap_or(&image.component);
    Ok(Some(Bring {
        add: crate::add_component::Add::into_component(root, target, &wanted)?,
        copy: match how {
            Bringing::Reference => Vec::new(),
            Bringing::Copy => claiming
                .iter()
                .map(|(library, path)| format!("{library}/{path}"))
                .collect(),
        },
        build: image.id.clone(),
    }))
}

enum Bringing {
    Reference,
    Copy,
}

enum Pin {
    Follow,
    Current,
    Named(String),
}

impl Pin {
    fn ask(prompt: &Prompt) -> Result<Self, Error> {
        let options = [
            Choice::new(copy::PIN_FOLLOW, ""),
            Choice::new(copy::PIN_CURRENT, ""),
            Choice::new(copy::PIN_TAG, ""),
            Choice::new(copy::PIN_COMMIT, ""),
        ];
        Ok(match prompt.choose(copy::PIN, &options)? {
            None | Some(0) => Self::Follow,
            Some(1) => Self::Current,
            Some(at) => {
                let question = match at {
                    2 => copy::PIN_TAG_NAME,
                    _ => copy::PIN_COMMIT_NAME,
                };
                Self::Named(prompt.text(None, question, "an answer", None)?)
            }
        })
    }
}

pub const CLAIMS_BY_HAND: &str = "nothing can be asked here; write `satisfies { <benchmark> \
                                  \"<number>\" }` into the component, and `buildc scap content` \
                                  prints the datastream those numbers are read out of";

pub struct Claims {
    file: PathBuf,
    profile: String,
    numbers: Vec<String>,
    claimed: usize,
}

impl Claims {
    pub fn collect(
        root: &Path,
        named: &str,
        datastream: Option<PathBuf>,
        prompt: &Prompt,
        say: crate::report::Say<'_>,
    ) -> Result<Option<Self>, Error> {
        let file = layout::under(root, &layout::manifest(root, named));
        if !root.join(&file).is_file() {
            return Err(Error::Invocation(format!(
                "`{named}` is not a component this repository holds; there is no {}",
                file.display()
            )));
        }
        let summary = parse::component::summary(&root.join(&file), named);
        let family = match summary.targets.first() {
            Some(family) => family.as_str(),
            None if datastream.is_some() => "",
            None => {
                return Err(Error::Invocation(format!(
                    "`{named}` declares no `targets` restriction, so it has no default SCAP \
                     content; give `--datastream <file>`"
                )))
            }
        };
        let path = crate::scap::content_path("", family, datastream.as_deref())?;
        let content = crate::scap::content_of(&path)?;
        if content.profiles.is_empty() {
            return Err(format!("{} carries no profile to claim against", path.display()).into());
        }

        let options = profile_options(&content);
        let Some(chosen) = prompt.choose(copy::WHICH_PROFILE, &options)? else {
            return Ok(None);
        };
        let profile = &content.profiles[chosen];

        let selected = content.selected(&profile.id);
        let rules = claimable_rules(&content, &selected);
        if rules.is_empty() {
            return Err(format!(
                "no rule `{}` selects carries a number, so nothing can be claimed against it",
                profile.name()
            )
            .into());
        }
        match selected.len() - rules.len() {
            0 => {}
            1 => say(&format!(
                "One of the rules `{}` selects is reached by no number of its own, so no \
                 `satisfies` can name it.\n\n",
                profile.name()
            )),
            missed => say(&format!(
                "{missed} of the rules `{}` selects are reached by no number of their own, so no \
                 `satisfies` can name them.\n\n",
                profile.name()
            )),
        }

        let first = |at: usize| rules[at].1.iter().next().expect("a number reaches it");
        let options = claim_options(&content, &rules);
        let held = crate::scap::reached(&content, summary.satisfies.iter());
        let on: Vec<usize> = (0..rules.len())
            .filter(|at| held.contains(rules[*at].0))
            .collect();

        let Answer::Chosen(chosen) =
            prompt.choose_many(&copy::claimed_rules(named), &options, &on)?
        else {
            return Ok(None);
        };

        let offered: BTreeSet<&String> = rules
            .iter()
            .flat_map(|(_, numbers)| numbers.iter())
            .collect();
        // A number that this profile offers no rule for stays. A claim written under another profile is never dropped.
        let mut numbers: Vec<String> = summary
            .satisfies
            .iter()
            .filter(|number| !offered.contains(number))
            .cloned()
            .collect();
        numbers.extend(chosen.iter().map(|at| first(*at).clone()));
        numbers.sort_by_key(|number| (ordinal(number), number.clone()));
        numbers.dedup();
        Ok(Some(Self {
            file,
            profile: profile.name().to_string(),
            numbers,
            claimed: chosen.len(),
        }))
    }

    pub fn apply(&self, root: &Path) -> Result<Vec<(PathBuf, Change)>, String> {
        let file = root.join(&self.file);
        let text =
            std::fs::read_to_string(&file).map_err(|err| format!("{}: {err}", file.display()))?;
        let out = self
            .spliced(&text)
            .map_err(|err| format!("{} {err}", file.display()))?;
        std::fs::write(&file, out).map_err(|err| format!("{}: {err}", file.display()))?;
        Ok(vec![(
            self.file.clone(),
            Change::Updated(match self.claimed {
                0 => format!("claiming no rule of `{}`", self.profile),
                1 => format!("claiming one rule of `{}`", self.profile),
                many => format!("claiming {many} rules of `{}`", self.profile),
            }),
        )])
    }

    fn spliced(&self, text: &str) -> Result<String, String> {
        splice(text, parse::component::satisfies_span(text), &self.block())
    }

    fn block(&self) -> String {
        match self.numbers.is_empty() {
            true => String::new(),
            false => format!(
                "satisfies {{\n    {} {}\n}}",
                self.profile,
                self.numbers
                    .iter()
                    .map(|number| format!("\"{number}\""))
                    .collect::<Vec<String>>()
                    .join(" \\\n        ")
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn written(name: &str, text: &str, library: &Library) -> String {
        let root = std::env::temp_dir().join(format!("buildc-set-{name}-{}", std::process::id()));
        crate::init::put(&root.join(layout::REPO_FILE), text).unwrap();
        let wrote = library
            .apply(&root)
            .expect("the repository file is writable");
        assert_eq!(wrote[0].0, PathBuf::from(layout::REPO_FILE));
        let out = std::fs::read_to_string(root.join(layout::REPO_FILE)).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        out
    }

    #[test]
    fn a_library_that_check_reports_without_blocking_an_edit_stays_written() {
        let root = std::env::temp_dir().join(format!("buildc-set-reported-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        crate::init::put(&root.join(layout::REPO_FILE), "schema-version 1\n").unwrap();
        std::fs::create_dir_all(root.join("mine")).unwrap();
        let library = Library {
            name: "mine".to_string(),
            source: "mine".to_string(),
            kind: Kind::Remote,
            root_library: None,
            creates_components: false,
        };
        let wrote = library.apply(&root);
        let text = std::fs::read_to_string(root.join(layout::REPO_FILE)).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert!(wrote.is_ok());
        assert!(text.contains("mine { source \"mine\" }"), "{text}");
    }

    #[test]
    fn a_library_whose_write_fails_is_undone() {
        let root = std::env::temp_dir().join(format!("buildc-set-undone-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let unclosed = "schema-version 1\nlibraries {\n";
        crate::init::put(&root.join(layout::REPO_FILE), unclosed).unwrap();
        let library = Library {
            name: "mine".to_string(),
            source: "https://example.invalid/mine".to_string(),
            kind: Kind::Remote,
            root_library: Some(layout::ROOT_LIBRARY.to_string()),
            creates_components: true,
        };
        let refused = library.apply(&root);
        let text = std::fs::read_to_string(root.join(layout::REPO_FILE)).unwrap();
        let folder = layout::components(&root).exists();
        std::fs::remove_dir_all(&root).unwrap();
        assert!(refused.is_err());
        assert_eq!(text, unclosed);
        assert!(!folder);
    }

    #[test]
    fn a_library_is_written_under_its_block() {
        let library = Library {
            name: "mine".to_string(),
            source: "https://example.invalid/mine".to_string(),
            kind: Kind::Remote,
            root_library: None,
            creates_components: false,
        };
        assert_eq!(
            library.summary(),
            "  location  https://example.invalid/mine\n  name      mine\n  kind      remote\n"
        );
        let out = written("new", "schema-version 1\n", &library);
        assert_eq!(
            out,
            "schema-version 1\n\nlibraries {\n    remote {\n        mine { source \"https://example.invalid/mine\" }\n    }\n}\n"
        );
    }

    #[test]
    fn a_one_line_libraries_block_takes_a_library_beside_the_other_block() {
        let library = Library {
            name: "core".to_string(),
            source: "https://example.invalid/core".to_string(),
            kind: Kind::Remote,
            root_library: None,
            creates_components: false,
        };
        let out = written(
            "one-line",
            "schema-version 1\n\nlibraries { local { one { source \"one\" } } }\n",
            &library,
        );
        let issues = crate::parse::schema::check_text(&out, &crate::parse::repo::REPO, false)
            .unwrap_or_else(|err| panic!("{err}: {out:?}"));
        assert!(issues.is_empty(), "{out}\n{}", issues.plain());
        let held = crate::parse::repo::libraries_in(&out);
        assert_eq!(held.len(), 2, "{out}");
        assert!(held[1].pin().is_some(), "{out}");
    }

    #[test]
    fn a_library_beside_an_inferred_root_declares_the_root_first() {
        let library = Library {
            name: "mine".to_string(),
            source: "mine".to_string(),
            kind: Kind::Local,
            root_library: Some("local-library".to_string()),
            creates_components: false,
        };
        assert!(library
            .summary()
            .contains("`local-library` declares this repository"));
        assert_eq!(
            written("root-new", "name \"Example\"\n", &library),
            "name \"Example\"\n\nlibraries default=\"local-library\" {\n    local {\n        local-library { source \".\" }\n        mine { source \"mine\" }\n    }\n}\n"
        );
        assert_eq!(
            written(
                "root-beside",
                "name \"Example\"\nlibraries {\n    remote {\n        buildc { source \"https://example.invalid/library\" }\n    }\n}\n",
                &library,
            ),
            "name \"Example\"\nlibraries default=\"local-library\" {\n    remote {\n        buildc { source \"https://example.invalid/library\" }\n    }\n    local {\n        local-library { source \".\" }\n        mine { source \"mine\" }\n    }\n}\n"
        );
    }

    #[test]
    fn opening_the_libraries_block_offers_to_create_the_local_library() {
        let buildc = DefaultLibrary {
            name: "buildc".to_string(),
            source: "https://example.invalid/library".to_string(),
            about: String::new(),
        };
        let run = |name: &str, answers: [&str; 2]| {
            let root =
                std::env::temp_dir().join(format!("buildc-set-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            crate::init::put(&root.join(layout::REPO_FILE), "name \"Example\"\n").unwrap();
            let prompt = Prompt::scripted(answers.map(str::to_string).to_vec());
            let library = Library::adding(&root, vec![buildc.clone()], None, None, &prompt)
                .unwrap()
                .expect("the answers choose a library");
            let wrote: Vec<PathBuf> = library
                .apply(&root)
                .unwrap()
                .into_iter()
                .map(|(path, _)| path)
                .collect();
            let text = std::fs::read_to_string(root.join(layout::REPO_FILE)).unwrap();
            std::fs::remove_dir_all(&root).unwrap();
            (wrote, text)
        };
        let (wrote, text) = run("create-yes", ["1", "yes"]);
        assert_eq!(
            wrote,
            [
                PathBuf::from("components/.gitkeep"),
                PathBuf::from(layout::REPO_FILE)
            ]
        );
        assert!(text.contains("libraries default=\"local-library\" {\n    local {\n        local-library { source \".\" }"), "{text}");
        let (wrote, text) = run("create-no", ["1", "no"]);
        assert_eq!(wrote, [PathBuf::from(layout::REPO_FILE)]);
        assert!(!text.contains("local-library"), "{text}");
    }

    #[test]
    fn the_root_is_inferred_only_while_it_holds_a_library_folder_and_no_local_library_is_declared()
    {
        let root = std::env::temp_dir().join(format!("buildc-set-inferred-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        assert_eq!(inferred_root(&root, &[]), None);
        for folder in ["components", "workflows", "plugins"] {
            std::fs::create_dir_all(root.join(folder)).unwrap();
            assert_eq!(
                inferred_root(&root, &[]).as_deref(),
                Some(layout::ROOT_LIBRARY),
                "{folder}"
            );
            std::fs::remove_dir_all(root.join(folder)).unwrap();
        }
        std::fs::create_dir_all(root.join("components")).unwrap();
        let local = parse::repo::libraries_in("libraries {\n    local {\n        mine\n    }\n}\n");
        assert_eq!(inferred_root(&root, &local), None);
        let remote = parse::repo::libraries_in(
            "libraries {\n    remote {\n        buildc { source \"https://example.invalid/library\" }\n    }\n}\n",
        );
        assert!(inferred_root(&root, &remote).is_some());
        std::fs::remove_dir_all(&root).unwrap();
    }

    fn git(root: &Path, args: &[&str]) {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(root)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[test]
    fn a_library_location_sets_its_kind_and_its_default_name() {
        let root = std::env::temp_dir().join(format!("buildc-set-kind-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        crate::init::put(&root.join(layout::REPO_FILE), "schema-version 1\n").unwrap();
        crate::init::put(&root.join("shared/a/component.kdl"), "").unwrap();
        crate::init::put(&root.join("loose/a/component.kdl"), "").unwrap();
        crate::init::put(&root.join("named/repo.kdl"), "name \"Team Library\"\n").unwrap();
        std::fs::create_dir_all(root.join("components")).unwrap();
        git(&root, &["init", "--quiet"]);
        git(&root, &["add", "shared"]);
        git(&root.join("loose"), &["init", "--quiet"]);
        git(&root.join("named"), &["init", "--quiet"]);
        let buildc = DefaultLibrary {
            name: "buildc".to_string(),
            source: "https://example.invalid/library".to_string(),
            about: "Feature components".to_string(),
        };
        let answer = |defaults: &[DefaultLibrary], answers: &[&str]| {
            let prompt = Prompt::scripted(answers.iter().map(|a| a.to_string()).collect());
            Library::adding(&root, defaults.to_vec(), None, None, &prompt)
                .expect("the answers are complete")
                .map(|library| (library.name, library.source, library.kind))
        };
        assert_eq!(
            answer(std::slice::from_ref(&buildc), &["1"]),
            Some(("buildc".into(), buildc.source.clone(), Kind::Remote))
        );
        assert_eq!(
            answer(std::slice::from_ref(&buildc), &["2", "shared", "", ""]),
            Some(("shared".into(), "shared".into(), Kind::Local))
        );
        assert_eq!(
            answer(&[], &["loose", "team", ""]),
            Some(("team".into(), "loose".into(), Kind::Remote))
        );
        assert_eq!(
            answer(&[], &["named", "", ""]),
            Some(("team-library".into(), "named".into(), Kind::Remote))
        );
        assert_eq!(answer(&[], &["shared", "", "cancel"]), None);
        for refused in [["http://example.invalid/x", ""], ["shared", "Bad Name"]] {
            let prompt = Prompt::scripted(refused.map(str::to_string).to_vec());
            assert!(Library::adding(&root, Vec::new(), None, None, &prompt).is_err());
        }
        std::fs::remove_dir_all(&root).unwrap();
    }

    fn arguments_repo(case: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("buildc-set-args-{case}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        crate::init::put(&root.join(layout::REPO_FILE), "schema-version 1\n").unwrap();
        root
    }

    fn defaults() -> Vec<DefaultLibrary> {
        vec![DefaultLibrary {
            name: "buildc".to_string(),
            source: "https://example.invalid/library".to_string(),
            about: String::new(),
        }]
    }

    #[test]
    fn a_name_alone_adds_that_default_library_without_a_terminal() {
        let root = arguments_repo("default");
        let library = Library::adding(
            &root,
            defaults(),
            Some("buildc".into()),
            None,
            &Prompt::silent(),
        )
        .ok()
        .flatten()
        .expect("the default library is added");
        let wrote = library.apply(&root);
        let text = std::fs::read_to_string(root.join(layout::REPO_FILE)).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert!(wrote.is_ok());
        assert!(
            text.contains("buildc { source \"https://example.invalid/library\" }"),
            "{text}"
        );
    }

    #[test]
    fn a_name_alone_asks_nothing_and_creates_no_components_folder() {
        let root = arguments_repo("no-folder");
        let library = Library::adding(
            &root,
            defaults(),
            Some("buildc".into()),
            None,
            &Prompt::scripted(Vec::new()),
        );
        let created = layout::components(&root).exists();
        std::fs::remove_dir_all(&root).unwrap();
        let library = library
            .ok()
            .flatten()
            .expect("the default library is added");
        assert!(!library.creates_components);
        assert_eq!(library.root_library, None);
        assert!(!created);
    }

    #[test]
    fn a_source_with_a_name_asks_nothing() {
        let root = arguments_repo("source");
        let library = Library::adding(
            &root,
            defaults(),
            Some("mine".into()),
            Some("https://example.invalid/org/mine".into()),
            &Prompt::silent(),
        )
        .ok()
        .flatten()
        .expect("the library at the source is added");
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(library.name, "mine");
        assert_eq!(library.kind, Kind::Remote);
    }

    #[test]
    fn a_source_that_derives_no_name_is_refused_naming_the_argument() {
        let root = arguments_repo("nameless");
        let refused = Library::adding(
            &root,
            defaults(),
            None,
            Some("Not_A_Name".into()),
            &Prompt::silent(),
        );
        std::fs::remove_dir_all(&root).unwrap();
        assert!(
            matches!(refused, Err(Error::Invocation(ref why)) if why.contains("buildc set library <name> --source")),
        );
    }

    #[test]
    fn a_source_that_is_no_https_url_is_refused_before_any_fetch() {
        let root = arguments_repo("ssh");
        let refused = Library::adding(
            &root,
            defaults(),
            None,
            Some("ssh://git@example.invalid/org/lib".into()),
            &Prompt::silent(),
        );
        let cached = root.join(".cache").exists();
        std::fs::remove_dir_all(&root).unwrap();
        assert!(
            matches!(refused, Err(Error::Operation(ref why)) if why.contains("not an HTTPS Git URL"))
        );
        assert!(!cached);
    }

    #[test]
    fn a_remote_folder_that_is_no_git_checkout_is_refused_before_any_write() {
        let root = arguments_repo("plain");
        std::fs::create_dir_all(root.join("plain")).unwrap();
        let refused = Library::adding(
            &root,
            defaults(),
            Some("mine".into()),
            Some("plain".into()),
            &Prompt::silent(),
        );
        let text = std::fs::read_to_string(root.join(layout::REPO_FILE)).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert!(
            matches!(refused, Err(Error::Operation(ref why)) if why.contains("is not a Git checkout"))
        );
        assert_eq!(text, "schema-version 1\n");
    }

    #[test]
    fn no_argument_and_no_terminal_is_refused() {
        let root = arguments_repo("silent");
        let refused = Library::adding(&root, defaults(), None, None, &Prompt::silent());
        let text = std::fs::read_to_string(root.join(layout::REPO_FILE)).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert!(
            matches!(refused, Err(Error::Invocation(ref why)) if why.contains("--source <url or path>"))
        );
        assert_eq!(text, "schema-version 1\n");
    }

    #[test]
    fn a_name_that_is_no_default_library_is_refused_naming_the_defaults() {
        let root = arguments_repo("unknown");
        let refused = Library::adding(
            &root,
            defaults(),
            Some("other".into()),
            None,
            &Prompt::silent(),
        );
        std::fs::remove_dir_all(&root).unwrap();
        assert!(
            matches!(refused, Err(Error::Invocation(ref why)) if why.contains("the libraries buildc offers are `buildc`"))
        );
    }

    #[test]
    fn a_declared_name_is_refused_and_a_declared_default_is_not_offered() {
        let root = std::env::temp_dir().join(format!("buildc-set-declared-{}", std::process::id()));
        crate::init::put(
            &root.join(layout::REPO_FILE),
            "libraries {\n    remote {\n        buildc { source \"https://example.invalid/library\" }\n    }\n}\n",
        )
        .unwrap();
        let buildc = DefaultLibrary {
            name: "buildc".to_string(),
            source: "https://example.invalid/library".to_string(),
            about: String::new(),
        };
        let prompt = Prompt::scripted(["../buildc", "", ""].map(str::to_string).to_vec());
        let refused = Library::adding(&root, vec![buildc], None, None, &prompt);
        std::fs::remove_dir_all(&root).unwrap();
        assert!(
            matches!(refused, Err(Error::Operation(ref why)) if why.contains("already a library")),
            "the picker is skipped, so the location is the first answer"
        );
    }

    #[test]
    fn a_library_name_is_derived_from_the_owner_or_the_folder() {
        assert_eq!(
            publisher("https://github.com/buildc-org/library").as_deref(),
            Some("buildc-org")
        );
        assert_eq!(
            publisher("https://github.com/Me/tools").as_deref(),
            Some("me")
        );
        assert_eq!(publisher("../my-library/").as_deref(), Some("my-library"));
        assert_eq!(publisher("https://example.invalid"), None);
        assert_eq!(publisher("../9lives"), None);
    }

    #[test]
    fn a_number_that_does_not_nest_goes_in_the_one_flat_branch() {
        assert_eq!(group("1.1.1.1"), "1.1.1");
        assert_eq!(group("5.2"), "5");
        assert_eq!(group("RHEL-09-232010"), "other");
    }

    fn claims(profile: &str, numbers: &[&str]) -> Claims {
        Claims {
            file: PathBuf::from("components/one/component.kdl"),
            profile: profile.to_string(),
            numbers: numbers.iter().map(|n| n.to_string()).collect(),
            claimed: numbers.len(),
        }
    }

    #[test]
    fn base_agnostic_claims_need_a_datastream() {
        let root = std::env::temp_dir().join(format!("buildc-claims-any-{}", std::process::id()));
        let component = root.join("components/editor");
        std::fs::create_dir_all(&component).unwrap();
        std::fs::write(
            component.join(layout::MANIFEST_FILE),
            "schema-version 1\n\npackages \"nano\"\n",
        )
        .unwrap();

        let result = Claims::collect(&root, "editor", None, &Prompt::silent(), &mut |_| {});
        let Err(err) = result else {
            panic!("base-agnostic claims unexpectedly chose content");
        };
        assert!(
            err.message().contains("has no default SCAP content")
                && err.message().contains("--datastream <file>"),
            "{}",
            err.message()
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_claims_replace_the_block_that_was_there() {
        assert_eq!(
            claims("ospp", &["1.1.1.1", "1.1.1.2"]).spliced(
                "description \"one\"\n\nsatisfies {\n    ospp \"9.9\"\n}\n\npackages \"curl\"\n"
            )
            .unwrap(),
            "description \"one\"\n\nsatisfies {\n    ospp \"1.1.1.1\" \\\n        \"1.1.1.2\"\n}\n\npackages \"curl\"\n"
        );
    }

    #[test]
    fn a_component_with_no_block_gets_one_at_the_end() {
        assert_eq!(
            claims("ospp", &["1.1.1.1"])
                .spliced("description \"one\"\n")
                .unwrap(),
            "description \"one\"\n\nsatisfies {\n    ospp \"1.1.1.1\"\n}\n"
        );
    }

    #[test]
    fn claiming_nothing_takes_the_block_and_its_blank_line() {
        assert_eq!(
            claims("ospp", &[])
                .spliced("description \"one\"\n\nsatisfies {\n    ospp \"9.9\"\n}\n")
                .unwrap(),
            "description \"one\"\n"
        );
    }

    #[test]
    fn an_unnamed_benchmark_comes_back_named_after_the_profile() {
        let manifest = "description \"one\"\n\nsatisfies {\n    \"\" \"1.1.1.1\"\n}\n";
        let dir = std::env::temp_dir().join(format!("buildc-claims-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join(layout::MANIFEST_FILE);
        std::fs::write(&file, manifest).unwrap();

        let held = parse::component::summary(&file, "one").satisfies;
        assert_eq!(held, ["1.1.1.1"]);
        std::fs::remove_dir_all(dir).unwrap();

        let carried: Vec<&str> = held.iter().map(String::as_str).collect();
        assert_eq!(
            claims("ospp", &carried).spliced(manifest).unwrap(),
            "description \"one\"\n\nsatisfies {\n    ospp \"1.1.1.1\"\n}\n"
        );
    }

    #[test]
    fn profile_and_filtered_claim_screens_snapshot_datastream_content() {
        use crate::screen_tests::{assert_screen, screen};

        let content = crate::scap::content_of(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/scap/datastream.xml"),
        )
        .unwrap();
        let profiles = profile_options(&content);
        assert_screen(
            "profile-picker",
            screen().picker(copy::WHICH_PROFILE, &profiles, None, "enter confirms", 0),
        );

        let profile = content
            .profiles
            .iter()
            .find(|profile| profile.name() == "standard")
            .expect("the fixture carries the standard profile");
        let selected = content.selected(&profile.id);
        let rules = claimable_rules(&content, &selected);
        let options = claim_options(&content, &rules);
        let filtered: Vec<Choice> = options
            .into_iter()
            .filter(|option| option.label.contains("aide"))
            .collect();
        assert_eq!(filtered.len(), 1, "the aide filter identifies one rule");
        assert_screen(
            "claims-filtered-tree",
            screen().picker(
                &copy::claimed_rules("sshd"),
                &filtered,
                Some(&[0]),
                "filter, space toggles, enter confirms",
                0,
            ),
        );
    }
}
