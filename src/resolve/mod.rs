pub mod collect;
pub mod graph;
pub mod imports;
pub mod name;
pub mod options;
pub mod order;
pub mod overlay;
pub mod workflow;

use collect::Collection;

pub fn targets(declared: &[String], family: &str) -> bool {
    family.is_empty() || declared.is_empty() || declared.iter().any(|has| has == family)
}

pub fn meets(declared: &str, required: &str) -> bool {
    declared
        .strip_prefix(required)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

pub fn answers<'a>(
    path: &str,
    mut provides: impl Iterator<Item = &'a str>,
    required: &str,
) -> bool {
    path == required || provides.any(|has| meets(has, required))
}

pub fn conflict(
    (one_flavour, one_exclusive): (Option<&str>, bool),
    (other_flavour, other_exclusive): (Option<&str>, bool),
) -> bool {
    (one_exclusive || other_exclusive) && meet(one_flavour, other_flavour)
}

pub fn meet(one: Option<&str>, other: Option<&str>) -> bool {
    one.is_none() || other.is_none() || one == other
}

pub struct Resolved {
    pub shipped: overlay::Index,
    pub collected: Collection,
    pub hashes: std::collections::BTreeMap<String, String>,
    pub scap: Option<crate::scap::pinned::Measured>,
    pub clones: Vec<crate::provenance::build::Cloned>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_ungated_entry_meets_every_target() {
        assert!(meet(None, Some("dev")));
        assert!(meet(Some("dev"), None));
        assert!(meet(Some("dev"), Some("dev")));
        assert!(!meet(Some("dev"), Some("prod")));
    }
}
