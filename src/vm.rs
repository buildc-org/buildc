use crate::copy;
use common::prompt::Prompt;
use common::ui::Choice;
use std::os::unix::process::CommandExt as _;
use std::path::Path;
use std::process::Command;

const SCRIPT: &str = "vm.sh";

use crate::parse::component::rpm;

const TYPES: [(&str, &str); 2] = [("qcow2", copy::DISK_QCOW2), ("raw", copy::DISK_RAW)];

pub struct Options {
    pub target: Option<String>,
    pub image: Option<String>,
    pub tag: Option<String>,
    pub ram: Option<String>,
    pub rebuild: bool,
}

#[derive(Default)]
struct Access {
    login: bool,
    ssh_key: Option<String>,
}

pub fn run(
    root: &Path,
    noun: &str,
    name: &str,
    given: Option<&str>,
    opts: &Options,
    prompt: &Prompt,
) -> Result<(), String> {
    let Some(kind) = kind(noun, name, given, prompt)? else {
        return Ok(());
    };
    let installer = installer(root, noun, kind, opts);
    let access = access(root, opts);
    let script = root.join(crate::layout::script(root, SCRIPT));
    if !script.is_file() {
        return Err(format!(
            "{} is not there; `buildc generate` writes it",
            script.display()
        ));
    }
    Err(format!(
        "{}: {}",
        script.display(),
        Command::new(&script)
            .args(argv(noun, kind, opts, installer, &access))
            .exec()
    ))
}

fn converts(root: &Path, noun: &str, kind: &str, opts: &Options) -> bool {
    if noun == "build" || opts.rebuild {
        return true;
    }
    !root.join(format!("out/{kind}/disk.{kind}")).is_file()
}

fn family(root: &Path, opts: &Options) -> Option<String> {
    if opts.image.is_some() {
        return None;
    }
    let list = crate::model::image::List::load(root).0;
    let target = target(&list, opts)?;
    let image = list.images.iter().find(|image| image.id == target.image)?;
    image
        .base
        .as_ref()
        .map(|base| base.family.clone())
        .filter(|family| !family.is_empty())
}

pub(crate) fn target(
    list: &crate::model::image::List,
    opts: &Options,
) -> Option<crate::model::image::Target> {
    match &opts.target {
        Some(named) => list.find_target(named).ok(),
        None => list.ungated_target(),
    }
}

fn access(root: &Path, opts: &Options) -> Access {
    if opts.image.is_some() {
        return Access::default();
    }
    let list = crate::model::image::List::load(root).0;
    let Some(target) = target(&list, opts) else {
        return Access::default();
    };
    let Some((_, _, entries)) = crate::emit::plan::of_target(&list, &target.to_string()) else {
        return Access::default();
    };
    let dirs: Vec<_> = entries.iter().map(|entry| entry.path.clone()).collect();
    let disk = crate::parse::disk::Disk::scan(root);
    let keys: Vec<_> = disk
        .keys
        .get("ssh")
        .into_iter()
        .flatten()
        .filter(|(dir, _)| dirs.contains(dir))
        .map(|(_, key)| key)
        .collect();
    let existing: Vec<_> = keys
        .iter()
        .map(|key| crate::layout::public_key(root, &key.public))
        .filter(|path| crate::layout::nonempty(path))
        .collect();
    Access {
        login: dirs.iter().any(|dir| imports_passwords(root, dir)),
        ssh_key: match existing.as_slice() {
            [path] => Some(path.display().to_string()),
            _ => None,
        },
    }
}

fn imports_passwords(root: &Path, component: &str) -> bool {
    let mut dirs = vec![crate::layout::component(root, component).join(crate::layout::OVERLAY)];
    while let Some(dir) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                dirs.push(path);
            } else if std::fs::read_to_string(path).is_ok_and(|text| {
                text.lines()
                    .any(|line| line.trim() == "ImportCredential=passwd.hashed-password.*")
            }) {
                return true;
            }
        }
    }
    false
}

