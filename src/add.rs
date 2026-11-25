use crate::copy;
use crate::diag::Source;
use crate::error::Error;
use crate::layout;
use crate::model::image::List;
use crate::parse;
use crate::parse::component::PackageLine;
use crate::parse::local::Place;
use crate::setup::Listing;
use common::prompt::Prompt;
use common::ui::tree::Change;
use common::ui::{Answer, Choice, Field, Filled};
use std::path::{Path, PathBuf};

pub struct Packages {
    path: String,
    group: Group,
}

enum Group {
    Existing {
        place: Place,
        source: Option<Source>,
        node: Option<crate::diag::Span>,
        already: Vec<String>,
        names: Vec<String>,
    },
    Creation(Creation),
}

enum Creation {
    Directory { file: PathBuf, text: String },
    Inline { listing: Listing, block: String },
}

#[derive(Clone)]
struct Candidate {
    path: String,
    place: Place,
    build: Option<String>,
    bare: Option<String>,
    held_by: String,
    label: String,
    about: String,
    hidden: bool,
}

fn candidates(root: &Path, list: &List) -> Vec<Candidate> {
    crate::parse::local::all(root, list)
        .into_iter()
        .map(|listed| {
            let inline = matches!(listed.place, Place::Inline { .. });
            let (label, about) = match &listed.place {
                Place::Directory { file } => (
                    listed.path.clone(),
                    layout::under(root, file).display().to_string(),
                ),
                Place::Inline { file, .. } => (
                    format!("{} ({})", listed.path, listed.held_by),
                    layout::under(root, file).display().to_string(),
                ),
            };
            let build = inline.then_some(listed.build.clone()).flatten();
            let bare = build.as_ref().map(|build| match list.scope(build) {
                Some(scope) => listed
                    .path
                    .strip_prefix(&format!("{scope}/"))
                    .unwrap_or(&listed.path)
                    .to_string(),
                None => listed.path.clone(),
            });
            Candidate {
                path: listed.path,
                place: listed.place,
                build,
                bare,
                held_by: listed.held_by,
                label,
                about,
                hidden: listed.hidden,
            }
        })
        .collect()
}

enum GroupChoice {
    Name(String),
    Component(usize),
}

enum Found {
    Existing(Candidate),
    New(String),
}

fn candidate_options(candidates: &[Candidate]) -> Vec<Choice> {
    candidates
        .iter()
        .map(|candidate| {
            let choice = Choice::new(&candidate.label, &candidate.about);
            match candidate.hidden {
                true => choice.hidden(),
                false => choice,
            }
        })
        .collect()
}

fn group_form(candidates: &[Candidate], prompt: &Prompt) -> Result<Option<GroupChoice>, Error> {
    let actions = [copy::ADD_PACKAGES, copy::CANCEL];
    let mut fields = vec![
        Field::text(copy::GROUP_NAME, "")
            .placeholder(&copy::group_name_hint())
            .default_value(copy::DEFAULT_GROUP),
        Field::action(copy::SELECT_COMPONENT, ""),
    ];
    let mut selected: Option<usize> = None;
    let mut visited = Vec::new();
    loop {
        match common::ui::form(
            &mut fields,
            &actions,
            |_| None,
            |_| Vec::new(),
            |fields| (0..fields.len()).collect(),
            copy::FORM_KEYS,
            &[],
            &[],
            0,
            &mut visited,
        )? {
            Filled::Took(0) => {
                let name = fields[0].value();
                return Ok(Some(match selected {
                    Some(at)
                        if candidates
                            .get(at)
                            .is_some_and(|candidate| candidate.path == name) =>
                    {
                        GroupChoice::Component(at)
                    }
                    _ => GroupChoice::Name(name),
                }));
            }
            Filled::Took(_) | Filled::Left => return Ok(None),
            Filled::Opened(1) => {
                let options = candidate_options(candidates);
                if let Some(at) = prompt.choose(copy::WHICH_COMPONENT, &options)? {
                    selected = Some(at);
                    if let Some(candidate) = candidates.get(at) {
                        fields[0] = Field::text(copy::GROUP_NAME, &candidate.path)
                            .placeholder(&copy::group_name_hint())
                            .default_value(copy::DEFAULT_GROUP);
                    }
                }
            }
            Filled::Opened(_) | Filled::Changed(_) | Filled::Table { .. } => {}
        }
    }
}

fn said_targets(found: &[&Candidate]) -> String {
    let named: Vec<String> = found
        .iter()
        .map(|candidate| candidate.held_by.clone())
        .collect();
    crate::said(&named)
}

fn check_build(candidate: &Candidate, builds: &[String]) -> Result<(), Error> {
    if builds.is_empty() {
        return Ok(());
    }
    match (&candidate.place, &candidate.build) {
        (Place::Directory { .. }, _) => Err(Error::Operation(format!(
            "`{}` is the component in {} and belongs to no build; leave out `--build`",
            candidate.path, candidate.about
        ))),
        (Place::Inline { .. }, None) => Err(Error::Operation(format!(
            "`{}` is inline in {} and belongs to no build; leave out `--build`",
            candidate.path, candidate.held_by
        ))),
        (Place::Inline { .. }, Some(target)) => {
            if builds.len() > 1 {
                return Err(Error::Operation(format!(
                    "`{}` is one inline component; name one `--build` or choose one with \
                     `select component`",
                    candidate.path
                )));
            }
            match builds[0] == *target {
                true => Ok(()),
                false => Err(Error::Operation(format!(
                    "`{}` is inline in {target}, not `{}`",
                    candidate.path, builds[0]
                ))),
            }
        }
    }
}

