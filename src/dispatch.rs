use crate::command::{Spec, Verb};
use crate::prompt::Prompt;
use crate::Command;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};

pub const USAGE_ERROR: u8 = 1;
pub const REPO_ERROR: u8 = 2;

pub enum Error {
    Invocation(String),
    Operation(String),
}

impl Error {
    pub fn message(&self) -> &str {
        match self {
            Self::Invocation(message) | Self::Operation(message) => message,
        }
    }
}

impl From<String> for Error {
    fn from(message: String) -> Self {
        Self::Operation(message)
    }
}

pub struct Flags {
    pub root: Option<PathBuf>,
    pub owner: Option<String>,
    pub host: Option<String>,
    pub images: Vec<String>,
    pub module: Option<String>,
    pub cn: Option<String>,
    pub base: Option<String>,
    pub format: Option<String>,
    pub target: Option<String>,
    pub datastream: Option<PathBuf>,
    pub baseline: Option<PathBuf>,
    pub base_scan: Option<PathBuf>,
    pub tags: Vec<String>,
    pub kernel: Option<String>,
    pub backend: Option<String>,
    pub oci_output: Option<String>,
    pub secrets: Vec<String>,
    pub pkgs: Vec<String>,
    pub with: Vec<(String, String)>,
    pub cache_to: bool,
    pub no_cache_from: bool,
}

fn one_name(rest: &[&str], spec: &Spec) -> Result<Option<String>, Error> {
    match rest {
        [] => Ok(None),
        [name] => Ok(Some((*name).to_string())),
        _ => Err(Error::Invocation(format!(
            "`{}` takes one name, not {}",
            spec.name(),
            rest.join(" ")
        ))),
    }
}

fn repo_root(given: Option<PathBuf>) -> Result<PathBuf, Error> {
    let root = match given {
        Some(root) => root,
        None => {
            let here = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            let found = crate::find_root(&here).ok_or_else(|| {
                Error::Invocation(format!(
                    "no repo.kdl in {} or any parent directory",
                    here.display()
                ))
            })?;
            match here.strip_prefix(&found).map(|d| d.components().count()) {
                Ok(0) => PathBuf::from("."),
                Ok(up) => (0..up).map(|_| "..").collect(),
                Err(_) => found,
            }
        }
    };
    note_pin(&root);
    Ok(root)
}

static PINNED: AtomicBool = AtomicBool::new(false);

fn note_pin(root: &Path) {
    if PINNED.swap(true, Ordering::Relaxed) {
        return;
    }
    if let Some(version) = crate::parse::repo::pinned_elsewhere(root) {
        eprintln!(
            "buildc: this repository pins buildc {version} and this is {}, so what it generates may \
             differ; `buildc generate` writes this release's output, `buildc-version` moves the pin, \
             and `scripts/buildc.sh` fetches the pinned release",
            crate::model::image::BUILDC_VERSION
        );
    }
}

fn why_on_host(command: Command, arg: Option<&str>) -> Result<ExitCode, Error> {
    use crate::provenance::build::{MANIFEST, RECORD};

    let Some(path) = arg else {
        return Err(Error::Invocation(
            "`why` needs a module: `buildc why <module>`".into(),
        ));
    };
    let (manifest, record) = crate::emit::why::baked(Path::new(MANIFEST), Path::new(RECORD))
        .map_err(|err| {
            Error::Invocation(format!(
                "{err}\n\nno repo.kdl here and no baked manifest either, so there is nothing to \
                 answer from"
            ))
        })?;

    let Some(why) = crate::emit::why::on_host(&manifest, record.as_ref(), path) else {
        let known = crate::emit::why::known_on_host(&manifest);
        return Err(Error::Invocation(format!(
            "`{path}` is not a module this image carries\n\nmodules: {}",
            known.join(", ")
        )));
    };
    print!(
        "{}",
        match command {
            Command::Why => why.markdown(),
            _ => why.json().render(),
        }
    );
    Ok(ExitCode::SUCCESS)
}

fn open(given: Option<PathBuf>) -> Result<Option<PathBuf>, Error> {
    let root = repo_root(given)?;
    let refused =
        crate::compatible(&root).report(&root.join(crate::layout::REPO_FILE).display().to_string());
    Ok((!refused).then_some(root))
}

