use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use buildc::model::image::BUILDC_VERSION;
use buildc::prompt::Prompt;
use buildc::ui::Choice;
use buildc::Command;

const HEAD: &str = "usage: buildc [--root <dir>] <command>\n";

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

const CREATE_REPO: &str = "\
\x20 create repo [name]  start a repository for your own images, here or in
                      `--root`. `--owner` is your account or org, on `--host`
";

const IN_REPO: &str = "\
\x20 create image [name] add an image: what it is called, and what it builds on
  create module [name]
                      write a module, with the packages it installs, and offer
                      to list it in an image
  import module [name]
                      copy a module in from a collection repo.kdl declares,
                      choosing from what they hold, and offer to list it in an
                      image
  create key <kind>   generate a key one of this repository's modules declares,
                      such as the one its published images are signed with
  check               read every manifest and say what is wrong with it
  generate            write the build files, and list what was written
  build [target]      verify the build files, then build the image
  section [image]     print the Containerfile section an image generates
  graph [--format md|json]
                      print what provides what, what requires it, and what the
                      base already carries
";

const RULE: &str = "\
Every command takes a flag for everything it needs. What no flag gave is asked
for, and `--no-tui` asks nothing, failing and naming the flag instead.

docs/commands.md is the reference. Data goes to stdout and diagnostics to
stderr; exit 1 is the invocation, exit 2 the repository.
";

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

