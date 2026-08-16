use crate::diag::Span;
use crate::provenance::Evidence;

pub struct Asset {
    pub name: String,
    pub pin: Evidence,
    pub span: Span,
}

impl Asset {
    pub fn env_prefix(&self) -> String {
        format!("ASSET_{}", self.name.to_uppercase().replace('-', "_"))
    }

    pub fn env(&self) -> Vec<(String, String)> {
        let prefix = self.env_prefix();
        let mut out = Vec::new();
        if let Some(version) = &self.pin.version {
            out.push((format!("{prefix}_VERSION"), version.clone()));
        }
        if let Some(url) = self.pin.url_resolved() {
            out.push((format!("{prefix}_URL"), url));
        }
        if let Some(sha256) = &self.pin.sha256 {
            out.push((format!("{prefix}_SHA256"), sha256.clone()));
        }
        out
    }
}
