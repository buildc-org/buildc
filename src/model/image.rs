use crate::diag::{Issue, Source, Span};
use crate::model::component::{Access, Capability, Component, Coverage, Refusal, Requirement};
use crate::model::options::Value;
use crate::model::remote::Collection;

pub const SCHEMA_VERSION: u32 = 1;

pub const BUILDC_VERSION: &str = env!("CARGO_PKG_VERSION");

pub const NO_FLAVOUR: &str = "none";

pub fn is_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

pub fn is_local_path(path: &str) -> bool {
    let (parents, last) = match path.rsplit_once('/') {
        Some((parents, last)) => (Some(parents), last),
        None => (None, path),
    };
    let last = match last.split_once('@') {
        Some((name, tag)) => {
            let tagged = !tag.is_empty()
                && tag
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
            if !tagged {
                return false;
            }
            name
        }
        None => last,
    };
    is_name(last) && parents.is_none_or(|parents| parents.split('/').all(is_name))
}

#[derive(Clone)]
pub struct Target {
    pub image: String,
    pub flavour: String,
}

impl Target {
    pub fn published(&self) -> String {
        match self.flavour.as_str() {
            NO_FLAVOUR => self.image.clone(),
            flavour => format!("{}-{flavour}", self.image),
        }
    }
}

impl std::fmt::Display for Target {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.flavour.as_str() {
            NO_FLAVOUR => write!(f, "{}", self.image),
            flavour => write!(f, "{}/{flavour}", self.image),
        }
    }
}

pub struct Image {
    pub src: Source,
    pub id: String,
    pub name: String,
    pub pretty_name: String,
    pub url: String,
    pub issues_url: String,
    pub description: String,
    pub keywords: Vec<String>,
    pub logo_url: String,
    pub conforms: String,
    pub boot: String,
    pub boot_at: Option<Span>,
    pub base: Option<Base>,
    pub base_entry: Option<BaseEntry>,
    pub defined: Vec<String>,
    pub layout: Option<Layout>,
    pub allows: Vec<Refusal>,
    pub flavours: Vec<Flavour>,
    pub entries: Vec<Entry>,
    pub suppressed: Vec<Entry>,
    pub span: Span,
}

impl Base {
    pub fn shipped(&self) -> impl Iterator<Item = &Capability> {
        self.provides
            .iter()
            .filter(|decl| decl.name != crate::parse::component::BASE_IMAGE)
    }

    pub fn answers(&self, required: &str) -> bool {
        self.provides
            .iter()
            .any(|decl| crate::resolve::meets(&decl.name, required))
    }

    pub fn covers<'a>(&self, provides: impl IntoIterator<Item = &'a str>) -> bool {
        // An empty set is not covered, so a component that declares no capability is not treated as one the base supplies.
        let mut provides = provides.into_iter().peekable();
        provides.peek().is_some()
            && provides.all(|name| self.provides.iter().any(|decl| decl.name == name))
    }

    pub fn packages<'a>(&'a self, required: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        self.provides
            .iter()
            .filter(move |decl| crate::resolve::meets(&decl.name, required))
            .filter_map(|decl| decl.package.as_deref())
    }
}

pub struct BaseEntry {
    pub entry: Entry,
    pub digest: Option<String>,
    pub signed: bool,
}

pub struct Base {
    pub path: String,
    pub image: String,
    pub family: String,
    pub provides: Vec<Capability>,
    pub requires: Vec<Requirement>,
    pub imports: Vec<crate::model::component::Import>,
    pub src: Source,
    pub satisfies: Vec<Coverage>,
    pub signed: bool,
    pub bootloaders: Vec<String>,
    pub files: Vec<crate::model::component::Located>,
    pub tracker: Option<crate::model::options::VersionTracker>,
    pub imported: Option<crate::provenance::lock::Record>,
    pub scap_content: String,
    pub scap_pin: Option<crate::provenance::Evidence>,
    pub span: Span,
}

pub struct Layout {
    pub filesystem: String,
    pub bootloader: String,
    pub composefs: Option<bool>,
    pub generic: Option<bool>,
    pub admin_group: String,
    pub span: Span,
}

pub use crate::model::component::Decl;

pub struct Flavour {
    pub name: String,
    pub default: bool,
    pub pr_build: bool,
    pub conforms: String,
    pub span: Span,
}

