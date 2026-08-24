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

pub struct Table {
    pub title: String,
    pub header: &'static [&'static str],
    pub rows: Vec<(Vec<String>, bool)>,
}
