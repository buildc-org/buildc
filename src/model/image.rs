use crate::diag::{Issue, Source, Span};
use crate::model::component::{
    Access, Capability, Component, Coverage, Refusal, Requirement, TrustRoot,
};
use crate::model::options::Setting;
use crate::model::remote::Collection;

pub const SCHEMA_VERSION: u32 = 1;

pub const BUILDC_VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn is_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

pub fn is_segment(segment: &str) -> bool {
    segment.split('.').all(is_name_part) && segment.starts_with(|c: char| c.is_ascii_lowercase())
}

fn is_name_part(part: &str) -> bool {
    !part.is_empty()
        && part
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
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
    is_segment(last) && parents.is_none_or(|parents| parents.split('/').all(is_segment))
}

#[derive(Clone)]
pub struct Target {
    pub image: String,
    publish: Option<String>,
    registry: Option<String>,
}

impl Target {
    pub fn published(&self) -> String {
        match &self.publish {
            Some(publish) => publish.rsplit('/').next().unwrap_or(publish).to_string(),
            None => self.image.clone(),
        }
    }

    /// The reference holds no tag and does not read the `IMAGE_REGISTRY` override.
    pub fn reference(&self) -> Option<String> {
        match &self.publish {
            Some(publish) if publish.contains('/') => Some(publish.clone()),
            _ => Some(format!("{}/{}", self.registry.as_ref()?, self.published())),
        }
    }
}

impl std::fmt::Display for Target {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.image)
    }
}

pub struct Image {
    pub src: Source,
    pub file: String,
    pub id: String,
    pub component: String,
    pub repo_base_url: String,
    pub description: String,
    pub keywords: Vec<String>,
    pub logo_url: String,
    pub conforms: String,
    pub base: Option<Base>,
    pub base_entry: Option<BaseEntry>,
    pub layout: Option<Layout>,
    pub allows: Vec<Refusal>,
    pub entries: Vec<Entry>,
    pub suppressed: Vec<Entry>,
    pub outside: bool,
    pub span: Span,
    pub fields: Vec<FieldSource>,
}

pub const FIELDS: [&str; 4] = ["description", "keywords", "logo-url", "conforms"];

pub struct FieldSource {
    pub name: &'static str,
    pub component: String,
}

impl Image {
    pub fn field_source(&mut self, name: &'static str, component: &str) {
        if self.fields.iter().all(|held| held.name != name) {
            self.fields.push(FieldSource {
                name,
                component: component.to_string(),
            });
        }
    }

    pub fn source_of(&self, name: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|held| held.name == name)
            .map(|held| held.component.as_str())
    }
}

impl Base {
    pub fn image_requirements<'a>(
        &'a self,
        components: impl IntoIterator<Item = &'a Component> + 'a,
    ) -> impl Iterator<Item = (Option<&'a Component>, &'a Requirement)> + 'a {
        let own = self.requires.iter().map(|requirement| (None, requirement));
        let components = components.into_iter().flat_map(|component| {
            component
                .requires
                .iter()
                .map(move |requirement| (Some(component), requirement))
        });
        own.chain(components)
    }

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

    /// An empty capability set is not covered.
    /// A component that declares no capability then never counts as one that the base supplies.
    pub fn covers<'a>(&self, provides: impl IntoIterator<Item = &'a str>) -> bool {
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
    pub verify: Option<TrustRoot>,
}

pub struct Base {
    pub path: String,
    pub library: Option<String>,
    pub image: String,
    pub family: String,
    pub provides: Vec<Capability>,
    pub requires: Vec<Requirement>,
    pub imports: Vec<crate::model::component::Import>,
    pub src: Source,
    pub satisfies: Vec<Coverage>,
    pub signed: bool,
    pub verify: Option<TrustRoot>,
    pub bootloaders: Vec<String>,
    pub files: Vec<crate::model::component::Located>,
    pub tracker: Option<crate::model::options::VersionTracker>,
    pub scap_content: String,
    pub scap_pin: Option<crate::provenance::Evidence>,
    pub span: Span,
}

#[derive(Clone)]
pub struct Layout {
    pub filesystem: String,
    pub bootloader: String,
    pub composefs: Option<bool>,
    pub generic: Option<bool>,
    pub admin_group: String,
    pub span: Span,
    pub src: Source,
}

pub use crate::model::component::Decl;

pub struct Workflow {
    pub name: String,
    pub span: Span,
    pub builds: Selection,
    pub triggers: Vec<Trigger>,
}

