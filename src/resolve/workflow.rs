use crate::diag::{Issue, Issues};
use crate::layout;
use crate::model::image::{Backend, List};
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

pub struct Shipped {
    pub stem: &'static str,
    pub body: &'static str,
    pub about: &'static str,
    pub needs: Needs,
    pub at: Option<(i64, &'static str)>,
}

impl Shipped {
    pub fn file(&self) -> String {
        format!("{}.yml", self.stem)
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
    ($stem:literal, $about:literal, $needs:expr, $at:expr) => {
        Shipped {
            stem: $stem,
            body: include_str!(concat!("../../assets/.github/workflows/", $stem, ".yml")),
            about: $about,
            needs: $needs,
            at: $at,
        }
    };
}

pub const SHIPPED: &[Shipped] = &[
    shipped!(
        "build",
        "builds, scans, signs and publishes every image",
        Needs::Nothing,
        Some((0, "*"))
    ),
    shipped!(
        "build-disk",
        "builds a disk image with bootc-image-builder",
        Needs::Selinux,
        None
    ),
    shipped!(
        "base-sig-probe",
        "measures what each base signs and ships, and fixes it",
        Needs::Nothing,
        Some((-6, "*"))
    ),
    shipped!(
        "cleanup-registry",
        "prunes old versions of the images off the registry",
        Needs::Nothing,
        Some((2, "*"))
    ),
    shipped!(
        "kernel-freshness",
        "tracks the kernel a component pins, and proposes a bump",
        Needs::Kernel,
        Some((-3, "*"))
    ),
    shipped!(
        "smoke-test",
        "installs the image to a disk and boots it under qemu",
        Needs::Nothing,
        Some((-9, "1"))
    ),
    shipped!(
        "update-components",
        "refreshes tracked components and opens a pull request",
        Needs::Nothing,
        Some((-7, "1"))
    ),
];

pub fn find(stem: &str) -> Option<&'static Shipped> {
    SHIPPED.iter().find(|shipped| shipped.stem == stem)
}

pub const DEFAULT_AT: (u32, u32) = (12, 30);

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

pub fn facts(
    basis: &Basis,
    scans_scheduled: bool,
    publishes_scheduled: bool,
    backend: Backend,
) -> Vec<&'static str> {
    let mut facts = match basis.kernel {
        true => vec!["kernel"],
        false => vec!["no-kernel"],
    };
    facts.push(match publishes_scheduled {
        true => "scheduled-publish",
        false => "push-publish",
    });
    facts.push(match scans_scheduled || publishes_scheduled {
        true => "scheduled-scan",
        false => "push-scan",
    });
    facts.push(match backend {
        Backend::Buildah => "backend-buildah",
        Backend::Buildx => "backend-buildx",
    });
    facts
}

pub struct Declared {
    pub file: String,
    pub schedule: Option<String>,
    pub body: &'static str,
}

pub fn resolve(list: &List, basis: &Basis, issues: &mut Issues) -> Vec<Declared> {
    for declared in &list.workflows {
        let Some(shipped) = find(&declared.name) else {
            let known: Vec<&str> = SHIPPED.iter().map(|s| s.stem).collect();
            issues.push(
                Issue::new(
                    format!("`{}` is not a workflow", declared.name),
                    &list.repo_src,
                )
                .at(declared.span, "nothing ships one by that name")
                .help(format!("workflows: {}", known.join(", "))),
            );
            continue;
        };
        if declared.schedule.is_some() && shipped.at.is_none() {
            issues.push(
                Issue::new(
                    format!("`{}` runs on no schedule", declared.name),
                    &list.repo_src,
                )
                .at(declared.span, "`schedule` has nothing to replace")
                .help("drop `schedule` from it"),
            );
        }
        if !shipped.met(basis) {
            issues.push(
                Issue::new(
                    format!("`{}` cannot run in this repository", declared.name),
                    &list.repo_src,
                )
                .at(declared.span, shipped.needs.unmet())
                .help("drop it from `workflows`, or declare what it needs"),
            );
        }
    }

    SHIPPED
        .iter()
        .filter_map(|shipped| {
            let declared = list.workflows.iter().find(|w| w.name == shipped.stem)?;
            Some(Declared {
                file: shipped.file(),
                schedule: shipped.at.map(|(offset, day)| {
                    declared
                        .schedule
                        .clone()
                        .unwrap_or_else(|| cron(list.workflows_at, offset, day))
                }),
                body: shipped.body,
            })
        })
        .collect()
}

fn cron((hour, minute): (u32, u32), offset: i64, day: &str) -> String {
    let hour = (i64::from(hour) + offset).rem_euclid(24);
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

const RETIRED: [&str; 3] = [
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
                shipped.at.is_some(),
                "{} disagrees about having a schedule",
                shipped.stem
            );
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
        assert!(met("build"));
        assert!(
            met("smoke-test"),
            "it installs out of the image, not a family"
        );
        assert!(!met("build-disk"), "the builder relabels with SELinux");
    }

    #[test]
    fn a_schedule_is_the_build_time_plus_its_offset() {
        assert_eq!(cron((12, 30), 0, "*"), "30 12 * * *");
        assert_eq!(cron((12, 30), 2, "*"), "30 14 * * *");
        assert_eq!(cron((12, 30), -6, "*"), "30 6 * * *");
        assert_eq!(cron((3, 0), -9, "1"), "0 18 * * 1");
    }

    #[test]
    fn publishing_on_schedule_also_schedules_scanning() {
        let basis = Basis {
            selinux: true,
            kernel: false,
        };
        assert_eq!(
            facts(&basis, false, false, Backend::Buildah),
            ["no-kernel", "push-publish", "push-scan", "backend-buildah"]
        );
        assert_eq!(
            facts(&basis, false, true, Backend::Buildah),
            [
                "no-kernel",
                "scheduled-publish",
                "scheduled-scan",
                "backend-buildah"
            ]
        );
        assert_eq!(
            facts(&basis, true, false, Backend::Buildah),
            [
                "no-kernel",
                "push-publish",
                "scheduled-scan",
                "backend-buildah"
            ]
        );
        assert_eq!(
            facts(&basis, true, true, Backend::Buildah),
            [
                "no-kernel",
                "scheduled-publish",
                "scheduled-scan",
                "backend-buildah"
            ]
        );
    }

    #[test]
    fn a_buildx_declaration_keeps_the_buildx_job() {
        let basis = Basis {
            selinux: true,
            kernel: false,
        };
        assert_eq!(
            facts(&basis, false, false, Backend::Buildx).last(),
            Some(&"backend-buildx")
        );
    }
}