pub struct Workflow {
    pub name: String,
    pub span: Span,
    pub schedule: Option<String>,
}

pub struct Entry {
    pub path: String,
    pub flavour: Option<String>,
    pub variant: Option<String>,
    pub options: Vec<(String, Vec<Value>, Span)>,
    pub span: Span,
    pub component: Option<Component>,
    pub imported_by: Option<String>,
    pub inline: Option<Vec<Span>>,
    pub organises: bool,
}

impl Entry {
    pub fn qualified(&self, publishes_as: &str) -> String {
        match self
            .component
            .as_ref()
            .and_then(|component| component.imported.as_ref())
        {
            Some(record) => format!("{}/{}", record.library, record.upstream),
            None => format!("{publishes_as}/{}", self.path),
        }
    }
}

pub struct Seed {
    pub image: String,
    pub collection: String,
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum Backend {
    #[default]
    Buildah,
    Buildx,
}

impl Backend {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "buildah" => Some(Self::Buildah),
            "buildx" => Some(Self::Buildx),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Buildah => "buildah",
            Self::Buildx => "buildx",
        }
    }
}

pub struct List {
    pub name: String,
    pub id: String,
    pub images: Vec<Image>,
    pub workflows: Vec<Workflow>,
    pub workflows_at: (u32, u32),
    pub publishes_scheduled: bool,
    pub scans_scheduled: bool,
    pub sources: Vec<Collection>,
    pub default_image_id: Option<String>,
    pub pr_image_id: Option<String>,
    pub seed: Option<Seed>,
    pub manifest_label: bool,
    pub backend: Backend,
    pub renovate: Option<Vec<(String, common::json::Json)>>,
    pub audit_enforce: bool,
    pub network: Network,
    pub schema_version: Option<u32>,
    pub repo_src: Source,
    pub files: Vec<String>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum NetRule {
    #[default]
    Allow,
    Strict,
    Deny,
}

impl NetRule {
    pub fn of(word: &str) -> Option<Self> {
        match word {
            "allow" => Some(Self::Allow),
            "strict" => Some(Self::Strict),
            "deny" => Some(Self::Deny),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Network {
    pub packages: NetRule,
    pub scripts: NetRule,
}

impl Network {
    pub fn access(&self, component: &Component) -> Access {
        Access {
            packages: self.packages != NetRule::Deny,
            scripts: match self.scripts {
                NetRule::Allow => true,
                NetRule::Strict => component.network.is_some(),
                NetRule::Deny => false,
            },
        }
    }
}

impl Image {
    pub fn components(&self) -> impl Iterator<Item = &Component> {
        self.entries.iter().filter_map(|e| e.component.as_ref())
    }

    pub fn base_answers(&self, required: &str) -> bool {
        self.base
            .as_ref()
            .is_some_and(|base| base.answers(required))
    }

    pub fn answers(&self, required: &str) -> bool {
        let kept = |name: &str| self.removing(name, |at| at.is_none()).is_none();
        self.base.as_ref().is_some_and(|base| {
            base.provides
                .iter()
                .any(|decl| crate::resolve::meets(&decl.name, required) && kept(&decl.name))
        }) || self.components().any(|component| {
            crate::resolve::answers(
                &component.path,
                component
                    .provides
                    .iter()
                    .map(|decl| decl.name.as_str())
                    .filter(|name| kept(name)),
                required,
            )
        })
    }

    pub fn is_bootc(&self) -> bool {
        self.answers("bootc")
    }

    pub fn remover(&self, name: &str, flavour: Option<&str>) -> Option<&Component> {
        self.removing(name, |at| crate::resolve::meet(at, flavour))
    }

    fn removing(&self, name: &str, reaches: impl Fn(Option<&str>) -> bool) -> Option<&Component> {
        self.components()
            .find(|component| component.withdraws(name) && reaches(component.flavour.as_deref()))
    }

    pub fn base_meets(&self, required: &str, flavour: Option<&str>) -> bool {
        self.base.as_ref().is_some_and(|base| {
            base.provides.iter().any(|decl| {
                crate::resolve::meets(&decl.name, required)
                    && self.remover(&decl.name, flavour).is_none()
            })
        })
    }

    pub fn component_meets(
        &self,
        component: &Component,
        required: &str,
        flavour: Option<&str>,
    ) -> bool {
        crate::resolve::answers(
            &component.path,
            component
                .provides
                .iter()
                .map(|decl| decl.name.as_str())
                .filter(|name| self.remover(name, flavour).is_none()),
            required,
        )
    }

    pub fn conforms_of(&self, flavour: &str) -> &str {
        match flavour {
            NO_FLAVOUR => &self.conforms,
            name => self
                .flavours
                .iter()
                .find(|f| f.name == name)
                .map_or("", |f| f.conforms.as_str()),
        }
    }

    pub fn declares_conforms(&self) -> bool {
        !self.conforms.is_empty() || self.flavours.iter().any(|f| !f.conforms.is_empty())
    }

    pub fn default_flavour(&self) -> Option<&str> {
        self.flavours
            .iter()
            .find(|f| f.default)
            .map(|f| f.name.as_str())
    }

    pub fn pr_flavour(&self) -> Option<&str> {
        self.flavours
            .iter()
            .find(|f| f.pr_build)
            .map(|f| f.name.as_str())
            .or_else(|| self.default_flavour())
    }
}

impl List {
    pub fn default_image(&self) -> Option<&Image> {
        match &self.default_image_id {
            Some(id) => self.images.iter().find(|i| &i.id == id),
            None => match self.images.len() {
                1 => self.images.first(),
                _ => None,
            },
        }
    }

    pub fn no_default(&self) -> Option<Issue> {
        let first = self.images.first().filter(|_| self.images.len() > 1)?;
        self.default_image_id.is_none().then(|| {
            Issue::new(
                format!(
                    "{} images are declared and none is the default",
                    self.images.len()
                ),
                &self.repo_src,
            )
            .help(format!(
                "the default image is what a command given no image answers about, and what a \
                 bare `buildc build` builds; a repository with one image falls back to that one, \
                 and a repository with more says which: `default-image \"{}\"` in {}",
                first.id,
                crate::layout::REPO_FILE
            ))
        })
    }

    pub fn pr_image(&self) -> Option<&Image> {
        match &self.pr_image_id {
            Some(id) => self.images.iter().find(|i| &i.id == id),
            None => self.default_image(),
        }
    }

    pub fn targets(&self) -> Vec<Target> {
        let mut out = Vec::new();
        for image in &self.images {
            out.push(Target {
                image: image.id.clone(),
                flavour: NO_FLAVOUR.to_string(),
            });
            out.extend(image.flavours.iter().map(|f| Target {
                image: image.id.clone(),
                flavour: f.name.clone(),
            }));
        }
        out
    }

    pub fn find_target(&self, name: &str) -> Result<Target, String> {
        let known = self.targets();
        known
            .iter()
            .find(|have| have.to_string() == name)
            .cloned()
            .ok_or_else(|| {
                format!(
                    "`{name}` is not a build target (have: {})",
                    known
                        .iter()
                        .map(Target::to_string)
                        .collect::<Vec<_>>()
                        .join(" ")
                )
            })
    }

    pub fn default_target(&self) -> Option<Target> {
        self.default_image().map(|image| Target {
            image: image.id.clone(),
            flavour: image.default_flavour().unwrap_or(NO_FLAVOUR).to_string(),
        })
    }

    pub fn ungated_target(&self) -> Option<Target> {
        self.default_image().map(|image| Target {
            image: image.id.clone(),
            flavour: NO_FLAVOUR.to_string(),
        })
    }

    pub fn cache_image(&self) -> Option<String> {
        self.default_image()
            .map(|image| format!("{}-cache", image.id))
    }

    pub fn pr_target(&self) -> Option<Target> {
        self.pr_image().map(|image| Target {
            image: image.id.clone(),
            flavour: image.pr_flavour().unwrap_or(NO_FLAVOUR).to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_local_path_admits_one_tag_on_its_last_part() {
        assert!(is_local_path("browser"));
        assert!(is_local_path("browser@1"));
        assert!(is_local_path("desktop/kde@2"));
        assert!(!is_local_path("browser@"));
        assert!(!is_local_path("browser@1@2"));
        assert!(!is_local_path("desktop@1/kde"));
        assert!(!is_local_path("Browser"));
        assert!(!is_local_path("/browser"));
    }
}
