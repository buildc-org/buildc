use crate::copy;
use crate::declaration::append;
use crate::diag::{Source, Span};
use crate::error::Error;
use crate::layout;
use crate::model::image::List;
use crate::parse;
use crate::parse::component::{PackageInsert, PackageLine};
use crate::setup::Listing;
use common::prompt::Prompt;
use common::ui::tree::Change;
use common::ui::{Answer, Choice, Field, Filled};
use std::path::{Path, PathBuf};

pub struct Packages {
    path: String,
    place: Place,
    source: Option<Source>,
    insert: Option<PackageInsert>,
    already: Vec<String>,
    names: Vec<String>,
    creation: Option<Inline>,
}

struct Inline {
    listing: Listing,
    block: String,
}

enum NewGroup {
    Directory,
    Inline(Inline),
}

#[derive(Clone)]
enum Place {
    Directory {
        file: PathBuf,
    },
    Inline {
        file: PathBuf,
        image: String,
        flavour: Option<String>,
        spans: Vec<Span>,
    },
}

impl Place {
    fn file(&self) -> &Path {
        match self {
            Self::Directory { file } | Self::Inline { file, .. } => file,
        }
    }

    fn target(&self) -> Option<String> {
        match self {
            Self::Directory { .. } => None,
            Self::Inline { image, flavour, .. } => Some(
                crate::model::image::Target {
                    image: image.clone(),
                    flavour: flavour
                        .clone()
                        .unwrap_or_else(|| crate::model::image::NO_FLAVOUR.to_string()),
                }
                .to_string(),
            ),
        }
    }
}

#[derive(Clone)]
struct Candidate {
    path: String,
    place: Place,
    label: String,
    about: String,
    hidden: bool,
}

fn candidates(root: &Path, list: &List) -> Vec<Candidate> {
    let components = layout::components(root);
    let mut found: Vec<Candidate> =
        layout::component_dirs(&components, layout::is_component, |name| {
            name.starts_with('.')
        })
        .into_iter()
        .filter_map(|dir| {
            let path = dir
                .strip_prefix(&components)
                .ok()?
                .to_string_lossy()
                .replace('\\', "/");
            if !crate::model::image::is_local_path(&path) {
                return None;
            }
            let file = dir.join(layout::MANIFEST_FILE);
            Some(Candidate {
                label: path.clone(),
                about: format!("components/{path}"),
                path,
                hidden: crate::parse::component::hidden(&file),
                place: Place::Directory { file },
            })
        })
        .collect();
    for image in &list.images {
        for entry in &image.entries {
            let Some(spans) = &entry.inline else {
                continue;
            };
            let place = Place::Inline {
                file: PathBuf::from(image.src.name()),
                image: image.id.clone(),
                flavour: entry.flavour.clone(),
                spans: spans.clone(),
            };
            found.push(Candidate {
                label: format!("{} ({})", entry.path, place.target().unwrap_or_default()),
                about: image.src.name().to_string(),
                path: entry.path.clone(),
                hidden: crate::parse::component::inline_hidden(&image.src, spans),
                place,
            });
        }
    }
    found.sort_by_cached_key(|candidate| (candidate.path.clone(), candidate.place.target()));
    found
}

fn pickable(root: &Path, candidates: &[Candidate]) -> Result<Vec<Candidate>, String> {
    let tracked = crate::provenance::lock::tracked(root)?;
    Ok(candidates
        .iter()
        .filter(|candidate| match candidate.place {
            Place::Directory { .. } => !tracked.iter().any(|record| record.path == candidate.path),
            // The lock file tracks component directories alone.
            Place::Inline { .. } => true,
        })
        .cloned()
        .collect())
}

