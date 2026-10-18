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

pub fn exclusive(providers: &[bool]) -> bool {
    providers.len() > 1 && providers.contains(&true)
}

pub struct Resolved {
    pub shipped: overlay::Index,
    pub collected: Collection,
}
