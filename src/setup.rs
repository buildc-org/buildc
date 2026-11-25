use crate::copy;
use crate::error::Error;
use crate::layout;
use crate::parse;
use crate::parse::declaration::{append, placed, wrap};
use crate::resolve::workflow::{Basis, Scope, Shipped, SHIPPED};
use common::prompt::Prompt;
use common::ui::tree::Change;
use common::ui::{Answer, Choice};
use std::path::{Path, PathBuf};

/// The variant order is the order that setup asks its fields. Re-entry keeps every answer before the field that the user resumes at.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Field {
    Name,
    Setup,
    Provider,
    Remote,
    Libraries,
    Image,
    Base,
    Workflows,
    Scripts,
}

#[derive(Clone, Default)]
pub struct Libraries {
    pub(crate) chosen: Vec<crate::model::remote::DefaultLibrary>,
}

impl Libraries {
    pub fn every(assets: &Path) -> Vec<crate::model::remote::DefaultLibrary> {
        crate::collection::defaults(assets)
    }

    pub fn collect(assets: &Path, held: Option<&Self>, prompt: &Prompt) -> Result<Self, Error> {
        let every = Self::every(assets);
        if every.is_empty() {
            return Ok(Self::default());
        }
        let options: Vec<Choice> = every
            .iter()
            .map(|library| Choice::new(library.name.clone(), library.about.clone()))
            .collect();
        let on: Vec<usize> = match held {
            Some(held) => every
                .iter()
                .enumerate()
                .filter(|(_, library)| held.chosen.iter().any(|held| held.name == library.name))
                .map(|(at, _)| at)
                .collect(),
            None => (0..every.len()).collect(),
        };
        let Answer::Chosen(chosen) = prompt.choose_many(copy::LIBRARIES, &options, &on)? else {
            unreachable!("choose_many answers with a choice or fails")
        };
        Ok(Self {
            chosen: chosen.into_iter().map(|at| every[at].clone()).collect(),
        })
    }

    pub(crate) fn row(&self) -> (Field, &'static str, String) {
        let mut aliases: Vec<&str> = Vec::new();
        for library in &self.chosen {
            if !aliases.contains(&library.name.as_str()) {
                aliases.push(&library.name);
            }
        }
        (
            Field::Libraries,
            copy::ROW_LIBRARIES,
            match aliases.is_empty() {
                true => copy::NONE.to_string(),
                false => aliases.join(", "),
            },
        )
    }
}

pub enum Listing {
    Cancelled,
    NoImage,
    Declined { asked: bool },
    In(Vec<Listed>),
}

pub struct Listed {
    file: PathBuf,
    image: String,
    scope: Option<String>,
}

impl Listed {
    fn of(list: &crate::model::image::List, target: &crate::model::image::Target) -> Self {
        let image = list
            .images
            .iter()
            .find(|image| image.id == target.image)
            .expect("a target names an image the list holds");
        Self {
            file: PathBuf::from(image.src.name()),
            image: image.id.clone(),
            scope: list.scope(&image.id).map(str::to_string),
        }
    }
}

impl Listing {
    pub fn collect(root: &Path, given: Vec<String>, prompt: &Prompt) -> Result<Self, Error> {
        let list = crate::model::image::List::editable(root).map_err(|issues| issues.plain())?;
        Self::collect_from(&list, given, prompt)
    }

    pub fn target(root: &Path, given: Option<String>, prompt: &Prompt) -> Result<Self, Error> {
        let list = crate::model::image::List::editable(root).map_err(|issues| issues.plain())?;
        Self::target_from(&list, given.as_deref(), copy::TARGET_IMAGE, prompt)
    }

    pub(crate) fn selected(
        list: &crate::model::image::List,
        targets: &[crate::model::image::Target],
    ) -> Self {
        Self::In(
            targets
                .iter()
                .map(|target| Listed::of(list, target))
                .collect(),
        )
    }

