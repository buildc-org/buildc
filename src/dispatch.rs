use crate::command::{self, Context, Spec, Verb};
use crate::copy;
use crate::prompt::Prompt;
use crate::Command;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};

pub const USAGE_ERROR: u8 = 1;
pub const REPO_ERROR: u8 = 2;

pub enum Error {
    Usage(String),
    Invocation(String),
    Operation(String),
}

impl Error {
    pub fn message(&self) -> &str {
        match self {
            Self::Usage(message) | Self::Invocation(message) | Self::Operation(message) => message,
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
    pub from: Option<String>,
    pub disk: Option<String>,
    pub hostname: Option<String>,
    pub user: Option<String>,
    pub password: Option<String>,
    pub encryption: Option<String>,
    pub passphrase: Option<String>,
    pub base: Option<String>,
    pub format: Option<String>,
    pub target: Option<String>,
    pub datastream: Option<PathBuf>,
    pub baseline: Option<PathBuf>,
    pub base_scan: Option<PathBuf>,
    pub tags: Vec<String>,
    pub kernel: Option<String>,
    pub ram: Option<String>,
    pub backend: Option<String>,
    pub oci_output: Option<String>,
    pub secrets: Vec<String>,
    pub pkgs: Vec<String>,
    pub with: Vec<(String, String)>,
    pub cache_to: bool,
    pub no_cache_from: bool,
    pub rebuild: bool,
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

fn repo_root(here: &Context) -> Result<PathBuf, Error> {
    let root = match here {
        Context::Repo(root) => root.clone(),
        Context::Host => {
            return Err(Error::Invocation(format!(
                "this is a buildc image, so there is no source tree to read\n\nhelp: {} \
                 off the documents the build baked",
                answers()
            )))
        }
        Context::Loose => {
            return Err(Error::Invocation(format!(
                "no repo.kdl in {} or any parent directory",
                std::env::current_dir()
                    .unwrap_or_else(|_| PathBuf::from("."))
                    .display()
            )))
        }
    };
    note_pin(&root);
    Ok(root)
}

fn answers() -> String {
    let rows: Vec<String> = command::on_host()
        .iter()
        .map(|spec| format!("`buildc {}`", spec.label()))
        .collect();
    match rows.split_last() {
        Some((last, [])) => format!("{last} answers"),
        Some((last, rest)) => format!("{} and {last} answer", rest.join(", ")),
        None => "nothing answers".to_string(),
    }
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

fn this_target(named: Option<&str>, scope: &crate::emit::why::Scope) -> Result<(), Error> {
    use crate::emit::why::Scope;
    let Some(named) = named else {
        return Ok(());
    };
    match scope {
        Scope::Built(name) if name == named => Ok(()),
        Scope::Built(name) => Err(Error::Invocation(format!(
            "this image was built as `{name}`, so it cannot answer for `{named}`"
        ))),
        _ => Err(Error::Invocation(
            "the build record does not name a target, so this image can answer only for \
             itself"
                .to_string(),
        )),
    }
}

fn unscoped(spec: &Spec) -> String {
    format!(
        "the build record does not name a target and the baked manifest holds more than one, so \
         `{}` cannot say which of them is running\n\nhelp: `buildc why <module>` still answers, \
         and says that it read across all of them",
        spec.name()
    )
}

fn on_host(
    spec: &Spec,
    rest: &[&str],
    format: Option<&str>,
    target: Option<&str>,
    prompt: &Prompt,
) -> Result<ExitCode, Error> {
    use crate::emit::why::{built_as, image_of};
    use crate::provenance::build::{MANIFEST, RECORD};

    let unwanted = || {
        Error::Invocation(format!(
            "`{}` does not take {}",
            spec.name(),
            rest.join(" ")
        ))
    };
    if spec.verb == Verb::Plan {
        if !matches!(rest, [] | ["--json"]) {
            return Err(unwanted());
        }
        let raw = std::fs::read_to_string(MANIFEST)
            .map_err(|err| Error::Invocation(format!("{MANIFEST}: {err}")))?;
        print!("{raw}");
        return Ok(ExitCode::SUCCESS);
    }

    let (manifest, record) = crate::emit::why::baked(Path::new(MANIFEST), Path::new(RECORD))
        .map_err(Error::Invocation)?;
    let (targets, scope) = built_as(&manifest, record.as_ref());

    if spec.verb == Verb::Why {
        return why_on_host(&manifest, record.as_ref(), rest, format, prompt);
    }

    this_target(
        match (spec.verb, rest) {
            (Verb::ScapContent, []) => target,
            (Verb::Summary, []) => None,
            (Verb::Summary, [named]) => Some(*named),
            _ => return Err(unwanted()),
        },
        &scope,
    )?;
    let [target] = targets.as_slice() else {
        return Err(Error::Invocation(unscoped(spec)));
    };
    match spec.verb {
        Verb::Summary => {
            print!("{}", crate::emit::summary::on_host(target));
            Ok(ExitCode::SUCCESS)
        }
        Verb::ScapContent => {
            let image = image_of(&manifest, target).ok_or_else(|| {
                Error::Invocation("the manifest names no image for this target".to_string())
            })?;
            Ok(match crate::scap::content_on_host(image, target)? {
                crate::scap::Verdict::Clean => ExitCode::SUCCESS,
                crate::scap::Verdict::Wrong => ExitCode::from(REPO_ERROR),
            })
        }
        verb => unreachable!("{verb:?} is not answered on a host"),
    }
}

fn why_on_host(
    manifest: &crate::emit::json::Json,
    record: Option<&crate::emit::json::Json>,
    rest: &[&str],
    format: Option<&str>,
    prompt: &Prompt,
) -> Result<ExitCode, Error> {
    let json = match format {
        Some("json") => true,
        None | Some("md") => false,
        Some(other) => {
            return Err(Error::Invocation(format!(
                "`--format` is md or json, not `{other}`"
            )))
        }
    };
    let [given] = rest else {
        return Err(Error::Invocation(
            "`why` needs a module: `buildc why <module>`".into(),
        ));
    };

    let known = crate::emit::why::known_on_host(manifest, record);
    let path = match crate::emit::why::matching(&known, given).as_slice() {
        [path] => path.clone(),
        [] => {
            return Err(Error::Invocation(format!(
                "`{given}` is not a module this image carries\n\nmodules: {}",
                crate::emit::why::display(&known).join(", ")
            )))
        }
        paths => {
            return Err(Error::Invocation(format!(
                "`{given}` names more than one module\n\nmodules: {}",
                paths.join(", ")
            )))
        }
    };
    let why = crate::emit::why::on_host(manifest, record, &path)
        .expect("the resolved module is one this image carries");
    let parts = why.parts(true);
    print!(
        "{}",
        match json {
            true => why.json().render(),
            false if prompt.draws() && crate::ui::fits(&parts) => crate::ui::parts(&parts),
            false => why.markdown(),
        }
    );
    Ok(ExitCode::SUCCESS)
}

fn open(here: &Context) -> Result<Option<PathBuf>, Error> {
    let root = repo_root(here)?;
    let refused =
        crate::compatible(&root).report(&root.join(crate::layout::REPO_FILE).display().to_string());
    Ok((!refused).then_some(root))
}

fn module_from_collection(
    name: Option<String>,
    root: &std::path::Path,
    list: &crate::model::image::List,
    images: Vec<String>,
    datastream: Option<&std::path::Path>,
    place: crate::import::Place,
    prompt: &Prompt,
) -> Result<(), Error> {
    crate::import::Module::collect(
        name,
        root,
        &list.sources,
        list.audit_enforce,
        images,
        datastream,
        place,
        prompt,
    )?
    .apply(root, &list.sources)?;
    Ok(())
}

fn collection_repo(here: &Context) -> Result<Option<(PathBuf, crate::model::image::List)>, Error> {
    let root = repo_root(here)?;
    let (list, issues, context) = crate::declarations(&root);
    Ok((!issues.report(&context)).then_some((root, list)))
}

pub fn dispatch(
    spec: &Spec,
    rest: &[&str],
    flags: Flags,
    prompt: &Prompt,
    here: &Context,
) -> Result<ExitCode, Error> {
    let Flags {
        root: root_arg,
        owner,
        host,
        images,
        module: module_arg,
        cn,
        from,
        disk,
        hostname,
        user,
        password,
        encryption,
        passphrase,
        base,
        format,
        target,
        datastream,
        baseline,
        base_scan,
        tags,
        kernel,
        ram,
        backend,
        oci_output,
        secrets,
        pkgs,
        with,
        cache_to,
        no_cache_from,
        rebuild,
    } = flags;
    if *here == Context::Host && spec.host {
        return on_host(spec, rest, format.as_deref(), target.as_deref(), prompt);
    }
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
        Verb::Installer => {
            if let [word, ..] = rest {
                return Err(Error::Invocation(format!(
                    "`{}` does not take {word}",
                    spec.name()
                )));
            }
            let root = match from {
                Some(from) => PathBuf::from(from),
                None => crate::install::root()?,
            };
            let found = crate::install::classify(&root)?;
            let payload = found.payload()?;
            eprintln!("buildc: {}, from {}", payload.image, root.display());
            crate::install::own_screen(payload, prompt);
            let given = crate::install::Given {
                disk,
                hostname,
                user,
                password,
                encryption,
                passphrase,
            };
            let Some(answers) = crate::install::Answers::collect(payload, given, prompt)? else {
                return Ok(ExitCode::SUCCESS);
            };
            crate::install::run(payload, &answers, prompt)?;
            Ok(ExitCode::SUCCESS)
        }
        Verb::CreateRepo => {
            let name = one_name(rest, spec)?;
            let image = images.last().cloned();
            if let Some(repo) =
                crate::create::Repo::collect(name, host, owner, image, base, root_arg, prompt)?
            {
                repo.apply()?;
            }
            Ok(ExitCode::SUCCESS)
        }
        Verb::CreateImage => {
            let name = one_name(rest, spec)?;
            let Some(root) = open(here)? else {
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
                crate::create::Field::Image,
                None,
                prompt,
            )?
            .apply(&root)?;
            crate::create::report(&root, &wrote);
            Ok(ExitCode::SUCCESS)
        }
        Verb::CreateFlavour => {
            let name = one_name(rest, spec)?;
            let Some(root) = open(here)? else {
                return Ok(ExitCode::from(REPO_ERROR));
            };
            let wrote =
                crate::create::Flavour::collect(&root, name, images, prompt)?.apply(&root)?;
            crate::create::report(&root, &wrote);
            Ok(ExitCode::SUCCESS)
        }
        Verb::CreateModule => {
            let name = one_name(rest, spec)?;
            let Some(root) = open(here)? else {
                return Ok(ExitCode::from(REPO_ERROR));
            };
            let wrote = crate::create::Module::collect(&root, name, pkgs, with, images, prompt)?
                .apply(&root)?;
            crate::create::report(&root, &wrote);
            Ok(ExitCode::SUCCESS)
        }
        Verb::CreateKey => {
            let kind = one_name(rest, spec)?;
            let Some(root) = open(here)? else {
                return Ok(ExitCode::from(REPO_ERROR));
            };
            crate::key::Key::collect(&root, kind, module_arg, cn, prompt)?.apply(&root)?;
            Ok(ExitCode::SUCCESS)
        }
        Verb::SetKey => {
            let kind = one_name(rest, spec)?;
            let Some(root) = open(here)? else {
                return Ok(ExitCode::from(REPO_ERROR));
            };
            crate::key::Recorded::collect(&root, kind, module_arg, from, prompt)?.apply(&root)?;
            Ok(ExitCode::SUCCESS)
        }
        Verb::ImportModule => {
            let name = one_name(rest, spec)?;
            let Some((root, list)) = collection_repo(here)? else {
                return Ok(ExitCode::from(REPO_ERROR));
            };
            module_from_collection(
                name,
                &root,
                &list,
                images,
                datastream.as_deref(),
                crate::import::Place::Reference,
                prompt,
            )?;
            Ok(ExitCode::SUCCESS)
        }
        Verb::CopyModule => {
            let name = one_name(rest, spec)?;
            let Some((root, list)) = collection_repo(here)? else {
                return Ok(ExitCode::from(REPO_ERROR));
            };
            module_from_collection(
                name,
                &root,
                &list,
                images,
                datastream.as_deref(),
                crate::import::Place::Vendored,
                prompt,
            )?;
            Ok(ExitCode::SUCCESS)
        }
        Verb::SetWorkflows => {
            if let [word, ..] = rest {
                return Err(Error::Invocation(format!(
                    "`{}` does not take {word}",
                    spec.name()
                )));
            }
            let Some(root) = open(here)? else {
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
                list.publishes_scheduled,
                list.scans_scheduled,
                crate::create::Field::Workflows,
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
            let Some(root) = open(here)? else {
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
            let Some(root) = open(here)? else {
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
            let root = repo_root(here)?;
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
            Ok(match crate::build::run(&repo_root(here)?, &opts)? {
                crate::build::Stopped::Repository => ExitCode::from(REPO_ERROR),
            })
        }
        Verb::VmBuild | Verb::VmRun | Verb::VmSpawn => {
            let root = repo_root(here)?;
            let opts = crate::vm::Options {
                target,
                image: images.last().cloned(),
                tag: tags.last().cloned(),
                ram,
                rebuild,
            };
            crate::vm::run(&root, spec, one_name(rest, spec)?.as_deref(), &opts, prompt)?;
            Ok(ExitCode::SUCCESS)
        }
        Verb::RegistryNamespace => {
            println!("{}", crate::registry::namespace(&repo_root(here)?)?);
            Ok(ExitCode::SUCCESS)
        }
        Verb::RegistryRef => {
            let root = repo_root(here)?;
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
        Verb::Recipe => {
            let root = repo_root(here)?;
            let (list, issues, context) = crate::declarations(&root);
            if issues.report(&context) {
                return Ok(ExitCode::from(REPO_ERROR));
            }
            let selected = match target.as_deref() {
                Some(name) => list.find_target(name)?,
                None => list
                    .ungated_target()
                    .ok_or_else(|| "no default image to take a target from".to_string())?,
            }
            .to_string();
            let imgref = crate::registry::reference(&list, &root, Some(&selected), tags.last())?;
            let image = images.last().cloned().unwrap_or_else(|| imgref.clone());
            let recipe = crate::emit::recipe::build(&list, &selected, &image, &imgref, &[])
                .ok_or_else(|| {
                    Error::Invocation(format!(
                        "`{selected}` declares no base family an installer has an answer for"
                    ))
                })?;
            print!("{}", recipe.render());
            Ok(ExitCode::SUCCESS)
        }
        Verb::ScapContent => Ok(
            match crate::scap::content(&repo_root(here)?, target.as_deref())? {
                crate::scap::Verdict::Clean => ExitCode::SUCCESS,
                crate::scap::Verdict::Wrong => ExitCode::from(REPO_ERROR),
            },
        ),
        Verb::ScapRules => {
            let Some(path) = datastream.as_deref() else {
                return Err(Error::Invocation(
                    "`scap rules` needs the content to resolve against: \
                     `buildc scap rules --datastream <file> <number>...`"
                        .into(),
                ));
            };
            let content = crate::scap::content_of(path)?;
            for number in rest {
                if let Some(rule) = content.rules.get(*number) {
                    println!("{}", crate::scap::rule_name(rule));
                }
            }
            Ok(ExitCode::SUCCESS)
        }
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
                match crate::scap::run(&repo_root(here)?, Path::new(arf), &opts)? {
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
        _ => reading(spec, rest, format.as_deref(), datastream, prompt, here),
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
    let (stdout, parts) = match json {
        true => (read_out.json().render(), read_out.parts()),
        false => (read_out.markdown(), read_out.parts()),
    };
    run.stdout = stdout;
    run.parts = parts;
    Ok(())
}

fn reading(
    spec: &Spec,
    rest: &[&str],
    format: Option<&str>,
    datastream: Option<PathBuf>,
    prompt: &Prompt,
    here: &Context,
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

    let root = repo_root(here)?;
    let picks = matches!(
        command,
        Command::Why | Command::WhyJson | Command::Coverage | Command::CoverageJson
    ) && arg.is_none()
        && prompt.draws();
    if command == Command::Generate {
        let (list, issues, _) = crate::declarations(&root);
        if issues.is_empty() {
            for line in crate::fetch::modules(&root, &list)? {
                eprintln!("buildc: {line}");
            }
        }
    }

    let mut chosen = None;
    let mut run = if picks {
        let loaded = crate::load(&root);
        let (question, known, shown) = match command {
            Command::Coverage | Command::CoverageJson => {
                let known: Vec<String> = loaded.list.images.iter().map(|i| i.id.clone()).collect();
                (copy::WHICH_IMAGE, known.clone(), known)
            }
            _ => {
                let known = crate::emit::why::known(&loaded.list);
                let shown = crate::emit::why::display(&known);
                (copy::WHICH_MODULE, known, shown)
            }
        };
        if known.is_empty() {
            crate::run_loaded(command, None, &root, loaded)
        } else {
            let options = shown
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
    let problems = run.issues.report(&run.context);
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
        if let Some(version) = crate::parse::repo::pinned_unverified(&root) {
            eprintln!(
                "buildc: `buildc-version` names {version} with no sha256, so the binary it fetches \
                 is whatever the release holds, unverified"
            );
        }
        for name in &run.modified {
            eprintln!("buildc: `{name}` has been edited since it was imported");
        }
        for line in run.index.hidden() {
            eprintln!("buildc: {line}");
        }
        match run.index.unsearched() {
            clause if clause.is_empty() => {}
            clause => eprintln!("buildc: {clause}"),
        }
        for line in crate::scap::conformance(&run.list, &run.index, datastream.as_deref())? {
            eprintln!("buildc: {line}");
        }
    }
    if problems {
        return Ok(ExitCode::from(REPO_ERROR));
    }
    if command == Command::Generate {
        let wrote: Vec<(PathBuf, crate::create::Change)> = run
            .files
            .iter()
            .map(|(path, _)| {
                let change = match root.join(path).exists() {
                    true => crate::create::Change::Updated(String::new()),
                    false => crate::create::Change::Created,
                };
                (path.clone(), change)
            })
            .collect();
        crate::write_generated(&root, &run.files)?;
        if prompt.draws() {
            crate::create::report(&root, &wrote);
            run.stdout.clear();
        }
    }
    print!(
        "{}",
        match matches!(command, Command::Graph | Command::Why | Command::Coverage)
            && prompt.draws()
            && crate::ui::fits(&run.parts)
        {
            false => run.stdout,
            true => crate::ui::parts(&run.parts),
        }
    );
    if command == Command::Check {
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
        if let Some(table) = run.parts.iter().find_map(|part| match part {
            crate::emit::Part::Table(table) => Some(table),
            _ => None,
        }) {
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