fn resolve(candidates: &[Candidate], path: String, builds: &[String]) -> Result<Found, Error> {
    let bare = |candidate: &Candidate| {
        candidate
            .build
            .as_ref()
            .is_some_and(|build| builds.contains(build))
            && candidate.bare.as_deref() == Some(path.as_str())
    };
    // A path below a named build's own component wins over the same text read as a full component path.
    let matching: Vec<&Candidate> = match candidates.iter().any(bare) {
        true => candidates
            .iter()
            .filter(|candidate| bare(candidate))
            .collect(),
        false => candidates
            .iter()
            .filter(|candidate| candidate.path == path)
            .collect(),
    };
    if matching.is_empty() {
        return Ok(Found::New(path));
    }
    let folders: Vec<String> = matching
        .iter()
        .filter_map(|candidate| match &candidate.place {
            Place::Directory { file } => Some(file.display().to_string()),
            Place::Inline { .. } => None,
        })
        .collect();
    let inline = matching
        .iter()
        .filter(|candidate| matches!(candidate.place, Place::Inline { .. }))
        .count();
    if inline > 0 && inline < matching.len() {
        let file = matching
            .iter()
            .find_map(|candidate| match &candidate.place {
                Place::Inline { file, .. } => Some(file.display().to_string()),
                Place::Directory { .. } => None,
            })
            .unwrap_or_default();
        return Err(Error::Operation(format!(
            "`{path}` is defined twice, as {} and inline in {file}; `buildc check` refuses the \
             pair",
            folders.join(", ")
        )));
    }
    let held: Vec<&Candidate> = match builds.is_empty() {
        true => matching.clone(),
        false => matching
            .iter()
            .copied()
            .filter(|candidate| match &candidate.build {
                None => true,
                Some(build) => builds.contains(build),
            })
            .collect(),
    };
    match held.len() {
        1 => {
            check_build(held[0], builds)?;
            Ok(Found::Existing(held[0].clone()))
        }
        0 => Err(Error::Operation(format!(
            "`{path}` is not inline in `{}`; it is inline in {}",
            builds.join("`, `"),
            said_targets(&matching)
        ))),
        _ => Err(Error::Operation(format!(
            "`{path}` is inline in {}; name one `--build` or choose one with `select component`",
            said_targets(&held)
        ))),
    }
}

/// Only a directory group reads `file`. An inline group writes into its build's file and needs no home library.
fn new_group(
    file: Result<PathBuf, String>,
    list: &List,
    path: &str,
    names: &[String],
    builds: Vec<String>,
    prompt: &Prompt,
) -> Result<Option<Creation>, Error> {
    if !crate::model::image::is_local_path(path) {
        return Err(Error::Operation(format!(
            "`{path}` is not a component path; each `/` segment takes lowercase letters, \
             digits and dashes and starts with a letter"
        )));
    }
    let targets = list.targets();
    let mut selected: Vec<crate::model::image::Target> = Vec::new();
    for build in builds {
        let Some(target) = targets.iter().find(|target| target.to_string() == build) else {
            return Err(Error::Operation(format!(
                "`{build}` is not a declared build"
            )));
        };
        if !selected
            .iter()
            .any(|held| held.to_string() == target.to_string())
        {
            selected.push(target.clone());
        }
    }
    if selected.is_empty() && prompt.asks() && !targets.is_empty() {
        let options: Vec<Choice> = targets
            .iter()
            .map(|target| Choice::new(target.to_string(), ""))
            .collect();
        let answer = prompt.choose_many(&copy::package_group_builds(path), &options, &[])?;
        let Answer::Chosen(at) = answer else {
            return Ok(None);
        };
        selected = at
            .iter()
            .filter_map(|at| targets.get(*at).cloned())
            .collect();
    }
    let inline = !selected.is_empty();
    crate::create::refuse_vocabulary(path, inline)?;
    if selected.is_empty() {
        let file = file.map_err(Error::Operation)?;
        let text = format!(
            "schema-version {}\n\n{}\n",
            crate::model::image::SCHEMA_VERSION,
            node(names)
        );
        return Ok(Some(Creation::Directory { file, text }));
    }
    let body: String = format!("{}\n", node(names))
        .lines()
        .map(|line| format!("    {line}\n"))
        .collect();
    let block = format!(" {{\n{body}}}");
    Ok(Some(Creation::Inline {
        listing: Listing::selected(list, &selected),
        block,
    }))
}

fn args(names: &[String]) -> String {
    names.iter().map(|name| format!(" \"{name}\"")).collect()
}

fn node(names: &[String]) -> String {
    format!("packages{}", args(names))
}

fn read(place: &Place) -> Result<(Option<Source>, PackageLine), String> {
    let file = place.file();
    let text = match std::fs::read_to_string(file) {
        Ok(text) => text,
        Err(err)
            if err.kind() == std::io::ErrorKind::NotFound
                && matches!(place, Place::Directory { .. }) =>
        {
            return Ok((None, PackageLine::default()));
        }
        Err(err) => return Err(format!("{}: {err}", file.display())),
    };
    let src = Source::new(file.display().to_string(), text);
    let line = match place {
        Place::Directory { .. } => parse::component::packages(src.text())
            .map_err(|err| format!("{} {err}", file.display()))?,
        Place::Inline { spans, .. } => parse::component::inline_packages(&src, spans),
    };
    Ok((Some(src), line))
}

