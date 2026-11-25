use crate::diag::{Issue, Issues};
use crate::layout;
use crate::model::image::{Backend, List, On};
use crate::parse::component::rpm;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Needs {
    Nothing,
    Selinux,
    Kernel,
}

impl Needs {
    pub fn unmet(self) -> &'static str {
        match self {
            Self::Nothing => "",
            Self::Selinux => "needs a fedora or rhel image",
            Self::Kernel => "needs a component taking a KERNEL arg",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Build,
    Repository,
}

#[derive(Clone, Copy)]
pub struct Schedule {
    pub hour: u32,
    pub weekly: bool,
}

pub struct Shipped {
    pub stem: &'static str,
    pub body: &'static str,
    pub about: &'static str,
    pub needs: Needs,
    pub scope: Scope,
    pub schedule: Option<Schedule>,
}

impl Shipped {
    pub fn file(&self) -> String {
        format!("{}.yml", self.stem)
    }

    /// The match reads the trigger keys at the two-space indent of the shipped workflow files.
    /// GitHub nests a tag filter as `tags:` under `push:`.
    pub fn ships(&self, on: &On) -> bool {
        let key = match on {
            On::Push => "  push:",
            On::Pr => "  pull_request:",
            On::Schedule { .. } => "  schedule:",
            On::Tag(_) => "    tags:",
        };
        let section = self
            .body
            .lines()
            .skip_while(|line| *line != "on:")
            .skip(1)
            .take_while(|line| line.is_empty() || line.starts_with(' ') || line.starts_with('#'));
        section.into_iter().any(|line| line == key)
    }

    pub fn met(&self, basis: &Basis) -> bool {
        match self.needs {
            Needs::Nothing => true,
            Needs::Selinux => basis.selinux,
            Needs::Kernel => basis.kernel,
        }
    }
}

macro_rules! shipped {
    ($stem:literal, $about:literal, $needs:expr, $scope:expr, $schedule:expr) => {
        Shipped {
            stem: $stem,
            body: include_str!(concat!("../../assets/.github/workflows/", $stem, ".yml")),
            about: $about,
            needs: $needs,
            scope: $scope,
            schedule: $schedule,
        }
    };
}

const fn daily(hour: u32) -> Option<Schedule> {
    Some(Schedule {
        hour,
        weekly: false,
    })
}

const fn weekly(hour: u32) -> Option<Schedule> {
    Some(Schedule { hour, weekly: true })
}

pub const IMAGE_BUILD: &str = "image-build";

pub const SHIPPED: &[Shipped] = &[
    shipped!(
        "image-build",
        "builds, scans, signs and publishes every image",
        Needs::Nothing,
        Scope::Build,
        daily(4)
    ),
    shipped!(
        "build-disk",
        "builds a disk image with bootc-image-builder",
        Needs::Selinux,
        Scope::Build,
        None
    ),
    shipped!(
        "base-sig-probe",
        "measures what each base signs and ships, and fixes it",
        Needs::Nothing,
        Scope::Repository,
        daily(3)
    ),
    shipped!(
        "cleanup-registry",
        "prunes old versions of the images off the registry",
        Needs::Nothing,
        Scope::Repository,
        daily(8)
    ),
    shipped!(
        "kernel-freshness",
        "tracks the kernel a component pins, and proposes a bump",
        Needs::Kernel,
        Scope::Repository,
        daily(3)
    ),
    shipped!(
        "smoke-test",
        "installs the image to a disk and boots it under qemu",
        Needs::Nothing,
        Scope::Build,
        weekly(6)
    ),
    shipped!(
        "update-components",
        "moves remote components and opens a pull request",
        Needs::Nothing,
        Scope::Repository,
        weekly(2)
    ),
];

pub fn find(stem: &str) -> Option<&'static Shipped> {
    SHIPPED.iter().find(|shipped| shipped.stem == stem)
}

pub struct Basis {
    pub selinux: bool,
    pub kernel: bool,
}

impl Basis {
    pub fn of(list: &List) -> Self {
        Self {
            selinux: list
                .images
                .iter()
                .any(|image| image.base.as_ref().is_some_and(|base| rpm(&base.family))),
            kernel: list.images.iter().any(|image| {
                image
                    .components()
                    .any(|component| component.args.iter().any(|arg| arg.name == KERNEL_ARG))
            }),
        }
    }

    pub fn scaffolding(family: &str) -> Self {
        Self {
            selinux: rpm(family),
            kernel: false,
        }
    }
}

const KERNEL_ARG: &str = "KERNEL";

