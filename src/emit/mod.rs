pub mod containerfile;
pub mod coverage;
pub mod finalize;
pub mod graph;
pub mod json;
pub mod module_build;
pub mod plan;
pub mod recipe;
pub mod sbom;
pub mod schema_md;
pub mod seed;
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
        "selinux-helpers.sh",
        include_str!("../../assets/lib/selinux-helpers.sh"),
    ),
    (
        "sign-helpers.sh",
        include_str!("../../assets/lib/sign-helpers.sh"),
    ),
    (
        "wrap-helpers.sh",
        include_str!("../../assets/lib/wrap-helpers.sh"),
    ),
];

pub(crate) const SCRIPTS: &[(&str, &str)] = &[
    (
        "scripts/lint.sh",
        include_str!("../../assets/scripts/lint.sh"),
    ),
    (
        "scripts/buildc.sh",
        include_str!("../../assets/scripts/buildc.sh"),
    ),
    ("scripts/vm.sh", include_str!("../../assets/scripts/vm.sh")),
];

#[cfg(test)]
mod tests {
    #[test]
    fn repository_lint_reads_generated_libraries() {
        let lint = super::SCRIPTS
            .iter()
            .find(|(path, _)| *path == "scripts/lint.sh")
            .unwrap()
            .1;
        assert!(lint.contains("find scripts generated/lib modules"));
    }
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