enum GroupChoice {
    Name(String),
    Component(Candidate),
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

fn group_name_field(value: &str) -> Field {
    Field::text(copy::GROUP_NAME, value)
        .placeholder(&copy::group_name_hint())
        .default_value(copy::DEFAULT_GROUP)
}

fn group_form(candidates: &[Candidate], prompt: &Prompt) -> Result<Option<GroupChoice>, Error> {
    let actions = [copy::ADD_PACKAGES, copy::CANCEL];
    let mut fields = vec![
        group_name_field(""),
        Field::action(copy::SELECT_COMPONENT, ""),
    ];
    let mut selected: Option<&Candidate> = None;
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
                // The chosen component names one inline copy where several images share the path.
                return Ok(Some(match selected {
                    Some(candidate) if candidate.path == name => {
                        GroupChoice::Component(candidate.clone())
                    }
                    _ => GroupChoice::Name(name),
                }));
            }
            Filled::Took(_) | Filled::Left => return Ok(None),
            Filled::Opened(1) => {
                let options = candidate_options(candidates);
                if let Some(candidate) = prompt
                    .choose(copy::WHICH_COMPONENT, &options)?
                    .and_then(|at| candidates.get(at))
                {
                    selected = Some(candidate);
                    fields[0] = group_name_field(&candidate.path);
                }
            }
            Filled::Opened(_) | Filled::Changed(_) | Filled::Table { .. } => {}
        }
    }
}

fn said_targets(found: &[&Candidate]) -> String {
    let named: Vec<String> = found
        .iter()
        .filter_map(|candidate| candidate.place.target())
        .collect();
    crate::said(&named)
}

fn check_image(candidate: &Candidate, images: &[String]) -> Result<(), Error> {
    if images.is_empty() {
        return Ok(());
    }
    match &candidate.place {
        Place::Directory { .. } => Err(Error::Operation(format!(
            "`{}` is the directory component components/{} and belongs to no image; leave out \
             `--image`",
            candidate.path, candidate.path
        ))),
        Place::Inline { .. } => {
            if images.len() > 1 {
                return Err(Error::Operation(format!(
                    "`{}` already exists inline in {}; name that image alone with `--image` or \
                     leave `--image` out",
                    candidate.path,
                    candidate.place.target().unwrap_or_default()
                )));
            }
            let target = candidate.place.target().unwrap_or_default();
            match images[0] == target {
                true => Ok(()),
                false => Err(Error::Operation(format!(
                    "`{}` is inline in {target}, not `{}`; name `--image {target}` or leave \
                     `--image` out",
                    candidate.path, images[0]
                ))),
            }
        }
    }
}

fn resolve(candidates: &[Candidate], path: String, images: &[String]) -> Result<Found, Error> {
    let matching: Vec<&Candidate> = candidates
        .iter()
        .filter(|candidate| candidate.path == path)
        .collect();
    if matching.is_empty() {
        return Ok(Found::New(path));
    }
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
            "`{path}` is defined twice as components/{path} and inline in {file}; `buildc \
             check` refuses the pair"
        )));
    }
    let held: Vec<&Candidate> = match images.is_empty() {
        true => matching.clone(),
        false => matching
            .iter()
            .copied()
            .filter(|candidate| {
                candidate.place.target().is_none()
                    || images
                        .iter()
                        .any(|image| candidate.place.target().as_deref() == Some(image.as_str()))
            })
            .collect(),
    };
    match held.len() {
        1 => {
            check_image(held[0], images)?;
            Ok(Found::Existing(held[0].clone()))
        }
        0 => Err(Error::Operation(format!(
            "`{path}` is not inline in `{}`; it is inline in {}",
            images.join("`, `"),
            said_targets(&matching)
        ))),
        _ => Err(Error::Operation(format!(
            "`{path}` is inline in {}; name one `--image` or choose one copy with `select component`",
            said_targets(&held)
        ))),
    }
}

fn refuse_tracked(root: &Path, path: &str) -> Result<(), Error> {
    let tracked = crate::provenance::lock::tracked(root)?;
    match tracked.into_iter().find(|record| record.path == path) {
        Some(record) => Err(Error::Operation(format!(
            "The `{path}` component is tracking `{}/{}`. To modify it, first detach it from `{}` \
             with `buildc detach component {path}`",
            record.library, record.upstream, record.library
        ))),
        None => Ok(()),
    }
}

