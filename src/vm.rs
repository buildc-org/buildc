use crate::command::Spec;
use crate::copy;
use crate::prompt::Prompt;
use crate::ui::Choice;
use std::os::unix::process::CommandExt as _;
use std::path::Path;
use std::process::Command;

const SCRIPT: &str = "scripts/vm.sh";

use crate::parse::module::rpm;

const TYPES: [(&str, &str); 3] = [
    ("qcow2", copy::DISK_QCOW2),
    ("raw", copy::DISK_RAW),
    ("iso", copy::DISK_ISO),
];

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
    spec: &Spec,
    given: Option<&str>,
    opts: &Options,
    prompt: &Prompt,
) -> Result<(), String> {
    let Some(kind) = kind(spec, given, prompt)? else {
        return Ok(());
    };
    let installer = installer(root, spec, kind, opts);
    let live = match kind == "iso" && converts(root, spec, kind, opts) {
        true => Some(stage(root, opts)?),
        false => None,
    };
    let access = access(root, kind, opts);
    let script = root.join(SCRIPT);
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
            .args(argv(spec, kind, opts, installer, live.as_deref(), &access))
            .exec()
    ))
}

const BOOTISO: &str = "out/bootiso";

fn documents(
    list: &crate::model::image::List,
    name: &str,
    image: &str,
    imgref: &str,
) -> Result<Vec<(&'static str, String)>, String> {
    use crate::emit::recipe;
    if imgref.starts_with("localhost/") {
        return Err(format!(
            "`{imgref}` is where the installed machine would look for its updates, \
             and nothing serves it.\n\nhelp: set $IMAGE_REGISTRY to where `{name}` \
             publishes"
        ));
    }
    let refuse = || {
        format!(
            "there is no measured install recipe for `{name}`: its base family is \
             none of `fedora`, `debian` or `ubuntu`, and guessing one erases a disk \
             before it fails to boot"
        )
    };
    let build = recipe::build(list, name, imgref, imgref, &[recipe::STORE.to_string()])
        .ok_or_else(refuse)?;
    let media = recipe::media(list, name, image, imgref).ok_or_else(refuse)?;
    Ok(vec![
        ("recipe.json", build.render()),
        ("media.json", media.render()),
        ("Containerfile", recipe::LIVE_ENV.to_string()),
        ("efi-from-image.patch", recipe::EFI_PATCH.to_string()),
    ])
}

fn stage(root: &Path, opts: &Options) -> Result<String, String> {
    let list = crate::model::image::List::load(root).0;
    let target = target(&list, opts)
        .ok_or("no target to build an installer iso for; name one with `--target`")?;
    let published = target.published();
    let tag = crate::registry::tag(opts.tag.as_ref());
    let imgref =
        crate::registry::reference(&list, root, opts.target.as_deref(), opts.tag.as_ref())?;
    let image = match &opts.image {
        Some(named) => format!("{named}:{tag}"),
        None => crate::registry::at("localhost", &published, &tag),
    };
    for (name, body) in documents(&list, &target.to_string(), &image, &imgref)? {
        crate::init::put(&root.join(BOOTISO).join(name), &body)?;
    }
    frontend(&root.join(BOOTISO).join("buildc"))?;
    Ok(crate::emit::recipe::live(&published))
}

fn frontend(to: &Path) -> Result<(), String> {
    let from = std::env::current_exe().map_err(|err| format!("this binary: {err}"))?;
    std::fs::copy(&from, to)
        .map(|_| ())
        .map_err(|err| format!("{} -> {}: {err}", from.display(), to.display()))
}

