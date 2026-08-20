use crate::diag::{Issue, Source, Span};
use crate::model::module::Module;
use crate::model::options::Value;
use crate::model::remote::{Collection, REMOTE_DIR};
use crate::provenance::Evidence;

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
    pub base: Option<Base>,
    pub flavours: Vec<Flavour>,
    pub entries: Vec<Entry>,
    pub suppressed: Vec<Entry>,
    pub span: Span,
}

pub struct Base {
    pub image: String,
    pub family: String,
    pub provides: Vec<Decl>,
    pub provides_files: Vec<Decl>,
    pub signed: bool,
    pub span: Span,
}

pub struct Decl {
    pub name: String,
    pub span: Span,
}

pub struct Flavour {
    pub name: String,
    pub default: bool,
    pub pr_build: bool,
    pub span: Span,
}

pub struct WorkflowToggle {
    pub name: String,
    pub enabled: bool,
    pub span: Span,
}

pub struct Entry {
    pub path: String,
    pub flavour: Option<String>,
    pub variant: Option<String>,
    pub options: Vec<(String, Vec<Value>, Span)>,
    pub remote: Option<Evidence>,
    pub span: Span,
    pub module: Option<Module>,
}

impl Entry {
    pub fn dir(&self) -> String {
        match self.remote {
            Some(_) => format!("{REMOTE_DIR}/{}", self.path),
            None => self.path.clone(),
        }
    }

    pub fn qualified(&self, publishes_as: &str) -> Option<String> {
        match (&self.remote, self.path.contains('/')) {
            (Some(_), _) => None,
            (None, true) => Some(self.path.clone()),
            (None, false) => Some(format!("{publishes_as}/{}", self.path)),
        }
    }
}

pub struct Seed {
    pub image: String,
    pub collection: String,
}

pub struct List {
    pub name: String,
    pub id: String,
    pub images: Vec<Image>,
    pub workflows: Vec<WorkflowToggle>,
    pub sources: Vec<Collection>,
    pub default_image_id: Option<String>,
    pub pr_image_id: Option<String>,
    pub seed: Option<Seed>,
    pub manifest_label: bool,
    pub audit_enforce: bool,
    pub schema_version: Option<u32>,
    pub(crate) schema_version_seen: bool,
    pub repo_src: Source,
    pub files: Vec<String>,
}

impl Image {
    pub fn modules(&self) -> impl Iterator<Item = &Module> {
        self.entries.iter().filter_map(|e| e.module.as_ref())
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
