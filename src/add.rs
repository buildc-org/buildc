use crate::copy;
use crate::declaration::append;
use crate::diag::{Source, Span};
use crate::layout;
use crate::model::image::List;
use crate::parse;
use crate::parse::component::{PackageInsert, PackageLine};
use common::prompt::Prompt;
use common::ui::tree::Change;
use common::ui::Choice;
use std::path::{Path, PathBuf};

pub struct Packages {
    path: String,
    place: Place,
    /// `None` says the directory component has no manifest file yet.
    source: Option<Source>,
    insert: Option<PackageInsert>,
    already: Vec<String>,
    names: Vec<String>,
}

enum Place {
    Directory {
        /// A component that holds only scripts has no manifest at this path yet.
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
}

struct Candidate {
    path: String,
    place: Place,
    label: String,
    about: String,
    target: Option<String>,
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
            Some(Candidate {
                label: path.clone(),
                about: format!("components/{path}"),
                path,
                place: Place::Directory {
                    file: dir.join(layout::MANIFEST_FILE),
                },
                target: None,
            })
        })
        .collect();
    for image in &list.images {
        for entry in &image.entries {
            let Some(spans) = &entry.inline else {
                continue;
            };
            let target = crate::model::image::Target {
                image: image.id.clone(),
                flavour: entry
                    .flavour
                    .clone()
                    .unwrap_or_else(|| crate::model::image::NO_FLAVOUR.to_string()),
            }
            .to_string();
            found.push(Candidate {
                label: format!("{} ({target})", entry.path),
                about: image.src.name().to_string(),
                path: entry.path.clone(),
                place: Place::Inline {
                    file: PathBuf::from(image.src.name()),
                    image: image.id.clone(),
                    flavour: entry.flavour.clone(),
                    spans: spans.clone(),
                },
                target: Some(target),
            });
        }
    }
    found.sort_by(|left, right| {
        (left.path.as_str(), left.target.as_deref())
            .cmp(&(right.path.as_str(), right.target.as_deref()))
    });
    found
}

fn targets(candidates: &[Candidate]) -> String {
    let named: Vec<String> = candidates
        .iter()
        .filter_map(|candidate| candidate.target.clone())
        .collect();
    crate::said(&named)
}

fn pick(found: Vec<Candidate>, prompt: &Prompt) -> Result<Option<Candidate>, String> {
    if found.is_empty() {
        return Err(
            "this repository holds no component; `buildc create component <name>` writes one"
                .to_string(),
        );
    }
    if found.len() == 1 {
        return Ok(found.into_iter().next());
    }
    if !prompt.asks() {
        return Err("give `--component` with the component that receives the packages".to_string());
    }
    let options: Vec<Choice> = found
        .iter()
        .map(|candidate| Choice::new(&candidate.label, &candidate.about))
        .collect();
    let Some(at) = prompt.choose(copy::WHICH_COMPONENT, &options)? else {
        return Ok(None);
    };
    Ok(found.into_iter().nth(at))
}

fn chosen_image(
    found: Vec<Candidate>,
    image: &str,
    named: &str,
    prompt: &Prompt,
) -> Result<Option<Candidate>, String> {
    let named_targets = targets(&found);
    let held: Vec<Candidate> = found
        .into_iter()
        .filter(|candidate| candidate.target.as_deref() == Some(image))
        .collect();
    if held.is_empty() {
        return Err(format!(
            "`{named}` is not inline in `{image}`; it is inline in {named_targets}"
        ));
    }
    pick(held, prompt)
}

fn one_inline(
    found: Vec<Candidate>,
    named: &str,
    prompt: &Prompt,
) -> Result<Option<Candidate>, String> {
    if found.len() == 1 {
        return Ok(found.into_iter().next());
    }
    let named_targets = targets(&found);
    if !prompt.asks() {
        return Err(format!(
            "`{named}` is inline in {named_targets}; name one with `--image`"
        ));
    }
    pick(found, prompt)
}