pub struct Trigger {
    pub on: On,
    pub builds: Selection,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum On {
    Push,
    Pr,
    Schedule {
        weekly: Option<bool>,
        time: Option<(u32, u32)>,
    },
    Tag(String),
}

#[derive(Clone, Default)]
pub struct Selection {
    pub named: Vec<String>,
    pub removed: Vec<String>,
    pub span: Option<Span>,
}

impl Selection {
    pub fn written(&self) -> bool {
        self.span.is_some()
    }

    pub fn of(&self, from: &[String]) -> Vec<String> {
        let held = match self.named.is_empty() {
            true => from.to_vec(),
            false => self.named.clone(),
        };
        held.into_iter()
            .filter(|build| !self.removed.contains(build))
            .collect()
    }
}

#[derive(Clone, PartialEq)]
pub struct Layer {
    pub by: Option<String>,
    pub settings: Vec<Setting>,
}

pub struct Setter {
    /// `None` marks the component that the build names.
    pub by: Option<String>,
    pub variant: Option<String>,
    pub pin: Option<String>,
    pub settings: Vec<Setting>,
    pub src: Source,
    pub span: Span,
}

impl Setter {
    pub(crate) fn layer(&self) -> Layer {
        Layer {
            by: self.by.clone(),
            settings: self.settings.clone(),
        }
    }
}

#[derive(Default)]
pub struct Entry {
    pub path: String,
    pub variant: Option<String>,
    pub variant_at: Option<usize>,
    pub pin: Option<String>,
    pub digest: Option<(String, Span)>,
    pub signed: Option<(bool, Span)>,
    pub key: Option<(String, Span)>,
    pub key_sha256: Option<(String, Span)>,
    pub identity: Option<(String, Span)>,
    pub issuer: Option<(String, Span)>,
    pub options: Vec<Layer>,
    pub setters: Vec<Setter>,
    pub span: Span,
    pub component: Option<Component>,
    pub imported_by: Option<String>,
    pub inline: Option<Inline>,
    pub held: Option<Held>,
    pub home: Option<std::path::PathBuf>,
}

#[derive(Clone)]
pub struct Inline {
    pub src: Source,
    pub spans: Vec<Span>,
}

#[derive(Clone)]
pub struct Held {
    pub library: String,
    pub tree: std::path::PathBuf,
    pub components: std::path::PathBuf,
    pub remote: Option<Revision>,
}

#[derive(Clone)]
pub struct Revision {
    pub source: String,
    pub commit: String,
}

impl Entry {
    pub(crate) fn settings(&self) -> impl Iterator<Item = &Setting> {
        self.options.iter().flat_map(|layer| &layer.settings)
    }

    pub fn qualified(&self, publishes_as: &str) -> String {
        match &self.held {
            Some(held) => format!("{}/{}", held.library, self.path),
            None => format!("{publishes_as}/{}", self.path),
        }
    }

    pub fn library(&self) -> Option<&str> {
        self.held.as_ref().map(|held| held.library.as_str())
    }

    pub fn dir(&self, root: &std::path::Path) -> std::path::PathBuf {
        match (&self.home, &self.held) {
            (Some(home), _) => home.clone(),
            (None, Some(held)) => held.components.join(&self.path),
            (None, None) => crate::layout::component(root, &self.path),
        }
    }

