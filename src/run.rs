use crate::command::{Arg, Command};
use crate::diag::{Issue, Issues};
use crate::emit;
use crate::layout;
use crate::model::image::{Image, List, Target};
use crate::provider;
use crate::resolve::workflow::Declared;
use crate::resolve::Resolved;
use crate::scap;
use std::path::{Path, PathBuf};

pub struct Run {
    pub report: Report,
    pub files: Vec<(PathBuf, String)>,
    pub copies: Vec<emit::resolved::CopyFile>,
    pub problems: Problems,
    pub counts: Counts,
    pub reading: Reading,
}

#[derive(Default)]
pub struct Report {
    pub stdout: String,
    pub parts: Vec<emit::Part>,
}

impl Report {
    fn text(stdout: String) -> Report {
        Report {
            stdout,
            parts: Vec::new(),
        }
    }
}

pub struct Problems {
    pub issues: Issues,
    pub context: String,
    pub unpinned: Vec<String>,
    pub ungoverned: Vec<String>,
}

impl Problems {
    pub fn report(&mut self) -> bool {
        std::mem::take(&mut self.issues).report(&self.context)
    }
}

pub struct Counts {
    pub images: usize,
    pub components: usize,
    pub suppressed: usize,
    pub flavours: usize,
}

pub struct Reading {
    pub list: List,
    pub index: provider::Index,
    pub resolved: Vec<crate::resolve::Resolved>,
}

pub fn run(command: Command, arg: Option<&str>, root: &Path) -> Run {
    run_loaded(command, arg, root, crate::load(root))
}

pub(crate) fn run_loaded(
    command: Command,
    arg: Option<&str>,
    root: &Path,
    loaded: crate::Loaded,
) -> Run {
    let (target_arg, image_arg, component_arg) = match command.arg() {
        Some(Arg::Target) => (arg, None, None),
        Some(Arg::Component) => (None, None, arg),
        Some(Arg::Image) | None => (None, arg, None),
    };
    let crate::Loaded {
        list,
        mut resolved,
        workflows,
        facts,
        index,
        mut issues,
        context,
        ungoverned,
    } = loaded;

    let mut copies = Vec::new();
    if matches!(command, Command::Plan | Command::Generate | Command::Verify) {
        copies = derive_content(&list, &mut resolved, root, &mut issues);
    }

    let mut unpinned = Vec::new();
    if command == Command::Check {
        scap::check_content(&list, &mut issues);
        unpinned = list
            .sources
            .iter()
            .filter(|c| c.unpinned())
            .map(|c| c.name.clone())
            .collect();
    }

    check_names(command, &list, image_arg, target_arg, &mut issues);

    let target = target_arg
        .map(str::to_string)
        .or_else(|| list.default_target().map(|t| t.to_string()));

    let one = match image_arg {
        Some(id) => list.images.iter().position(|i| i.id == id),
        None => list
            .default_image()
            .and_then(|d| list.images.iter().position(|i| i.id == d.id)),
    };

    let skeleton = match command {
        Command::Check | Command::Generate | Command::Verify => crate::skeleton(root, &mut issues),
        _ => None,
    };

    let mut files = Vec::new();
    if matches!(command, Command::Generate | Command::Verify) {
        files = generated_files(
            &list,
            &resolved,
            &workflows,
            &facts,
            root,
            skeleton.as_deref(),
        );
        files.push(emit::snapshot::file(&files, &copies));
    }
    if command == Command::Verify {
        crate::verify(root, &files, &copies, &mut issues);
    }

    let report = match command {
        Command::Plan => Report::text(emit::plan::build(&list, &resolved, &workflows).render()),
        Command::Summary => Report::text(
            target
                .and_then(|name| emit::summary::render(&list, &name))
                .unwrap_or_default(),
        ),
        Command::Sbom => Report::text(
            target
                .and_then(|name| emit::sbom::build(&list, &name))
                .map(|json| json.render())
                .unwrap_or_default(),
        ),
        Command::Graph | Command::GraphJson => one
            .map(|i| graph_report(command, &list.images[i]))
            .unwrap_or_default(),
        Command::Section => Report::text(match one {
            Some(i) => emit::containerfile::section(
                &list.images[i],
                root,
                list.audit_enforce,
                &resolved[i].collected,
            ),
            None => String::new(),
        }),
        Command::Generate => Report::text(
            files
                .iter()
                .map(|(path, _)| format!("{}\n", path.display()))
                .chain(
                    copies
                        .iter()
                        .map(|copy| format!("{}\n", copy.path.display())),
                )
                .collect(),
        ),
        Command::Why | Command::WhyJson => {
            why_report(command, &list, component_arg, root, &mut issues)
        }
        Command::Check | Command::Verify | Command::Coverage | Command::CoverageJson => {
            Report::default()
        }
    };

    let counts = Counts {
        images: list.images.len(),
        components: list.images.iter().map(|i| i.components().count()).sum(),
        suppressed: list.images.iter().map(|i| i.suppressed.len()).sum(),
        flavours: list.images.iter().map(|i| i.flavours.len()).sum(),
    };
    Run {
        report,
        files,
        copies,
        problems: Problems {
            issues,
            context,
            unpinned,
            ungoverned,
        },
        counts,
        reading: Reading {
            list,
            index,
            resolved,
        },
    }
}

