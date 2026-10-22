use clap::error::ErrorKind;
use clap::{ArgMatches, CommandFactory, FromArgMatches};
use common::prompt::Prompt;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use buildc::command::{self, Cli, Context, Verb};
use buildc::copy;
use buildc::dispatch::{self, Error, USAGE_ERROR};
use buildc::model::image::BUILDC_VERSION;

const COMMANDS: &str = "Run 'buildc --help' to list the commands";

static GREETED: AtomicBool = AtomicBool::new(false);

fn banner(failing: bool) {
    if GREETED.swap(true, Ordering::Relaxed) {
        return;
    }
    match failing {
        true => eprintln!("{} v{BUILDC_VERSION}\n", copy::PRODUCT),
        false => println!("{} v{BUILDC_VERSION}\n", copy::PRODUCT),
    }
}

fn path_of(matches: &ArgMatches) -> Vec<String> {
    let mut path = Vec::new();
    let mut at = matches;
    while let Some((name, sub)) = at.subcommand() {
        path.push(name.to_string());
        at = sub;
    }
    path
}

fn deepest(matches: &ArgMatches) -> &ArgMatches {
    let mut at = matches;
    while let Some((_, sub)) = at.subcommand() {
        at = sub;
    }
    at
}

fn command_at<'a>(root: &'a clap::Command, path: &[String]) -> Option<&'a clap::Command> {
    let mut at = root;
    for name in path {
        at = at
            .get_subcommands()
            .find(|command| command.get_name() == name)?;
    }
    Some(at)
}

fn positionals(matches: &ArgMatches, command: &clap::Command) -> Vec<String> {
    let mut rest = Vec::new();
    for arg in command.get_positionals() {
        if let Some(values) = matches.get_many::<String>(arg.get_id().as_str()) {
            rest.extend(values.cloned());
        }
    }
    rest
}

fn flags(matches: &ArgMatches) -> Result<dispatch::Flags, Error> {
    let one = |id: &str| command::flag::<String>(matches, id);
    let many = |id: &str| command::flag_all::<String>(matches, id);
    let path = |id: &str| command::flag::<PathBuf>(matches, id);
    let switch = |id: &str| command::flag::<bool>(matches, id).unwrap_or(false);
    let with = command::flag_all::<String>(matches, "with")
        .iter()
        .map(|pair| match pair.split_once('=') {
            Some((verb, value)) => Ok((verb.to_string(), value.to_string())),
            None => Err(Error::Invocation(format!(
                "`--with` takes `verb=value`, got `{pair}`"
            ))),
        })
        .collect::<Result<Vec<_>, Error>>()?;
    Ok(dispatch::Flags {
        root: path("root"),
        owner: one("owner"),
        host: one("host"),
        images: many("image"),
        module: one("module"),
        cn: one("cn"),
        from: one("from"),
        base: one("base"),
        format: one("format"),
        target: one("target"),
        datastream: path("datastream"),
        baseline: path("baseline"),
        base_scan: path("base_scan"),
        tags: many("tag"),
        kernel: one("kernel"),
        ram: one("ram"),
        backend: one("backend"),
        oci_output: one("oci_output"),
        secrets: many("secret"),
        pkgs: many("pkg"),
        with,
        cache_to: switch("cache_to"),
        no_cache_from: switch("no_cache_from"),
        rebuild: switch("rebuild"),
        dependencies: switch("dependencies"),
        untracked: switch("untracked"),
    })
}

fn root_anywhere(words: &[String]) -> Option<PathBuf> {
    for (at, word) in words.iter().enumerate() {
        if word == "--root" {
            return words.get(at + 1).map(PathBuf::from);
        }
        if let Some(value) = word.strip_prefix("--root=") {
            return Some(PathBuf::from(value));
        }
    }
    None
}

fn help_requested(words: &[String]) -> Option<Option<PathBuf>> {
    let mut at = 0;
    while at < words.len() {
        match words[at].as_str() {
            "-h" | "--help" => return Some(root_anywhere(words)),
            "--root" => at += 2,
            other if other.starts_with("--root=") => at += 1,
            "--no-tui" => at += 1,
            _ => return None,
        }
    }
    None
}