    pub fn tree<'a>(&'a self, root: &'a std::path::Path) -> &'a std::path::Path {
        match (&self.home, &self.held) {
            (None, Some(held)) => &held.tree,
            _ => root,
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

#[derive(Clone)]
pub struct Build {
    pub name: String,
    pub component: String,
    pub publish: Option<String>,
    pub install: Option<Layout>,
    pub span: Span,
}

pub(crate) fn child(scope: Option<&str>, name: &str) -> String {
    match scope {
        Some(scope) => format!("{scope}/{name}"),
        None => name.to_string(),
    }
}

pub struct List {
    pub name: String,
    pub base_url: String,
    pub id: String,
    pub images: Vec<Image>,
    pub builds: Vec<Build>,
    pub registry: Option<String>,
    pub workflows: Vec<Workflow>,
    pub libraries: Vec<Collection>,
    pub root_library: Option<String>,
    pub default_library: Option<(String, Span)>,
    pub resolver: crate::resolve::library::Libraries,
    pub default_image_id: Option<String>,
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
    pub fn family(&self) -> &str {
        self.base.as_ref().map_or("", |base| base.family.as_str())
    }

    pub fn components(&self) -> impl Iterator<Item = &Component> {
        self.entries.iter().filter_map(|e| e.component.as_ref())
    }

    pub fn base_answers(&self, required: &str) -> bool {
        self.base
            .as_ref()
            .is_some_and(|base| base.answers(required))
    }

    pub fn answers(&self, required: &str) -> bool {
        let kept = |name: &str| self.remover(name).is_none();
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

    /// A fragment that reads `${SPLIT_BASE}` takes part in the two-pass seal of a UKI.
    pub fn seals_uki(&self) -> bool {
        self.components().any(|component| {
            component
                .fragment
                .as_deref()
                .is_some_and(|fragment| fragment.contains("${SPLIT_BASE}"))
        })
    }

    pub fn is_bootc(&self) -> bool {
        self.answers("bootc")
    }

    pub fn remover(&self, name: &str) -> Option<&Component> {
        self.components()
            .find(|component| component.withdraws(name))
    }

    pub fn base_meets(&self, required: &str) -> bool {
        self.base.as_ref().is_some_and(|base| {
            base.provides.iter().any(|decl| {
                crate::resolve::meets(&decl.name, required) && self.remover(&decl.name).is_none()
            })
        })
    }

    pub fn component_meets(&self, component: &Component, required: &str) -> bool {
        crate::resolve::answers(
            &component.path,
            component
                .provides
                .iter()
                .map(|decl| decl.name.as_str())
                .filter(|name| self.remover(name).is_none()),
            required,
        )
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
                    "{} builds are declared and none is the default",
                    self.images.len()
                ),
                &self.repo_src,
            )
            .help(format!(
                "the default build is what a command given no build answers about, and what a \
                 bare `buildc build` builds; a repository with one build falls back to that one, \
                 and a repository with more says which: `builds default=\"{}\"` in {}",
                first.id,
                crate::layout::REPO_FILE
            ))
        })
    }

    pub fn pr_image(&self) -> Option<&Image> {
        let built = crate::resolve::workflow::builds_on(
            self,
            crate::resolve::workflow::IMAGE_BUILD,
            &On::Pr,
        );
        match built.first() {
            Some(id) => self.images.iter().find(|i| &i.id == id),
            None => self.default_image(),
        }
    }

    /// A `./` build component has no path in a library.
    /// A child path of that build takes no prefix.
    pub(crate) fn scope(&self, build: &str) -> Option<&str> {
        let build = self.builds.iter().find(|held| held.name == build)?;
        match build.component.starts_with("./") {
            true => None,
            false => Some(&build.component),
        }
    }

    pub fn targets(&self) -> Vec<Target> {
        self.images.iter().map(|image| self.target(image)).collect()
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
        self.default_image().map(|image| self.target(image))
    }

    pub fn cache_image(&self) -> Option<String> {
        self.default_image()
            .map(|image| format!("{}-cache", image.id))
    }

    pub fn pr_target(&self) -> Option<Target> {
        self.pr_image().map(|image| self.target(image))
    }

    fn target(&self, image: &Image) -> Target {
        Target {
            image: image.id.clone(),
            publish: self
                .builds
                .iter()
                .find(|build| build.name == image.id)
                .and_then(|build| build.publish.clone()),
            registry: self.registry.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_build_publishes_under_its_registry_unless_publish_holds_a_reference() {
        let target = |publish: Option<&str>, registry: Option<&str>| Target {
            image: "work".into(),
            publish: publish.map(str::to_string),
            registry: registry.map(str::to_string),
        };
        let reference = |target: Target| target.reference();
        assert_eq!(
            reference(target(None, Some("ghcr.io/o"))).as_deref(),
            Some("ghcr.io/o/work")
        );
        assert_eq!(
            reference(target(Some("nightly"), Some("ghcr.io/o"))).as_deref(),
            Some("ghcr.io/o/nightly")
        );
        let full = target(Some("quay.io/team/box"), Some("ghcr.io/o"));
        assert_eq!(full.published(), "box");
        assert_eq!(reference(full).as_deref(), Some("quay.io/team/box"));
        assert_eq!(reference(target(Some("nightly"), None)), None);
    }

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
        assert!(is_local_path("images/workstation.image"));
        assert!(!is_local_path(".hidden"));
        assert!(!is_local_path("a..b"));
    }
}