    pub(crate) fn target_from(
        list: &crate::model::image::List,
        given: Option<&str>,
        title: &str,
        prompt: &Prompt,
    ) -> Result<Self, Error> {
        if list.images.is_empty() {
            return Ok(Self::NoImage);
        }
        let targets = list.targets();
        let at = match given {
            Some(name) => targets
                .iter()
                .position(|known| known.to_string() == name)
                .ok_or_else(|| {
                    let named: Vec<String> = targets.iter().map(ToString::to_string).collect();
                    format!(
                        "`{name}` is not a declared build; there is {}",
                        named.join(", ")
                    )
                })?,
            None => {
                let mut rows = target_rows(&targets);
                rows.push(Choice::new(copy::TARGET_NONE, ""));
                match prompt.choose(title, &rows)? {
                    None => return Ok(Self::Declined { asked: false }),
                    Some(at) if at == targets.len() => return Ok(Self::Declined { asked: true }),
                    Some(at) => at,
                }
            }
        };
        Ok(Self::In(vec![Listed::of(list, &targets[at])]))
    }

    pub(crate) fn collect_from(
        list: &crate::model::image::List,
        given: Vec<String>,
        prompt: &Prompt,
    ) -> Result<Self, Error> {
        if list.images.is_empty() {
            return Ok(Self::NoImage);
        }
        let targets = list.targets();
        let named: Vec<String> = targets.iter().map(ToString::to_string).collect();

        let chosen: Vec<usize> = match given.is_empty() {
            true => match ask(&targets, prompt)? {
                common::ui::Answer::Cancelled => return Ok(Self::Cancelled),
                common::ui::Answer::Chosen(chosen) => chosen,
            },
            false => given
                .iter()
                .map(|name| {
                    named.iter().position(|known| known == name).ok_or_else(|| {
                        format!(
                            "`{name}` is not a declared build; there is {}",
                            named.join(", ")
                        )
                    })
                })
                .collect::<Result<_, _>>()?,
        };
        let mut unique = Vec::new();
        for at in chosen {
            if !unique.contains(&at) {
                unique.push(at);
            }
        }
        if unique.is_empty() {
            return Ok(Self::Declined {
                asked: prompt.asks(),
            });
        }
        let chosen: Vec<crate::model::image::Target> =
            unique.iter().map(|at| targets[*at].clone()).collect();
        Ok(Self::selected(list, &chosen))
    }

    pub(crate) fn pick_from(
        list: &crate::model::image::List,
        prompt: &Prompt,
    ) -> Result<Self, Error> {
        if list.images.is_empty() {
            return Ok(Self::NoImage);
        }
        if !prompt.asks() {
            return Ok(Self::Declined { asked: false });
        }
        let targets = list.targets();
        match prompt.choose_many(copy::LIST_IN_BUILDS, &target_rows(&targets), &[])? {
            Answer::Cancelled => Ok(Self::Cancelled),
            Answer::Chosen(at) if at.is_empty() => Ok(Self::Declined { asked: true }),
            Answer::Chosen(at) => {
                let chosen: Vec<crate::model::image::Target> =
                    at.iter().map(|at| targets[*at].clone()).collect();
                Ok(Self::selected(list, &chosen))
            }
        }
    }

    pub fn refuse_duplicate(
        &self,
        list: &crate::model::image::List,
        name: &str,
    ) -> Result<(), String> {
        let Self::In(listed) = self else {
            return Ok(());
        };
        match listed.iter().find(|into| holds(list, into, name).is_some()) {
            Some(into) => Err(format!("`{}` already lists `{name}`", into.image)),
            None => Ok(()),
        }
    }

    pub(crate) fn child(&self, name: &str) -> Result<String, String> {
        let Self::In(listed) = self else {
            return Ok(name.to_string());
        };
        let mut paths: Vec<(String, &str)> = Vec::new();
        for target in listed {
            let path = crate::model::image::child(target.scope.as_deref(), name);
            if !paths.iter().any(|(held, _)| *held == path) {
                paths.push((path, &target.image));
            }
        }
        match paths.as_slice() {
            [] => Ok(name.to_string()),
            [(path, _)] => Ok(path.clone()),
            [(one, first), (two, second), ..] => Err(format!(
                "a bare `{name}` reads `{one}` in `{first}` and `{two}` in `{second}`; name the \
                 builds that read it from one folder"
            )),
        }
    }

