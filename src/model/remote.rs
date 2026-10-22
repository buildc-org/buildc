use crate::diag::Span;
use crate::provenance::Evidence;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Modules,
    BaseImages,
}

impl Kind {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "modules" => Some(Self::Modules),
            "base-images" => Some(Self::BaseImages),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Modules => "modules",
            Self::BaseImages => "base-images",
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
