use crate::diag::Span;
use crate::provenance::Evidence;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Components,
}

impl Kind {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "components" => Some(Self::Components),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Components => "components",
        }
    }
}

pub struct Collection {
    pub kind: Kind,
    pub name: String,
    pub at: At,
    pub span: Span,
}

pub enum At {
    Dir(String),
    Git(Evidence),
}

impl Collection {
    pub fn subtree(&self) -> Option<&str> {
        match &self.at {
            At::Dir(_) => None,
            At::Git(pin) => pin.path.as_deref(),
        }
    }

    pub fn pin(&self) -> Option<&Evidence> {
        match &self.at {
            At::Dir(_) => None,
            At::Git(pin) => Some(pin),
        }
    }

    pub fn unpinned(&self) -> bool {
        self.pin().is_some_and(|pin| pin.sha256.is_none())
    }
}