    pub fn cancelled(&self) -> bool {
        matches!(self, Self::Cancelled)
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        let Self::In(listed) = self else {
            return Ok(());
        };
        for target in listed {
            let text = std::fs::read_to_string(&target.file)
                .map_err(|err| format!("{}: {err}", target.file.display()))?;
            if crate::parse::image::block_close(&text, &[]).is_none() {
                return Err(format!(
                    "{} is not valid KDL, so `{}` cannot be edited",
                    target.file.display(),
                    target.image
                ));
            }
        }
        Ok(())
    }

    pub fn images(&self) -> Vec<&str> {
        let Self::In(listed) = self else {
            return Vec::new();
        };
        let mut out: Vec<&str> = Vec::new();
        for target in listed {
            if !out.contains(&target.image.as_str()) {
                out.push(&target.image);
            }
        }
        out
    }

    pub(crate) fn apply_children(
        &self,
        list: &crate::model::image::List,
        declarations: &[(&str, &str)],
    ) -> Result<Vec<(PathBuf, Change)>, String> {
        match self {
            Self::Cancelled => Ok(Vec::new()),
            Self::NoImage => {
                println!("no image lists it yet; `buildc create image <name>` writes one");
                Ok(Vec::new())
            }
            Self::Declined { .. } => {
                for (name, block) in declarations {
                    let (chain, line) = placed(true, name, block);
                    let shown = wrap(&chain, &line);
                    println!(
                        "next, to build it, list it in an image:\n {}",
                        shown.replace('\n', "\n ")
                    );
                }
                Ok(Vec::new())
            }
            Self::In(listed) => {
                let mut wrote: Vec<(PathBuf, Vec<String>)> = Vec::new();
                for target in listed {
                    let mut taken: Vec<String> = Vec::new();
                    for (name, block) in declarations {
                        let path = crate::model::image::child(target.scope.as_deref(), name);
                        if holds(list, target, &path).is_some() {
                            continue;
                        }
                        let (chain, line) = placed(true, name, block);
                        append(&target.file, &chain, &line)?;
                        taken.push((*name).to_string());
                    }
                    if taken.is_empty() {
                        continue;
                    }
                    let Some((_, added)) = wrote.iter_mut().find(|(file, _)| *file == target.file)
                    else {
                        wrote.push((target.file.clone(), taken));
                        continue;
                    };
                    for name in taken {
                        if !added.contains(&name) {
                            added.push(name);
                        }
                    }
                }
                Ok(wrote
                    .into_iter()
                    .map(|(file, added)| {
                        (
                            file,
                            Change::Updated(format!("{} added to components", crate::said(&added))),
                        )
                    })
                    .collect())
            }
        }
    }
}

fn holds<'a>(
    list: &'a crate::model::image::List,
    into: &Listed,
    name: &str,
) -> Option<&'a crate::model::image::Entry> {
    let image = list.images.iter().find(|image| image.id == into.image)?;
    image.entries.iter().find(|entry| entry.path == name)
}

fn ask(
    targets: &[crate::model::image::Target],
    prompt: &Prompt,
) -> Result<common::ui::Answer, Error> {
    if let [only] = targets {
        let listed = prompt.confirm(&copy::list_in(&only.to_string()), copy::YES, copy::NO)?;
        return Ok(common::ui::Answer::Chosen(match listed {
            true => vec![0],
            false => Vec::new(),
        }));
    }
    Ok(prompt.choose_many(copy::LIST_IN_BUILDS, &target_rows(targets), &[])?)
}

fn target_rows(targets: &[crate::model::image::Target]) -> Vec<Choice> {
    targets
        .iter()
        .map(|target| Choice::new(target.to_string(), ""))
        .collect()
}

// The image build runs after the Renovate window.
const SCAFFOLD: [(&str, &str); 4] = [
    (
        "image-build",
        "image-build {\n    pr\n    push\n    schedule frequency=\"daily\" time=\"12:30\"\n}",
    ),
    ("smoke-test", "smoke-test { schedule frequency=\"weekly\" }"),
    (
        "update-components",
        "update-components { schedule frequency=\"weekly\" time=\"05:30\" }",
    ),
    ("cleanup-registry", "cleanup-registry"),
];

#[derive(Clone)]
pub struct Workflows {
    chosen: Vec<&'static str>,
    entries: Vec<(&'static str, String)>,
}

impl Workflows {
    pub fn sensible() -> Self {
        Self {
            chosen: SCAFFOLD.iter().map(|(stem, _)| *stem).collect(),
            entries: Vec::new(),
        }
        .scaffolded()
    }

