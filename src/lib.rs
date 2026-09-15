pub mod base;
pub mod build;
pub mod command;
pub mod copy;
pub mod create;
pub mod diag;
pub mod dispatch;
pub mod emit;
pub mod fetch;
pub mod import;
pub mod init;
pub mod install;
pub mod key;
pub mod layout;
pub mod model;
pub mod parse;
pub mod prompt;
pub mod provenance;
pub mod provider;
pub mod registry;
pub mod resolve;
pub mod runtime;
pub mod scap;
pub mod set;
pub mod ui;
pub mod upgrade;
pub mod vm;

pub use command::{Arg, Command};
use diag::Issue;
use diag::Issues;
use diag::Source;
use model::image::{List, Target};
use model::module::Module;
pub use parse::repo::compatible;
use resolve::Resolved;
use std::path::{Path, PathBuf};

pub fn find_root(from: &Path) -> Option<PathBuf> {
    from.ancestors()
        .find(|dir| dir.join(layout::REPO_FILE).is_file())
        .map(Path::to_path_buf)
}

pub fn declarations(root: &Path) -> (List, Issues, String) {
    let (list, issues) = List::load(root);
    let context = context(&list, root);
    (list, issues, context)
}

fn context(list: &List, root: &Path) -> String {
    match list.files.is_empty() {
        true => root.display().to_string(),
        false => list.files.join(", "),
    }
}

pub struct Run {
    pub stdout: String,
    pub parts: Vec<emit::Part>,
    pub files: Vec<(PathBuf, String)>,
    pub issues: Issues,
    pub context: String,
    pub images: usize,
    pub modules: usize,
    pub suppressed: usize,
    pub flavours: usize,
    pub shadowed: Vec<base::Shadow>,
    pub unpinned: Vec<String>,
    pub modified: Vec<String>,
    pub(crate) index: provider::Index,
    pub(crate) list: List,
    pub(crate) resolved: Vec<Resolved>,
}

fn skeleton(root: &Path, needs_tail: bool, issues: &mut Issues) -> Option<String> {
    use emit::containerfile::{BEGIN, END, SKELETON, TAIL_BEGIN, TAIL_END};

    let text = std::fs::read_to_string(root.join(SKELETON)).ok()?;
    let src = Source::new(SKELETON, text.clone());
    let mut found = true;
    let markers = if needs_tail {
        &[BEGIN, END, TAIL_BEGIN, TAIL_END][..]
    } else {
        &[BEGIN, END][..]
    };
    for marker in markers {
        if !text.lines().any(|line| line == *marker) {
            issues.push(
                Issue::new(format!("`{SKELETON}` has no `{marker}` line"), &src)
                    .help("the generated module layers go between the two markers"),
            );
            found = false;
        }
    }
    if needs_tail && !text.contains("--mount=type=tmpfs,target=/run") {
        issues.push(
            Issue::new(
                format!("`{SKELETON}` does not isolate `/run` from the final image"),
                &src,
            )
            .help("mount `/run` as tmpfs in the final `validate-image` RUN before adding a UKI"),
        );
        found = false;
    }
    found.then_some(text)
}

pub(crate) struct Loaded {
    list: List,
    resolved: Vec<Resolved>,
    workflows: Vec<resolve::workflow::Declared>,
    facts: Vec<&'static str>,
    index: provider::Index,
    issues: Issues,
    context: String,
}

