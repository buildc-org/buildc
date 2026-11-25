use crate::diag::Span;
use crate::provenance::Evidence;

pub struct Collection {
    pub name: String,
    pub at: At,
    pub span: Span,
}

#[derive(Clone)]
pub struct DefaultLibrary {
    pub name: String,
    pub source: String,
    pub about: String,
}

pub enum At {
    Dir(String),
    Git(Evidence),
}

impl Collection {
    pub fn pin(&self) -> Option<&Evidence> {
        match &self.at {
            At::Dir(_) => None,
            At::Git(pin) => Some(pin),
        }
    }
}
