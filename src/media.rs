use crate::parse::component::rpm;
use crate::vm::Options;
use std::os::unix::process::CommandExt as _;
use std::path::Path;
use std::process::Command;

const SCRIPT: &str = "media.sh";

const BOOTISO: &str = "out/bootiso";

pub fn run(root: &Path, noun: &str, opts: &Options) -> Result<(), String> {
    let live = match noun == "run" && !opts.rebuild && iso(root).is_file() {
        true => None,
        false => Some(stage(root, opts)?),
    };
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
            .args(argv(noun, opts, live.as_deref()))
            .exec()
    ))
}

fn iso(root: &Path) -> std::path::PathBuf {
    root.join(BOOTISO).join("install.iso")
}

type Documents = Vec<(&'static str, String)>;

fn documents(
    list: &crate::model::image::List,
    name: &str,
    image: &str,
    imgref: &str,
) -> Result<Documents, String> {
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

fn staged(root: &Path, opts: &Options) -> Result<(String, Documents), String> {
    let list = crate::load(root).list;
    let target = crate::vm::target(&list, opts)
        .ok_or("no target to build installer media for; name one with `--target`")?;
    let published = target.published();
    let tag = crate::registry::tag(opts.tag.as_deref());
    let imgref =
        crate::registry::reference(&list, root, opts.target.as_deref(), opts.tag.as_deref())?;
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

fn argv(noun: &str, opts: &Options, live: Option<&str>) -> Vec<String> {
    let mut args = vec![noun.to_string()];
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
    args
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
            argv("run", &opts, None),
            ["run", "--target", "desktop/dx", "--tag", "42", "--rebuild"]
        );
        assert_eq!(
            argv("build", &opts, Some("localhost/x-installer:latest"))[..3],
            ["build", "--live-image", "localhost/x-installer:latest"]
        );
    }

    #[test]
    fn the_media_stages_both_recipes_and_refuses_a_local_update_origin() {
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
        assert_eq!(named, ["recipe.json", "media.json", "Containerfile"]);
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
    fn a_component_provided_capability_reaches_the_staged_recipe() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/repos/media-recipe");
        let root = std::env::temp_dir().join(format!("buildc-media-recipe-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for file in [
            "repo.kdl",
            "deb.image.kdl",
            "components/core/luks/manifest.kdl",
            "components/base-image/debian-forky/manifest.kdl",
        ] {
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
}
