use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use buildc::command::{self, Spec, Verb};
use buildc::dispatch::{self, Error, USAGE_ERROR};
use buildc::model::image::BUILDC_VERSION;
use buildc::prompt::Prompt;

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
                true => eprint!("{}", command::usage()),
                false => eprintln!("{COMMANDS}"),
            }
            ExitCode::from(USAGE_ERROR)
        }
    }
}

fn picking(words: &[&str], prompt: &Prompt) -> bool {
    prompt.draws()
        && match words {
            [] => true,
            [word] => command::all_nouns(word),
            _ => false,
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
    let flags = dispatch::Flags {
        root: args.flag("root")?.map(PathBuf::from),
        owner: args.flag("owner")?,
        host: args.flag("host")?,
        images: args.flags("image")?,
        module: args.flag("module")?,
        cn: args.flag("cn")?,
        base: args.flag("base")?,
        format: args.flag("format")?,
        target: args.flag("target")?,
        datastream: args.flag("datastream")?.map(PathBuf::from),
        baseline: args.flag("baseline")?.map(PathBuf::from),
        tags: args.flags("tag")?,
        kernel: args.flag("kernel")?,
        backend: args.flag("backend")?,
        oci_output: args.flag("oci-output")?,
        secrets: args.flags("secret")?,
        pkgs: args.flags("pkg")?,
        with: args
            .flags("with")?
            .iter()
            .map(|pair| match pair.split_once('=') {
                Some((verb, value)) => Ok((verb.to_string(), value.to_string())),
                None => Err(Error::Invocation(format!(
                    "`--with` is `verb=value`, not `{pair}`"
                ))),
            })
            .collect::<Result<Vec<_>, Error>>()?,
        cache_to,
        no_cache_from,
    };

    let words: Vec<&str> = args.words.iter().map(String::as_str).collect();
    let (spec, rest): (&Spec, &[&str]) = if matches!(words.first(), Some(&"-h") | Some(&"--help")) {
        banner(false);
        print!("{}", command::usage());
        return Ok(ExitCode::SUCCESS);
    } else if picking(&words, &prompt) {
        let rows = match words.first() {
            Some(word) => command::nouns(word),
            None => command::listed(),
        };
        banner(false);
        match buildc::ui::select("which command", &command::choices(&rows))? {
            Some(at) => (rows[at], &[]),
            None => return Ok(ExitCode::SUCCESS),
        }
    } else if words.is_empty() {
        banner(true);
        eprint!("{}", command::usage());
        return Ok(ExitCode::from(USAGE_ERROR));
    } else {
        command::resolve(&words).map_err(Error::Invocation)?
    };

    args.only(spec)?;
    if matches!(
        spec.verb,
        Verb::CreateRepo
            | Verb::CreateImage
            | Verb::CreateModule
            | Verb::CreateKey
            | Verb::ImportModule
            | Verb::CopyModule
            | Verb::Check
    ) {
        banner(false);
    }

    dispatch::dispatch(spec, rest, flags, &prompt)
}
