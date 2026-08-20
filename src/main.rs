use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use buildc::command::{self, Spec, Verb};
use buildc::model::image::BUILDC_VERSION;
use buildc::prompt::Prompt;
use buildc::ui::Choice;
use buildc::Command;

const COMMANDS: &str = "You can find the available commands by typing 'buildc' or 'buildc --help'";

static GREETED: AtomicBool = AtomicBool::new(false);

fn banner(failing: bool) {
    if GREETED.swap(true, Ordering::Relaxed) {
        return;
    }
    match failing {
        true => eprintln!("Buildc v{BUILDC_VERSION}\n"),
        false => println!("Buildc v{BUILDC_VERSION}\n"),
    }
}

const USAGE_ERROR: u8 = 1;
const REPO_ERROR: u8 = 2;

enum Error {
    Invocation(String),
    Operation(String),
}

impl Error {
    fn message(&self) -> &str {
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

fn in_repo() -> bool {
    std::env::current_dir()
        .ok()
        .and_then(|here| buildc::find_root(&here))
        .is_some()
}

struct Args {
    words: Vec<String>,
    given: Vec<&'static str>,
}

impl Args {
    fn flags(&mut self, flag: &'static str) -> Result<Vec<String>, Error> {
        let mut values = Vec::new();
        let mut i = 0;
        while i < self.words.len() {
            let taken = if let Some(v) = self.words[i].strip_prefix(&format!("--{flag}=")) {
                values.push(v.to_string());
                1
            } else if self.words[i] == format!("--{flag}") {
                values.push(
                    self.words
                        .get(i + 1)
                        .ok_or_else(|| Error::Invocation(format!("`--{flag}` takes a value")))?
                        .clone(),
                );
                2
            } else {
                i += 1;
                continue;
            };
            self.words.drain(i..i + taken);
        }
        if !values.is_empty() {
            self.given.push(flag);
        }
        Ok(values)
    }

    fn flag(&mut self, flag: &'static str) -> Result<Option<String>, Error> {
        Ok(self.flags(flag)?.pop())
    }

    fn switch(&mut self, flag: &str) -> bool {
        let before = self.words.len();
        self.words.retain(|arg| arg != &format!("--{flag}"));
        self.words.len() != before
    }

    fn only(&self, spec: &Spec) -> Result<(), Error> {
        match self.given.iter().find(|flag| !spec.takes.contains(flag)) {
            Some(flag) => Err(Error::Invocation(format!(
                "`{}` does not take `--{flag}`",
                spec.name()
            ))),
            None => Ok(()),
        }
    }
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
    if let Some(root) = given {
        return Ok(root);
    }
    let here = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let found = buildc::find_root(&here).ok_or_else(|| {
        Error::Invocation(format!(
            "no repo.kdl in {} or any parent directory",
            here.display()
        ))
    })?;
    Ok(
        match here.strip_prefix(&found).map(|d| d.components().count()) {
            Ok(0) => PathBuf::from("."),
            Ok(up) => (0..up).map(|_| "..").collect(),
            Err(_) => found,
        },
    )
}

fn why_on_host(command: Command, arg: Option<&str>) -> Result<ExitCode, Error> {
    use buildc::provenance::build::{MANIFEST, RECORD};

    let Some(path) = arg else {
        return Err(Error::Invocation(
            "`why` needs a module: `buildc why <module>`".into(),
        ));
    };
    let (manifest, record) = buildc::emit::why::baked(Path::new(MANIFEST), Path::new(RECORD))
        .map_err(|err| {
            Error::Invocation(format!(
                "{err}\n\nno repo.kdl here and no baked manifest either, so there is nothing to \
                 answer from"
            ))
        })?;

    let Some(why) = buildc::emit::why::on_host(&manifest, record.as_ref(), path) else {
        let known = buildc::emit::why::known_on_host(&manifest);
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
        buildc::compatible(&root).report(&root.join(buildc::layout::REPO_FILE).display().to_string());
    Ok((!refused).then_some(root))
}

struct Import {
    from: buildc::import::Found,
    dest: PathBuf,
    path: String,
    listing: buildc::create::Listing,
}

impl Import {
    fn collect(
        name: Option<String>,
        root: &Path,
        sources: &[buildc::model::remote::Collection],
        enforce: bool,
        images: Vec<String>,
        prompt: &Prompt,
    ) -> Result<Self, Error> {
        let name = match name {
            Some(name) => name,
            None => buildc::import::choose(root, sources, prompt)?,
        };
        let mut found = buildc::import::find(root, sources, &name, enforce)?;
        let module = buildc::import::split(&name).1;
        let at = match found.as_slice() {
            [_] => 0,
            many => {
                let owners: Vec<String> = many.iter().map(|f| f.owner.clone()).collect();
                let listed = owners.join(", ");
                let options: Vec<Choice> =
                    owners.iter().map(|owner| Choice::new(owner, "")).collect();
                prompt
                    .choose(&format!("`{module}` is in {listed}; which one"), &options)?
                    .ok_or_else(|| {
                        format!(
                            "`{module}` is in {listed}; name which one, as `{}/{module}`",
                            owners[0]
                        )
                    })?
            }
        };
        let from = found.swap_remove(at);
        let dest = buildc::import::destination(root, &from, module)?;
        let path = format!("{}/{module}", from.owner);
        let listing = buildc::create::Listing::collect(root, images, prompt)?;
        Ok(Self {
            from,
            dest,
            path,
            listing,
        })
    }

    fn apply(
        &self,
        root: &Path,
        sources: &[buildc::model::remote::Collection],
    ) -> Result<(), String> {
        let mut wrote: Vec<(PathBuf, buildc::create::Change)> =
            buildc::import::vendor(root, sources, &self.from, &self.dest)?
                .into_iter()
                .map(|path| (path, buildc::create::Change::Created))
                .collect();
        wrote.extend(self.listing.apply(&self.path)?);
        buildc::create::report(root, &wrote);
        Ok(())
    }
}

fn write_generated(root: &Path, files: &[(PathBuf, String)]) -> Result<(), String> {
    let dir = buildc::layout::generated(root);
    if dir.is_dir() {
        std::fs::remove_dir_all(&dir).map_err(|err| format!("{}: {err}", dir.display()))?;
    }
    for (path, text) in files {
        let path = root.join(path);
        let dir = path.parent().unwrap_or(&path);
        std::fs::create_dir_all(dir).map_err(|err| format!("{}: {err}", dir.display()))?;
        std::fs::write(&path, text).map_err(|err| format!("{}: {err}", path.display()))?;
    }
    Ok(())
}

fn main() -> ExitCode {
    unsafe { libc::signal(libc::SIGPIPE, libc::SIG_DFL) };

    match run() {
        Ok(code) => code,
        Err(error) => {
            banner(true);
            let invocation = matches!(error, Error::Invocation(_));
            let message = error.message();
            let stop = match message.ends_with(['.', '!', '?']) || message.contains('\n') {
                true => "",
                false => ".",
            };
            eprintln!("Error: {message}{stop}\n");
            match invocation {
                true => eprint!("{}", command::usage(in_repo())),
                false => eprintln!("{COMMANDS}"),
            }
            ExitCode::from(USAGE_ERROR)
        }
    }
}

fn run() -> Result<ExitCode, Error> {
    let mut args = Args {
        words: std::env::args().skip(1).collect(),
        given: Vec::new(),
    };
    let prompt = Prompt::new(args.switch("no-tui"));
    let cache_to = args.switch("cache-to");
    let no_cache_from = args.switch("no-cache-from");
    let root_arg = args.flag("root")?.map(PathBuf::from);
    let owner = args.flag("owner")?;
    let host = args.flag("host")?;
    let images = args.flags("image")?;
    let module_arg = args.flag("module")?;
    let cn = args.flag("cn")?;
    let base = args.flag("base")?;
    let format = args.flag("format")?;
    let target = args.flag("target")?;
    let datastream = args.flag("datastream")?;
    let baseline = args.flag("baseline")?;
    let tags = args.flags("tag")?;
    let kernel = args.flag("kernel")?;
    let backend = args.flag("backend")?;
    let oci_output = args.flag("oci-output")?;
    let secrets = args.flags("secret")?;
    let pkgs = args.flags("pkg")?;
    let with = args
        .flags("with")?
        .iter()
        .map(|pair| match pair.split_once('=') {
            Some((verb, value)) => Ok((verb.to_string(), value.to_string())),
            None => Err(Error::Invocation(format!(
                "`--with` is `verb=value`, not `{pair}`"
            ))),
        })
        .collect::<Result<Vec<_>, Error>>()?;

    let words: Vec<&str> = args.words.iter().map(String::as_str).collect();
    match words.first() {
        None => {
            eprint!("{}", command::usage(in_repo()));
            return Ok(ExitCode::from(USAGE_ERROR));
        }
        Some(&"-h") | Some(&"--help") => {
            print!("{}", command::usage(in_repo()));
            return Ok(ExitCode::SUCCESS);
        }
        Some(_) => {}
    }

    let (spec, rest) = command::resolve(&words).map_err(Error::Invocation)?;
    args.only(spec)?;
    if matches!(
        spec.verb,
        Verb::CreateRepo
            | Verb::CreateImage
            | Verb::CreateModule
            | Verb::CreateKey
            | Verb::ImportModule
            | Verb::Check
    ) {
        banner(false);
    }

    match spec.verb {
        Verb::CreateRepo => {
            let name = one_name(rest, spec)?;
            let image = images.last().cloned();
            buildc::create::Repo::collect(name, host, owner, image, base, root_arg, &prompt)?
                .apply()?;
            Ok(ExitCode::SUCCESS)
        }
        Verb::CreateImage => {
            let name = one_name(rest, spec)?;
            let Some(root) = open(root_arg)? else {
                return Ok(ExitCode::from(REPO_ERROR));
            };
            let repo = buildc::create::named_after_root(&root).unwrap_or_default();
            let url = owner.map(|owner| {
                format!(
                    "{}/{repo}",
                    buildc::create::origin(buildc::create::HOST, &owner)
                )
            });
            let wrote = buildc::create::Image::collect(
                &root,
                name,
                base,
                &repo,
                url,
                "a name argument",
                &prompt,
            )?
            .apply(&root)?;
            buildc::create::report(&root, &wrote);
            Ok(ExitCode::SUCCESS)
        }
        Verb::CreateModule => {
            let name = one_name(rest, spec)?;
            let Some(root) = open(root_arg)? else {
                return Ok(ExitCode::from(REPO_ERROR));
            };
            let wrote = buildc::create::Module::collect(&root, name, pkgs, with, images, &prompt)?
                .apply(&root)?;
            buildc::create::report(&root, &wrote);
            Ok(ExitCode::SUCCESS)
        }
        Verb::CreateKey => {
            let kind = one_name(rest, spec)?;
            let Some(root) = open(root_arg)? else {
                return Ok(ExitCode::from(REPO_ERROR));
            };
            buildc::key::Key::collect(&root, kind, module_arg, cn, &prompt)?.apply(&root)?;
            Ok(ExitCode::SUCCESS)
        }
        Verb::ImportModule => {
            let name = one_name(rest, spec)?;
            let root = repo_root(root_arg)?;
            let (list, issues, context) = buildc::declarations(&root);
            if issues.report(&context) {
                return Ok(ExitCode::from(REPO_ERROR));
            }
            Import::collect(
                name,
                &root,
                &list.sources,
                list.audit_enforce,
                images,
                &prompt,
            )?
            .apply(&root, &list.sources)?;
            Ok(ExitCode::SUCCESS)
        }
        Verb::FetchModules => {
            let root = repo_root(root_arg)?;
            let (list, issues, context) = buildc::declarations(&root);
            if issues.report(&context) {
                return Ok(ExitCode::from(REPO_ERROR));
            }
            for line in buildc::fetch::modules(&root, &list)? {
                eprintln!("buildc: {line}");
            }
            Ok(ExitCode::SUCCESS)
        }
        Verb::Build => {
            let opts = buildc::build::Options {
                target: one_name(rest, spec)?.or(target),
                kernel,
                tags,
                secrets,
                backend,
                oci_output,
                no_cache_from,
                cache_to,
            };
            Ok(match buildc::build::run(&repo_root(root_arg)?, &opts)? {
                buildc::build::Stopped::Repository => ExitCode::from(REPO_ERROR),
            })
        }
        Verb::RegistryNamespace => {
            println!("{}", buildc::registry::namespace(&repo_root(root_arg)?)?);
            Ok(ExitCode::SUCCESS)
        }
        Verb::RegistryRef => {
            let root = repo_root(root_arg)?;
            let (list, issues, context) = buildc::declarations(&root);
            if issues.report(&context) {
                return Ok(ExitCode::from(REPO_ERROR));
            }
            println!(
                "{}",
                buildc::registry::reference(&list, &root, target.as_deref(), tags.last())?
            );
            Ok(ExitCode::SUCCESS)
        }
        Verb::ScapContent => Ok(
            match buildc::scap::content(&repo_root(root_arg)?, target.as_deref())? {
                buildc::scap::Verdict::Clean => ExitCode::SUCCESS,
                buildc::scap::Verdict::Wrong => ExitCode::from(REPO_ERROR),
            },
        ),
        Verb::Scap => {
            let [arf] = rest else {
                return Err(Error::Invocation(
                    "`scap` takes one report: `buildc scap <arf.xml>`, or `scap content`".into(),
                ));
            };
            let opts = buildc::scap::Options {
                target,
                datastream: datastream.map(PathBuf::from),
                baseline: baseline.map(PathBuf::from),
            };
            Ok(
                match buildc::scap::run(&repo_root(root_arg)?, Path::new(arf), &opts)? {
                    buildc::scap::Verdict::Clean => ExitCode::SUCCESS,
                    buildc::scap::Verdict::Wrong => ExitCode::from(REPO_ERROR),
                },
            )
        }
        Verb::OsRelease => {
            buildc::runtime::os_release()?;
            Ok(ExitCode::SUCCESS)
        }
        Verb::BuildRecord => {
            buildc::provenance::build::write()?;
            Ok(ExitCode::SUCCESS)
        }
        Verb::ValidateImage => {
            buildc::runtime::validate_image()?;
            Ok(ExitCode::SUCCESS)
        }
        Verb::Fetch => {
            buildc::runtime::fetch(rest)?;
            Ok(ExitCode::SUCCESS)
        }
        _ => reading(spec, rest, format.as_deref(), root_arg),
    }
}

fn reading(
    spec: &Spec,
    rest: &[&str],
    format: Option<&str>,
    root_arg: Option<PathBuf>,
) -> Result<ExitCode, Error> {
    let command = spec.verb.reads().expect("a command run reads");
    let command = match (command, format) {
        (Command::Graph, None | Some("md")) => Command::Graph,
        (Command::Graph, Some("json")) => Command::GraphJson,
        (Command::Why, None | Some("md")) => Command::Why,
        (Command::Why, Some("json")) => Command::WhyJson,
        (Command::Graph | Command::Why, Some(other)) => {
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

    if matches!(command, Command::Why | Command::WhyJson) && repo_root(root_arg.clone()).is_err() {
        return why_on_host(command, arg);
    }

    let root = repo_root(root_arg)?;
    let run = buildc::run(command, arg, &root);

    if run.issues.report(&run.context) {
        return Ok(ExitCode::from(REPO_ERROR));
    }
    if command == Command::Generate {
        write_generated(&root, &run.files)?;
    }
    print!("{}", run.stdout);
    if command == Command::Check {
        for shadow in &run.shadowed {
            eprintln!(
                "buildc: {}/{} replaces the tool's own entry for {}",
                shadow.collection,
                buildc::base::BASES_FILE,
                shadow.image
            );
        }
        for name in &run.unpinned {
            eprintln!(
                "buildc: `{name}` is unpinned, so an import of it takes whatever its ref holds \
                 then, unverified"
            );
        }
        for name in &run.modified {
            eprintln!("buildc: `{name}` has been edited since it was imported");
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