fn new_group(
    root: &Path,
    list: &List,
    path: &str,
    names: &[String],
    images: Vec<String>,
    prompt: &Prompt,
) -> Result<Option<NewGroup>, Error> {
    if !crate::model::image::is_local_path(path) {
        return Err(Error::Operation(format!(
            "`{path}` is not a component path; each `/` segment takes lowercase letters, \
             digits and dashes and starts with a letter"
        )));
    }
    refuse_tracked(root, path)?;
    let targets = list.targets();
    let mut selected: Vec<crate::model::image::Target> = Vec::new();
    for image in images {
        let Some(target) = targets.iter().find(|target| target.to_string() == image) else {
            let declared: Vec<String> = targets.iter().map(ToString::to_string).collect();
            return Err(Error::Operation(format!(
                "`{image}` is not a declared image or flavour; the repository declares {}",
                crate::said(&declared)
            )));
        };
        selected.push(target.clone());
    }
    if selected.is_empty() && prompt.asks() && !targets.is_empty() {
        let options: Vec<Choice> = targets
            .iter()
            .map(|target| Choice::new(target.to_string(), ""))
            .collect();
        let answer = prompt.choose_many(&copy::package_group_images(path), &options, &[])?;
        let Answer::Chosen(at) = answer else {
            return Ok(None);
        };
        selected = at
            .iter()
            .filter_map(|at| targets.get(*at).cloned())
            .collect();
    }
    if selected.is_empty() {
        return Ok(Some(NewGroup::Directory));
    }
    let listing = Listing::selected(list, &selected)?;
    listing
        .refuse_duplicate(list, path)
        .map_err(Error::Operation)?;
    Ok(Some(NewGroup::Inline(Inline {
        listing,
        block: format!(" {{\n    {}\n}}", node(names)),
    })))
}

fn args(names: &[String]) -> String {
    names.iter().map(|name| format!(" \"{name}\"")).collect()
}

fn node(names: &[String]) -> String {
    format!("packages{}", args(names))
}

fn values(names: &[String]) -> String {
    names
        .iter()
        .map(|name| format!("\"{name}\""))
        .collect::<Vec<String>>()
        .join("\n")
}

fn spliced(text: &str, insert: PackageInsert, names: &[String]) -> String {
    match insert {
        PackageInsert::Args { at } => {
            let mut out = text.to_string();
            out.insert_str(at, &args(names));
            out
        }
        PackageInsert::Children { close } => {
            crate::declaration::inside(text, close, &values(names))
        }
    }
}