fn collection_repo(
    given: Option<PathBuf>,
) -> Result<Option<(PathBuf, crate::model::image::List)>, Error> {
    let root = repo_root(given)?;
    let (list, issues, context) = crate::declarations(&root);
    Ok((!issues.report(&context)).then_some((root, list)))
}

pub fn dispatch(
    spec: &Spec,
    rest: &[&str],
    flags: Flags,
    prompt: &Prompt,
) -> Result<ExitCode, Error> {
    let Flags {
        root: root_arg,
        owner,
        host,
        images,
        module: module_arg,
        cn,
        base,
        format,
        target,
        datastream,
        baseline,
        base_scan,
        tags,
        kernel,
        backend,
        oci_output,
        secrets,
        pkgs,
        with,
        cache_to,
        no_cache_from,
    } = flags;
    match spec.verb {
        Verb::Upgrade => {
            if let [word, ..] = rest {
                return Err(Error::Invocation(format!(
                    "`{}` does not take {word}",
                    spec.name()
                )));
            }
            crate::upgrade::run()?;
            Ok(ExitCode::SUCCESS)
        }
        Verb::CreateRepo => {
            let name = one_name(rest, spec)?;
            let image = images.last().cloned();
            crate::create::Repo::collect(name, host, owner, image, base, root_arg, prompt)?
                .apply()?;
            Ok(ExitCode::SUCCESS)
        }
        Verb::CreateImage => {
            let name = one_name(rest, spec)?;
            let Some(root) = open(root_arg)? else {
                return Ok(ExitCode::from(REPO_ERROR));
            };
            let repo = crate::create::named_after_root(&root).unwrap_or_default();
            let url = owner.map(|owner| {
                format!(
                    "{}/{repo}",
                    crate::create::origin(crate::create::HOST, &owner)
                )
            });
            let wrote = crate::create::Image::collect(
                &root,
                name,
                base,
                &repo,
                url,
                "a name argument",
                prompt,
            )?
            .apply(&root)?;
            crate::create::report(&root, &wrote);
            Ok(ExitCode::SUCCESS)
        }
        Verb::CreateFlavour => {
            let name = one_name(rest, spec)?;
            let Some(root) = open(root_arg)? else {
                return Ok(ExitCode::from(REPO_ERROR));
            };
            let wrote =
                crate::create::Flavour::collect(&root, name, images, prompt)?.apply(&root)?;
            crate::create::report(&root, &wrote);
            Ok(ExitCode::SUCCESS)
        }
        Verb::CreateModule => {
            let name = one_name(rest, spec)?;
            let Some(root) = open(root_arg)? else {
                return Ok(ExitCode::from(REPO_ERROR));
            };
            let wrote = crate::create::Module::collect(&root, name, pkgs, with, images, prompt)?
                .apply(&root)?;
            crate::create::report(&root, &wrote);
            Ok(ExitCode::SUCCESS)
        }
        Verb::CreateKey => {
            let kind = one_name(rest, spec)?;
            let Some(root) = open(root_arg)? else {
                return Ok(ExitCode::from(REPO_ERROR));
            };
            crate::key::Key::collect(&root, kind, module_arg, cn, prompt)?.apply(&root)?;
            Ok(ExitCode::SUCCESS)
        }
        Verb::ImportModule => {
            let name = one_name(rest, spec)?;
            let Some((root, list)) = collection_repo(root_arg)? else {
                return Ok(ExitCode::from(REPO_ERROR));
            };
            crate::import::Module::collect(
                name,
                &root,
                &list.sources,
                list.audit_enforce,
                images,
                datastream.as_deref(),
                prompt,
            )?
            .apply(&root)?;
            Ok(ExitCode::SUCCESS)
        }
        Verb::CopyModule => {
            let name = one_name(rest, spec)?;
            let Some((root, list)) = collection_repo(root_arg)? else {
                return Ok(ExitCode::from(REPO_ERROR));
            };
            crate::import::Copy::collect(
                name,
                &root,
                &list.sources,
                list.audit_enforce,
                images,
                prompt,
            )?
            .apply(&root, &list.sources)?;
            Ok(ExitCode::SUCCESS)
        }
        Verb::SetWorkflows => {
            if let [word, ..] = rest {
                return Err(Error::Invocation(format!(
                    "`{}` does not take {word}",
                    spec.name()
                )));
            }
            let Some(root) = open(root_arg)? else {
                return Ok(ExitCode::from(REPO_ERROR));
            };
            if !prompt.asks() {
                return Err(Error::Invocation(crate::set::BY_HAND.to_string()));
            }
            let list = crate::load(&root).list;
            let on: Vec<&str> = list.workflows.iter().map(|w| w.name.as_str()).collect();
            let Some(set) = crate::set::Workflows::collect(
                &crate::resolve::workflow::Basis::of(&list),
                &on,
                list.workflows_at,
                prompt,
            )?
            else {
                return Ok(ExitCode::SUCCESS);
            };
            let wrote = set.apply(&root)?;
            crate::create::report(&root, &wrote);
            println!("\nnext, in {}:\n\x20 buildc generate\n", root.display());
            Ok(ExitCode::SUCCESS)
        }
        Verb::SetConforms => {
            let name = one_name(rest, spec)?;
            let Some(root) = open(root_arg)? else {
                return Ok(ExitCode::from(REPO_ERROR));
            };
            if !prompt.asks() {
                return Err(Error::Invocation(crate::set::CONFORMS_BY_HAND.to_string()));
            }
            let Some(set) = crate::set::Conforms::collect(&root, name, datastream, prompt)? else {
                return Ok(ExitCode::SUCCESS);
            };
            let wrote = set.apply(&root)?;
            crate::create::report(&root, &wrote);
            Ok(ExitCode::SUCCESS)
        }
        Verb::SetClaims => {
            let named = one_name(rest, spec)?.ok_or_else(|| {
                Error::Invocation(format!("`{}` takes the module to claim for", spec.name()))
            })?;
            let Some(root) = open(root_arg)? else {
                return Ok(ExitCode::from(REPO_ERROR));
            };
            if !prompt.asks() {
                return Err(Error::Invocation(crate::set::CLAIMS_BY_HAND.to_string()));
            }
            let Some(set) = crate::set::Claims::collect(&root, &named, datastream, prompt)? else {
                return Ok(ExitCode::SUCCESS);
            };
            let wrote = set.apply(&root)?;
            crate::create::report(&root, &wrote);
            Ok(ExitCode::SUCCESS)
        }
        Verb::FetchModules => {
            let root = repo_root(root_arg)?;
            let (list, issues, context) = crate::declarations(&root);
            if issues.report(&context) {
                return Ok(ExitCode::from(REPO_ERROR));
            }
            for line in crate::fetch::modules(&root, &list)? {
                eprintln!("buildc: {line}");
            }
            Ok(ExitCode::SUCCESS)
        }
        Verb::Build => {
            let opts = crate::build::Options {
                target: one_name(rest, spec)?.or(target),
                kernel,
                tags,
                secrets,
                backend,
                oci_output,
                no_cache_from,
                cache_to,
            };
            Ok(match crate::build::run(&repo_root(root_arg)?, &opts)? {
                crate::build::Stopped::Repository => ExitCode::from(REPO_ERROR),
            })
        }
        Verb::RegistryNamespace => {
            println!("{}", crate::registry::namespace(&repo_root(root_arg)?)?);
            Ok(ExitCode::SUCCESS)
        }
        Verb::RegistryRef => {
            let root = repo_root(root_arg)?;
            let (list, issues, context) = crate::declarations(&root);
            if issues.report(&context) {
                return Ok(ExitCode::from(REPO_ERROR));
            }
            println!(
                "{}",
                crate::registry::reference(&list, &root, target.as_deref(), tags.last())?
            );
            Ok(ExitCode::SUCCESS)
        }
        Verb::ScapContent => Ok(
            match crate::scap::content(&repo_root(root_arg)?, target.as_deref())? {
                crate::scap::Verdict::Clean => ExitCode::SUCCESS,
                crate::scap::Verdict::Wrong => ExitCode::from(REPO_ERROR),
            },
        ),
        Verb::Scap => {
            let [arf] = rest else {
                return Err(Error::Invocation(
                    "`scap` takes one report: `buildc scap <arf.xml>`, or `scap content`".into(),
                ));
            };
            let opts = crate::scap::Options {
                target,
                datastream,
                baseline,
                base: base_scan,
            };
            Ok(
                match crate::scap::run(&repo_root(root_arg)?, Path::new(arf), &opts)? {
                    crate::scap::Verdict::Clean => ExitCode::SUCCESS,
                    crate::scap::Verdict::Wrong => ExitCode::from(REPO_ERROR),
                },
            )
        }
        Verb::OsRelease => {
            crate::runtime::os_release()?;
            Ok(ExitCode::SUCCESS)
        }
        Verb::BuildRecord => {
            crate::provenance::build::write()?;
            Ok(ExitCode::SUCCESS)
        }
        Verb::ValidateImage => {
            crate::runtime::validate_image()?;
            Ok(ExitCode::SUCCESS)
        }
        Verb::Fetch => {
            crate::runtime::fetch(rest)?;
            Ok(ExitCode::SUCCESS)
        }
        _ => reading(spec, rest, format.as_deref(), root_arg, datastream, prompt),
    }
}