fn refused(error: clap::Error) -> ExitCode {
    match error.kind() {
        ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => {
            print!("{error}");
            ExitCode::SUCCESS
        }
        _ => {
            banner(true);
            let message = error.to_string();
            let message = message.strip_prefix("error: ").unwrap_or(&message);
            eprintln!("Error: {message}");
            ExitCode::from(USAGE_ERROR)
        }
    }
}

fn main() -> ExitCode {
    unsafe { libc::signal(libc::SIGPIPE, libc::SIG_DFL) };

    match run() {
        Ok(code) => code,
        Err(Error::Cancelled) => ExitCode::SUCCESS,
        Err(error) => {
            banner(true);
            let message = error.message();
            let stop = match message.ends_with(['.', '!', '?']) || message.contains('\n') {
                true => "",
                false => ".",
            };
            eprintln!("Error: {message}{stop}\n");
            eprintln!("{COMMANDS}");
            ExitCode::from(USAGE_ERROR)
        }
    }
}

fn run() -> Result<ExitCode, Error> {
    let words: Vec<String> = std::env::args().skip(1).collect();
    if words == ["--version"] {
        println!("{} v{BUILDC_VERSION}", copy::PRODUCT);
        return Ok(ExitCode::SUCCESS);
    }
    if let Some(root) = help_requested(&words) {
        banner(false);
        print!("{}", command::usage(&Context::of(root.as_deref())));
        return Ok(ExitCode::SUCCESS);
    }
    let matches = match Cli::command()
        .try_get_matches_from(std::iter::once("buildc".to_string()).chain(words))
    {
        Ok(matches) => matches,
        Err(error) => return Ok(refused(error)),
    };
    let cli =
        Cli::from_arg_matches(&matches).expect("the crate's own parser accepts its own matches");
    let prompt = Prompt::new(cli.no_tui);
    let here = Context::of(cli.root.as_deref());
    let path = path_of(&matches);
    let typed = path.join(" ");
    let (verb, name) = match command::verb(&typed) {
        Some(verb) => (verb, typed),
        None => {
            let word = path.first().map(String::as_str);
            let (kept, options) = command::choices(&command::rows(word), &here);
            if prompt.draws() && !kept.is_empty() {
                banner(false);
                match common::ui::select(copy::WHICH_COMMAND, &options)? {
                    Some(at) => {
                        let row = kept[at]
                            .as_ref()
                            .expect("the picker never answers with a heading row");
                        (row.verb, row.path.clone())
                    }
                    None => return Ok(ExitCode::SUCCESS),
                }
            } else if path.is_empty() {
                banner(true);
                eprint!("{}", command::usage(&here));
                return Ok(ExitCode::from(USAGE_ERROR));
            } else {
                banner(true);
                return Err(Error::Invocation(format!(
                    "`{typed}` takes {}",
                    command::takes(&typed)
                )));
            }
        }
    };

    let tree = Cli::command();
    let rest: Vec<String> = match path.is_empty() {
        true => Vec::new(),
        false => positionals(
            deepest(&matches),
            command_at(&tree, &path).expect("the parsed words name a command"),
        ),
    };
    let flags = flags(&matches)?;
    if matches!(
        verb,
        Verb::CreateRepo
            | Verb::CreateGit
            | Verb::CreateImage
            | Verb::CreateFlavour
            | Verb::CreateModule
            | Verb::CreateKey
            | Verb::CreateScripts
            | Verb::SetKey
            | Verb::SetLibrary
            | Verb::ImportModule
            | Verb::Check
    ) {
        banner(false);
    }
    let rest: Vec<&str> = rest.iter().map(String::as_str).collect();
    if let Some(flag) = command::ignored(&matches).first() {
        return Err(Error::Invocation(format!(
            "`{name}` does not take `--{flag}`"
        )));
    }
    dispatch::dispatch(verb, &name, &rest, flags, &prompt, &here)
}
