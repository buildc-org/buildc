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

const FETCHES: [&str; 8] = [
    "curl ",
    "wget ",
    "git clone",
    "pip install",
    "pip3 install",
    "npm install",
    "cargo install",
    "go install",
];

const SCRIPTS: [&str; 2] = ["module.sh", "finalize.sh"];

pub fn check_fetch(
    module: &crate::model::module::Module,
    dir: &std::path::Path,
    issues: &mut crate::diag::Issues,
) {
    if !module.assets.is_empty() {
        return;
    }
    for script in SCRIPTS {
        let Ok(text) = std::fs::read_to_string(dir.join(script)) else {
            continue;
        };
        let Some(verb) = FETCHES.into_iter().find(|verb| {
            text.lines()
                .map(str::trim_start)
                .any(|line| !line.starts_with('#') && line.contains(verb))
        }) else {
            continue;
        };
        issues.push(
            crate::diag::Issue::new(
                format!(
                    "`{}` fetches in {script} with nothing declaring what",
                    module.path
                ),
                &module.src,
            )
            .at(
                Span::default(),
                format!("`{}` reaches the network", verb.trim()),
            )
            .help(
                "declare an `asset` with the url, the version and the sha256, and let the layer \
                 read it out of ASSET_*; an undeclared fetch is the one build input no record \
                 can describe after the fact",
            ),
        );
        return;
    }
}

pub fn enforce_build(
    enforce: bool,
    resolved_base: Option<&str>,
    source_commit: Option<&str>,
) -> Result<(), String> {
    if !enforce {
        return Ok(());
    }
    if resolved_base.is_none() {
        return Err(
            "the base did not resolve to a manifest digest, so the build record would \
                    not name what it built on; `audit { enforce #true }` makes that an error"
                .into(),
        );
    }
    if source_commit.is_none() {
        return Err(
            "the repository is at no commit, so the build record would not bind this \
                    image to a tree anyone can read; `audit { enforce #true }` makes that an \
                    error"
                .into(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enforcement_is_a_lever_over_a_record_that_always_exists() {
        assert!(enforce_build(false, None, None).is_ok());
        assert!(enforce_build(true, Some("repo@sha256:abc"), Some("deadbeef")).is_ok());

        let no_base = enforce_build(true, None, Some("deadbeef")).unwrap_err();
        assert!(no_base.contains("did not resolve"), "{no_base}");
        let no_commit = enforce_build(true, Some("repo@sha256:abc"), None).unwrap_err();
        assert!(no_commit.contains("no commit"), "{no_commit}");
    }
}