pub fn unlocked(list: &List, args: &[String]) -> Vec<&'static Shipped> {
    if list.workflows.is_empty() || list.workflows.iter().any(|w| find(&w.name).is_none()) {
        return Vec::new();
    }
    let was = Basis::of(list);
    let now = Basis {
        selinux: was.selinux,
        kernel: was.kernel || args.iter().any(|arg| arg == KERNEL_ARG),
    };
    SHIPPED
        .iter()
        .filter(|shipped| shipped.met(&now) && !shipped.met(&was))
        .filter(|shipped| !list.workflows.iter().any(|w| w.name == shipped.stem))
        .collect()
}

pub fn facts(basis: &Basis, backend: Backend) -> Vec<&'static str> {
    let mut facts = match basis.kernel {
        true => vec!["kernel"],
        false => vec!["no-kernel"],
    };
    facts.push(match backend {
        Backend::Buildah => "backend-buildah",
        Backend::Buildx => "backend-buildx",
    });
    facts
}

pub struct Declared {
    pub file: String,
    pub body: &'static str,
    pub triggers: Option<Vec<On>>,
    pub cron: Option<String>,
}

fn names(list: &List) -> Vec<String> {
    list.builds.iter().map(|build| build.name.clone()).collect()
}

fn default_build(list: &List) -> Option<String> {
    match (&list.default_image_id, list.builds.as_slice()) {
        (Some(name), _) => Some(name.clone()),
        (None, [only]) => Some(only.name.clone()),
        (None, _) => None,
    }
}

/// Listed triggers replace the shipped triggers of the workflow.
/// A pull request with no `builds` of its own builds the default build alone if the workflow selects it.
pub fn builds_on(list: &List, stem: &str, on: &On) -> Vec<String> {
    let Some(workflow) = list.workflows.iter().find(|w| w.name == stem) else {
        return Vec::new();
    };
    let held = workflow.builds.of(&names(list));
    let trigger = workflow
        .triggers
        .iter()
        .find(|trigger| std::mem::discriminant(&trigger.on) == std::mem::discriminant(on));
    let pr = matches!(on, On::Pr);
    match trigger {
        Some(trigger) if pr && !trigger.builds.written() => default_build(list)
            .filter(|build| held.contains(build))
            .into_iter()
            .collect(),
        Some(trigger) => trigger.builds.of(&held),
        None if !workflow.triggers.is_empty() => Vec::new(),
        None if !find(stem).is_some_and(|shipped| shipped.ships(on)) => Vec::new(),
        None if pr => default_build(list)
            .filter(|build| held.contains(build))
            .into_iter()
            .collect(),
        None => held,
    }
}

pub fn dispatched_builds(list: &List, stem: &str) -> Vec<String> {
    list.workflows
        .iter()
        .find(|w| w.name == stem)
        .map_or_else(Vec::new, |workflow| workflow.builds.of(&names(list)))
}

pub fn resolve(list: &List, basis: &Basis, issues: &mut Issues) -> Vec<Declared> {
    let known = names(list);
    for declared in &list.workflows {
        let Some(shipped) = find(&declared.name) else {
            let stems: Vec<&str> = SHIPPED.iter().map(|s| s.stem).collect();
            issues.push(
                Issue::new(
                    format!("`{}` is not a workflow", declared.name),
                    &list.repo_src,
                )
                .at(declared.span, "nothing ships one by that name")
                .help(format!("workflows: {}", stems.join(", "))),
            );
            continue;
        };
        if !shipped.met(basis) {
            issues.push(
                Issue::new(
                    format!("`{}` cannot run in this repository", declared.name),
                    &list.repo_src,
                )
                .at(declared.span, shipped.needs.unmet())
                .help("drop it from `github { workflows }`, or declare what it needs"),
            );
        }
        let selections = std::iter::once(&declared.builds)
            .chain(declared.triggers.iter().map(|trigger| &trigger.builds));
        for selection in selections {
            let Some(span) = selection.span else {
                continue;
            };
            if shipped.scope == Scope::Repository {
                issues.push(
                    Issue::new(
                        format!("`{}` runs once for the repository", declared.name),
                        &list.repo_src,
                    )
                    .at(span, "it selects no build")
                    .help("drop `builds` from it"),
                );
                continue;
            }
            for name in selection
                .named
                .iter()
                .chain(&selection.removed)
                .filter(|name| !known.contains(name))
            {
                issues.push(
                    Issue::new(
                        format!("`{}` selects `{name}`, which is not a build", declared.name),
                        &list.repo_src,
                    )
                    .at(span, "no build has that name")
                    .help(format!("builds: {}", known.join(", "))),
                );
            }
        }
        for trigger in &declared.triggers {
            if let On::Schedule { time: None, .. } = trigger.on {
                if shipped.schedule.is_none() {
                    issues.push(
                        Issue::new(
                            format!("`{}` ships no schedule", declared.name),
                            &list.repo_src,
                        )
                        .at(trigger.span, "no time to default to")
                        .help("give the schedule a time: `schedule time=\"12:30\"`"),
                    );
                }
            }
        }
    }

    SHIPPED
        .iter()
        .filter_map(|shipped| {
            let declared = list.workflows.iter().find(|w| w.name == shipped.stem)?;
            let schedule = declared
                .triggers
                .iter()
                .find_map(|trigger| match trigger.on {
                    On::Schedule { weekly, time } => Some((weekly, time)),
                    _ => None,
                });
            let cron = match (schedule, shipped.schedule) {
                (Some((weekly, time)), shipped) => {
                    let weekly = weekly.unwrap_or(shipped.is_some_and(|s| s.weekly));
                    let time = time.or(shipped.map(|s| (s.hour, 0)));
                    time.map(|time| cron(time, weekly))
                }
                (None, Some(shipped)) => Some(cron((shipped.hour, 0), shipped.weekly)),
                (None, None) => None,
            };
            Some(Declared {
                file: shipped.file(),
                body: shipped.body,
                triggers: (!declared.triggers.is_empty())
                    .then(|| declared.triggers.iter().map(|t| t.on.clone()).collect()),
                cron,
            })
        })
        .collect()
}