fn converts(root: &Path, spec: &Spec, kind: &str, opts: &Options) -> bool {
    if spec.noun == "build" || opts.rebuild {
        return true;
    }
    let at = match kind {
        "iso" => format!("{BOOTISO}/install.iso"),
        kind => format!("out/{kind}/disk.{kind}"),
    };
    !root.join(at).is_file()
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

fn target(list: &crate::model::image::List, opts: &Options) -> Option<crate::model::image::Target> {
    match &opts.target {
        Some(named) => list.find_target(named).ok(),
        None => list.ungated_target(),
    }
}

fn access(root: &Path, kind: &str, opts: &Options) -> Access {
    if kind == "iso" || opts.image.is_some() {
        return Access::default();
    }
    let list = crate::model::image::List::load(root).0;
    let Some(target) = target(&list, opts) else {
        return Access::default();
    };
    let Some((_, _, entries)) = crate::emit::plan::of_target(&list, &target.to_string()) else {
        return Access::default();
    };
    let dirs: Vec<_> = entries.iter().map(|entry| entry.dir()).collect();
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

fn imports_passwords(root: &Path, module: &str) -> bool {
    let mut dirs = vec![crate::layout::module(root, module).join(crate::layout::OVERLAY)];
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

fn installer(root: &Path, spec: &Spec, kind: &str, opts: &Options) -> Option<&'static str> {
    if kind == "iso" || !converts(root, spec, kind, opts) {
        return None;
    }
    family(root, opts)
        .filter(|family| !rpm(family))
        .map(|_| BOOTC)
}

const BOOTC: &str = "bootc";

fn argv(
    spec: &Spec,
    kind: &str,
    opts: &Options,
    installer: Option<&str>,
    live: Option<&str>,
    access: &Access,
) -> Vec<String> {
    let mut args = vec![spec.noun.to_string(), kind.to_string()];
    if let Some(installer) = installer {
        args.extend(["--installer".to_string(), installer.to_string()]);
    }
    if let Some(live) = live {
        args.extend(["--live-image".to_string(), live.to_string()]);
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
            argv(
                Verb::VmRun.spec(),
                "qcow2",
                &opts,
                None,
                None,
                &Access::default()
            ),
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
            argv(
                Verb::VmBuild.spec(),
                "raw",
                &opts,
                Some(BOOTC),
                None,
                &Access::default()
            )[..4],
            ["build", "raw", "--installer", "bootc"]
        );
    }

    #[test]
    fn spawn_shows_the_iso_it_cannot_boot_rather_than_hiding_it() {
        let iso = |spec| rows(spec).pop().expect("iso is the last row");
        assert!(!iso(Verb::VmSpawn.spec()).available);
        assert!(iso(Verb::VmRun.spec()).available);
    }

    #[test]
    fn the_family_names_the_converter_before_anything_is_pulled() {
        let root = std::env::temp_dir().join(format!("buildc-vm-family.{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        crate::init::put(&root.join("repo.kdl"), "schema-version 1\nname \"Deb\"\n").unwrap();
        crate::init::put(
            &root.join("deb.image.kdl"),
            "image {\n    name \"deb\"\n\n    base \"docker.io/library/debian:forky\" {\n\
             \x20       family \"debian\"\n    }\n\n    modules {\n        module \"login\"\n    }\n}\n",
        )
        .unwrap();
        crate::init::put(
            &root.join("modules/login/module.kdl"),
            "description \"login\"\nsupports \"debian\"\nkey \"ssh\" {\n    generator \"ssh-keygen\"\n    public \"/usr/lib/buildc/authorized_keys\"\n    private \"id_ed25519\"\n}\n",
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
        let declared = access(&root, "raw", &opts(None));
        assert!(!declared.login);
        assert!(declared.ssh_key.is_none());
        crate::init::put(
            &root.join(
                "modules/login/files/usr/lib/systemd/system/systemd-sysusers.service.d/login.conf",
            ),
            "[Service]\nImportCredential=passwd.hashed-password.*\n",
        )
        .unwrap();
        assert!(access(&root, "raw", &opts(None)).login);
        crate::init::put(
            &crate::layout::public_key(&root, "/usr/lib/buildc/authorized_keys"),
            "ssh-ed25519 AAAA test\n",
        )
        .unwrap();
        let recorded = access(&root, "raw", &opts(None));
        assert!(recorded.login);
        assert!(recorded.ssh_key.is_some());
        let args = argv(
            Verb::VmRun.spec(),
            "raw",
            &opts(None),
            Some(BOOTC),
            None,
            &recorded,
        );
        assert!(args.contains(&"--login".to_string()));
        assert!(args.contains(&"--ssh-key".to_string()));
        assert!(!access(&root, "raw", &opts(Some("localhost/other"))).login);
        assert!(!access(&root, "iso", &opts(None)).login);
        let refuse = |kind, opts: &Options| {
            run(
                &root,
                Verb::VmRun.spec(),
                Some(kind),
                opts,
                &Prompt::silent(),
            )
            .unwrap_err()
        };

        let build = Verb::VmBuild.spec();
        for kind in ["qcow2", "raw"] {
            assert_eq!(installer(&root, build, kind, &opts(None)), Some(BOOTC));
        }
        let installed = refuse("qcow2", &opts(None));
        assert!(installed.contains("vm.sh is not there"), "{installed}");
        assert_eq!(installer(&root, build, "iso", &opts(None)), None);

        let unasked = run(
            &root,
            Verb::VmRun.spec(),
            None,
            &opts(None),
            &Prompt::silent(),
        )
        .unwrap_err();
        assert!(
            unasked.contains("takes `qcow2`, `raw` or `iso`"),
            "{unasked}"
        );

        crate::init::put(&root.join("out/raw/disk.raw"), "").unwrap();
        assert!(!converts(&root, Verb::VmSpawn.spec(), "raw", &opts(None)));
        assert!(!converts(&root, Verb::VmRun.spec(), "raw", &opts(None)));
        assert_eq!(
            installer(&root, Verb::VmRun.spec(), "raw", &opts(None)),
            None
        );
        let booted = refuse("raw", &opts(None));
        assert!(booted.contains("vm.sh is not there"), "{booted}");

        assert!(converts(&root, Verb::VmBuild.spec(), "raw", &opts(None)));
        assert!(converts(&root, Verb::VmRun.spec(), "qcow2", &opts(None)));
        let rebuilding = Options {
            rebuild: true,
            ..opts(None)
        };
        assert!(converts(&root, Verb::VmSpawn.spec(), "raw", &rebuilding));

        assert_eq!(family(&root, &opts(Some("localhost/other"))), None);

        crate::init::put(
            &root.join("fed.image.kdl"),
            "image {\n    name \"fed\"\n\n    base \"quay.io/fedora/fedora-bootc:44\" {\n\
             \x20       family \"fedora\"\n    }\n\n    modules {\n    }\n}\n",
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
        for kind in ["qcow2", "raw", "iso"] {
            assert_eq!(installer(&root, build, kind, &opts(None)), None);
        }

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_iso_stages_both_recipes_and_refuses_a_local_update_origin() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/repos/deb-families");
        let list = crate::model::image::List::load(&root).0;
        let staged = documents(
            &list,
            "forky",
            "localhost/forky:latest",
            "ghcr.io/someone/forky:latest",
        )
        .expect("a debian target has a measured recipe");
        let named: Vec<_> = staged.iter().map(|(name, _)| *name).collect();
        assert_eq!(
            named,
            [
                "recipe.json",
                "media.json",
                "Containerfile",
                "efi-from-image.patch"
            ]
        );
        let at = |want| staged.iter().find(|(name, _)| *name == want).unwrap();
        assert!(at("recipe.json").1.contains(crate::emit::recipe::STORE));
        assert!(at("recipe.json")
            .1
            .contains("\"image\": \"ghcr.io/someone/forky:latest\""));
        assert!(!at("recipe.json").1.contains("localhost/"));
        assert!(at("Containerfile").1.contains(crate::emit::recipe::STORE));
        assert!(at("media.json")
            .1
            .contains("\"source\": \"localhost/forky:latest\""));
        assert!(at("media.json")
            .1
            .contains("\"ref\": \"ghcr.io/someone/forky:latest\""));

        let local = documents(
            &list,
            "forky",
            "localhost/forky:latest",
            "localhost/forky:latest",
        )
        .unwrap_err();
        assert!(local.contains("IMAGE_REGISTRY"), "{local}");
        let unknown =
            documents(&list, "not-a-target", "image", "ghcr.io/someone/x:latest").unwrap_err();
        assert!(unknown.contains("no measured install recipe"), "{unknown}");
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
