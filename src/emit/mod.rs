pub mod containerfile;
pub mod coverage;
pub mod finalize;
pub mod graph;
pub mod json;
pub mod module_build;
pub mod plan;
pub mod sbom;
pub mod schema_md;
pub mod seed;
pub mod summary;
pub mod why;
pub mod workflows;

pub(crate) const LIBRARIES: &[(&str, &str)] = &[
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
        "repo-helpers.sh",
        include_str!("../../assets/lib/repo-helpers.sh"),
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
