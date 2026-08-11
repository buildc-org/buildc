use crate::diag::Span;

pub const REMOTE_DIR: &str = ".remote";

pub struct Remote {
    pub url: String,
    pub git_ref: String,
    pub sha256: String,
    pub unpinned: Option<String>,
    pub path: Option<String>,
    pub span: Span,
}

impl Remote {
    pub fn url_resolved(&self) -> String {
        self.url.replace("{ref}", &self.git_ref)
    }
}

pub struct Collection {
    pub name: String,
    pub at: At,
    pub span: Span,
}

pub enum At {
    Dir(String),
    Archive(Remote),
}

impl Collection {
    pub fn subtree(&self) -> Option<&str> {
        match &self.at {
            At::Dir(_) => None,
            At::Archive(remote) => remote.path.as_deref(),
        }
    }

    pub fn unpinned(&self) -> bool {
        match &self.at {
            At::Dir(_) => false,
            At::Archive(remote) => remote.unpinned.is_some(),
        }
    }
}