fn installer(root: &Path, noun: &str, kind: &str, opts: &Options) -> Option<&'static str> {
    if !converts(root, noun, kind, opts) {
        return None;
    }
    family(root, opts)
        .filter(|family| !rpm(family))
        .map(|_| BOOTC)
}

const BOOTC: &str = "bootc";

fn argv(
    noun: &str,
    kind: &str,
    opts: &Options,
    installer: Option<&str>,
    access: &Access,
) -> Vec<String> {
    let mut args = vec![noun.to_string(), kind.to_string()];
    if let Some(installer) = installer {
        args.extend(["--installer".to_string(), installer.to_string()]);
    }
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
    if access.login {
        args.push("--login".to_string());
    }
    if let Some(key) = &access.ssh_key {
        args.extend(["--ssh-key".to_string(), key.clone()]);
    }
    args
}

fn kind(
    noun: &str,
    name: &str,
    given: Option<&str>,
    prompt: &Prompt,
) -> Result<Option<&'static str>, String> {
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
        if named == "iso" {
            return Err("an installer iso is `buildc media build`, not a vm disk".to_string());
        }
        return match TYPES.iter().find(|(name, _)| *name == named) {
            Some((name, _)) => Ok(Some(name)),
            None => Err(format!("`{}` takes {}, not `{named}`", name, names())),
        };
    }
    if !prompt.draws() {
        return Err(format!("`{name}` takes {}", names()));
    }
    Ok(prompt
        .choose(copy::WHICH_DISK, &rows(noun))?
        .map(|at| TYPES[at].0))
}