fn choose(
    found: Vec<Candidate>,
    component: Option<String>,
    image: Option<String>,
    prompt: &Prompt,
) -> Result<Option<Candidate>, String> {
    let Some(named) = component else {
        if let Some(image) = image {
            return Err(format!(
                "`--image` needs `--component`; give the inline component that `{image}` holds"
            ));
        }
        if !prompt.asks() {
            return Err(
                "give `--component` with the component that receives the packages".to_string(),
            );
        }
        return pick(found, prompt);
    };
    if !crate::model::image::is_local_path(&named) {
        return Err(format!(
            "`{named}` is not a component path; each `/` segment takes lowercase letters, \
             digits and dashes and starts with a letter"
        ));
    }
    let matching: Vec<Candidate> = found
        .into_iter()
        .filter(|candidate| candidate.path == named)
        .collect();
    if matching.is_empty() {
        return Err(format!(
            "`{named}` is not a component this repository holds; `buildc create component \
             {named}` writes one"
        ));
    }
    let (inline, directory): (Vec<Candidate>, Vec<Candidate>) = matching
        .into_iter()
        .partition(|candidate| candidate.target.is_some());
    if let Some(directory) = directory.into_iter().next() {
        if !inline.is_empty() {
            return Err(format!(
                "`{named}` is defined twice, as components/{named} and inline in {}; `buildc \
                 check` refuses the pair",
                inline[0].place.file().display()
            ));
        }
        if let Some(image) = image {
            return Err(format!(
                "`{named}` is the directory component components/{named} and belongs to no \
                 image; leave out `--image {image}`"
            ));
        }
        return Ok(Some(directory));
    }
    match image {
        Some(image) => chosen_image(inline, &image, &named, prompt),
        None => one_inline(inline, &named, prompt),
    }
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
        image: Option<String>,
        prompt: &Prompt,
    ) -> Result<Option<Self>, String> {
        let names: Vec<String> = match names.is_empty() {
            true => prompt
                .text(
                    None,
                    copy::PACKAGE_NAMES,
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
                return Err(format!(
                    "`{name}` is not a package name because it {problem}"
                ));
            }
            if !wanted.contains(&name) {
                wanted.push(name);
            }
        }
        let list = List::editable(root).map_err(|issues| issues.plain())?;
        let Some(chosen) = choose(candidates(root, &list), component, image, prompt)? else {
            return Ok(None);
        };
        let (source, line) = read(&chosen.place)?;
        if let Place::Directory { .. } = &chosen.place {
            if crate::provenance::lock::entry(root, &chosen.path)
                .is_some_and(|record| record.tracked)
            {
                return Err(format!(
                    "components/{} tracks its library and an edit would no longer match its \
                     lock entry; run `buildc set component {} --untracked` first",
                    chosen.path, chosen.path
                ));
            }
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
        }))
    }

    pub fn apply(&self, root: &Path) -> Result<Vec<(PathBuf, Change)>, String> {
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
        image: Option<&str>,
        prompt: &Prompt,
    ) -> Result<Vec<(PathBuf, Change)>, String> {
        Packages::collect(
            root,
            names.iter().map(|name| (*name).to_string()).collect(),
            component.map(str::to_string),
            image.map(str::to_string),
            prompt,
        )?
        .ok_or_else(|| "the flow was cancelled".to_string())?
        .apply(root)
    }

    fn add(root: &Path, names: &[&str], component: &str) -> Result<Vec<(PathBuf, Change)>, String> {
        run(root, names, Some(component), None, &Prompt::silent())
    }

    fn refused(result: Result<Vec<(PathBuf, Change)>, String>) -> String {
        match result {
            Ok(_) => panic!("the command was expected to refuse"),
            Err(error) => error,
        }
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
            refused.contains("inline in one and two") && refused.contains("--image"),
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
            Some("one"),
            &Prompt::silent(),
        ));
        assert!(refused.contains("not inline in `one`"), "{refused}");
        run(
            &root,
            &["htop"],
            Some("apps"),
            Some("one/dev"),
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
        // The image list holds entries only from files that exist.
        // The case builds the place by hand to reach an image file that is gone.
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
            Some("one"),
            &Prompt::silent(),
        ));
        std::fs::remove_dir_all(&root).unwrap();
        assert!(refused.contains("directory component"), "{refused}");
    }

    #[test]
    fn a_tracked_component_is_refused_until_it_is_untracked() {
        let root = repository("tracked");
        manifest(&root, "tool", "schema-version 1\n");
        crate::init::put(
            &layout::lock(&root),
            "component \"tool\" {\n    library \"buildc\"\n    path \"tool\"\n    content \"abc\"\n    track\n}\n",
        )
        .unwrap();
        let refused = refused(add(&root, &["htop"], "tool"));
        std::fs::remove_dir_all(&root).unwrap();
        assert!(
            refused.contains("tracks its library")
                && refused.contains("buildc set component tool --untracked"),
            "{refused}"
        );
    }

    #[test]
    fn an_unknown_component_names_the_command_that_writes_one() {
        let root = repository("unknown");
        let refused = refused(add(&root, &["htop"], "nope"));
        std::fs::remove_dir_all(&root).unwrap();
        assert!(
            refused.contains("not a component this repository holds")
                && refused.contains("buildc create component nope"),
            "{refused}"
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
    fn a_picker_chooses_the_component_when_the_flag_is_absent() {
        let root = repository("picker");
        manifest(&root, "alpha", "schema-version 1\n");
        manifest(&root, "beta", "schema-version 1\n");
        let prompt = Prompt::scripted(["2"].map(str::to_string).to_vec());
        run(&root, &["htop"], None, None, &prompt).unwrap();
        let alpha = std::fs::read_to_string(root.join("components/alpha/manifest.kdl")).unwrap();
        let beta = std::fs::read_to_string(root.join("components/beta/manifest.kdl")).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(alpha, "schema-version 1\n");
        assert!(beta.ends_with("packages \"htop\"\n"), "{beta}");
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
    fn the_picker_leaves_out_a_directory_whose_name_is_no_component_path() {
        let root = repository("picker-path");
        manifest(&root, "alpha", "schema-version 1\n");
        manifest(&root, "Bad_Name", "schema-version 1\n");
        let prompt = Prompt::scripted(["1"].map(str::to_string).to_vec());
        run(&root, &["htop"], None, None, &prompt).unwrap();
        let alpha = std::fs::read_to_string(root.join("components/alpha/manifest.kdl")).unwrap();
        let bad = std::fs::read_to_string(root.join("components/Bad_Name/manifest.kdl")).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert!(alpha.ends_with("packages \"htop\"\n"), "{alpha}");
        assert_eq!(bad, "schema-version 1\n");
    }

    #[test]
    fn an_image_without_a_component_flag_is_refused() {
        let root = repository("image-alone");
        manifest(&root, "tool", "schema-version 1\n");
        let refused = refused(run(&root, &["htop"], None, Some("one"), &Prompt::silent()));
        std::fs::remove_dir_all(&root).unwrap();
        assert!(refused.contains("needs `--component`"), "{refused}");
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