fn cron((hour, minute): (u32, u32), weekly: bool) -> String {
    let day = match weekly {
        true => "1",
        false => "*",
    };
    format!("{minute} {hour} * * {day}")
}

pub fn orphans(root: &Path, generated: &[(PathBuf, String)]) -> Vec<PathBuf> {
    SHIPPED
        .iter()
        .map(|shipped| PathBuf::from(layout::WORKFLOW_DIR).join(shipped.file()))
        .chain(RETIRED.iter().map(PathBuf::from))
        .filter(|path| root.join(path).is_file())
        .filter(|path| !generated.iter().any(|(written, _)| written == path))
        .collect()
}

const RETIRED: [&str; 4] = [
    ".github/workflows/build.yml",
    ".github/workflows/reconcile-workflows.yml",
    "scripts/render-iso-config.sh",
    "disk_config/iso.template.toml",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_an_earlier_release_wrote_is_swept() {
        let root = std::env::temp_dir().join(format!("buildc-retired-{}", std::process::id()));
        let retired = root.join("scripts/render-iso-config.sh");
        std::fs::create_dir_all(retired.parent().unwrap()).unwrap();
        std::fs::write(&retired, "").unwrap();
        std::fs::write(root.join("scripts/own.sh"), "").unwrap();
        assert_eq!(
            orphans(&root, &[]),
            [PathBuf::from("scripts/render-iso-config.sh")]
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn every_shipped_body_carries_the_markers_its_row_claims() {
        for shipped in SHIPPED {
            let scheduled = shipped.body.contains("# buildc:schedule");
            assert_eq!(
                scheduled,
                shipped.schedule.is_some(),
                "{} disagrees about having a schedule",
                shipped.stem
            );
            if let Some(schedule) = shipped.schedule {
                let day = if schedule.weekly { "1" } else { "*" };
                let line = format!("- cron: '0 {} * * {day}'  # buildc:schedule", schedule.hour);
                assert!(
                    shipped.body.contains(&line),
                    "{} ships another time",
                    shipped.stem
                );
            }
            assert!(
                shipped.about.len() <= 55,
                "{} does not fit a picker row",
                shipped.stem
            );
        }
    }

    #[test]
    fn a_deb_repository_may_smoke_test_and_may_not_build_a_disk() {
        let deb = Basis {
            selinux: false,
            kernel: false,
        };
        let met = |stem: &str| find(stem).expect(stem).met(&deb);
        assert!(met(IMAGE_BUILD));
        assert!(
            met("smoke-test"),
            "it installs out of the image, not a family"
        );
        assert!(!met("build-disk"), "the builder relabels with SELinux");
    }

    #[test]
    fn a_weekly_schedule_runs_on_monday() {
        assert_eq!(cron((4, 0), false), "0 4 * * *");
        assert_eq!(cron((12, 30), true), "30 12 * * 1");
    }

    #[test]
    fn a_buildx_declaration_keeps_the_buildx_job() {
        let basis = Basis {
            selinux: true,
            kernel: false,
        };
        assert_eq!(
            facts(&basis, Backend::Buildx),
            ["no-kernel", "backend-buildx"]
        );
    }
}