    pub fn scaffolded(mut self) -> Self {
        self.entries = SCAFFOLD
            .iter()
            .filter(|(stem, _)| self.chosen.contains(stem))
            .map(|(stem, entry)| (*stem, entry.to_string()))
            .collect();
        self
    }

    pub fn adding(list: &crate::model::image::List, more: &[&'static str]) -> Self {
        Self {
            chosen: SHIPPED
                .iter()
                .filter(|shipped| {
                    more.contains(&shipped.stem)
                        || list.workflows.iter().any(|w| w.name == shipped.stem)
                })
                .map(|shipped| shipped.stem)
                .collect(),
            entries: Vec::new(),
        }
    }

    pub fn named(
        text: &str,
        list: &crate::model::image::List,
        stem: &str,
        on: &[String],
        builds: &[String],
    ) -> Result<Self, Error> {
        let shipped = SHIPPED
            .iter()
            .find(|shipped| shipped.stem == stem)
            .ok_or_else(|| {
                let have: Vec<&str> = SHIPPED.iter().map(|shipped| shipped.stem).collect();
                Error::Invocation(format!(
                    "`{stem}` is not a workflow buildc ships (have: {})",
                    have.join(" ")
                ))
            })?;
        if !shipped.met(&Basis::of(list)) {
            return Err(Error::Operation(format!(
                "`{stem}` {}",
                shipped.needs.unmet()
            )));
        }
        let mut triggers: Vec<String> = Vec::new();
        for given in on {
            let trigger = match given.split_once('=') {
                Some(("tag", pattern)) if !pattern.is_empty() => format!(
                    "tag {:?}",
                    parse::declaration::quotable(pattern).map_err(Error::Invocation)?
                ),
                None if ["push", "pr", "schedule"].contains(&given.as_str()) => given.clone(),
                _ => {
                    return Err(Error::Invocation(format!(
                        "`--on {given}` is not a trigger; `--on` takes `push`, `pr`, `schedule` \
                         or `tag=<pattern>`"
                    )))
                }
            };
            let kind = trigger.split(' ').next().unwrap_or_default();
            if triggers
                .iter()
                .any(|held| held.split(' ').next() == Some(kind))
            {
                return Err(Error::Invocation(format!(
                    "`--on {kind}` is given twice; a workflow runs once on each trigger"
                )));
            }
            triggers.push(trigger);
        }
        if !builds.is_empty() && shipped.scope == Scope::Repository {
            return Err(Error::Invocation(format!(
                "`{stem}` runs once for the repository, so it takes no `--builds`"
            )));
        }
        let declared: Vec<&str> = list
            .builds
            .iter()
            .map(|build| build.name.as_str())
            .collect();
        if let Some(unknown) = builds
            .iter()
            .find(|build| !declared.contains(&build.as_str()))
        {
            return Err(Error::Invocation(format!(
                "`{unknown}` is not a build (have: {})",
                declared.join(" ")
            )));
        }
        let entry = parse::repo::workflow_entry(text, shipped.stem, &triggers, builds);
        let listed: Vec<&str> = list.workflows.iter().map(|w| w.name.as_str()).collect();
        Ok(Self {
            chosen: SHIPPED
                .iter()
                .filter(|held| held.stem == stem || listed.contains(&held.stem))
                .map(|held| held.stem)
                .collect(),
            entries: entry
                .into_iter()
                .map(|entry| (shipped.stem, entry))
                .collect(),
        })
    }

    pub fn ask_builds(
        &mut self,
        listed: &[&str],
        builds: &[String],
        prompt: &Prompt,
    ) -> Result<bool, Error> {
        if builds.len() < 2 {
            return Ok(true);
        }
        let fresh: Vec<&'static str> = self
            .chosen
            .iter()
            .copied()
            .filter(|stem| !listed.contains(stem))
            .filter(|stem| {
                SHIPPED
                    .iter()
                    .any(|shipped| shipped.stem == *stem && shipped.scope == Scope::Build)
            })
            .collect();
        let rows: Vec<Choice> = builds.iter().map(|build| Choice::new(build, "")).collect();
        let every: Vec<usize> = (0..builds.len()).collect();
        for stem in fresh {
            let Answer::Chosen(picked) =
                prompt.choose_many(&copy::workflow_builds(stem), &rows, &every)?
            else {
                return Ok(false);
            };
            if picked.is_empty() {
                return Err(Error::Operation(format!(
                    "`{stem}` runs for at least one build; keep every build ticked to run it for each"
                )));
            }
            if picked.len() < builds.len() {
                let named: Vec<String> = picked
                    .iter()
                    .map(|at| format!("{:?}", builds[*at]))
                    .collect();
                self.entries.push((
                    stem,
                    format!("{stem} {{\n    builds {}\n}}", named.join(" ")),
                ));
            }
        }
        Ok(true)
    }

    pub fn collect(
        basis: &Basis,
        on: &[&str],
        from: Field,
        prompt: &Prompt,
        say: crate::report::Say<'_>,
    ) -> Result<Option<Self>, Error> {
        let chosen: Vec<&'static Shipped> = match from > Field::Workflows {
            true => SHIPPED
                .iter()
                .filter(|shipped| on.contains(&shipped.stem))
                .collect(),
            false => {
                let options = workflow_options(basis);
                for shipped in SHIPPED
                    .iter()
                    .filter(|shipped| on.contains(&shipped.stem) && !shipped.met(basis))
                {
                    say(&format!(
                        "{}\n",
                        copy::cleared(shipped.stem, shipped.needs.unmet())
                    ));
                }
                let held: Vec<usize> = SHIPPED
                    .iter()
                    .enumerate()
                    .filter(|(_, shipped)| on.contains(&shipped.stem) && shipped.met(basis))
                    .map(|(at, _)| at)
                    .collect();

                let Answer::Chosen(chosen) =
                    prompt.choose_many(copy::WORKFLOWS, &options, &held)?
                else {
                    return Ok(None);
                };
                chosen.iter().map(|at| &SHIPPED[*at]).collect()
            }
        };
        if let Some(short) = chosen.iter().find(|shipped| !shipped.met(basis)) {
            return Err(Error::Operation(format!(
                "`{}` {}",
                short.stem,
                short.needs.unmet()
            )));
        }
        Ok(Some(Self {
            chosen: chosen.iter().map(|shipped| shipped.stem).collect(),
            entries: Vec::new(),
        }))
    }

