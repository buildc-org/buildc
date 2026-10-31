use crate::diag::{Source, Span};
use crate::model::asset::Asset;
use crate::model::options::{Opt, Value, Variant};
use crate::provenance::lock::Record;

#[derive(Debug)]
pub struct PackageGroup {
    pub family: String,
    pub packages: Vec<String>,
    pub enablerepo: Option<String>,
    pub span: Span,
}

pub struct Copr {
    pub owner: String,
    pub project: String,
    pub family: Vec<String>,
    pub span: Span,
}

impl Copr {
    pub fn name(&self) -> String {
        format!("{}/{}", self.owner, self.project)
    }

    pub fn url(&self) -> String {
        format!(
            "https://copr.fedorainfracloud.org/coprs/{}/{}/",
            self.owner, self.project
        )
    }

    pub fn env_name(&self) -> String {
        format!(
            "COPR_{}",
            self.name()
                .to_uppercase()
                .replace(['/', '-', '.', '+'], "_")
        )
    }

    pub fn selector(&self) -> String {
        format!(
            "copr:copr.fedorainfracloud.org:{}:{}",
            self.owner, self.project
        )
    }
}

pub struct Coverage {
    pub benchmark: String,
    pub rules: Vec<String>,
    pub span: Span,
}

pub struct Decl {
    pub name: String,
    pub span: Span,
}

pub struct Import {
    pub path: String,
    pub options: Vec<(String, Vec<Value>)>,
    pub span: Span,
}

pub struct BaseFacts {
    pub image: String,
    pub template: String,
    pub family: String,
    pub bootloaders: Vec<String>,
    pub scap_content: String,
}

pub struct BaseComponent {
    pub name: String,
    pub owner: Option<String>,
    pub image: String,
    pub family: String,
    pub provides: Vec<String>,
    pub requires: Vec<String>,
    pub imports: Vec<String>,
    pub about: String,
    pub version: Option<String>,
    pub scap_content: String,
    pub bootloaders: Vec<String>,
}

pub struct Located {
    pub name: String,
    pub file: String,
    pub build_only: bool,
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

pub struct FileMode {
    pub path: String,
    pub mode: u32,
    pub span: Span,
}

pub struct Key {
    pub kind: String,
    pub generator: String,
    pub profile: Option<String>,
    pub bits: u32,
    pub public: String,
    pub format: String,
    pub private: String,
    pub span: Span,
}

pub struct VerifyException {
    pub class: String,
    pub unit: String,
    pub span: Span,
}

pub struct Refusal {
    pub rule: String,
    pub because: String,
    pub span: Span,
}

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum Position {
    #[default]
    Before,
    After,
    Tail,
}

pub struct Component {
    pub path: String,
    pub dir: String,
    pub src: Source,
    pub description: String,
    pub targets: Vec<String>,
    pub provides: Vec<Decl>,
    pub exclusive: Vec<String>,
    pub requires: Vec<Decl>,
    pub base: Option<BaseFacts>,
    pub imports: Vec<Import>,
    pub removes: Vec<Decl>,
    pub after: Vec<Decl>,
    pub policies: Vec<&'static str>,
    pub files: Vec<Located>,
    pub overrides: Vec<Decl>,
    pub verify_exceptions: Vec<VerifyException>,
    pub refuses: Vec<Refusal>,
    pub flavour: Option<String>,
    pub collects: Vec<Collect>,
    pub contributes: Vec<Contribution>,
    pub modes: Vec<FileMode>,
    pub keys: Vec<Key>,
    pub refused_keys: Vec<String>,
    pub secrets: Vec<Decl>,
    pub args: Vec<Decl>,
    pub options: Vec<Opt>,
    pub variants: Vec<Variant>,
    pub assets: Vec<Asset>,
    pub packages: Vec<PackageGroup>,
    pub groups: Vec<PackageGroup>,
    pub coprs: Vec<Copr>,
    pub helpers: Vec<Decl>,
    pub satisfies: Vec<Coverage>,
    pub resolved: Vec<(String, String)>,
    pub fragment: Option<String>,
    pub fragment_position: Position,
    pub standard_layer: bool,
    pub content: Option<String>,
    pub imported: Option<Record>,
    pub repo: bool,
    pub network: Option<Span>,
    pub access: Access,
}

impl Component {
    pub fn withdraws(&self, name: &str) -> bool {
        self.removes
            .iter()
            .any(|decl| crate::resolve::meets(name, &decl.name))
    }

    pub fn answers(&self, required: &str) -> bool {
        crate::resolve::answers(
            &self.path,
            self.provides.iter().map(|decl| decl.name.as_str()),
            required,
        )
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Access {
    pub packages: bool,
    pub scripts: bool,
}

impl Access {
    pub const OPEN: Self = Self {
        packages: true,
        scripts: true,
    };
}
