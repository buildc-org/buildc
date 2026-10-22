pub mod collect;
pub mod graph;
pub mod name;
pub mod options;
pub mod order;
pub mod overlay;
pub mod workflow;

use collect::Collection;

pub fn supports(declared: &[String], family: &str) -> bool {
    declared.is_empty() || declared.iter().any(|has| has == family)
}

pub fn conflict(one: (Option<&str>, bool), other: (Option<&str>, bool)) -> bool {
    (one.1 || other.1) && meet(one.0, other.0)
}

pub fn meet(one: Option<&str>, other: Option<&str>) -> bool {
    one.is_none() || other.is_none() || one == other
}

pub struct Resolved {
    pub shipped: overlay::Index,
    pub collected: Collection,
}