    pub fn again(
        &self,
        basis: &Basis,
        from: Field,
        prompt: &Prompt,
        say: crate::report::Say<'_>,
    ) -> Result<Option<Self>, Error> {
        Self::collect(basis, &self.chosen, from, prompt, say)
    }

    pub fn rows(&self) -> Vec<(Field, &'static str, String)> {
        vec![(
            Field::Workflows,
            copy::ROW_WORKFLOWS,
            match self.chosen.is_empty() {
                true => copy::NONE.to_string(),
                false => self.chosen.join(", "),
            },
        )]
    }

    pub fn apply(&self, root: &Path) -> Result<Vec<(PathBuf, Change)>, String> {
        let file = root.join(layout::REPO_FILE);
        let text =
            std::fs::read_to_string(&file).map_err(|err| format!("{}: {err}", file.display()))?;
        let out = self
            .spliced(&text)
            .map_err(|err| format!("{} {err}", file.display()))?;
        std::fs::write(&file, out).map_err(|err| format!("{}: {err}", file.display()))?;
        Ok(vec![(
            PathBuf::from(layout::REPO_FILE),
            Change::Updated("the workflows it generates".to_string()),
        )])
    }

    pub(crate) fn spliced(&self, text: &str) -> Result<String, String> {
        parse::declaration::workflows(text, &self.chosen, &self.entries)
    }
}

pub(crate) fn workflow_options(basis: &Basis) -> Vec<Choice> {
    SHIPPED
        .iter()
        .map(|shipped| match shipped.met(basis) {
            true => Choice::new(shipped.stem, shipped.about),
            false => Choice::new(shipped.stem, shipped.needs.unmet()).unavailable(),
        })
        .collect()
}

pub const BY_HAND: &str = "nothing can be asked here; edit the `github` block in repo.kdl, \
                           then run `buildc generate`";

#[cfg(test)]
mod tests {
    use super::*;

