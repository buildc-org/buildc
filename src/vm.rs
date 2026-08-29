use crate::command::Spec;
use crate::copy;
use crate::prompt::Prompt;
use crate::ui::Choice;
use std::os::unix::process::CommandExt as _;
use std::path::Path;
use std::process::Command;

const SCRIPT: &str = "scripts/vm.sh";

const TYPES: [(&str, &str); 3] = [
    ("qcow2", copy::DISK_QCOW2),
    ("raw", copy::DISK_RAW),
    ("iso", copy::DISK_ISO),
];

const DEFAULT_SIZE: &str = "buildc: the disk is the image builder's default size; the \
                            root `disk_config/disk.toml` declares is in a block it \
                            does not apply";

pub struct Options {
    pub target: Option<String>,
    pub image: Option<String>,
    pub tag: Option<String>,
    pub ram: Option<String>,
    pub rebuild: bool,
}

pub fn run(
    root: &Path,
    spec: &Spec,
    given: Option<&str>,
    opts: &Options,
    prompt: &Prompt,
) -> Result<(), String> {
    let Some(kind) = kind(spec, given, prompt)? else {
        return Ok(());
    };
    let script = root.join(SCRIPT);
    if !script.is_file() {
        return Err(format!(
            "{} is not there; `buildc generate` writes it",
            script.display()
        ));
    }
    if kind != "iso" {
        eprintln!("{DEFAULT_SIZE}");
    }
    Err(format!(
        "{}: {}",
        script.display(),
        Command::new(&script).args(argv(spec, kind, opts)).exec()
    ))
}

fn argv(spec: &Spec, kind: &str, opts: &Options) -> Vec<String> {
    let mut args = vec![spec.noun.to_string(), kind.to_string()];
    for (flag, value) in [
        ("--target", &opts.target),
        ("--image", &opts.image),
        ("--tag", &opts.tag),
        ("--ram", &opts.ram),
    ] {
        if let Some(value) = value {
            args.extend([flag.to_string(), value.clone()]);
        }
    }
    if opts.rebuild {
        args.push("--rebuild".to_string());
    }
    args
}

fn kind(spec: &Spec, given: Option<&str>, prompt: &Prompt) -> Result<Option<&'static str>, String> {
    let names = || match TYPES
        .iter()
        .map(|(name, _)| format!("`{name}`"))
        .collect::<Vec<_>>()
        .split_last()
    {
        Some((last, rest)) => format!("{} or {last}", rest.join(", ")),
        None => String::new(),
    };
    if let Some(named) = given {
        return match TYPES.iter().find(|(name, _)| *name == named) {
            Some((name, _)) => Ok(Some(name)),
            None => Err(format!(
                "`{}` takes {}, not `{named}`",
                spec.name(),
                names()
            )),
        };
    }
    if !prompt.draws() {
        return Err(format!("`{}` takes {}", spec.name(), names()));
    }
    Ok(prompt
        .choose(copy::WHICH_DISK, &rows(spec))?
        .map(|at| TYPES[at].0))
}

fn rows(spec: &Spec) -> Vec<Choice> {
    TYPES
        .iter()
        .map(|(name, about)| match (*name, spec.noun) {
            ("iso", "spawn") => Choice::new(*name, copy::NO_ISO_SPAWN).unavailable(),
            _ => Choice::new(*name, *about),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::Verb;

    #[test]
    fn the_script_it_execs_is_one_the_repository_generates() {
        assert!(crate::emit::SCRIPTS.iter().any(|(path, _)| *path == SCRIPT));
    }

    #[test]
    fn the_noun_is_the_scripts_command_and_the_flags_are_its_own() {
        let opts = Options {
            target: Some("desktop/dx".into()),
            image: None,
            tag: Some("42".into()),
            ram: None,
            rebuild: true,
        };
        assert_eq!(
            argv(Verb::VmRun.spec(), "qcow2", &opts),
            [
                "run",
                "qcow2",
                "--target",
                "desktop/dx",
                "--tag",
                "42",
                "--rebuild"
            ]
        );
    }

    #[test]
    fn spawn_shows_the_iso_it_cannot_boot_rather_than_hiding_it() {
        let iso = |spec| rows(spec).pop().expect("iso is the last row");
        assert!(!iso(Verb::VmSpawn.spec()).available);
        assert!(iso(Verb::VmRun.spec()).available);
    }

    #[test]
    fn a_type_that_is_not_one_of_the_three_is_refused_by_name() {
        let silent = Prompt::silent();
        let spec = Verb::VmBuild.spec();
        assert!(kind(spec, Some("vmdk"), &silent).is_err());
        assert_eq!(kind(spec, Some("raw"), &silent).unwrap(), Some("raw"));
        assert!(kind(spec, None, &silent).is_err());
    }
}