fn usage(in_repo: bool) -> String {
    match in_repo {
        true => format!("{HEAD}\n{CREATE_REPO}{IN_REPO}\n{RULE}"),
        false => format!(
            "{HEAD}\n{CREATE_REPO}\nthese need a repository, and there is none here or above:\n\n\
             {IN_REPO}\n{RULE}"
        ),
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

    fn only(&self, takes: &[&str], command: &str) -> Result<(), Error> {
        match self.given.iter().find(|flag| !takes.contains(flag)) {
            Some(flag) => Err(Error::Invocation(format!(
                "`{command}` does not take `--{flag}`"
            ))),
            None => Ok(()),
        }
    }
}

fn one_name(rest: &[&str], command: &str) -> Result<Option<String>, Error> {
    match rest {
        [] => Ok(None),
        [name] => Ok(Some((*name).to_string())),
        _ => Err(Error::Invocation(format!(
            "`{command}` takes one name, not {}",
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

fn open(given: Option<PathBuf>) -> Result<Option<PathBuf>, Error> {
    let root = repo_root(given)?;
    let refused = buildc::compatible(&root).report(&root.join("repo.kdl").display().to_string());
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
        images: Vec<String>,
        prompt: &Prompt,
    ) -> Result<Self, Error> {
        let name = match name {
            Some(name) => name,
            None => buildc::import::choose(root, sources, prompt)?,
        };
        let mut found = buildc::import::find(root, sources, &name)?;
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
        buildc::import::vendor(root, sources, &self.from, &self.dest)?;
        println!("imported {}", self.dest.display());
        self.listing.apply(&self.path)
    }
}

fn write_generated(root: &Path, files: &[(PathBuf, String)]) -> Result<(), String> {
    let dir = root.join("generated");
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
                true => eprint!("{}", usage(in_repo())),
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
            eprint!("{}", usage(in_repo()));
            return Ok(ExitCode::from(USAGE_ERROR));
        }
        Some(&"-h") | Some(&"--help") => {
            print!("{}", usage(in_repo()));
            return Ok(ExitCode::SUCCESS);
        }
        Some(_) => {}
    }

    if matches!(
        words.as_slice(),
        ["create", ..] | ["import", ..] | ["check", ..]
    ) {
        banner(false);
    }

    if let ["create", "repo", rest @ ..] = words.as_slice() {
        args.only(&["root", "host", "owner", "image", "base"], "create repo")?;
        let name = one_name(rest, "create repo")?;
        let image = images.last().cloned();
        buildc::create::Repo::collect(name, host, owner, image, base, root_arg, &prompt)?.apply()?;
        return Ok(ExitCode::SUCCESS);
    }

    if let ["fetch", "modules"] = words.as_slice() {
        args.only(&["root"], "fetch modules")?;
        let root = repo_root(root_arg)?;
        let (list, issues, context) = buildc::declarations(&root);
        if issues.report(&context) {
            return Ok(ExitCode::from(REPO_ERROR));
        }
        for line in buildc::fetch::modules(&root, &list)? {
            eprintln!("buildc: {line}");
        }
        return Ok(ExitCode::SUCCESS);
    }

    let in_layer = match words.as_slice() {
        ["os-release"] => Some(buildc::runtime::os_release()),
        ["build-record"] => Some(buildc::provenance::build::write()),
        ["validate-image"] => Some(buildc::runtime::validate_image()),
        ["fetch", rest @ ..] => Some(buildc::runtime::fetch(rest)),
        _ => None,
    };
    if let Some(result) = in_layer {
        result?;
        return Ok(ExitCode::SUCCESS);
    }

    match words.as_slice() {
        ["create", "image", rest @ ..] => {
            args.only(&["root", "owner", "base"], "create image")?;
            let name = one_name(rest, "create image")?;
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
            buildc::create::Image::collect(
                &root,
                name,
                base,
                &repo,
                url,
                "a name argument",
                &prompt,
            )?
            .apply(&root)?;
            return Ok(ExitCode::SUCCESS);
        }
        ["create", "module", rest @ ..] => {
            args.only(&["root", "image", "pkg", "with"], "create module")?;
            let name = one_name(rest, "create module")?;
            let Some(root) = open(root_arg)? else {
                return Ok(ExitCode::from(REPO_ERROR));
            };
            buildc::create::Module::collect(&root, name, pkgs, with, images, &prompt)?.apply()?;
            return Ok(ExitCode::SUCCESS);
        }
        ["create", "key", rest @ ..] => {
            args.only(&["root", "module", "cn"], "create key")?;
            let kind = one_name(rest, "create key")?;
            let Some(root) = open(root_arg)? else {
                return Ok(ExitCode::from(REPO_ERROR));
            };
            buildc::key::Key::collect(&root, kind, module_arg, cn, &prompt)?.apply(&root)?;
            return Ok(ExitCode::SUCCESS);
        }
        ["import", "module", rest @ ..] => {
            args.only(&["root", "image"], "import module")?;
            let name = one_name(rest, "import module")?;
            let root = repo_root(root_arg)?;
            let (list, issues, context) = buildc::declarations(&root);
            if issues.report(&context) {
                return Ok(ExitCode::from(REPO_ERROR));
            }
            Import::collect(name, &root, &list.sources, images, &prompt)?
                .apply(&root, &list.sources)?;
            return Ok(ExitCode::SUCCESS);
        }
        ["build", rest @ ..] => {
            args.only(
                &[
                    "root",
                    "target",
                    "tag",
                    "kernel",
                    "backend",
                    "oci-output",
                    "secret",
                ],
                "build",
            )?;
            let opts = buildc::build::Options {
                target: one_name(rest, "build")?.or(target),
                kernel,
                tags,
                secrets,
                backend,
                oci_output,
                no_cache_from,
                cache_to,
            };
            return Ok(match buildc::build::run(&repo_root(root_arg)?, &opts)? {
                buildc::build::Stopped::Repository => ExitCode::from(REPO_ERROR),
            });
        }
        ["registry", "namespace"] => {
            args.only(&["root"], "registry namespace")?;
            println!("{}", buildc::registry::namespace(&repo_root(root_arg)?)?);
            return Ok(ExitCode::SUCCESS);
        }
        ["registry", "ref"] => {
            args.only(&["root", "target", "tag"], "registry ref")?;
            let root = repo_root(root_arg)?;
            let (list, issues, context) = buildc::declarations(&root);
            if issues.report(&context) {
                return Ok(ExitCode::from(REPO_ERROR));
            }
            println!(
                "{}",
                buildc::registry::reference(&list, &root, target.as_deref(), tags.last())?
            );
            return Ok(ExitCode::SUCCESS);
        }
        ["registry", ..] => {
            return Err(Error::Invocation(
                "`registry` takes `namespace` or `ref`".into(),
            ))
        }
        ["create", ..] => {
            return Err(Error::Invocation(
                "`create` takes `repo <name>`, `image <name>`, `module <name>` \
                 or `key <kind>`"
                    .into(),
            ))
        }
        ["import", ..] => return Err(Error::Invocation("`import` takes `module <name>`".into())),
        _ => {}
    }

    let Some(command) = Command::parse(words[0]) else {
        return Err(Error::Invocation(format!("unknown command `{}`", words[0])));
    };
    args.only(
        match command {
            Command::Graph => &["root", "format"],
            _ => &["root"],
        },
        words[0],
    )?;
    let command = match (command, format.as_deref()) {
        (Command::Graph, None | Some("md")) => Command::Graph,
        (Command::Graph, Some("json")) => Command::GraphJson,
        (Command::Graph, Some(other)) => {
            return Err(Error::Invocation(format!(
                "`--format` is md or json, not `{other}`"
            )));
        }
        (command, _) => command,
    };

    let rest = &words[1..];
    let arg = match rest {
        [] => None,
        ["--json"] if command == Command::Plan => None,
        [one] if command.arg().is_some() => Some(*one),
        _ => {
            return Err(Error::Invocation(format!(
                "`{}` does not take {}",
                words[0],
                rest.join(" ")
            )));
        }
    };

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
