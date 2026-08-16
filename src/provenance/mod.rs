pub mod build;
pub mod evidence;
pub mod record;

use crate::diag::Span;
use crate::emit::json::Json;

#[derive(Clone, Copy, PartialEq)]
pub enum ShaFrom {
    Asset,
    Sidecar,
    Manual,
}

impl ShaFrom {
    pub(crate) fn parse(name: &str) -> Option<Self> {
        match name {
            "asset" => Some(Self::Asset),
            "sidecar" => Some(Self::Sidecar),
            "manual" => Some(Self::Manual),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Asset => "asset",
            Self::Sidecar => "sidecar",
            Self::Manual => "manual",
        }
    }
}

pub enum Tracker {
    Renovate {
        datasource: String,
        dep_name: String,
    },
    Manual(String),
    Unpinned(String),
    None,
}

impl Tracker {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Renovate { .. } => "renovate",
            Self::Manual(_) => "manual",
            Self::Unpinned(_) => "unpinned",
            Self::None => "none",
        }
    }

    pub fn reason(&self) -> Option<String> {
        match self {
            Self::Manual(why) | Self::Unpinned(why) => Some(why.clone()),
            _ => None,
        }
    }
}

pub struct Evidence {
    pub url: Option<String>,
    pub version: Option<String>,
    pub sha256: Option<String>,
    pub from: ShaFrom,
    pub path: Option<String>,
    pub tracker: Tracker,
    pub span: Span,
}

impl Evidence {
    pub fn new(span: Span) -> Self {
        Evidence {
            url: None,
            version: None,
            sha256: None,
            from: ShaFrom::Asset,
            path: None,
            tracker: Tracker::None,
            span,
        }
    }

    pub fn url_resolved(&self) -> Option<String> {
        let url = self.url.as_ref()?;
        Some(match &self.version {
            Some(version) => url.replace("{version}", version),
            None => url.clone(),
        })
    }

    pub fn cloned(&self) -> bool {
        self.url.as_deref().is_some_and(|url| url.ends_with(".git"))
    }

    pub fn unpinned(&self) -> bool {
        matches!(self.tracker, Tracker::Unpinned(_))
    }

    pub fn json(&self) -> Json {
        Json::object([
            ("locator", Json::optional(self.url.clone())),
            ("selector", Json::optional(self.version.clone())),
            ("verifier", Json::optional(self.sha256.clone())),
            ("tracker", Json::string(self.tracker.as_str())),
            ("reason", Json::optional(self.tracker.reason())),
            ("from", Json::string(self.from.as_str())),
            ("path", Json::optional(self.path.clone())),
            ("resolved", Json::optional(self.url_resolved())),
        ])
    }
}