impl Packages {
    pub fn collect(
        root: &Path,
        names: Vec<String>,
        component: Option<String>,
        mut builds: Vec<String>,
        prompt: &Prompt,
    ) -> Result<Option<Self>, Error> {
        let names: Vec<String> = match names.is_empty() {
            true => prompt
                .text_with_placeholder(
                    None,
                    copy::PACKAGE_NAMES,
                    copy::PACKAGE_NAMES_HINT,
                    "package names as arguments",
                    None,
                )?
                .split_whitespace()
                .map(str::to_string)
                .collect(),
            false => names,
        };
        let mut wanted: Vec<String> = Vec::new();
        for name in names {
            if let Some(problem) = parse::component::check_package(&name) {
                return Err(Error::Operation(format!(
                    "`{name}` is not a package name because it {problem}"
                )));
            }
            if !wanted.contains(&name) {
                wanted.push(name);
            }
        }
        if wanted.is_empty() {
            return Err(Error::Operation(
                "give at least one package name".to_string(),
            ));
        }
        let mut unique = Vec::new();
        for build in builds.drain(..) {
            if !unique.contains(&build) {
                unique.push(build);
            }
        }
        let builds = unique;
        let list = List::editable(root).map_err(|issues| issues.plain())?;
        let candidates = candidates(root, &list);
        let group = match component {
            Some(path) => GroupChoice::Name(path),
            None if prompt.draws() => match group_form(&candidates, prompt)? {
                Some(choice) => choice,
                None => return Ok(None),
            },
            None if prompt.asks() => GroupChoice::Name(prompt.text(
                None,
                copy::GROUP_NAME,
                "`--component`",
                Some(copy::DEFAULT_GROUP),
            )?),
            None => {
                return Err(Error::Operation(
                    "give `--component` with the group that receives the packages".to_string(),
                ))
            }
        };
        let (chosen, new_path) = match group {
            GroupChoice::Component(at) => (candidates.get(at).cloned(), None),
            GroupChoice::Name(path) => match resolve(&candidates, path.clone(), &builds)? {
                Found::Existing(candidate) => (Some(candidate), None),
                Found::New(path) => (None, Some(path)),
            },
        };
        if let Some(chosen) = chosen {
            let (source, line) = read(&chosen.place)?;
            let (already, names): (Vec<String>, Vec<String>) = wanted
                .into_iter()
                .partition(|name| line.names.contains(name));
            return Ok(Some(Self {
                path: chosen.path,
                group: Group::Existing {
                    place: chosen.place,
                    source,
                    node: line.node,
                    already,
                    names,
                },
            }));
        }
        let path = new_path.expect("a missing component is a new group");
        let file = list
            .resolver
            .home()
            .map(|home| layout::manifest_at(&home.join(&path)));
        let Some(creation) = new_group(file, &list, &path, &wanted, builds, prompt)? else {
            return Ok(None);
        };
        Ok(Some(Self {
            path: path.clone(),
            group: Group::Creation(creation),
        }))
    }

    pub fn apply(&self, root: &Path) -> Result<Vec<(PathBuf, Change)>, String> {
        match &self.group {
            Group::Creation(creation) => self.create(root, creation),
            Group::Existing {
                place,
                source,
                node,
                already,
                names,
            } => self.add(root, place, source, node, already, names),
        }
    }

    fn create(&self, root: &Path, creation: &Creation) -> Result<Vec<(PathBuf, Change)>, String> {
        match creation {
            Creation::Directory { file, text } => {
                crate::init::put(file, text)?;
                Ok(vec![(layout::under(root, file), Change::Created)])
            }
            Creation::Inline { listing, block } => {
                let list = List::editable(root).map_err(|issues| issues.plain())?;
                listing.apply_children(&list, &[(self.path.as_str(), block.as_str())])
            }
        }
    }

    fn add(
        &self,
        root: &Path,
        place: &Place,
        source: &Option<Source>,
        held_node: &Option<crate::diag::Span>,
        already: &[String],
        names: &[String],
    ) -> Result<Vec<(PathBuf, Change)>, String> {
        if !already.is_empty() {
            println!("`{}` already lists {}", self.path, crate::said(already));
        }
        if names.is_empty() {
            return Ok(Vec::new());
        }
        let file = place.file();
        let Some(source) = source else {
            let text = format!(
                "schema-version {}\n\n{}\n",
                crate::model::image::SCHEMA_VERSION,
                node(names)
            );
            crate::init::put(file, &text)?;
            return Ok(vec![(layout::under(root, file), Change::Created)]);
        };
        let written = match (place, held_node) {
            (_, Some(at)) => crate::parse::declaration::packages(source.text(), *at, names)
                .map_err(|err| format!("{} {err}", file.display()))?,
            (Place::Directory { .. }, None) => {
                crate::parse::declaration::splice(source.text(), None, &node(names))
                    .map_err(|err| format!("{} {err}", file.display()))?
            }
            (Place::Inline { node: at, .. }, None) => {
                crate::parse::declaration::append_inside(file, *at, &node(names))?;
                return Ok(vec![(layout::under(root, file), self.change(names))]);
            }
        };
        std::fs::write(file, written).map_err(|err| format!("{}: {err}", file.display()))?;
        Ok(vec![(layout::under(root, file), self.change(names))])
    }

