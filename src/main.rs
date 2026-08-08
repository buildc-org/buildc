use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "\
usage: buildc [--root <dir>] <command>

  init [name]       write a new repository: the manifests, the module
                    directory and the scaffolding, into `--root`, else a
                    directory named for the image, else here. `--owner`
                    names who it belongs to on github
  plan [--json]     every fact this repository derives, as one JSON
                    document: the images, each image's targets, and what
                    each target is made of. Read a field out of it rather
                    than deriving anything from a name
  section [image]   the generated Containerfile module section for an
                    image; the default image when none is given
  check             validate every manifest, printing what is wrong

Inside a build layer, where the binary is mounted and there is no
repository to read:

  os-release        write the image identity the ARGs carry into
                    /usr/lib/os-release
  fetch <what> <url> <sha256> [target] [extra...]
                    download, verify against the hash, and place it: `file`
                    keeps it, `tree` unpacks it, `bin` installs one
                    executable, `rpm` installs the package
  validate-image    every check a built image has to pass

The repository is the nearest directory at or above the working directory
holding a repo.kdl, or `--root`. Data goes to stdout and diagnostics to
stderr; exit 1 is the invocation, exit 2 the repository.
";

const USAGE_ERROR: u8 = 1;
const REPO_ERROR: u8 = 2;

fn usage_error(message: String) -> ExitCode {
    eprintln!("buildc: {message}");
    eprint!("{USAGE}");
    ExitCode::from(USAGE_ERROR)
}

fn take_flag(args: &mut Vec<String>, flag: &str) -> Result<Option<String>, String> {
    let mut value = None;
    let mut i = 0;
    while i < args.len() {
        let taken = if let Some(v) = args[i].strip_prefix(&format!("--{flag}=")) {
            value = Some(v.to_string());
            1
        } else if args[i] == format!("--{flag}") {
            value = Some(
                args.get(i + 1)
                    .ok_or(format!("`--{flag}` takes a value"))?
                    .clone(),
            );
            2
        } else {
            i += 1;
            continue;
        };
        args.drain(i..i + taken);
    }
    Ok(value)
}

const OWNERSHIP: &str = "your account or org on github (not buildc-org)";

fn ask(question: &str) -> Result<String, String> {
    if !std::io::stdin().is_terminal() {
        return Err(format!("nothing to read an answer from: {question}"));
    }
    print!("{question}: ");
    std::io::stdout().flush().map_err(|err| err.to_string())?;
    let mut answer = String::new();
    std::io::stdin()
        .read_line(&mut answer)
        .map_err(|err| err.to_string())?;
    Ok(answer.trim().to_string())
}

fn init(args: &[&str], root_arg: Option<PathBuf>, owner: Option<String>) -> Result<(), String> {
    let name = match args {
        [] => None,
        [name] => Some((*name).to_string()),
        _ => return Err(format!("`init` takes one name, not {}", args.join(" "))),
    };

    let root = match (&root_arg, &name) {
        (Some(root), _) => root.clone(),
        (None, Some(name)) => PathBuf::from(buildc::init::id(name)?),
        (None, None) => PathBuf::from("."),
    };

    let name = match name {
        Some(name) => name,
        None => std::fs::canonicalize(&root)
            .ok()
            .as_deref()
            .and_then(Path::file_name)
            .map(|n| n.to_string_lossy().into_owned())
            .ok_or_else(|| format!("cannot name an image after {}", root.display()))?,
    };

    let owner = match owner {
        Some(owner) => owner,
        None => ask(&format!("owner, {OWNERSHIP}"))?,
    };
    if owner.is_empty() {
        return Err(format!("`--owner` is {OWNERSHIP}"));
    }

    let assets = buildc::init::assets()?;
    buildc::init::write(&root, &name, &owner, &assets)?;

    let id = buildc::init::id(&name)?;
    println!(
        "wrote {} into {}\n\n\
         next, in that directory:\n\
         \x20 git init && git add -A && git commit\n\
         \x20 gh repo create {owner}/{id} --source=. --push\n",
        name,
        root.display()
    );
    Ok(())
}

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let root_arg = match take_flag(&mut args, "root") {
        Ok(root) => root.map(PathBuf::from),
        Err(message) => return usage_error(message),
    };
    let owner_arg = match take_flag(&mut args, "owner") {
        Ok(owner) => owner,
        Err(message) => return usage_error(message),
    };

    let command = match args.first().map(String::as_str) {
        Some("-h") | Some("--help") => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Some(c) => c,
        None => {
            eprint!("{USAGE}");
            return ExitCode::from(USAGE_ERROR);
        }
    };

    let rest: Vec<&str> = args[1..].iter().map(String::as_str).collect();
    if command == "init" {
        return match init(&rest, root_arg, owner_arg) {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => usage_error(message),
        };
    }
    if owner_arg.is_some() {
        return usage_error(format!("`{command}` does not take `--owner`"));
    }

    let in_layer = match command {
        "os-release" => Some(buildc::runtime::os_release()),
        "validate-image" => Some(buildc::runtime::validate_image()),
        "fetch" => Some(buildc::runtime::fetch(&rest)),
        _ => None,
    };
    if let Some(result) = in_layer {
        return match result {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => {
                eprintln!("buildc: {message}");
                ExitCode::from(USAGE_ERROR)
            }
        };
    }

    let image_arg = match (command, rest.as_slice()) {
        ("plan", []) | ("plan", ["--json"]) => None,
        ("check", []) => None,
        ("section", []) => None,
        ("section", [image]) => Some(*image),
        ("plan" | "check" | "section", _) => {
            return usage_error(format!("`{command}` does not take {}", rest.join(" ")))
        }
        (other, _) => return usage_error(format!("unknown command `{other}`")),
    };

    let root = match root_arg {
        Some(root) => root,
        None => {
            let here = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            match buildc::find_root(&here) {
                Some(root) => root,
                None => {
                    return usage_error(format!(
                        "no repo.kdl in {} or any parent directory",
                        here.display()
                    ))
                }
            }
        }
    };

    let run = buildc::run(command, image_arg, &root);

    if run.issues.report(&run.context) {
        return ExitCode::from(REPO_ERROR);
    }
    print!("{}", run.stdout);
    if command == "check" {
        eprintln!(
            "buildc: {} images, {} modules, {} flavours",
            run.images, run.modules, run.flavours
        );
    }
    ExitCode::SUCCESS
}