    fn set(chosen: &[&'static str]) -> Workflows {
        Workflows {
            chosen: chosen.to_vec(),
            entries: Vec::new(),
        }
    }

    fn named(text: &str, stem: &str, on: &[&str], builds: &[&str]) -> Result<String, Error> {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "buildc-named-workflow-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join(layout::REPO_FILE), text).unwrap();
        let (list, _, _) = crate::declarations(&root);
        let _ = std::fs::remove_dir_all(&root);
        let owned = |values: &[&str]| {
            values
                .iter()
                .map(|value| value.to_string())
                .collect::<Vec<_>>()
        };
        let set = Workflows::named(text, &list, stem, &owned(on), &owned(builds))?;
        Ok(set.spliced(text).unwrap())
    }

    const TWO: &str = "schema-version 1\nbuilds default=\"one\" {\n    one\n    two\n}\n";

    #[test]
    fn a_named_workflow_is_written_with_its_triggers_and_builds() {
        let out = named(TWO, "image-build", &["push", "tag=v*"], &["one"]).unwrap();
        assert!(
            out.ends_with("github {\n    workflows {\n        image-build {\n            push\n            tag \"v*\"\n            builds \"one\"\n        }\n    }\n}\n"),
            "{out}"
        );
    }

    #[test]
    fn each_flag_replaces_only_its_own_part_of_the_entry() {
        let text = format!(
            "{TWO}github {{\n    workflows {{\n        cleanup-registry\n        image-build pr schedule {{\n            tag \"v*\"\n            builds \"one\"\n        }}\n    }}\n}}\n"
        );
        let rebuilt = named(&text, "image-build", &[], &["two"]).unwrap();
        assert!(
            rebuilt.contains("        image-build pr schedule {\n            tag \"v*\"\n            builds \"two\"\n        }\n"),
            "{rebuilt}"
        );
        let retriggered = named(&text, "image-build", &["push"], &[]).unwrap();
        assert!(
            retriggered.contains(
                "        image-build {\n            push\n            builds \"one\"\n        }\n"
            ),
            "{retriggered}"
        );
        assert!(
            retriggered.contains("        cleanup-registry\n"),
            "{retriggered}"
        );
    }

    #[test]
    fn a_named_workflow_with_no_flags_keeps_its_entry() {
        let text =
            format!("{TWO}github {{\n    workflows {{\n        image-build pr\n    }}\n}}\n");
        assert_eq!(named(&text, "image-build", &[], &[]).unwrap(), text);
    }

    #[test]
    fn the_named_form_refuses_what_check_would_refuse() {
        let refused = |on: &[&str], builds: &[&str], stem: &str| match named(TWO, stem, on, builds)
        {
            Err(err) => err.to_string(),
            Ok(out) => panic!("{out}"),
        };
        assert!(refused(&[], &["one"], "cleanup-registry").contains("runs once for the repository"));
        assert!(refused(&[], &["three"], "image-build").contains("`three` is not a build"));
        assert!(refused(&["nightly"], &[], "image-build").contains("is not a trigger"));
        assert!(refused(&["tag="], &[], "image-build").contains("is not a trigger"));
        assert!(refused(&["push", "push"], &[], "image-build").contains("given twice"));
        assert!(refused(&["tag=a", "tag=b"], &[], "image-build").contains("given twice"));
        assert!(refused(&[], &[], "deploy").contains("not a workflow buildc ships"));
    }

    #[test]
    fn a_partial_build_selection_writes_builds_and_a_full_one_does_not() {
        let builds = ["one".to_string(), "two".to_string()];
        let mut chosen = set(&["image-build", "smoke-test", "cleanup-registry"]);
        let prompt = Prompt::scripted(["1", ""].map(str::to_string).to_vec());
        assert!(chosen.ask_builds(&[], &builds, &prompt).unwrap());
        let out = chosen.spliced("schema-version 1\n").unwrap();
        assert!(
            out.contains("        image-build {\n            builds \"one\"\n        }\n        smoke-test\n        cleanup-registry\n"),
            "{out}"
        );
    }

    #[test]
    fn the_block_replaces_the_one_that_was_there_and_leaves_the_rest() {
        let text = "name \"Example\"\n\ngithub {\n    workflows {\n        image-build\n    }\n}\n\nlibraries {\n}\n";
        let out = set(&["image-build", "smoke-test"]).spliced(text).unwrap();
        assert_eq!(
            out,
            "name \"Example\"\n\ngithub {\n    workflows {\n        image-build\n        smoke-test\n    }\n}\n\nlibraries {\n}\n"
        );
    }

    #[test]
    fn a_rewritten_block_keeps_each_written_trigger() {
        let text = "github {\n    workflows {\n        image-build\n        update-components {\n            schedule frequency=\"daily\"\n        }\n    }\n}\n";
        assert_eq!(
            set(&["image-build", "update-components"])
                .spliced(text)
                .unwrap(),
            text
        );
    }

    #[test]
    fn a_repository_with_no_block_gets_one_at_the_end() {
        let out = set(&["image-build"]).spliced("name \"Example\"\n").unwrap();
        assert_eq!(
            out,
            "name \"Example\"\n\ngithub {\n    workflows {\n        image-build\n    }\n}\n"
        );
    }

    #[test]
    fn a_workflow_the_basis_cannot_run_opens_cleared_rather_than_refused() {
        let held = set(&["image-build", "build-disk"]);
        let prompt = Prompt::scripted([""].map(str::to_string).to_vec());
        let mut said = String::new();
        let again = held
            .again(
                &Basis::scaffolding(""),
                Field::Workflows,
                &prompt,
                &mut |text| said.push_str(text),
            )
            .unwrap()
            .unwrap();
        assert_eq!(again.chosen, ["image-build"]);
        assert!(said.contains("build-disk"), "{said}");
    }

    #[test]
    fn the_review_row_names_the_chosen_workflows() {
        let rows = set(&["image-build", "smoke-test"]).rows();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].2, "image-build, smoke-test");
        assert_eq!(set(&[]).rows()[0].2, copy::NONE);
    }