pub(crate) fn load(root: &Path) -> Loaded {
    let (mut list, mut issues) = List::load(root);
    let context = context(&list, root);

    let disk = parse::disk::Disk::scan(root);
    let index = provider::Index::scan(root, &list.sources, &disk, false);
    parse::module::check_unlisted(&list, root, &disk, &mut issues);

    let mut resolved: Vec<Resolved> = Vec::new();
    for image in &mut list.images {
        let mut entries = std::mem::take(&mut image.entries);
        for entry in &mut entries {
            entry.module = Module::load(entry, image, root, &mut issues);
        }
        image.entries = entries;

        resolve::graph::suppress(image);
        let order = resolve::order::sort(image, &mut issues);
        resolve::order::apply(image, &order);
        resolve::graph::check_graph(image, &index, &mut issues);
        resolve::graph::check_fragments(image, &mut issues);
        let shipped = resolve::overlay::index(image, &disk);
        resolve::overlay::check(image, &shipped, &mut issues);
        let collected = resolve::collect::resolve_collects(image, root, &disk, &mut issues);

        resolved.push(Resolved { shipped, collected });
    }

    let basis = resolve::workflow::Basis::of(&list);
    let workflows = resolve::workflow::resolve(&list, &basis, &mut issues);
    let facts = resolve::workflow::facts(&basis, list.scans_scheduled, list.publishes_scheduled);

    Loaded {
        list,
        resolved,
        workflows,
        facts,
        index,
        issues,
        context,
    }
}

pub fn run(command: Command, arg: Option<&str>, root: &Path) -> Run {
    run_loaded(command, arg, root, load(root))
}

