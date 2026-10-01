use crate::copy;
use common::prompt::Prompt;
use common::ui::Choice;
use std::os::unix::process::CommandExt as _;
use std::path::Path;
use std::process::Command;

const SCRIPT: &str = "vm.sh";

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
    let live = match kind == "iso" && converts(root, noun, kind, opts) {
        true => Some(stage(root, opts)?),
        false => None,
    };
    let access = access(root, kind, opts);
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
            .args(argv(noun, kind, opts, installer, live.as_deref(), &access))
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
    let refuse = || recipe::refusal(list, name);
    let build = recipe::build(list, name, imgref, imgref, &[recipe::STORE.to_string()])
        .ok_or_else(refuse)?;
    let media = recipe::media(list, name, image, imgref).ok_or_else(refuse)?;
    Ok(vec![
        ("recipe.json", build.render()),
        ("media.json", media.render()),
        ("Containerfile", live_env(list, name, image)),
        ("efi-from-image.patch", recipe::EFI_PATCH.to_string()),
    ])
}

fn live_env(list: &crate::model::image::List, name: &str, image: &str) -> String {
    let rpm = list
        .targets()
        .into_iter()
        .find(|t| t.to_string() == name)
        .and_then(|t| list.images.iter().find(|i| i.id == t.image))
        .and_then(|i| i.base.as_ref())
        .is_some_and(|base| rpm(&base.family));
    let env = crate::emit::recipe::LIVE_ENV;
    if !rpm {
        return env.to_string();
    }
    env.lines()
        .map(|line| match line.starts_with("ARG LIVE_BASE=") {
            true => format!("ARG LIVE_BASE={image}\n"),
            false => format!("{line}\n"),
        })
        .collect()
}

fn staged_list(root: &Path) -> crate::model::image::List {
    crate::load(root).list
}

fn staged(root: &Path, opts: &Options) -> Result<(String, Vec<(&'static str, String)>), String> {
    let list = staged_list(root);
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
    let documents = documents(&list, &target.to_string(), &image, &imgref)?;
    Ok((crate::emit::recipe::live(&published), documents))
}

fn stage(root: &Path, opts: &Options) -> Result<String, String> {
    let (live, documents) = staged(root, opts)?;
    for (name, body) in documents {
        crate::init::put(&root.join(BOOTISO).join(name), &body)?;
    }
    Ok(live)
}

fn converts(root: &Path, noun: &str, kind: &str, opts: &Options) -> bool {
    if noun == "build" || opts.rebuild {
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

fn installer(root: &Path, noun: &str, kind: &str, opts: &Options) -> Option<&'static str> {
    if kind == "iso" || !converts(root, noun, kind, opts) {
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
    live: Option<&str>,
    access: &Access,
) -> Vec<String> {
    let mut args = vec![noun.to_string(), kind.to_string()];
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

fn rows(noun: &str) -> Vec<Choice> {
    TYPES
        .iter()
        .map(|(name, about)| match (*name, noun) {
            ("iso", "spawn") => Choice::new(*name, copy::NO_ISO_SPAWN).unavailable(),
            _ => Choice::new(*name, *about),
        })
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
            argv("run", "qcow2", &opts, None, None, &Access::default()),
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
            argv("build", "raw", &opts, Some(BOOTC), None, &Access::default())[..4],
            ["build", "raw", "--installer", "bootc"]
        );
    }

    #[test]
    fn spawn_shows_the_iso_it_cannot_boot_rather_than_hiding_it() {
        let iso = |noun| rows(noun).pop().expect("iso is the last row");
        assert!(!iso("spawn").available);
        assert!(iso("run").available);
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
        let args = argv("run", "raw", &opts(None), Some(BOOTC), None, &recorded);
        assert!(args.contains(&"--login".to_string()));
        assert!(args.contains(&"--ssh-key".to_string()));
        assert!(!access(&root, "raw", &opts(Some("localhost/other"))).login);
        assert!(!access(&root, "iso", &opts(None)).login);
        let refuse = |kind, opts: &Options| {
            run(&root, "run", "vm run", Some(kind), opts, &Prompt::silent()).unwrap_err()
        };

        let build = "build";
        for kind in ["qcow2", "raw"] {
            assert_eq!(installer(&root, build, kind, &opts(None)), Some(BOOTC));
        }
        let installed = refuse("qcow2", &opts(None));
        assert!(installed.contains("vm.sh is not there"), "{installed}");
        assert_eq!(installer(&root, build, "iso", &opts(None)), None);

        let unasked =
            run(&root, "run", "vm run", None, &opts(None), &Prompt::silent()).unwrap_err();
        assert!(
            unasked.contains("takes `qcow2`, `raw` or `iso`"),
            "{unasked}"
        );

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
        assert!(unknown.contains("not a target"), "{unknown}");
    }

    #[test]
    fn a_module_provided_capability_reaches_the_staged_recipe() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/repos/media-recipe");
        let root = std::env::temp_dir().join(format!("buildc-media-recipe-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for file in ["repo.kdl", "deb.image.kdl", "modules/core/luks/module.kdl"] {
            let body = std::fs::read_to_string(fixture.join(file)).expect("the fixture file reads");
            crate::init::put(&root.join(file), &body).expect("the fixture file writes");
        }
        let pinned = std::env::var("IMAGE_REGISTRY").ok();
        std::env::set_var("IMAGE_REGISTRY", "ghcr.io/example");
        let opts = Options {
            target: Some("deb".to_string()),
            image: None,
            tag: None,
            ram: None,
            rebuild: false,
        };
        let answered = staged(&root, &opts);
        match pinned {
            Some(previous) => std::env::set_var("IMAGE_REGISTRY", previous),
            None => std::env::remove_var("IMAGE_REGISTRY"),
        }
        let (_, files) = answered.expect("the fixture answers its recipe");
        let recipe = files
            .into_iter()
            .find(|(name, _)| *name == "recipe.json")
            .expect("recipe.json is staged")
            .1;
        assert!(recipe.contains("\"luksInitramfs\": true"), "{recipe}");
        let bare = documents(
            &crate::model::image::List::load(&root).0,
            "deb",
            "localhost/deb:latest",
            "ghcr.io/example/deb:latest",
        )
        .expect("the fixture answers its recipe")
        .into_iter()
        .find(|(name, _)| *name == "recipe.json")
        .expect("recipe.json is staged")
        .1;
        assert!(!bare.contains("luksInitramfs"), "{bare}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_live_environment_is_the_payload_on_a_dnf_family() {
        let base = |repo: &str| {
            let root = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/repos")
                .join(repo);
            let list = crate::model::image::List::load(&root).0;
            let name = list.ungated_target().unwrap().to_string();
            live_env(&list, &name, "localhost/payload:latest")
                .lines()
                .filter(|line| line.starts_with("ARG LIVE_BASE="))
                .map(str::to_string)
                .collect::<Vec<_>>()
        };
        assert_eq!(base("minimal"), ["ARG LIVE_BASE=localhost/payload:latest"]);
        assert_eq!(
            base("deb-families"),
            ["ARG LIVE_BASE=quay.io/fedora/fedora-bootc:44"]
        );
    }

    #[test]
    fn a_type_that_is_not_one_of_the_three_is_refused_by_name() {
        let silent = Prompt::silent();
        assert!(kind("build", "vm build", Some("vmdk"), &silent).is_err());
        assert_eq!(
            kind("build", "vm build", Some("raw"), &silent).unwrap(),
            Some("raw")
        );
        assert!(kind("build", "vm build", None, &silent).is_err());
    }
}