fn read(place: &Place) -> Result<(Option<Source>, PackageLine), String> {
    let file = place.file();
    let text = match std::fs::read_to_string(file) {
        Ok(text) => text,
        // A component directory that holds only scripts has no manifest yet.
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
        mut images: Vec<String>,
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
        for image in images.drain(..) {
            if !unique.contains(&image) {
                unique.push(image);
            }
        }
        let images = unique;
        let list = List::editable(root).map_err(|issues| issues.plain())?;
        crate::provenance::lock::parses(root)?;
        let candidates = candidates(root, &list);
        let group = match component {
            Some(path) => GroupChoice::Name(path),
            None if prompt.draws() => match group_form(&pickable(root, &candidates)?, prompt)? {
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
        let found = match group {
            GroupChoice::Component(candidate) => {
                check_image(&candidate, &images)?;
                Found::Existing(candidate)
            }
            GroupChoice::Name(path) => resolve(&candidates, path, &images)?,
        };
        let chosen = match found {
            Found::Existing(chosen) => chosen,
            Found::New(path) => {
                let Some(group) = new_group(root, &list, &path, &wanted, images, prompt)? else {
                    return Ok(None);
                };
                return Ok(Some(Self {
                    place: Place::Directory {
                        file: layout::manifest(root, &path),
                    },
                    path,
                    source: None,
                    insert: None,
                    already: Vec::new(),
                    names: wanted,
                    creation: match group {
                        NewGroup::Directory => None,
                        NewGroup::Inline(inline) => Some(inline),
                    },
                }));
            }
        };
        let (source, line) = read(&chosen.place)?;
        if let Place::Directory { .. } = &chosen.place {
            refuse_tracked(root, &chosen.path)?;
        }
        let (already, names): (Vec<String>, Vec<String>) = wanted
            .into_iter()
            .partition(|name| line.names.contains(name));
        Ok(Some(Self {
            path: chosen.path,
            place: chosen.place,
            source,
            insert: line.insert,
            already,
            names,
            creation: None,
        }))
    }

    pub fn apply(&self, root: &Path) -> Result<Vec<(PathBuf, Change)>, String> {
        if let Some(Inline { listing, block }) = &self.creation {
            let list = List::editable(root).map_err(|issues| issues.plain())?;
            return listing.apply_declaration(&list, &[(self.path.as_str(), block.as_str())]);
        }
        if !self.already.is_empty() {
            println!(
                "`{}` already lists {}",
                self.path,
                crate::said(&self.already)
            );
        }
        if self.names.is_empty() {
            return Ok(Vec::new());
        }
        let file = self.place.file();
        let Some(source) = &self.source else {
            let text = format!(
                "schema-version {}\n\n{}\n",
                crate::model::image::SCHEMA_VERSION,
                node(&self.names)
            );
            crate::init::put(file, &text)?;
            return Ok(vec![(layout::under(root, file), Change::Created)]);
        };
        let written = match (&self.place, self.insert) {
            (_, Some(insert)) => spliced(source.text(), insert, &self.names),
            (Place::Directory { .. }, None) => {
                crate::declaration::splice(source.text(), None, &node(&self.names))
            }
            (Place::Inline { image, flavour, .. }, None) => {
                append(
                    file,
                    image,
                    &crate::declaration::chain(flavour.as_deref(), &self.path),
                    &node(&self.names),
                )?;
                return Ok(vec![(layout::under(root, file), self.change())]);
            }
        };
        std::fs::write(file, written).map_err(|err| format!("{}: {err}", file.display()))?;
        Ok(vec![(layout::under(root, file), self.change())])
    }

    fn change(&self) -> Change {
        Change::Updated(format!("{} added to `packages`", crate::said(&self.names)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repository(case: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("buildc-add-{case}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        crate::init::put(&root.join("repo.kdl"), "schema-version 1\n").unwrap();
        root
    }

    fn image(root: &Path, text: &str) {
        crate::init::put(&root.join("image.kdl"), text).unwrap();
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
        run(root, names, Some(component), &[], &Prompt::silent())
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
        image(
            &root,
            "schema-version 1\n\nimage \"one\" {\n    tool\n    apps {\n        hidden #true\n    }\n}\n",
        );
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
    fn the_picker_leaves_out_a_tracked_directory_component() {
        let root = repository("pickable");
        image(
            &root,
            "schema-version 1\n\nimage \"one\" {\n    tool {\n        packages \"htop\"\n    }\n}\n",
        );
        manifest(&root, "tool", "schema-version 1\n");
        manifest(&root, "kept", "schema-version 1\n");
        // A filter on the path alone would drop the inline `tool` group.
        // A filter on the lock entry alone would drop the `kept` component.
        crate::init::put(
            &layout::lock(&root),
            "component \"tool\" {\n    library \"buildc\"\n    path \"tool\"\n    content \"abc\"\n    track\n}\ncomponent \"kept\" {\n    library \"buildc\"\n    path \"kept\"\n    content \"abc\"\n}\n",
        )
        .unwrap();
        let list = List::editable(&root)
            .map_err(|issues| issues.plain())
            .unwrap();
        let picked: Vec<(String, bool)> = pickable(&root, &candidates(&root, &list))
            .unwrap()
            .into_iter()
            .map(|candidate| {
                (
                    candidate.path,
                    matches!(candidate.place, Place::Directory { .. }),
                )
            })
            .collect();
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(
            picked,
            [("kept".to_string(), true), ("tool".to_string(), false)]
        );
    }

    #[test]
    fn the_package_location_form_names_the_group_and_the_component_choice() {
        use crate::screen_tests::{assert_screen, screen};
        use common::ui::testing::{FormFrame, FormMode};
        let fields = [
            group_name_field(""),
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
        image(
            &root,
            "schema-version 1\n\nimage \"one\" {\n    plain\n    off\n    visible {\n        description \"Visible\"\n    }\n    off-inline {\n        hidden #false\n    }\n}\n",
        );
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
    }

    #[test]
    fn candidate_options_keep_the_label_and_mark_the_hidden() {
        let candidates = vec![
            Candidate {
                path: "base".into(),
                place: Place::Directory {
                    file: PathBuf::from("components/base/manifest.kdl"),
                },
                label: "base".into(),
                about: "components/base".into(),
                hidden: true,
            },
            Candidate {
                path: "apps".into(),
                place: Place::Directory {
                    file: PathBuf::from("components/apps/manifest.kdl"),
                },
                label: "apps".into(),
                about: "components/apps".into(),
                hidden: false,
            },
        ];
        let options = candidate_options(&candidates);
        assert_eq!(options.len(), 2);
        assert!(options[0].hidden);
        assert_eq!(options[1].label, "apps");
    }

    #[test]
    fn a_new_inline_group_is_refused_where_the_image_lists_the_path() {
        let root = repository("listed-path");
        image(&root, "schema-version 1\n\nimage \"one\" {\n    tools\n}\n");
        let refused = refused(run(
            &root,
            &["htop"],
            Some("tools"),
            &["one"],
            &Prompt::silent(),
        ));
        let text = std::fs::read_to_string(root.join("image.kdl")).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert!(refused.contains("`one` already lists `tools`"), "{refused}");
        assert!(!text.contains("htop"), "{text}");
    }

    #[test]
    fn a_package_group_image_question_names_the_group() {
        assert_eq!(
            copy::package_group_images("tools"),
            "select image to add \"tools\" package group"
        );
    }

    #[test]
    fn an_existing_inline_group_takes_one_image() {
        let root = repository("check-image-two");
        image(
            &root,
            "schema-version 1\n\nimage \"one\" {\n    apps { description \"Apps\" }\n}\n\nimage \"two\" {\n}\n",
        );
        let refused = refused(run(
            &root,
            &["htop"],
            Some("apps"),
            &["one", "two"],
            &Prompt::silent(),
        ));
        std::fs::remove_dir_all(&root).unwrap();
        assert!(
            refused.contains("already exists inline in one")
                && refused.contains("leave `--image` out"),
            "{refused}"
        );
    }

    #[test]
    fn a_named_image_does_not_ask_for_more_images() {
        let root = repository("named-image");
        image(&root, "schema-version 1\n\nimage \"one\" {\n}\n");
        let prompt = Prompt::scripted(vec![]);
        run(&root, &["htop"], Some("tools"), &["one"], &prompt).unwrap();
        let text = std::fs::read_to_string(root.join("image.kdl")).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert!(
            text.contains("    tools {\n        packages \"htop\"\n    }\n"),
            "{text}"
        );
    }

    #[test]
    fn a_new_group_refuses_an_image_and_its_flavour() {
        let root = repository("image-flavour");
        image(
            &root,
            "schema-version 1\n\nimage \"one\" {\n    flavours { dev; }\n    flavour \"dev\" {\n    }\n}\n",
        );
        let refused = refused(run(
            &root,
            &["htop"],
            Some("tools"),
            &["one", "one/dev"],
            &Prompt::silent(),
        ));
        std::fs::remove_dir_all(&root).unwrap();
        assert!(
            refused.contains("inside `one`") && refused.contains("list only one of them"),
            "{refused}"
        );
    }

    #[test]
    fn a_repeated_image_name_is_one_target() {
        let root = repository("duplicate-image");
        image(
            &root,
            "schema-version 1\n\nimage \"one\" {\n    apps { description \"Apps\" }\n}\n",
        );
        run(
            &root,
            &["htop"],
            Some("apps"),
            &["one", "one"],
            &Prompt::silent(),
        )
        .unwrap();
        let text = std::fs::read_to_string(root.join("image.kdl")).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(
            text.matches("apps { description \"Apps\"; packages \"htop\" }")
                .count(),
            1,
            "{text}"
        );
    }

    #[test]
    fn a_new_directory_group_refuses_a_tracked_lock_entry() {
        let root = repository("new-tracked");
        crate::init::put(
            &layout::lock(&root),
            "component \"tool\" {\n    library \"buildc\"\n    path \"tool\"\n    content \"abc\"\n    track\n}\n",
        )
        .unwrap();
        let refused = refused(add(&root, &["htop"], "tool"));
        let exists = root.join("components/tool/manifest.kdl").exists();
        std::fs::remove_dir_all(&root).unwrap();
        assert!(
            refused.contains("is tracking `buildc/tool`")
                && refused.contains("buildc detach component tool"),
            "{refused}"
        );
        assert!(!exists);
    }

    #[test]
    fn an_inline_packages_line_gains_a_name_after_its_last_one() {
        let root = repository("inline-append");
        image(
            &root,
            "schema-version 1\n\nimage \"one\" {\n    apps {\n        description \"Apps\"\n        packages \"firefox\"\n    }\n}\n",
        );
        add(&root, &["htop"], "apps").unwrap();
        let text = std::fs::read_to_string(root.join("image.kdl")).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert!(
            text.contains("        packages \"firefox\" \"htop\"\n"),
            "{text}"
        );
    }

    #[test]
    fn a_one_line_inline_block_stays_on_one_line() {
        let root = repository("inline-one-line");
        image(
            &root,
            "schema-version 1\n\nimage \"one\" { apps { description \"Apps\" } }\n",
        );
        add(&root, &["htop"], "apps").unwrap();
        let text = std::fs::read_to_string(root.join("image.kdl")).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert!(
            text.ends_with("image \"one\" { apps { description \"Apps\"; packages \"htop\" } }\n"),
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
        image(
            &root,
            "schema-version 1\n\nimage \"one\" {\n    apps { description \"Apps\" }\n}\n\nimage \"two\" {\n    apps { description \"Apps\" }\n}\n",
        );
        let refused = refused(add(&root, &["htop"], "apps"));
        assert!(
            refused.contains("inline in one and two") && refused.contains("select component"),
            "{refused}"
        );
        run(&root, &["htop"], Some("apps"), &["two"], &Prompt::silent()).unwrap();
        let text = std::fs::read_to_string(root.join("image.kdl")).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert!(
            text.contains(
                "image \"two\" {\n    apps { description \"Apps\"; packages \"htop\" }\n}"
            ),
            "{text}"
        );
        assert!(
            text.contains("image \"one\" {\n    apps { description \"Apps\" }\n}"),
            "{text}"
        );
    }

    #[test]
    fn a_flavour_names_the_inline_component_by_its_target() {
        let root = repository("flavour");
        image(
            &root,
            "schema-version 1\n\nimage \"one\" {\n    flavours { dev; }\n    flavour \"dev\" {\n        apps { description \"Apps\" }\n    }\n}\n",
        );
        let refused = refused(run(
            &root,
            &["htop"],
            Some("apps"),
            &["one"],
            &Prompt::silent(),
        ));
        assert!(refused.contains("not inline in `one`"), "{refused}");
        run(
            &root,
            &["htop"],
            Some("apps"),
            &["one/dev"],
            &Prompt::silent(),
        )
        .unwrap();
        let text = std::fs::read_to_string(root.join("image.kdl")).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert!(
            text.contains("apps { description \"Apps\"; packages \"htop\" }"),
            "{text}"
        );
    }

    #[test]
    fn an_inline_block_packages_node_gains_a_child() {
        let root = repository("inline-block");
        image(
            &root,
            "schema-version 1\n\nimage \"one\" {\n    apps {\n        packages {\n            \"firefox\"\n        }\n    }\n}\n",
        );
        add(&root, &["htop"], "apps").unwrap();
        let text = std::fs::read_to_string(root.join("image.kdl")).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert!(
            text.contains(
                "        packages {\n            \"firefox\"\n            \"htop\"\n        }\n"
            ),
            "{text}"
        );
    }

    #[test]
    fn an_inline_definition_whose_file_is_gone_is_refused() {
        let root = repository("gone");
        let place = Place::Inline {
            file: root.join("gone.image.kdl"),
            image: "one".to_string(),
            flavour: None,
            spans: Vec::new(),
        };
        let Err(refused) = read(&place) else {
            panic!("an image file that is gone is refused");
        };
        std::fs::remove_dir_all(&root).unwrap();
        assert!(refused.contains("gone.image.kdl"), "{refused}");
    }

    #[test]
    fn a_directory_and_an_inline_definition_are_refused_together() {
        let root = repository("pair");
        image(
            &root,
            "schema-version 1\n\nimage \"one\" {\n    apps { description \"Apps\" }\n}\n",
        );
        manifest(&root, "apps", "schema-version 1\n\npackages \"vim\"\n");
        let refused = refused(add(&root, &["htop"], "apps"));
        std::fs::remove_dir_all(&root).unwrap();
        assert!(
            refused.contains("defined twice")
                && refused.contains("components/apps")
                && refused.contains("image.kdl"),
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
            &["one"],
            &Prompt::silent(),
        ));
        std::fs::remove_dir_all(&root).unwrap();
        assert!(refused.contains("directory component"), "{refused}");
    }

    #[test]
    fn a_lock_that_does_not_parse_refuses_the_edit() {
        let root = repository("broken-lock");
        manifest(&root, "tool", "schema-version 1\n");
        crate::init::put(&layout::lock(&root), "component \"tool\" {\n").unwrap();
        let refused = refused(add(&root, &["htop"], "tool"));
        let manifest = std::fs::read_to_string(root.join("components/tool/manifest.kdl")).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert!(refused.contains("does not parse"), "{refused}");
        assert!(!manifest.contains("htop"), "{manifest}");
    }

    #[test]
    fn a_lock_that_does_not_parse_refuses_an_inline_group() {
        let root = repository("broken-lock-inline");
        image(
            &root,
            "schema-version 1\n\nimage \"one\" {\n    apps {\n        packages \"vim\"\n    }\n}\n",
        );
        crate::init::put(&layout::lock(&root), "component \"apps\" {\n").unwrap();
        let refused = refused(add(&root, &["htop"], "apps"));
        std::fs::remove_dir_all(&root).unwrap();
        assert!(refused.contains("does not parse"), "{refused}");
    }

    #[test]
    fn a_detached_component_takes_packages() {
        let root = repository("detached");
        manifest(&root, "tool", "schema-version 1\n");
        crate::init::put(
            &layout::lock(&root),
            "component \"tool\" {\n    library \"buildc\"\n    path \"tool\"\n    content \"abc\"\n}\n",
        )
        .unwrap();
        let added = add(&root, &["htop"], "tool");
        let manifest = std::fs::read_to_string(root.join("components/tool/manifest.kdl")).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert!(added.is_ok());
        assert!(manifest.contains("htop"), "{manifest}");
    }

    #[test]
    fn a_tracked_component_is_refused_until_it_is_detached() {
        let root = repository("tracked");
        manifest(&root, "tool", "schema-version 1\n");
        crate::init::put(
            &layout::lock(&root),
            "component \"tool\" {\n    library \"buildc\"\n    path \"tools/tool\"\n    content \"abc\"\n    track\n}\n",
        )
        .unwrap();
        let refused = refused(add(&root, &["htop"], "tool"));
        std::fs::remove_dir_all(&root).unwrap();
        assert!(
            refused.contains("is tracking `buildc/tools/tool`")
                && refused.contains("buildc detach component tool"),
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

    #[test]
    fn a_new_group_with_one_image_writes_an_inline_block() {
        let root = repository("new-inline");
        image(&root, "schema-version 1\n\nimage \"one\" {\n}\n");
        run(&root, &["htop"], Some("tools"), &["one"], &Prompt::silent()).unwrap();
        let text = std::fs::read_to_string(root.join("image.kdl")).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert!(
            text.contains("    tools {\n        packages \"htop\"\n    }\n"),
            "{text}"
        );
    }

    #[test]
    fn a_new_group_with_two_images_writes_an_inline_block_in_each() {
        let root = repository("new-inline-two");
        image(
            &root,
            "schema-version 1\n\nimage \"one\" {\n}\n\nimage \"two\" {\n}\n",
        );
        run(
            &root,
            &["htop"],
            Some("tools"),
            &["one", "two"],
            &Prompt::silent(),
        )
        .unwrap();
        let text = std::fs::read_to_string(root.join("image.kdl")).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(
            text.matches("    tools {\n        packages \"htop\"\n    }\n")
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
        run(&root, &[], Some("tool"), &[], &prompt).unwrap();
        let text = std::fs::read_to_string(root.join("components/tool/manifest.kdl")).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert!(text.ends_with("packages \"nano\" \"curl\"\n"), "{text}");
    }

    #[test]
    fn an_empty_group_name_uses_apps() {
        let root = repository("default-group");
        let prompt = Prompt::scripted(vec![String::new()]);
        run(&root, &["htop"], None, &[], &prompt).unwrap();
        let text = std::fs::read_to_string(root.join("components/apps/manifest.kdl")).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(text, "schema-version 1\n\npackages \"htop\"\n");
    }

    #[test]
    fn a_scripted_flow_selects_an_image_for_a_new_group() {
        let root = repository("scripted-image");
        image(&root, "schema-version 1\n\nimage \"one\" {\n}\n");
        let prompt = Prompt::scripted(["tools".into(), "1".into()].to_vec());
        run(&root, &["htop"], None, &[], &prompt).unwrap();
        let text = std::fs::read_to_string(root.join("image.kdl")).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert!(
            text.contains("    tools {\n        packages \"htop\"\n    }\n"),
            "{text}"
        );
    }

    #[test]
    fn a_scripted_flow_takes_the_group_name() {
        let root = repository("scripted-group");
        manifest(&root, "alpha", "schema-version 1\n");
        let prompt = Prompt::scripted(["alpha"].map(str::to_string).to_vec());
        run(&root, &["htop"], None, &[], &prompt).unwrap();
        let alpha = std::fs::read_to_string(root.join("components/alpha/manifest.kdl")).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert!(alpha.ends_with("packages \"htop\"\n"), "{alpha}");
    }

    #[test]
    fn no_terminal_refuses_a_missing_component_flag() {
        let root = repository("silent");
        manifest(&root, "alpha", "schema-version 1\n");
        manifest(&root, "beta", "schema-version 1\n");
        let refused = refused(run(&root, &["htop"], None, &[], &Prompt::silent()));
        std::fs::remove_dir_all(&root).unwrap();
        assert!(refused.contains("give `--component`"), "{refused}");
    }

    #[test]
    fn no_terminal_refuses_a_missing_component_flag_with_one_component() {
        let root = repository("silent-one");
        manifest(&root, "alpha", "schema-version 1\n");
        let refused = refused(run(&root, &["htop"], None, &[], &Prompt::silent()));
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
        let refused = refused(run(&root, &["htop"], None, &["one"], &Prompt::silent()));
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