pub(crate) fn run_loaded(command: Command, arg: Option<&str>, root: &Path, loaded: Loaded) -> Run {
    let (target_arg, image_arg, module_arg) = match command.arg() {
        Some(Arg::Target) => (arg, None, None),
        Some(Arg::Module) => (None, None, arg),
        _ => (None, arg, None),
    };
    let Loaded {
        list,
        resolved,
        workflows,
        facts,
        index,
        mut issues,
        context,
    } = loaded;

    let (shadowed, unpinned, modified) = match command {
        Command::Check => (
            base::catalog(root, &list.sources, &mut issues).1,
            list.sources
                .iter()
                .filter(|c| c.unpinned())
                .map(|c| c.name.clone())
                .collect(),
            provenance::record::modified(root),
        ),
        _ => (Vec::new(), Vec::new(), Vec::new()),
    };
    if list.audit_enforce {
        for name in &modified {
            issues.push(
                Issue::new(
                    format!("`{name}` no longer matches the record it was imported with"),
                    &list.repo_src,
                )
                .help(
                    "`audit { enforce #true }` makes a fork an error; re-import it, or drop the \
                     `provenance.kdl` beside its manifest to own the module outright",
                ),
            );
        }
    }

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
        Command::Graph | Command::GraphJson | Command::Section => image_arg.is_none(),
        Command::Why | Command::WhyJson => false,
        Command::Coverage | Command::CoverageJson => image_arg.is_none(),
        Command::Plan => true,
        Command::Check | Command::Generate | Command::Verify => false,
    };
    if needs_default {
        if let Some(issue) = list.no_default() {
            issues.push(issue);
        }
    }

    let target = target_arg
        .map(str::to_string)
        .or_else(|| list.default_target().map(|t| t.to_string()));

    let one = match image_arg {
        Some(id) => list.images.iter().position(|i| i.id == id),
        None => list
            .default_image()
            .and_then(|d| list.images.iter().position(|i| i.id == d.id)),
    };

    let needs_tail = list
        .images
        .iter()
        .flat_map(|image| image.modules())
        .any(|module| module.fragment_position == model::module::Position::Tail);
    let skeleton = skeleton(root, needs_tail, &mut issues);

    let mut files: Vec<(PathBuf, String)> = Vec::new();
    if matches!(command, Command::Generate | Command::Verify) {
        files.extend(emit::LIBRARIES.iter().map(|(name, body)| {
            (
                PathBuf::from(layout::GENERATED)
                    .join(layout::LIB)
                    .join(name),
                (*body).to_string(),
            )
        }));
        files.extend(
            emit::SCRIPTS
                .iter()
                .map(|(path, body)| (PathBuf::from(path), (*body).to_string())),
        );
        for (image, resolved) in list.images.iter().zip(&resolved) {
            if let Some(skeleton) = &skeleton {
                let (section, tails) = emit::containerfile::sections(image, root);
                files.push((
                    emit::containerfile::path(image),
                    emit::containerfile::file(skeleton, image, &section, &tails),
                ));
            }
            files.extend(emit::module_build::scripts(
                image,
                &resolved.collected,
                root,
            ));
            files.push(emit::finalize::script(image, &resolved.collected, root));
            files.extend(emit::graph::files(image));
        }
        files.extend(emit::seed::file(&list));
        files.push((
            PathBuf::from(layout::GENERATED).join("plan.json"),
            emit::plan::build(&list, &resolved, &workflows).render(),
        ));
        files.extend(workflows.iter().map(|declared| {
            (
                PathBuf::from(layout::WORKFLOW_DIR).join(&declared.file),
                emit::workflows::render(declared.body, declared.schedule.as_deref(), &facts),
            )
        }));
    }

    if command == Command::Verify {
        verify(root, &files, &mut issues);
    }

    let mut parts = Vec::new();
    let stdout = match command {
        Command::Plan => emit::plan::build(&list, &resolved, &workflows).render(),
        Command::Summary => target
            .and_then(|name| emit::summary::render(&list, &name))
            .unwrap_or_default(),
        Command::Sbom => target
            .and_then(|name| emit::sbom::build(&list, &name))
            .map(|json| json.render())
            .unwrap_or_default(),
        Command::Graph | Command::GraphJson => match one {
            Some(i) => {
                let graph = emit::graph::of(&list.images[i]);
                match command {
                    Command::Graph => {
                        parts = graph.parts();
                        graph.markdown()
                    }
                    _ => graph.json().render(),
                }
            }
            None => String::new(),
        },
        Command::Section => match one {
            Some(i) => emit::containerfile::section(&list.images[i], root),
            None => String::new(),
        },
        Command::Generate => files
            .iter()
            .map(|(path, _)| format!("{}\n", path.display()))
            .collect(),
        Command::Why | Command::WhyJson => match module_arg {
            Some(given) => match emit::why::matching(&emit::why::known(&list), given).as_slice() {
                [path] => match emit::why::of(&list, path, root) {
                    Some(why) => match command {
                        Command::Why => {
                            parts = why.parts(true);
                            why.markdown()
                        }
                        _ => why.json().render(),
                    },
                    None => {
                        issues.push(
                            Issue::new(
                                format!("`{path}` is listed but was not read"),
                                &list.repo_src,
                            )
                            .help("the problem reading it is above"),
                        );
                        String::new()
                    }
                },
                [] => {
                    let known = emit::why::display(&emit::why::known(&list));
                    issues.push(
                        Issue::new(
                            format!("`{given}` is not a module this repository lists"),
                            &list.repo_src,
                        )
                        .help(match known.is_empty() {
                            true => "no image lists a module yet".to_string(),
                            false => format!("modules: {}", known.join(", ")),
                        }),
                    );
                    String::new()
                }
                paths => {
                    issues.push(
                        Issue::new(
                            format!("`{given}` names more than one module"),
                            &list.repo_src,
                        )
                        .help(format!("modules: {}", paths.join(", "))),
                    );
                    String::new()
                }
            },
            None => {
                issues.push(
                    Issue::new("`why` needs a module", &list.repo_src)
                        .help("`buildc why <module>`, the path an image lists it under"),
                );
                String::new()
            }
        },
        Command::Check | Command::Verify | Command::Coverage | Command::CoverageJson => {
            String::new()
        }
    };

    Run {
        stdout,
        parts,
        files,
        issues,
        context,
        images: list.images.len(),
        modules: list.images.iter().map(|i| i.modules().count()).sum(),
        suppressed: list.images.iter().map(|i| i.suppressed.len()).sum(),
        flavours: list.images.iter().map(|i| i.flavours.len()).sum(),
        shadowed,
        unpinned,
        modified,
        index,
        list,
        resolved,
    }
}

