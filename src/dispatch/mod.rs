mod build;
mod create;
mod host;
mod import;
mod read;
mod scap;
mod set;
mod tool;

#[cfg(test)]
pub(crate) use read::picker_options;

use crate::command::{self, Context, Verb};
use crate::error::Error;
use common::prompt::Prompt;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};

pub const USAGE_ERROR: u8 = 1;
pub const REPO_ERROR: u8 = 2;

impl From<crate::scap::Verdict> for ExitCode {
    fn from(verdict: crate::scap::Verdict) -> Self {
        match verdict {
            crate::scap::Verdict::Clean => ExitCode::SUCCESS,
            crate::scap::Verdict::Wrong => ExitCode::from(REPO_ERROR),
        }
    }
}

impl From<crate::build::Stopped> for ExitCode {
    fn from(stopped: crate::build::Stopped) -> Self {
        match stopped {
            crate::build::Stopped::Repository => ExitCode::from(REPO_ERROR),
            crate::build::Stopped::Built => ExitCode::SUCCESS,
        }
    }
}

#[derive(Default)]
pub struct Flags {
    pub root: Option<PathBuf>,
    pub owner: Option<String>,
    pub host: Option<String>,
    pub images: Vec<String>,
    pub component: Option<String>,
    pub cn: Option<String>,
    pub from: Option<String>,
    pub base: Option<String>,
    pub format: Option<String>,
    pub target: Option<String>,
    pub datastream: Option<PathBuf>,
    pub baseline: Option<PathBuf>,
    pub base_scan: Option<PathBuf>,
    pub tags: Vec<String>,
    pub kernel: Option<String>,
    pub ram: Option<String>,
    pub backend: Option<String>,
    pub oci_output: Option<String>,
    pub secrets: Vec<String>,
    pub pkgs: Vec<String>,
    pub with: Vec<(String, String)>,
    pub cache_to: bool,
    pub no_cache_from: bool,
    pub rebuild: bool,
    pub dependencies: bool,
    pub untracked: bool,
}

fn one_name(rest: &[&str], name: &str) -> Result<Option<String>, Error> {
    match rest {
        [] => Ok(None),
        [one] => Ok(Some((*one).to_string())),
        _ => Err(Error::Invocation(format!(
            "`{name}` takes one name, not {}",
            rest.join(" ")
        ))),
    }
}

fn repo_root(here: &Context) -> Result<PathBuf, Error> {
    let root = match here {
        Context::Repo(root) => root.clone(),
        Context::Host => {
            return Err(Error::Invocation(format!(
                "this is a buildc image, so there is no source tree to read\n\nhelp: {} \
                 off the documents the build baked",
                answers()
            )))
        }
        Context::Loose => {
            return Err(Error::Invocation(format!(
                "no repo.kdl in {} or any parent directory",
                std::env::current_dir()
                    .unwrap_or_else(|_| PathBuf::from("."))
                    .display()
            )))
        }
    };
    note_pin(&root);
    Ok(root)
}

fn answers() -> String {
    let rows: Vec<String> = command::host_labels()
        .into_iter()
        .map(|label| format!("`buildc {label}`"))
        .collect();
    match rows.len() {
        0 => "nothing answers".to_string(),
        1 => format!("{} answers", crate::said(&rows)),
        _ => format!("{} answer", crate::said(&rows)),
    }
}

static PINNED: AtomicBool = AtomicBool::new(false);

fn note_pin(root: &Path) {
    if PINNED.swap(true, Ordering::Relaxed) {
        return;
    }
    if let Some(version) = crate::parse::repo::pinned_elsewhere(root) {
        eprintln!(
            "buildc: this repository pins buildc {version} and this is {}, so what it generates may \
             differ; `buildc generate` writes this release's output, `buildc-version` moves the pin, \
             and `buildc.sh` fetches the pinned release",
            crate::model::image::BUILDC_VERSION
        );
    }
}

fn open(here: &Context) -> Result<Option<PathBuf>, Error> {
    let root = repo_root(here)?;
    let refused =
        crate::compatible(&root).report(&root.join(crate::layout::REPO_FILE).display().to_string());
    Ok((!refused).then_some(root))
}

fn collection_repo(here: &Context) -> Result<(PathBuf, crate::model::image::List), Error> {
    let root = repo_root(here)?;
    let list = crate::model::image::List::editable(&root)
        .map_err(|issues| Error::Operation(issues.plain()))?;
    Ok((root, list))
}

struct Call<'a> {
    name: &'a str,
    rest: &'a [&'a str],
    prompt: &'a Prompt,
    here: &'a Context,
    flags: Flags,
}