fn rows(_noun: &str) -> Vec<Choice> {
    TYPES
        .iter()
        .map(|(name, about)| Choice::new(*name, *about))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_script_it_execs_is_one_the_repository_generates() {
        assert!(crate::emit::SCRIPTS
            .iter()
            .any(|script| script.name == SCRIPT));
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
            argv("run", "qcow2", &opts, None, &Access::default()),
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
        assert_eq!(
            argv("build", "raw", &opts, Some(BOOTC), &Access::default())[..4],
            ["build", "raw", "--installer", "bootc"]
        );
    }

    #[test]
    fn the_rows_are_the_two_disk_types_and_every_one_is_available() {
        let rows = rows("spawn");
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|row| row.available));
    }

    #[test]
    fn the_family_names_the_converter_before_anything_is_pulled() {
        let root = std::env::temp_dir().join(format!("buildc-vm-family.{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        crate::init::put(&root.join("repo.kdl"), "schema-version 1\nname \"Deb\"\n").unwrap();
        crate::init::put(
            &root.join("deb.image.kdl"),
            "schema-version 1\n\nimage \"deb\" {\n    \n    base-image {\n        debian\n    }\n\n    login\n}\n",
        )
        .unwrap();
        for (path, image, family) in [
            ("debian", "docker.io/library/debian:forky", "debian"),
            ("fedora", "quay.io/fedora/fedora-bootc:44", "fedora"),
        ] {
            crate::init::put(
                &root.join(format!("components/base-image/{path}/manifest.kdl")),
                &format!("schema-version 1\n\nbase \"{image}\" {{ family \"{family}\"; }}\n"),
            )
            .unwrap();
        }
        crate::init::put(
            &root.join("components/login/manifest.kdl"),
            "schema-version 1\n\ndescription \"login\"\ntargets \"debian\"\nkey \"ssh\" {\n    generator \"ssh-keygen\"\n    public \"/usr/lib/buildc/authorized_keys\"\n    private \"id_ed25519\"\n}\n",
        )
        .unwrap();
        let opts = |image: Option<&str>| Options {
            target: None,
            image: image.map(str::to_string),
            tag: None,
            ram: None,
            rebuild: false,
        };
        assert_eq!(family(&root, &opts(None)).as_deref(), Some("debian"));
        let declared = access(&root, &opts(None));
        assert!(!declared.login);
        assert!(declared.ssh_key.is_none());
        crate::init::put(
            &root.join(
                "components/login/files/usr/lib/systemd/system/systemd-sysusers.service.d/login.conf",
            ),
            "[Service]\nImportCredential=passwd.hashed-password.*\n",
        )
        .unwrap();
        assert!(access(&root, &opts(None)).login);
        crate::init::put(
            &crate::layout::public_key(&root, "/usr/lib/buildc/authorized_keys"),
            "ssh-ed25519 AAAA test\n",
        )
        .unwrap();
        let recorded = access(&root, &opts(None));
        assert!(recorded.login);
        assert!(recorded.ssh_key.is_some());
        let args = argv("run", "raw", &opts(None), Some(BOOTC), &recorded);
        assert!(args.contains(&"--login".to_string()));
        assert!(args.contains(&"--ssh-key".to_string()));
        assert!(!access(&root, &opts(Some("localhost/other"))).login);
        let refuse = |kind, opts: &Options| {
            run(&root, "run", "vm run", Some(kind), opts, &Prompt::silent()).unwrap_err()
        };

        let build = "build";
        for kind in ["qcow2", "raw"] {
            assert_eq!(installer(&root, build, kind, &opts(None)), Some(BOOTC));
        }
        let installed = refuse("qcow2", &opts(None));
        assert!(installed.contains("vm.sh is not there"), "{installed}");

        let unasked =
            run(&root, "run", "vm run", None, &opts(None), &Prompt::silent()).unwrap_err();
        assert!(unasked.contains("takes `qcow2` or `raw`"), "{unasked}");

        crate::init::put(&root.join("out/raw/disk.raw"), "").unwrap();
        assert!(!converts(&root, "spawn", "raw", &opts(None)));
        assert!(!converts(&root, "run", "raw", &opts(None)));
        assert_eq!(installer(&root, "run", "raw", &opts(None)), None);
        let booted = refuse("raw", &opts(None));
        assert!(booted.contains("vm.sh is not there"), "{booted}");

        assert!(converts(&root, "build", "raw", &opts(None)));
        assert!(converts(&root, "run", "qcow2", &opts(None)));
        let rebuilding = Options {
            rebuild: true,
            ..opts(None)
        };
        assert!(converts(&root, "spawn", "raw", &rebuilding));

        assert_eq!(family(&root, &opts(Some("localhost/other"))), None);

        crate::init::put(
            &root.join("fed.image.kdl"),
            "schema-version 1\n\nimage \"fed\" {\n    \n    base-image {\n        fedora\n    }\n\n}\n",
        )
        .unwrap();
        crate::init::put(
            &root.join("repo.kdl"),
            "schema-version 1\nname \"Deb\"\ndefault-image \"fed\"\n",
        )
        .unwrap();
        assert_eq!(family(&root, &opts(None)).as_deref(), Some("fedora"));
        let fedora = refuse("qcow2", &opts(None));
        assert!(fedora.contains("vm.sh is not there"), "{fedora}");
        for kind in ["qcow2", "raw"] {
            assert_eq!(installer(&root, build, kind, &opts(None)), None);
        }

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_type_that_is_not_a_disk_is_refused_by_name() {
        let silent = Prompt::silent();
        assert!(kind("build", "vm build", Some("vmdk"), &silent).is_err());
        assert_eq!(
            kind("build", "vm build", Some("raw"), &silent).unwrap(),
            Some("raw")
        );
        assert!(kind("build", "vm build", None, &silent).is_err());
    }

    #[test]
    fn an_iso_points_at_the_media_command() {
        let refused = kind("build", "vm build", Some("iso"), &Prompt::silent()).unwrap_err();
        assert!(refused.contains("buildc media build"), "{refused}");
    }
}
