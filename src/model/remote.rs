use crate::diag::Span;

pub const REMOTE_DIR: &str = ".remote";

pub struct Remote {
    pub url: String,
    pub git_ref: String,
    pub sha256: String,
    pub path: Option<String>,
    pub span: Span,
}

impl Remote {
    pub fn url_resolved(&self) -> String {
        self.url.replace("{ref}", &self.git_ref)
    }
}
