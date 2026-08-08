use crate::diag::Span;

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

pub struct Asset {
    pub name: String,
    pub version: Option<String>,
    pub url: Option<String>,
    pub sha256: Option<String>,
    pub from: ShaFrom,
    pub span: Span,
}

impl Asset {
    pub fn env_prefix(&self) -> String {
        format!("ASSET_{}", self.name.to_uppercase().replace('-', "_"))
    }

    pub fn url_resolved(&self) -> Option<String> {
        let url = self.url.as_ref()?;
        Some(match &self.version {
            Some(version) => url.replace("{version}", version),
            None => url.clone(),
        })
    }

    pub fn env(&self) -> Vec<(String, String)> {
        let prefix = self.env_prefix();
        let mut out = Vec::new();
        if let Some(version) = &self.version {
            out.push((format!("{prefix}_VERSION"), version.clone()));
        }
        if let Some(url) = self.url_resolved() {
            out.push((format!("{prefix}_URL"), url));
        }
        if let Some(sha256) = &self.sha256 {
            out.push((format!("{prefix}_SHA256"), sha256.clone()));
        }
        out
    }
}