fn verify(root: &Path, files: &[(PathBuf, String)], issues: &mut Issues) {
    const HELP: &str = "run `buildc generate` and commit what it writes";

    let mut absent: Vec<String> = Vec::new();
    for (path, generated) in files {
        let name = path.display().to_string();
        let Ok(found) = std::fs::read_to_string(root.join(path)) else {
            absent.push(name);
            continue;
        };
        if found == *generated {
            continue;
        }
        let (span, line) = difference(&found, generated);
        issues.push(
            Issue::new(
                format!("`{name}` is not what this repository generates"),
                &Source::new(&name, found),
            )
            .at(span, line)
            .help(HELP),
        );
    }

    match absent.as_slice() {
        [] => {}
        [name] => issues
            .push(Issue::new(format!("`{name}` is not there"), &Source::new(name, "")).help(HELP)),
        many => issues.push(
            Issue::new(
                format!("{} generated files are not there", many.len()),
                &Source::new(layout::GENERATED, ""),
            )
            .help(format!("{HELP}; it writes all {} of them", many.len())),
        ),
    }

    let stale = tracked(&layout::generated(root))
        .into_iter()
        .filter_map(|path| path.strip_prefix(root).map(Path::to_path_buf).ok())
        .filter(|path| !files.iter().any(|(emitted, _)| emitted == path))
        .chain(resolve::workflow::orphans(root, files));
    for path in stale {
        let name = path.display().to_string();
        issues.push(
            Issue::new(
                format!("nothing generates `{name}`"),
                &Source::new(&name, ""),
            )
            .help("delete it, or declare what it belongs to"),
        );
    }
}

pub fn write_generated(root: &Path, files: &[(PathBuf, String)]) -> Result<(), String> {
    let dir = layout::generated(root);
    if dir.is_dir() {
        std::fs::remove_dir_all(&dir).map_err(|err| format!("{}: {err}", dir.display()))?;
    }
    for path in resolve::workflow::orphans(root, files) {
        let path = root.join(path);
        std::fs::remove_file(&path).map_err(|err| format!("{}: {err}", path.display()))?;
    }
    for (path, text) in files {
        let path = root.join(path);
        let dir = path.parent().unwrap_or(&path);
        std::fs::create_dir_all(dir).map_err(|err| format!("{}: {err}", dir.display()))?;
        std::fs::write(&path, text).map_err(|err| format!("{}: {err}", path.display()))?;
    }
    for (path, _) in emit::SCRIPTS {
        use std::os::unix::fs::PermissionsExt;
        let path = root.join(path);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .map_err(|err| format!("{}: {err}", path.display()))?;
    }
    Ok(())
}

fn difference(found: &str, generated: &str) -> (diag::Span, String) {
    let mut offset = 0;
    let mut theirs = found.lines();
    let mut ours = generated.lines();
    loop {
        match (theirs.next(), ours.next()) {
            (Some(a), Some(b)) if a == b => offset += a.len() + 1,
            (a, b) => {
                let span = diag::Span {
                    offset,
                    len: a.map_or(0, str::len),
                };
                let label = match b {
                    Some(line) => format!("generated: {line}"),
                    None => "generated: nothing, the file ends above".to_string(),
                };
                return (span, label);
            }
        }
    }
}

pub(crate) fn tracked(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();
    let mut out = Vec::new();
    for path in paths {
        if path.is_dir() {
            out.extend(tracked(&path));
        } else {
            out.push(path);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tail_requires_an_ephemeral_run_directory() {
        let root = std::env::temp_dir().join(format!("buildc-skeleton-{}", std::process::id()));
        let path = root.join(emit::containerfile::SKELETON);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let stale = include_str!("../assets/scripts/Containerfile.skeleton").replace(
            "--mount=type=tmpfs,target=/run",
            "--mount=type=cache,target=/run",
        );
        std::fs::write(&path, stale).unwrap();

        assert!(skeleton(&root, false, &mut Issues::default()).is_some());
        let mut issues = Issues::default();
        assert!(skeleton(&root, true, &mut issues).is_none());
        assert!(issues
            .findings()
            .iter()
            .any(|message| message.contains("does not isolate `/run`")));

        std::fs::remove_dir_all(root).unwrap();
    }
}