    #[test]
    fn an_edit_keeps_the_comments_and_the_untouched_entries_of_the_block() {
        let text = "github {\n    // nightly only\n    workflows {\n        // kept as written\n        image-build pr {\n            tag \"v*\" {\n                builds \"one\"\n            }\n        }\n        smoke-test\n    }\n}\n";
        let out = set(&["image-build", "cleanup-registry"])
            .spliced(text)
            .unwrap();
        assert_eq!(
            out,
            "github {\n    // nightly only\n    workflows {\n        // kept as written\n        image-build pr {\n            tag \"v*\" {\n                builds \"one\"\n            }\n        }\n        cleanup-registry\n    }\n}\n"
        );
        let rebuilt = named(&format!("{TWO}{text}"), "image-build", &[], &["two"]).unwrap();
        assert!(
            rebuilt.contains("        // kept as written\n        image-build pr {\n            tag \"v*\" {\n                builds \"one\"\n            }\n            builds \"two\"\n        }\n"),
            "{rebuilt}"
        );
    }

    #[test]
    fn create_repo_scaffolds_the_triggers_of_its_workflows() {
        let out = Workflows::sensible().spliced("schema-version 1\n").unwrap();
        assert_eq!(
            out,
            "schema-version 1\n\ngithub {\n    workflows {\n        image-build {\n            pr\n            push\n            schedule frequency=\"daily\" time=\"12:30\"\n        }\n        smoke-test { schedule frequency=\"weekly\" }\n        update-components { schedule frequency=\"weekly\" time=\"05:30\" }\n        cleanup-registry\n    }\n}\n"
        );
    }

    #[test]
    fn choosing_nothing_takes_the_block_away() {
        let text = "name \"Example\"\n\ngithub {\n    workflows {\n        image-build\n    }\n}\n";
        assert_eq!(set(&[]).spliced(text).unwrap(), "name \"Example\"\n");
    }

    #[test]
    fn workflow_screens_snapshot_production_rows() {
        use crate::screen_tests::{assert_screen, assert_style_at, screen};

        let basis = Basis::scaffolding("");
        let options = workflow_options(&basis);
        let selected = [0, 2, 3, 5];
        let rendered = assert_screen(
            "workflow-picker",
            screen().picker(
                copy::WORKFLOWS,
                &options,
                Some(&selected),
                "space toggles",
                0,
            ),
        );
        assert_style_at(
            &rendered,
            0,
            2,
            "modifier: DIM",
            "the unavailable build-disk workflow",
        );
    }
}