fn coverage(
    run: &mut crate::Run,
    json: bool,
    arg: Option<&str>,
    datastream: &Path,
) -> Result<(), Error> {
    let content = crate::scap::content_of(datastream)?;

    let image = match arg {
        Some(id) => match run.list.images.iter().find(|image| image.id == id) {
            Some(image) => image,
            None => return Ok(()),
        },
        None => match run.list.default_image() {
            Some(image) => image,
            None => return Ok(()),
        },
    };
    if image.conforms.is_empty() {
        run.issues.push(
            crate::diag::Issue::new(format!("`{}` declares no `conforms`", image.id), &image.src)
                .at(image.span, "this image is measured against nothing")
                .help(
                    "`conforms \"<profile>\"` is the profile a scan measures it against, and what \
                 this read-out is coverage of",
                ),
        );
        return Ok(());
    }
    let Some(read_out) = crate::emit::coverage::of(image, &content, &run.index) else {
        run.issues.push(
            crate::diag::Issue::new(
                format!(
                    "`{}` conforms to `{}`, which is none of the profiles this datastream carries",
                    image.id, image.conforms
                ),
                &image.src,
            )
            .at(image.span, "this is what it is measured against")
            .help(format!(
                "the datastream carries: {}",
                crate::scap::profile_names(&content)
            )),
        );
        return Ok(());
    };
    let (stdout, tables) = match json {
        true => (read_out.json().render(), read_out.tables()),
        false => (read_out.markdown(), read_out.tables()),
    };
    run.stdout = stdout;
    run.tables = tables;
    Ok(())
}

