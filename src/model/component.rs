use crate::diag::{Source, Span};
use crate::model::asset::Asset;
use crate::model::options::{Opt, Variant};
use crate::model::version::Version;
use crate::provenance::origins::Origin;

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

pub struct Capability {
    pub name: String,
    pub span: Span,
    pub version: Option<Version>,
    pub package: Option<String>,
}

pub struct Requirement {
    pub name: String,
    pub span: Span,
    pub min: Option<Version>,
    pub prefer: Option<String>,
}

pub struct Import {
    pub path: String,
    pub variant: Option<String>,
    pub library: Option<String>,
    pub pin: Option<String>,
    pub options: Vec<crate::model::options::Setting>,
    pub span: Span,
    /// The segment only holds further segments.
    /// It lists the component at its path only if one exists.
    pub organises: bool,
    /// The path was written from a library root.
    pub rooted: bool,
    pub root_file: bool,
    pub digest: Option<(String, Span)>,
    pub signed: Option<(bool, Span)>,
    pub key: Option<(String, Span)>,
    pub key_sha256: Option<(String, Span)>,
    pub identity: Option<(String, Span)>,
    pub issuer: Option<(String, Span)>,
    pub inline: Option<Vec<Span>>,
}

#[derive(Clone)]
pub struct TrustRoot {
    pub key: Option<String>,
    pub key_sha256: Option<String>,
    pub identity: Option<String>,
    pub issuer: Option<String>,
}

impl TrustRoot {
    pub fn of(
        key: Option<&(String, Span)>,
        key_sha256: Option<&(String, Span)>,
        identity: Option<&(String, Span)>,
        issuer: Option<&(String, Span)>,
    ) -> Option<Self> {
        let key = key.map(|(key, _)| key.clone());
        let key_sha256 = key_sha256.map(|(sha256, _)| sha256.clone());
        let identity = identity.map(|(identity, _)| identity.clone());
        let issuer = issuer
            .map(|(issuer, _)| issuer.clone())
            .or_else(|| identity.as_ref().map(|_| GITHUB_OIDC_ISSUER.to_string()));
        (key.is_some() || identity.is_some()).then_some(Self {
            key,
            key_sha256,
            identity,
            issuer,
        })
    }
}

/// A keyless verifier uses this issuer when the base names none.
pub const GITHUB_OIDC_ISSUER: &str = "https://token.actions.githubusercontent.com";

pub struct BaseFacts {
    pub image: String,
    pub template: String,
    pub family: String,
    pub bootloaders: Vec<String>,
    pub scap_content: String,
    pub scap_pin: Option<crate::provenance::Evidence>,
    pub digest: Option<String>,
    pub signed: bool,
    pub verify: Option<TrustRoot>,
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

#[derive(Clone)]
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
    pub home: std::path::PathBuf,
    pub library: Option<String>,
    pub src: Source,
    pub description: String,
    pub targets: Vec<String>,
    pub hidden: bool,
    pub provides: Vec<Capability>,
    pub exclusive: Vec<String>,
    pub requires: Vec<Requirement>,
    pub base: Option<BaseFacts>,
    pub imports: Vec<Import>,
    pub removes: Vec<Decl>,
    pub after: Vec<Decl>,
    pub policies: Vec<&'static str>,
    pub files: Vec<Located>,
    pub overrides: Vec<Decl>,
    pub verify_exceptions: Vec<VerifyException>,
    pub refuses: Vec<Refusal>,
    pub collects: Vec<Collect>,
    pub contributes: Vec<Contribution>,
    pub modes: Vec<FileMode>,
    pub keys: Vec<Key>,
    pub refused_keys: Vec<String>,
    pub secrets: Vec<Decl>,
    pub args: Vec<Decl>,
    pub options: Vec<Opt>,
    pub variants: Vec<Variant>,
    pub default_variant: Option<String>,
    pub variant: Option<String>,
    pub variant_default: bool,
    pub assets: Vec<Asset>,
    pub packages: Vec<PackageGroup>,
    pub groups: Vec<PackageGroup>,
    pub coprs: Vec<Copr>,
    pub helpers: Vec<Decl>,
    pub satisfies: Vec<Coverage>,
    pub resolved: Vec<(String, String)>,
    pub changes: Vec<crate::model::options::Change>,
    pub fragment: Option<String>,
    pub fragment_declared: bool,
    pub fragment_position: Position,
    pub standard_layer: bool,
    pub content: Option<String>,
    pub imported: Option<Origin>,
    pub repo: bool,
    pub network: Option<Span>,
    pub access: Access,
    pub fields: Vec<(String, Span)>,
    pub keywords: Vec<String>,
    pub logo_url: String,
    pub conforms: String,
    pub allows: Vec<Refusal>,
    /// The manifest sits outside the components folder of every library.
    pub outside: bool,
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

    pub fn builds_only(&self, name: &str) -> bool {
        self.files
            .iter()
            .any(|file| file.name == name && file.build_only)
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