fn derive_content(
    list: &List,
    resolved: &mut [Resolved],
    root: &Path,
    issues: &mut Issues,
) -> Vec<emit::resolved::CopyFile> {
    let mut copies: Vec<emit::resolved::CopyFile> = Vec::new();
    let mut unrecorded: Vec<String> = Vec::new();
    for (image, held) in list.images.iter().zip(resolved.iter_mut()) {
        let content = emit::resolved::of(image, &held.collected, root);
        held.hashes = content.hashes;
        copies.extend(content.copies);
        held.clones = crate::provenance::build::clones(&image.components().collect::<Vec<_>>());
        if list.audit_enforce {
            for why in held
                .clones
                .iter()
                .filter_map(|one| one.resolved.as_ref().err())
            {
                if !unrecorded.contains(why) {
                    unrecorded.push(why.clone());
                }
            }
        }
        match scap::pinned::resolve(image, root) {
            Ok(Some((measured, written))) => {
                held.scap = Some(measured);
                copies.extend(written);
            }
            Ok(None) => {}
            Err(why) => issues.push(
                Issue::new(why, &image.src).at(
                    image
                        .base
                        .as_ref()
                        .map_or_else(Default::default, |base| base.span),
                    "the base whose content measures this image",
                ),
            ),
        }
    }
    for why in unrecorded {
        issues.push(Issue::new(
            format!("{why}, and an enforced audit builds nothing it cannot record"),
            &list.repo_src,
        ));
    }
    copies
}

fn check_names(
    command: Command,
    list: &List,
    image_arg: Option<&str>,
    target_arg: Option<&str>,
    issues: &mut Issues,
) {
    if let Some(unknown) = image_arg.filter(|id| !list.images.iter().any(|i| i.id == *id)) {
        let known: Vec<&str> = list.images.iter().map(|i| i.id.as_str()).collect();
        issues.push(
            Issue::new(
                format!("`{unknown}` is not a declared image"),
                &list.repo_src,
            )
            .help(format!("images: {}", known.join(", "))),
        );
    }

    let targets: Vec<String> = list.targets().iter().map(Target::to_string).collect();
    if let Some(unknown) = target_arg.filter(|name| !targets.iter().any(|have| have == name)) {
        issues.push(
            Issue::new(format!("`{unknown}` is not a build target"), &list.repo_src)
                .help(format!("targets: {}", targets.join(", "))),
        );
    }
    let needs_default = match command {
        Command::Summary | Command::Sbom => target_arg.is_none(),
        Command::Graph
        | Command::GraphJson
        | Command::Section
        | Command::Coverage
        | Command::CoverageJson => image_arg.is_none(),
        Command::Plan => true,
        Command::Why | Command::WhyJson | Command::Check | Command::Generate | Command::Verify => {
            false
        }
    };
    if needs_default {
        if let Some(issue) = list.no_default() {
            issues.push(issue);
        }
    }
}

