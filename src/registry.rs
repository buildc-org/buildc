use crate::model::image::List;

pub fn namespace(list: &List) -> Result<String, String> {
    environment()
        .or_else(|| list.registry.clone())
        .ok_or_else(|| NOWHERE.to_string())
}

pub fn reference(list: &List, target: Option<&str>, tag: Option<&str>) -> Result<String, String> {
    let target = match target {
        Some(name) => list.find_target(name)?,
        None => list
            .default_target()
            .ok_or("no default image to take a target from")?,
    };
    let reference = overridden(target.reference().as_deref(), &target.published())
        .ok_or_else(|| format!("`{target}` publishes nowhere: {NOWHERE}"))?;
    Ok(format!("{reference}:{}", self::tag(tag)))
}

const NOWHERE: &str = "repo.kdl names no `registry`, such as `registry \"ghcr.io/owner\"`, \
    and `IMAGE_REGISTRY` is not set";

// IMAGE_REGISTRY replaces the namespace of a full reference. A publish target follows the caller's registry.
pub fn overridden(declared: Option<&str>, published: &str) -> Option<String> {
    match environment() {
        Some(namespace) => Some(format!("{namespace}/{published}")),
        None => declared.map(str::to_string),
    }
}

pub fn namespace_of(reference: &str) -> &str {
    reference
        .rsplit_once('/')
        .map_or(reference, |(namespace, _)| namespace)
}

fn environment() -> Option<String> {
    std::env::var("IMAGE_REGISTRY")
        .ok()
        .filter(|set| !set.is_empty())
        .map(|set| set.to_lowercase())
}

pub fn tag(given: Option<&str>) -> String {
    match given {
        Some(tag) => tag.to_string(),
        None => std::env::var("DEFAULT_TAG")
            .ok()
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| "latest".to_string()),
    }
}

pub fn at(namespace: &str, published: &str, tag: &str) -> String {
    format!("{namespace}/{published}:{tag}")
}