pub fn dispatch(
    verb: Verb,
    name: &str,
    rest: &[&str],
    flags: Flags,
    prompt: &Prompt,
    here: &Context,
) -> Result<ExitCode, Error> {
    if *here == Context::Host && command::on_host(verb) {
        return host::on_host(
            verb,
            name,
            rest,
            flags.format.as_deref(),
            flags.target.as_deref(),
            prompt,
        );
    }
    let call = Call {
        name,
        rest,
        prompt,
        here,
        flags,
    };
    match verb {
        Verb::Upgrade => tool::upgrade(call),
        Verb::CreateRepo => create::repo(call),
        Verb::CreateGit => create::git(call),
        Verb::CreateImage => create::image(call),
        Verb::CreateFlavour => create::flavour(call),
        Verb::CreateModule => create::component(call),
        Verb::CreateKey => create::key(call),
        Verb::CreateScripts => create::scripts(call),
        Verb::SetKey => set::key(call),
        Verb::ImportModule => import::component(call),
        Verb::FetchModules => import::components(call),
        Verb::SetModule => set::component(call),
        Verb::SetWorkflows => set::workflows(call),
        Verb::SetLibrary => set::library(call),
        Verb::SetConforms => set::conforms(call),
        Verb::SetClaims => set::claims(call),
        Verb::Build => build::image(call),
        Verb::Validate => build::validate(call),
        Verb::Attribute => build::attribute(call),
        Verb::VmBuild | Verb::VmRun | Verb::VmSpawn => build::vm(call),
        Verb::MediaBuild | Verb::MediaRun => build::media(call),
        Verb::RegistryNamespace => build::registry_namespace(call),
        Verb::RegistryRef => build::registry_ref(call),
        Verb::Recipe => build::recipe(call),
        Verb::ScapContent => scap::content(call),
        Verb::ScapTailoring => scap::tailoring(call),
        Verb::ScapRules => scap::rules(call),
        Verb::Scap => scap::report(call),
        Verb::BuildRecord => tool::build_record(call),
        Verb::ValidateImage => tool::validate_image(call),
        Verb::Fetch => tool::fetch(call),
        Verb::Check
        | Verb::Generate
        | Verb::Plan
        | Verb::Verify
        | Verb::Section
        | Verb::Graph
        | Verb::Why
        | Verb::Coverage
        | Verb::Summary
        | Verb::Sbom => read::reading(verb, call),
    }
}

#[cfg(test)]
mod tests {
    use super::{dispatch, Context, Error, Flags, Verb};
    use common::prompt::Prompt;

    fn untracks(name: &str, untracked: bool, prompt: &Prompt) -> (bool, Result<(), String>) {
        let root = std::env::temp_dir().join(format!(
            "buildc-set-component-{name}-{}",
            std::process::id()
        ));
        crate::init::put(&root.join("repo.kdl"), "schema-version 1\n").unwrap();
        crate::init::put(
            &crate::layout::lock(&root),
            "component \"browser\" {\n    library \"one\"\n    path \"browser\"\n    content \"0\"\n    track\n}\n",
        )
        .unwrap();
        let ran = dispatch(
            Verb::SetModule,
            "set component",
            &["components/browser"],
            Flags {
                untracked,
                ..Flags::default()
            },
            prompt,
            &Context::Repo(root.clone()),
        )
        .map(drop)
        .map_err(|err| match err {
            Error::Invocation(said) | Error::Operation(said) => said,
            Error::Cancelled => "cancelled".to_string(),
        });
        let tracked = crate::provenance::lock::entry(&root, "browser")
            .expect("the entry stays")
            .tracked;
        std::fs::remove_dir_all(root).unwrap();
        (tracked, ran)
    }

    #[test]
    fn set_component_untracks_with_the_flag_or_a_yes() {
        assert_eq!(untracks("flag", true, &Prompt::silent()), (false, Ok(())));
        assert_eq!(
            untracks("yes", false, &Prompt::scripted(vec!["y".into()])),
            (false, Ok(()))
        );
    }

    #[test]
    fn set_component_keeps_tracking_on_a_no_or_with_nobody_to_ask() {
        assert_eq!(
            untracks("no", false, &Prompt::scripted(vec!["n".into()])),
            (true, Ok(()))
        );
        let (tracked, refused) = untracks("silent", false, &Prompt::silent());
        assert!(tracked);
        assert!(refused.unwrap_err().contains("--untracked"));
    }

    #[test]
    fn the_host_sentence_names_each_command_and_agrees_in_number() {
        let labels = crate::command::host_labels();
        let said = super::answers();
        for label in &labels {
            assert!(said.contains(&format!("`buildc {label}`")), "{said}");
        }
        let verb = match labels.len() {
            0 => "nothing answers",
            1 => " answers",
            _ => " answer",
        };
        assert!(said.ends_with(verb), "{said}");
    }
}