    fn change(&self, names: &[String]) -> Change {
        Change::Updated(format!("{} added to `packages`", crate::said(names)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diag::Span;

    fn repository(case: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("buildc-add-{case}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        crate::init::put(&root.join("repo.kdl"), "schema-version 1\n").unwrap();
        root
    }

    fn manifest(root: &Path, path: &str, text: &str) {
        crate::init::put(
            &root
                .join("components")
                .join(path)
                .join(layout::MANIFEST_FILE),
            text,
        )
        .unwrap();
    }

    fn run(
        root: &Path,
        names: &[&str],
        component: Option<&str>,
        image: Option<&str>,
        prompt: &Prompt,
    ) -> Result<Vec<(PathBuf, Change)>, Error> {
        let images: Vec<&str> = image.into_iter().collect();
        run_images(root, names, component, &images, prompt)
    }

    fn run_images(
        root: &Path,
        names: &[&str],
        component: Option<&str>,
        images: &[&str],
        prompt: &Prompt,
    ) -> Result<Vec<(PathBuf, Change)>, Error> {
        Ok(Packages::collect(
            root,
            names.iter().map(|name| (*name).to_string()).collect(),
            component.map(str::to_string),
            images.iter().map(|image| (*image).to_string()).collect(),
            prompt,
        )?
        .ok_or_else(|| Error::Operation("the flow was cancelled".to_string()))?
        .apply(root)?)
    }

    fn add(root: &Path, names: &[&str], component: &str) -> Result<Vec<(PathBuf, Change)>, Error> {
        run(root, names, Some(component), None, &Prompt::silent())
    }

    fn refused(result: Result<Vec<(PathBuf, Change)>, Error>) -> String {
        match result {
            Ok(_) => panic!("the command was expected to refuse"),
            Err(error) => error.message().to_string(),
        }
    }

    #[test]
    fn a_hidden_component_is_marked_in_the_candidates() {
        let root = repository("hidden-candidate");
        crate::testing::images(&root, &[("one", "tool\napps {\n    hidden #true\n}\n")]);
        manifest(&root, "tool", "schema-version 1\nhidden #true\n");
        let list = List::editable(&root)
            .map_err(|issues| issues.plain())
            .unwrap();
        let found = candidates(&root, &list);
        std::fs::remove_dir_all(&root).unwrap();
        assert!(found
            .iter()
            .any(|candidate| candidate.path == "tool" && candidate.hidden));
        assert!(found
            .iter()
            .any(|candidate| candidate.path == "apps" && candidate.hidden));
    }

    #[test]
    fn the_package_location_form_names_the_group_and_the_component_choice() {
        use crate::screen_tests::{assert_screen, screen};
        use common::ui::testing::{FormFrame, FormMode};
        let fields = [
            Field::text(copy::GROUP_NAME, "")
                .placeholder(&copy::group_name_hint())
                .default_value(copy::DEFAULT_GROUP),
            Field::action(copy::SELECT_COMPONENT, ""),
        ];
        let visible = [0, 1];
        let actions = [copy::ADD_PACKAGES, copy::CANCEL];
        let frame = FormFrame {
            fields: &fields,
            visible: &visible,
            cursor: 0,
            button: 0,
            mode: &FormMode::Rows,
            actions: &actions,
            blocked: None,
            tried: false,
            keys: copy::FORM_KEYS,
            header: &[],
            sections: &[],
        };
        assert_screen("add-package-location", frame.render(&screen()));
    }

    #[test]
    fn a_plain_component_is_not_hidden() {
        let root = repository("plain-candidate");
        crate::testing::images(&root, &[("one", "tool\nplain\noff\nvisible {\n    description \"Visible\"\n}\noff-inline {\n    hidden #false\n}\n")]);
        manifest(&root, "tool", "schema-version 1\nhidden #true\n");
        manifest(&root, "plain", "schema-version 1\n");
        manifest(&root, "off", "schema-version 1\nhidden #false\n");
        let list = List::editable(&root)
            .map_err(|issues| issues.plain())
            .unwrap();
        let found = candidates(&root, &list);
        std::fs::remove_dir_all(&root).unwrap();
        for path in ["plain", "off", "visible", "off-inline"] {
            assert!(
                found
                    .iter()
                    .any(|candidate| candidate.path == path && !candidate.hidden),
                "{path} is not a hidden candidate"
            );
        }
        assert!(found
            .iter()
            .any(|candidate| candidate.path == "tool" && candidate.hidden));
    }

    #[test]
    fn candidate_options_keep_the_label_and_mark_the_hidden() {
        let root = repository("candidate-options");
        let candidates = vec![
            Candidate {
                path: "base".into(),
                place: Place::Directory {
                    file: root.join("components/base/manifest.kdl"),
                },
                build: None,
                bare: None,
                held_by: "base".into(),
                label: "base".into(),
                about: "components/base".into(),
                hidden: true,
            },
            Candidate {
                path: "apps".into(),
                place: Place::Directory {
                    file: root.join("components/apps/manifest.kdl"),
                },
                build: None,
                bare: None,
                held_by: "apps".into(),
                label: "apps".into(),
                about: "components/apps".into(),
                hidden: false,
            },
        ];
        let options = candidate_options(&candidates);
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(options.len(), 2);
        assert!(options[0].hidden);
        assert_eq!(options[1].label, "apps");
    }

    #[test]
    fn a_package_group_image_question_names_the_group() {
        assert_eq!(
            copy::package_group_builds("tools"),
            "select build to add \"tools\" package group"
        );
    }

    #[test]
    fn an_existing_inline_group_takes_one_image() {
        let root = repository("check-image-two");
        crate::testing::images(
            &root,
            &[("one", "apps { description \"Apps\" }\n"), ("two", "")],
        );
        let refused = refused(run_images(
            &root,
            &["htop"],
            Some("apps"),
            &["one", "two"],
            &Prompt::silent(),
        ));
        std::fs::remove_dir_all(&root).unwrap();
        assert!(
            refused.contains("one inline component") && refused.contains("select component"),
            "{refused}"
        );
    }

    #[test]
    fn a_named_build_does_not_ask_for_more_builds() {
        let root = repository("named-image");
        crate::testing::images(&root, &[("one", "")]);
        let prompt = Prompt::scripted(vec![]);
        run_images(&root, &["htop"], Some("tools"), &["one"], &prompt).unwrap();
        let text = crate::testing::image_text(&root, "one");
        std::fs::remove_dir_all(&root).unwrap();
        assert!(
            text.contains("\ntools {\n    packages \"htop\"\n}\n"),
            "{text}"
        );
    }

    #[test]
    fn a_second_call_with_the_build_finds_its_bare_group() {
        let root = repository("bare-again");
        crate::testing::images(&root, &[("one", "")]);
        run_images(&root, &["htop"], Some("tools"), &["one"], &Prompt::silent()).unwrap();
        run_images(&root, &["vim"], Some("tools"), &["one"], &Prompt::silent()).unwrap();
        let text = crate::testing::image_text(&root, "one");
        std::fs::remove_dir_all(&root).unwrap();
        assert!(
            text.contains("\ntools {\n    packages \"htop\" \"vim\"\n}\n"),
            "{text}"
        );
    }

    #[test]
    fn without_a_build_the_group_path_is_the_library_path() {
        let root = repository("bare-unnamed");
        crate::testing::images(&root, &[("one", "")]);
        run_images(&root, &["htop"], Some("tools"), &["one"], &Prompt::silent()).unwrap();
        run_images(&root, &["vim"], Some("tools"), &[], &Prompt::silent()).unwrap();
        let group = std::fs::read_to_string(root.join("components/tools/manifest.kdl"));
        let image = crate::testing::image_text(&root, "one");
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(group.unwrap(), "schema-version 1\n\npackages \"vim\"\n");
        assert!(!image.contains("vim"), "{image}");
    }

    #[test]
    fn a_group_inline_in_another_build_is_refused_naming_it() {
        let root = repository("other-build");
        crate::testing::images(
            &root,
            &[("one", "apps { description \"Apps\" }\n"), ("two", "")],
        );
        let refused = refused(run_images(
            &root,
            &["htop"],
            Some("apps"),
            &["two"],
            &Prompt::silent(),
        ));
        std::fs::remove_dir_all(&root).unwrap();
        assert!(
            refused.contains("`apps` is not inline in `two`; it is inline in one"),
            "{refused}"
        );
    }

    #[test]
    fn a_bare_group_without_packages_gains_them_where_it_is() {
        let root = repository("bare-chain");
        crate::testing::images(&root, &[("one", "")]);
        crate::init::put(
            &crate::testing::image_file(&root, "one"),
            "schema-version 1\n\napps {\n    description \"Apps\"\n}\n",
        )
        .unwrap();
        run_images(&root, &["htop"], Some("apps"), &["one"], &Prompt::silent()).unwrap();
        let text = crate::testing::image_text(&root, "one");
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(
            text,
            "schema-version 1\n\napps {\n    description \"Apps\"\n    packages \"htop\"\n}\n"
        );
    }

    #[test]
    fn a_repeated_image_name_is_one_target() {
        let root = repository("duplicate-image");
        crate::testing::images(&root, &[("one", "apps { description \"Apps\" }\n")]);
        run_images(
            &root,
            &["htop"],
            Some("apps"),
            &["one", "one"],
            &Prompt::silent(),
        )
        .unwrap();
        let text = crate::testing::image_text(&root, "one");
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(
            text.matches("apps { description \"Apps\"; packages \"htop\" }")
                .count(),
            1,
            "{text}"
        );
    }

    #[test]
    fn an_inline_packages_line_gains_a_name_after_its_last_one() {
        let root = repository("inline-append");
        crate::testing::images(
            &root,
            &[(
                "one",
                "apps {\n    description \"Apps\"\n    packages \"firefox\"\n}\n",
            )],
        );
        add(&root, &["htop"], "apps").unwrap();
        let text = crate::testing::image_text(&root, "one");
        std::fs::remove_dir_all(&root).unwrap();
        assert!(
            text.contains("    packages \"firefox\" \"htop\"\n"),
            "{text}"
        );
    }

    #[test]
    fn a_one_line_inline_block_stays_on_one_line() {
        let root = repository("inline-one-line");
        crate::testing::images(&root, &[("one", "apps { description \"Apps\" }\n")]);
        add(&root, &["htop"], "apps").unwrap();
        let text = crate::testing::image_text(&root, "one");
        std::fs::remove_dir_all(&root).unwrap();
        assert!(
            text.ends_with("\n    apps { description \"Apps\"; packages \"htop\" }\n}\n"),
            "{text}"
        );
    }

    #[test]
    fn a_directory_component_gains_a_packages_node() {
        let root = repository("directory-create");
        manifest(&root, "tool", "schema-version 1\n\ndescription \"Tool\"\n");
        add(&root, &["htop"], "tool").unwrap();
        let text = std::fs::read_to_string(root.join("components/tool/manifest.kdl")).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(
            text,
            "schema-version 1\n\ndescription \"Tool\"\n\npackages \"htop\"\n"
        );
    }

    #[test]
    fn a_directory_packages_node_gains_a_name_after_its_last_one() {
        let root = repository("directory-append");
        manifest(
            &root,
            "tool",
            "schema-version 1\n\npackages \"htop\" enablerepo=\"tools\"\n",
        );
        add(&root, &["vim"], "tool").unwrap();
        let text = std::fs::read_to_string(root.join("components/tool/manifest.kdl")).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(
            text,
            "schema-version 1\n\npackages \"htop\" \"vim\" enablerepo=\"tools\"\n"
        );
    }

    #[test]
    fn a_block_packages_node_gains_a_child() {
        let root = repository("block");
        manifest(
            &root,
            "tool",
            "schema-version 1\n\npackages {\n    \"bash-completion\"\n    \"htop\"\n}\n",
        );
        add(&root, &["tmux"], "tool").unwrap();
        let text = std::fs::read_to_string(root.join("components/tool/manifest.kdl")).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(
            text,
            "schema-version 1\n\npackages {\n    \"bash-completion\"\n    \"htop\"\n    \"tmux\"\n}\n"
        );
    }

    #[test]
    fn a_brace_in_a_trailing_comment_does_not_move_the_block_close() {
        let root = repository("comment-brace");
        manifest(
            &root,
            "tool",
            "schema-version 1\n\npackages {\n    \"htop\"\n} // closes }\n",
        );
        add(&root, &["tmux"], "tool").unwrap();
        let text = std::fs::read_to_string(root.join("components/tool/manifest.kdl")).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(
            text,
            "schema-version 1\n\npackages {\n    \"htop\"\n    \"tmux\"\n} // closes }\n"
        );
    }

    #[test]
    fn a_block_package_a_family_gate_holds_is_not_added_again() {
        let root = repository("gated-block");
        let before = "schema-version 1\n\npackages \"qemu-user-binfmt\"\n\non \"fedora\" {\n    packages {\n        \"libvirt\"\n    }\n}\n";
        manifest(&root, "tool", before);
        let wrote = add(&root, &["libvirt"], "tool").unwrap();
        let text = std::fs::read_to_string(root.join("components/tool/manifest.kdl")).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert!(wrote.is_empty());
        assert_eq!(text, before);
    }

    #[test]
    fn a_script_only_component_gains_a_manifest() {
        let root = repository("bare");
        crate::init::put(&root.join("components/tool/component.sh"), "true\n").unwrap();
        add(&root, &["htop"], "tool").unwrap();
        let text = std::fs::read_to_string(root.join("components/tool/manifest.kdl")).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(
            text,
            format!(
                "schema-version {}\n\npackages \"htop\"\n",
                crate::model::image::SCHEMA_VERSION
            )
        );
    }

    #[test]
    fn a_name_a_family_gate_holds_is_not_added_again() {
        let root = repository("gated");
        let before = "schema-version 1\n\non \"fedora\" {\n    packages \"nano\"\n}\n";
        manifest(&root, "tool", before);
        let wrote = add(&root, &["nano"], "tool").unwrap();
        let text = std::fs::read_to_string(root.join("components/tool/manifest.kdl")).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert!(wrote.is_empty());
        assert_eq!(text, before);
    }

    #[test]
    fn one_inline_component_in_two_images_needs_the_image() {
        let root = repository("two-images");
        crate::testing::images(
            &root,
            &[
                ("one", "apps { description \"Apps\" }\n"),
                ("two", "apps { description \"Apps\" }\n"),
            ],
        );
        let refused = refused(add(&root, &["htop"], "apps"));
        assert!(
            refused.contains("inline in one and two") && refused.contains("select component"),
            "{refused}"
        );
        run(
            &root,
            &["htop"],
            Some("apps"),
            Some("two"),
            &Prompt::silent(),
        )
        .unwrap();
        let one = crate::testing::image_text(&root, "one");
        let two = crate::testing::image_text(&root, "two");
        std::fs::remove_dir_all(&root).unwrap();
        assert!(
            two.ends_with("\n    apps { description \"Apps\"; packages \"htop\" }\n}\n"),
            "{two}"
        );
        assert!(
            one.ends_with("\n    apps { description \"Apps\" }\n}\n"),
            "{one}"
        );
    }

    #[test]
    fn an_inline_block_packages_node_gains_a_child() {
        let root = repository("inline-block");
        crate::testing::images(
            &root,
            &[(
                "one",
                "apps {\n    packages {\n        \"firefox\"\n    }\n}\n",
            )],
        );
        add(&root, &["htop"], "apps").unwrap();
        let text = crate::testing::image_text(&root, "one");
        std::fs::remove_dir_all(&root).unwrap();
        assert!(
            text.contains(
                "        packages {\n            \"firefox\"\n            \"htop\"\n        }\n"
            ),
            "{text}"
        );
    }

    #[test]
    fn an_inline_definition_in_a_component_body_takes_its_packages() {
        let root = repository("inline-body");
        manifest(
            &root,
            "desktop",
            "schema-version 1\n\ntools {\n    packages \"htop\"\n}\n",
        );
        add(&root, &["curl"], "desktop/tools").unwrap();
        let text = std::fs::read_to_string(root.join("components/desktop/manifest.kdl")).unwrap();
        let directory = root.join("components/desktop/tools").exists();
        std::fs::remove_dir_all(&root).unwrap();
        assert!(
            text.ends_with("tools {\n    packages \"htop\" \"curl\"\n}\n"),
            "{text}"
        );
        assert!(!directory);
    }

    #[test]
    fn an_inline_definition_in_a_component_body_gains_a_packages_node() {
        let root = repository("inline-body-gains");
        manifest(
            &root,
            "desktop",
            "schema-version 1\n\ntools {\n    description \"Tools\"\n}\n",
        );
        add(&root, &["curl"], "desktop/tools").unwrap();
        let text = std::fs::read_to_string(root.join("components/desktop/manifest.kdl")).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert!(
            text.ends_with("tools {\n    description \"Tools\"\n    packages \"curl\"\n}\n"),
            "{text}"
        );
    }

    #[test]
    fn a_nested_inline_definition_in_a_component_body_is_found() {
        let root = repository("inline-nested");
        manifest(
            &root,
            "desktop",
            "schema-version 1\n\ntools {\n    editor {\n        description \"Editor\"\n    }\n}\n",
        );
        add(&root, &["vim"], "desktop/tools/editor").unwrap();
        let text = std::fs::read_to_string(root.join("components/desktop/manifest.kdl")).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert!(
            text.contains(
                "editor {\n        description \"Editor\"\n        packages \"vim\"\n    }"
            ),
            "{text}"
        );
    }

    #[test]
    fn a_build_does_not_hold_an_inline_definition_in_a_directory_component() {
        let root = repository("inline-body-build");
        manifest(
            &root,
            "desktop",
            "schema-version 1\n\ntools {\n    packages \"htop\"\n}\n",
        );
        let refused = refused(run(
            &root,
            &["curl"],
            Some("desktop/tools"),
            Some("one"),
            &Prompt::silent(),
        ));
        std::fs::remove_dir_all(&root).unwrap();
        assert!(
            refused.contains("inline in desktop") && refused.contains("belongs to no build"),
            "{refused}"
        );
    }

    #[test]
    fn the_candidates_list_a_builds_component_first() {
        let root = repository("candidates-first");
        manifest(&root, "alpha", "schema-version 1\n");
        crate::testing::images(&root, &[("one", "kept\n")]);
        let list = List::editable(&root)
            .map_err(|issues| issues.plain())
            .unwrap();
        let found = candidates(&root, &list);
        std::fs::remove_dir_all(&root).unwrap();
        let paths: Vec<&str> = found
            .iter()
            .map(|candidate| candidate.path.as_str())
            .collect();
        assert_eq!(paths.first(), Some(&"one"), "{paths:?}");
    }

    #[test]
    fn an_inline_definition_whose_file_is_gone_is_refused() {
        let root = repository("gone");
        let place = Place::Inline {
            file: root.join("gone.kdl"),
            src: Source::new("gone.kdl", String::new()),
            spans: Vec::new(),
            node: Span::default(),
        };
        let Err(refused) = read(&place) else {
            panic!("an image file that is gone is refused");
        };
        std::fs::remove_dir_all(&root).unwrap();
        assert!(refused.contains("gone.kdl"), "{refused}");
    }

    #[test]
    fn a_directory_and_an_inline_definition_are_refused_together() {
        let root = repository("pair");
        crate::testing::images(&root, &[("one", "apps { description \"Apps\" }\n")]);
        manifest(&root, "apps", "schema-version 1\n\npackages \"vim\"\n");
        let refused = refused(add(&root, &["htop"], "apps"));
        std::fs::remove_dir_all(&root).unwrap();
        assert!(
            refused.contains("defined twice")
                && refused.contains("components/apps")
                && refused.contains("one.kdl"),
            "{refused}"
        );
    }

    #[test]
    fn an_image_with_a_directory_component_is_refused() {
        let root = repository("image-directory");
        manifest(&root, "tool", "schema-version 1\n");
        let refused = refused(run(
            &root,
            &["htop"],
            Some("tool"),
            Some("one"),
            &Prompt::silent(),
        ));
        std::fs::remove_dir_all(&root).unwrap();
        assert!(
            refused.contains("is the component in") && refused.contains("belongs to no build"),
            "{refused}"
        );
    }

    #[test]
    fn a_new_group_with_no_image_writes_a_directory_component() {
        let root = repository("new-group");
        let wrote = add(&root, &["htop"], "nope").unwrap();
        let text = std::fs::read_to_string(root.join("components/nope/manifest.kdl")).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(text, "schema-version 1\n\npackages \"htop\"\n");
        assert!(!wrote.is_empty());
    }

    const SHELF_DEFAULT: &str = "schema-version 1\n\nlibraries default=\"shelf\" {\n    local {\n        local-library { source \".\" }\n        shelf { source \"shelf\" }\n    }\n}\n";

    #[test]
    fn a_new_group_lands_in_the_local_default_library() {
        let root = repository("default-library");
        crate::init::put(&root.join("repo.kdl"), SHELF_DEFAULT).unwrap();
        std::fs::create_dir_all(root.join("shelf")).unwrap();
        add(&root, &["htop"], "tools").unwrap();
        let text = std::fs::read_to_string(root.join("shelf/tools/manifest.kdl"));
        let stray = root.join("components/tools").exists();
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(text.unwrap(), "schema-version 1\n\npackages \"htop\"\n");
        assert!(!stray);
    }

    #[test]
    fn a_remote_default_library_is_refused() {
        let root = repository("remote-default");
        crate::init::put(
            &root.join("repo.kdl"),
            "schema-version 1\n\nlibraries default=\"upstream\" {\n    remote {\n        upstream { source \"https://example.invalid/library\" }\n    }\n}\n",
        )
        .unwrap();
        let refused = refused(add(&root, &["htop"], "tools"));
        let stray = root.join("components/tools").exists();
        std::fs::remove_dir_all(&root).unwrap();
        assert!(refused.contains("names a remote library"), "{refused}");
        assert!(!stray);
    }

    #[test]
    fn an_existing_group_takes_packages_under_a_remote_default_library() {
        let root = repository("remote-default-existing");
        crate::init::put(
            &root.join("repo.kdl"),
            "schema-version 1\n\nlibraries default=\"upstream\" {\n    local {\n        local-library { source \".\" }\n    }\n    remote {\n        upstream { source \"https://example.invalid/library\" }\n    }\n}\n",
        )
        .unwrap();
        manifest(&root, "tools", "schema-version 1\n");
        let wrote = add(&root, &["htop"], "tools");
        let text = std::fs::read_to_string(root.join("components/tools/manifest.kdl")).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert!(wrote.is_ok());
        assert!(text.ends_with("packages \"htop\"\n"), "{text}");
    }

    #[test]
    fn a_new_group_beside_two_local_libraries_and_no_default_is_refused() {
        let root = repository("no-default");
        crate::init::put(
            &root.join("repo.kdl"),
            "schema-version 1\n\nlibraries {\n    local {\n        a { source \"a\" }\n        b { source \"b\" }\n    }\n}\n",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("a")).unwrap();
        std::fs::create_dir_all(root.join("b")).unwrap();
        let refused = refused(add(&root, &["htop"], "tools"));
        let stray = root.join("components/tools").exists();
        std::fs::remove_dir_all(&root).unwrap();
        assert!(refused.contains("names no `default=`"), "{refused}");
        assert!(!stray);
    }

    #[test]
    fn a_group_in_two_folders_takes_the_packages_in_the_default_library() {
        let root = repository("two-folders");
        crate::init::put(&root.join("repo.kdl"), SHELF_DEFAULT).unwrap();
        crate::init::put(&root.join("shelf/tools/manifest.kdl"), "schema-version 1\n").unwrap();
        manifest(&root, "tools", "schema-version 1\n");
        let wrote = add(&root, &["htop"], "tools");
        let shelf = std::fs::read_to_string(root.join("shelf/tools/manifest.kdl")).unwrap();
        let own = std::fs::read_to_string(root.join("components/tools/manifest.kdl")).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert!(wrote.is_ok());
        assert!(shelf.ends_with("packages \"htop\"\n"), "{shelf}");
        assert_eq!(own, "schema-version 1\n");
    }

    #[test]
    fn a_new_group_beside_the_root_library_and_another_with_no_default_lands_in_components() {
        let root = repository("root-no-default");
        crate::init::put(
            &root.join("repo.kdl"),
            "schema-version 1\n\nlibraries {\n    local {\n        local-library { source \".\" }\n        shelf { source \"shelf\" }\n    }\n}\n",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("shelf")).unwrap();
        let wrote = add(&root, &["htop"], "tools");
        let written = root.join("components/tools/manifest.kdl").is_file();
        std::fs::remove_dir_all(&root).unwrap();
        assert!(wrote.is_ok());
        assert!(written);
    }

    #[test]
    fn a_new_group_named_after_a_manifest_node_is_refused() {
        let root = repository("new-keyword");
        let refused = refused(add(&root, &["htop"], "conforms"));
        let written = root.join("components/conforms").exists();
        std::fs::remove_dir_all(&root).unwrap();
        assert!(
            refused.contains("`conforms` is a manifest node"),
            "{refused}"
        );
        assert!(!written);
    }

    #[test]
    fn a_new_inline_group_holding_a_manifest_node_name_is_refused() {
        let root = repository("new-inline-keyword");
        crate::testing::images(&root, &[("one", "")]);
        let refused = refused(run_images(
            &root,
            &["htop"],
            Some("tools/packages"),
            &["one"],
            &Prompt::silent(),
        ));
        let text = crate::testing::image_text(&root, "one");
        std::fs::remove_dir_all(&root).unwrap();
        assert!(refused.contains("the segment `packages`"), "{refused}");
        assert!(!text.contains("tools"), "{text}");
    }

    #[test]
    fn a_new_group_with_one_build_writes_a_bare_inline_block() {
        let root = repository("new-inline");
        crate::testing::images(&root, &[("one", "")]);
        run_images(&root, &["htop"], Some("tools"), &["one"], &Prompt::silent()).unwrap();
        let text = crate::testing::image_text(&root, "one");
        std::fs::remove_dir_all(&root).unwrap();
        assert!(
            text.contains("\ntools {\n    packages \"htop\"\n}\n"),
            "{text}"
        );
    }

    #[test]
    fn a_new_group_with_two_builds_writes_a_bare_inline_block_in_each() {
        let root = repository("new-inline-two");
        crate::testing::images(&root, &[("one", ""), ("two", "")]);
        run_images(
            &root,
            &["htop"],
            Some("tools"),
            &["one", "two"],
            &Prompt::silent(),
        )
        .unwrap();
        let text =
            crate::testing::image_text(&root, "one") + &crate::testing::image_text(&root, "two");
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(
            text.matches("\ntools {\n    packages \"htop\"\n}\n")
                .count(),
            2,
            "{text}"
        );
    }

    #[test]
    fn a_name_that_would_not_survive_the_run_line_is_refused() {
        let root = repository("bad-name");
        manifest(&root, "tool", "schema-version 1\n");
        let refused = refused(add(&root, &["bad name"], "tool"));
        let text = std::fs::read_to_string(root.join("components/tool/manifest.kdl")).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert!(refused.contains("is not a package name"), "{refused}");
        assert_eq!(text, "schema-version 1\n");
    }

    #[test]
    fn a_name_asked_for_is_split_into_package_names() {
        let root = repository("asked");
        manifest(&root, "tool", "schema-version 1\n");
        let prompt = Prompt::scripted(["nano curl"].map(str::to_string).to_vec());
        run(&root, &[], Some("tool"), None, &prompt).unwrap();
        let text = std::fs::read_to_string(root.join("components/tool/manifest.kdl")).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert!(text.ends_with("packages \"nano\" \"curl\"\n"), "{text}");
    }

    #[test]
    fn an_empty_group_name_uses_apps() {
        let root = repository("default-group");
        let prompt = Prompt::scripted(vec![String::new()]);
        run(&root, &["htop"], None, None, &prompt).unwrap();
        let text = std::fs::read_to_string(root.join("components/apps/manifest.kdl")).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(text, "schema-version 1\n\npackages \"htop\"\n");
    }

    #[test]
    fn a_scripted_flow_selects_a_build_for_a_new_group() {
        let root = repository("scripted-image");
        crate::testing::images(&root, &[("one", "")]);
        let prompt = Prompt::scripted(["tools".into(), "1".into()].to_vec());
        run(&root, &["htop"], None, None, &prompt).unwrap();
        let text = crate::testing::image_text(&root, "one");
        std::fs::remove_dir_all(&root).unwrap();
        assert!(
            text.contains("\ntools {\n    packages \"htop\"\n}\n"),
            "{text}"
        );
    }

    #[test]
    fn a_scripted_flow_takes_the_group_name() {
        let root = repository("scripted-group");
        manifest(&root, "alpha", "schema-version 1\n");
        let prompt = Prompt::scripted(["alpha"].map(str::to_string).to_vec());
        run(&root, &["htop"], None, None, &prompt).unwrap();
        let alpha = std::fs::read_to_string(root.join("components/alpha/manifest.kdl")).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert!(alpha.ends_with("packages \"htop\"\n"), "{alpha}");
    }

    #[test]
    fn no_terminal_refuses_a_missing_component_flag() {
        let root = repository("silent");
        manifest(&root, "alpha", "schema-version 1\n");
        manifest(&root, "beta", "schema-version 1\n");
        let refused = refused(run(&root, &["htop"], None, None, &Prompt::silent()));
        std::fs::remove_dir_all(&root).unwrap();
        assert!(refused.contains("give `--component`"), "{refused}");
    }

    #[test]
    fn no_terminal_refuses_a_missing_component_flag_with_one_component() {
        let root = repository("silent-one");
        manifest(&root, "alpha", "schema-version 1\n");
        let refused = refused(run(&root, &["htop"], None, None, &Prompt::silent()));
        let text = std::fs::read_to_string(root.join("components/alpha/manifest.kdl")).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert!(refused.contains("give `--component`"), "{refused}");
        assert_eq!(text, "schema-version 1\n");
    }

    #[test]
    fn a_package_name_may_reference_an_option() {
        let root = repository("reference");
        manifest(&root, "tool", "schema-version 1\n");
        add(&root, &["firefox={version}"], "tool").unwrap();
        let text = std::fs::read_to_string(root.join("components/tool/manifest.kdl")).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert!(text.ends_with("packages \"firefox={version}\"\n"), "{text}");
    }

    #[test]
    fn the_candidates_leave_out_a_directory_whose_name_is_no_component_path() {
        let root = repository("candidate-path");
        manifest(&root, "alpha", "schema-version 1\n");
        manifest(&root, "Bad_Name", "schema-version 1\n");
        let list = List::editable(&root)
            .map_err(|issues| issues.plain())
            .unwrap();
        let found = candidates(&root, &list);
        std::fs::remove_dir_all(&root).unwrap();
        assert!(found.iter().any(|candidate| candidate.path == "alpha"));
        assert!(!found.iter().any(|candidate| candidate.path == "Bad_Name"));
    }

    #[test]
    fn an_image_without_a_component_flag_is_refused() {
        let root = repository("image-alone");
        manifest(&root, "tool", "schema-version 1\n");
        let refused = refused(run(&root, &["htop"], None, Some("one"), &Prompt::silent()));
        std::fs::remove_dir_all(&root).unwrap();
        assert!(refused.contains("give `--component`"), "{refused}");
    }

    #[test]
    fn two_new_names_on_a_multi_line_block_each_take_their_own_line() {
        let root = repository("block-two");
        manifest(
            &root,
            "tool",
            "schema-version 1\n\npackages {\n    \"htop\"\n}\n",
        );
        add(&root, &["tmux", "vim"], "tool").unwrap();
        let text = std::fs::read_to_string(root.join("components/tool/manifest.kdl")).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(
            text,
            "schema-version 1\n\npackages {\n    \"htop\"\n    \"tmux\"\n    \"vim\"\n}\n"
        );
    }

    #[test]
    fn two_new_names_on_a_one_line_block_stay_on_one_line() {
        let root = repository("block-two-one-line");
        manifest(&root, "tool", "schema-version 1\n\npackages { \"htop\" }\n");
        add(&root, &["tmux", "vim"], "tool").unwrap();
        let text = std::fs::read_to_string(root.join("components/tool/manifest.kdl")).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(
            text,
            "schema-version 1\n\npackages { \"htop\"; \"tmux\"; \"vim\" }\n"
        );
    }
}