fn generated_files(
    list: &List,
    resolved: &[Resolved],
    workflows: &[Declared],
    facts: &[&str],
    root: &Path,
    skeleton: Option<&str>,
) -> Vec<(PathBuf, String)> {
    let lib = PathBuf::from(layout::GENERATED).join(layout::LIB);
    let mut files: Vec<(PathBuf, String)> = emit::LIBRARIES
        .iter()
        .map(|(name, body)| (lib.join(name), (*body).to_string()))
        .collect();
    let early = list
        .images
        .iter()
        .any(|image| !emit::measure::early(image).is_empty());
    let record = list
        .images
        .iter()
        .any(|image| !emit::measure::record(image, false).is_empty());
    files.extend(
        emit::measure::libraries(emit::measure::Measured { early, record })
            .into_iter()
            .map(|(name, body)| (lib.join(name), body.to_string())),
    );
    files.push((emit::record::path(), emit::record::script()));
    files.extend(
        emit::SCRIPTS
            .iter()
            // A script that the user keeps in the repository's scripts directory takes the place of the generated copy.
            .filter(|script| !root.join(layout::SCRIPTS).join(script.name).is_file())
            .map(|script| {
                (
                    PathBuf::from(layout::GENERATED)
                        .join(layout::SCRIPTS)
                        .join(script.name),
                    emit::located(root, script.body),
                )
            }),
    );
    for (image, resolved) in list.images.iter().zip(resolved) {
        if let Some(skeleton) = skeleton {
            let regions =
                emit::containerfile::sections(image, root, list.audit_enforce, &resolved.collected);
            files.push((
                emit::containerfile::path(image),
                emit::containerfile::file(skeleton, image, &regions),
            ));
        }
        files.extend(emit::component_build::scripts(
            image,
            &resolved.collected,
            root,
        ));
        files.push(emit::finalize::script(image, &resolved.collected, root));
        files.extend(emit::graph::files(image));
    }
    files.extend(emit::seed::file(list));
    files.extend(emit::renovate::file(list).map(|text| (emit::renovate::path(), text)));
    files.push((
        PathBuf::from(layout::GENERATED).join("plan.json"),
        emit::plan::build(list, resolved, workflows).render(),
    ));
    files.extend(workflows.iter().map(|declared| {
        (
            PathBuf::from(layout::WORKFLOW_DIR).join(&declared.file),
            emit::located(
                root,
                &emit::workflows::render(declared.body, declared.schedule.as_deref(), facts),
            ),
        )
    }));
    files
}

fn graph_report(command: Command, image: &Image) -> Report {
    let graph = emit::graph::of(image);
    match command {
        Command::Graph => Report {
            stdout: graph.markdown(),
            parts: graph.parts(),
        },
        Command::Plan
        | Command::Check
        | Command::Generate
        | Command::Verify
        | Command::Section
        | Command::GraphJson
        | Command::Summary
        | Command::Sbom
        | Command::Why
        | Command::WhyJson
        | Command::Coverage
        | Command::CoverageJson => Report::text(graph.json().render()),
    }
}

fn why_report(
    command: Command,
    list: &List,
    given: Option<&str>,
    root: &Path,
    issues: &mut Issues,
) -> Report {
    let Some(given) = given else {
        issues.push(
            Issue::new("`why` needs a component", &list.repo_src)
                .help("`buildc why <component>`, the path an image lists it under"),
        );
        return Report::default();
    };
    let known = emit::why::known(list);
    let matching = emit::why::matching(&known, given);
    let path = match matching.as_slice() {
        [path] => path,
        [] => {
            let known = emit::why::display(&known);
            issues.push(
                Issue::new(
                    format!("`{given}` is not a component this repository lists"),
                    &list.repo_src,
                )
                .help(match known.is_empty() {
                    true => "no image lists a component yet".to_string(),
                    false => format!("components: {}", known.join(", ")),
                }),
            );
            return Report::default();
        }
        paths => {
            issues.push(
                Issue::new(
                    format!("`{given}` names more than one component"),
                    &list.repo_src,
                )
                .help(format!("components: {}", paths.join(", "))),
            );
            return Report::default();
        }
    };
    let Some(why) = emit::why::of(list, path, root) else {
        issues.push(
            Issue::new(
                format!("`{path}` is listed but was not read"),
                &list.repo_src,
            )
            .help("the problem reading it is above"),
        );
        return Report::default();
    };
    match command {
        Command::Why => Report {
            stdout: why.markdown(),
            parts: why.parts(true),
        },
        Command::Plan
        | Command::Check
        | Command::Generate
        | Command::Verify
        | Command::Section
        | Command::Graph
        | Command::GraphJson
        | Command::Summary
        | Command::Sbom
        | Command::WhyJson
        | Command::Coverage
        | Command::CoverageJson => Report::text(why.json().render()),
    }
}
