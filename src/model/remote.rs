use crate::diag::Span;
use crate::provenance::Evidence;

pub const REMOTE_DIR: &str = ".remote";

pub struct Collection {
    pub name: String,
    pub at: At,
    pub span: Span,
}

pub enum At {
    Dir(String),
    Archive(Evidence),
}

impl Collection {
    pub fn subtree(&self) -> Option<&str> {
        match &self.at {
            At::Dir(_) => None,
            At::Archive(pin) => pin.path.as_deref(),
        }
    }

    pub fn pin(&self) -> Option<&Evidence> {
        match &self.at {
            At::Dir(_) => None,
            At::Archive(pin) => Some(pin),
        }
    }

    pub fn unpinned(&self) -> bool {
        self.pin().is_some_and(Evidence::unpinned)
    }
}
