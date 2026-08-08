use crate::diag::{Source, Span};
use crate::model::asset::Asset;
use crate::model::options::{Opt, Variant};

#[derive(Debug)]
pub struct PackageGroup {
    pub family: String,
    pub packages: Vec<String>,
    pub enablerepo: Option<String>,
    pub span: Span,
}

pub struct Decl {
    pub name: String,
    pub span: Span,
}

pub struct Collect {
    pub file: String,
    pub into: String,
    pub priority: u32,
    pub span: Span,
}

pub struct Contribution {
    pub file: String,
    pub priority: u32,
    pub span: Span,
}

pub struct VerifyException {
    pub class: String,
    pub unit: String,
    pub span: Span,
}

pub struct Module {
    pub path: String,
    pub dir: String,
    pub src: Source,
    pub description: String,
    pub supports: Vec<String>,
    pub provides: Vec<Decl>,
    pub requires: Vec<Decl>,
    pub after: Vec<Decl>,
    pub provides_files: Vec<Decl>,
    pub provides_files_build_only: Vec<String>,
    pub requires_files: Vec<Decl>,
    pub overrides: Vec<Decl>,
    pub verify_exceptions: Vec<VerifyException>,
    pub flavour: Option<String>,
    pub collects: Vec<Collect>,
    pub contributes: Vec<Contribution>,
    pub secrets: Vec<Decl>,
    pub args: Vec<Decl>,
    pub options: Vec<Opt>,
    pub variants: Vec<Variant>,
    pub assets: Vec<Asset>,
    pub packages: Vec<PackageGroup>,
    pub resolved: Vec<(String, String)>,
    pub fragment: Option<String>,
    pub fragment_after: bool,
    pub standard_layer: bool,
}