fn reading(
    spec: &Spec,
    rest: &[&str],
    format: Option<&str>,
    root_arg: Option<PathBuf>,
    datastream: Option<PathBuf>,
    prompt: &Prompt,
) -> Result<ExitCode, Error> {
    let command = spec.verb.reads().expect("a command run reads");
    let command = match (command, format) {
        (Command::Graph, None | Some("md")) => Command::Graph,
        (Command::Graph, Some("json")) => Command::GraphJson,
        (Command::Why, None | Some("md")) => Command::Why,
        (Command::Why, Some("json")) => Command::WhyJson,
        (Command::Coverage, None | Some("md")) => Command::Coverage,
        (Command::Coverage, Some("json")) => Command::CoverageJson,
        (Command::Graph | Command::Why | Command::Coverage, Some(other)) => {
            return Err(Error::Invocation(format!(
                "`--format` is md or json, not `{other}`"
            )));
        }
        (command, _) => command,
    };

    let arg = match rest {
        [] => None,
        ["--json"] if command == Command::Plan => None,
        [one] if command.arg().is_some() => Some(*one),
        _ => {
            return Err(Error::Invocation(format!(
                "`{}` does not take {}",
                spec.name(),
                rest.join(" ")
            )));
        }
    };

    if matches!(command, Command::Coverage | Command::CoverageJson) && datastream.is_none() {
        return Err(Error::Invocation(
            "`coverage` reads the profile out of a SCAP datastream, and never probes this machine \
             for one: `buildc coverage --datastream <file>`\n\nhelp: `buildc scap content` prints the \
             path a scan of this repository uses"
                .into(),
        ));
    }

    if matches!(command, Command::Why | Command::WhyJson) && repo_root(root_arg.clone()).is_err() {
        return why_on_host(command, arg);
    }

    let root = repo_root(root_arg)?;
    let picks = matches!(
        command,
        Command::Why | Command::WhyJson | Command::Coverage | Command::CoverageJson
    ) && arg.is_none()
        && prompt.draws();
    let mut chosen = None;
    let mut run = if picks {
        let loaded = crate::load(&root);
        let (question, known) = match command {
            Command::Coverage | Command::CoverageJson => (
                "which image",
                loaded.list.images.iter().map(|i| i.id.clone()).collect(),
            ),
            _ => ("which module", crate::emit::why::known(&loaded.list)),
        };
        if known.is_empty() {
            crate::run_loaded(command, None, &root, loaded)
        } else {
            let options = known
                .iter()
                .map(|name| crate::ui::Choice::new(name, ""))
                .collect::<Vec<_>>();
            let Some(at) = prompt.choose(question, &options)? else {
                return Ok(ExitCode::SUCCESS);
            };
            chosen = Some(known[at].clone());
            crate::run_loaded(command, chosen.as_deref(), &root, loaded)
        }
    } else {
        crate::run(command, arg, &root)
    };
    let arg = chosen.as_deref().or(arg);

    if matches!(command, Command::Coverage | Command::CoverageJson) {
        coverage(
            &mut run,
            command == Command::CoverageJson,
            arg,
            datastream
                .as_deref()
                .expect("coverage checked its datastream"),
        )?;
    }
    if run.issues.report(&run.context) {
        return Ok(ExitCode::from(REPO_ERROR));
    }
    if command == Command::Generate {
        crate::write_generated(&root, &run.files)?;
    }
    print!(
        "{}",
        match matches!(command, Command::Graph | Command::Coverage) && prompt.draws() {
            false => run.stdout,
            true => run
                .tables
                .iter()
                .map(|t| crate::ui::table::render(&t.title, t.header, &t.rows))
                .collect::<Vec<String>>()
                .join("\n"),
        }
    );
    if command == Command::Check {
        for shadow in &run.shadowed {
            eprintln!(
                "buildc: {}/{} replaces the tool's own entry for {}",
                shadow.collection,
                crate::base::BASES_FILE,
                shadow.image
            );
        }
        for name in &run.unpinned {
            eprintln!(
                "buildc: `{name}` is unpinned, so a fetch from it takes whatever its ref holds \
                 then, unverified"
            );
        }
        for name in &run.modified {
            eprintln!("buildc: `{name}` has been edited since it was imported");
        }
        for line in crate::scap::conformance(&run.list, &run.index, datastream.as_deref())? {
            eprintln!("buildc: {line}");
        }
        match run.images {
            0 => eprintln!("buildc: no image yet; `buildc create image <name>` writes one"),
            _ => eprintln!(
                "buildc: {} images, {} modules, {} flavours{}",
                run.images,
                run.modules,
                run.flavours,
                match run.suppressed {
                    0 => String::new(),
                    n => format!(", {n} the base already provides"),
                }
            ),
        }
    }
    if matches!(command, Command::Coverage | Command::CoverageJson) {
        if let Some(table) = run.tables.first() {
            eprintln!(
                "buildc: {} of {} rules are unclaimed",
                table.rows.iter().filter(|(_, defect)| *defect).count(),
                table.rows.len()
            );
        }
        match run.index.unsearched() {
            clause if clause.is_empty() => {}
            clause => eprintln!("buildc: {clause}"),
        }
    }
    if command == Command::Generate && run.files.is_empty() {
        eprintln!("buildc: nothing to generate; `buildc create image <name>` writes an image");
    }
    if command == Command::Verify {
        let count = run.files.len();
        eprintln!(
            "buildc: {count} generated file{} match the manifests",
            if count == 1 { "" } else { "s" }
        );
    }
    Ok(ExitCode::SUCCESS)
}
