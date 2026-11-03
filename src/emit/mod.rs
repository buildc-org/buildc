pub mod component_build;
pub mod containerfile;
pub mod coverage;
pub mod finalize;
pub mod graph;
pub mod plan;
pub mod recipe;
pub mod record;
pub mod renovate;
pub mod resolved;
pub mod sbom;
pub mod schema_md;
pub mod seed;
pub mod snapshot;
pub mod summary;
pub mod why;
pub mod workflows;

pub(crate) const LIBRARIES: &[(&str, &str)] = &[
    (
        "apparmor-helpers.sh",
        include_str!("../../assets/lib/apparmor-helpers.sh"),
    ),
    (
        "dkms-helpers.sh",
        include_str!("../../assets/lib/dkms-helpers.sh"),
    ),
    (
        "fetch-helpers.sh",
        include_str!("../../assets/lib/fetch-helpers.sh"),
    ),
    (
        "kernel-helpers.sh",
        include_str!("../../assets/lib/kernel-helpers.sh"),
    ),
    (
        "media/90buildc-live/module-setup.sh",
        include_str!("../../assets/lib/media/90buildc-live/module-setup.sh"),
    ),
    (
        "media/90buildc-live/parse-live.sh",
        include_str!("../../assets/lib/media/90buildc-live/parse-live.sh"),
    ),
    (
        "media/90buildc-live/live-generator.sh",
        include_str!("../../assets/lib/media/90buildc-live/live-generator.sh"),
    ),
    (
        "media/90buildc-live/live-root.sh",
        include_str!("../../assets/lib/media/90buildc-live/live-root.sh"),
    ),
    (
        "selinux-helpers.sh",
        include_str!("../../assets/lib/selinux-helpers.sh"),
    ),
    (
        "sign-helpers.sh",
        include_str!("../../assets/lib/sign-helpers.sh"),
    ),
    (
        "vm-helpers.sh",
        include_str!("../../assets/lib/vm-helpers.sh"),
    ),
    (
        "wrap-helpers.sh",
        include_str!("../../assets/lib/wrap-helpers.sh"),
    ),
];

pub(crate) struct Script {
    pub name: &'static str,
    pub about: &'static str,
    pub body: &'static str,
}

pub(crate) const SCRIPTS: &[Script] = &[
    Script {
        name: "lint.sh",
        about: "the repository's lint, as CI runs it",
        body: include_str!("../../assets/scripts/lint.sh"),
    },
    Script {
        name: "buildc.sh",
        about: "fetches and runs the buildc release that repo.kdl pins",
        body: include_str!("../../assets/scripts/buildc.sh"),
    },
    Script {
        name: "smoke.sh",
        about: "boots a built image under qemu and waits for ssh",
        body: include_str!("../../assets/scripts/smoke.sh"),
    },
    Script {
        name: "vm.sh",
        about: "builds a disk image and boots it",
        body: include_str!("../../assets/scripts/vm.sh"),
    },
    Script {
        name: "media.sh",
        about: "assembles the installer media and boots it",
        body: include_str!("../../assets/scripts/media.sh"),
    },
];

pub(crate) fn located(root: &std::path::Path, text: &str) -> String {
    SCRIPTS.iter().fold(text.to_string(), |text, script| {
        text.replace(
            &format!("{}/{}", crate::layout::SCRIPTS, script.name),
            &crate::layout::script(root, script.name),
        )
    })
}

pub enum Part {
    Heading(String),
    Text(String),
    Table(Table),
}

pub struct Table {
    pub title: String,
    pub header: &'static [&'static str],
    pub rows: Vec<(Vec<String>, bool)>,
}

#[cfg(test)]
mod tests {
    #[test]
    fn repository_lint_reads_generated_libraries() {
        let lint = super::SCRIPTS
            .iter()
            .find(|script| script.name == "lint.sh")
            .unwrap()
            .body;
        assert!(lint.contains("roots=(generated/lib components)"));
    }
}
