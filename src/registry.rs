use crate::model::image::List;
use std::path::Path;
use std::process::Command;

const GITHUB: [&str; 3] = [
    "git@github.com:",
    "ssh://git@github.com/",
    "https://github.com/",
];

pub fn namespace(root: &Path) -> Result<String, String> {
    if let Some(set) = std::env::var("IMAGE_REGISTRY").ok().filter(|s| !s.is_empty()) {
        return Ok(set.to_lowercase());
    }
    let url = origin(root)
        .ok_or("no IMAGE_REGISTRY set and no github origin remote to derive one from")?;
    let owner = owner(&url)
        .ok_or_else(|| format!("`{url}` is not a github remote to derive a namespace from"))?;
    Ok(format!("ghcr.io/{owner}").to_lowercase())
}

pub fn reference(
    list: &List,
    root: &Path,
    target: Option<&str>,
    tag: Option<&String>,
) -> Result<String, String> {
    let tag = match tag {
        Some(tag) => tag.clone(),
        None => std::env::var("DEFAULT_TAG")
            .ok()
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| "latest".to_string()),
    };
    let target = match target {
        Some(name) => name.to_string(),
        None => list
            .ungated_target()
            .ok_or("no default image to take a target from")?
            .to_string(),
    };
    let published = list
        .targets()
        .iter()
        .find(|have| have.to_string() == target)
        .ok_or_else(|| format!("`{target}` is not a build target"))?
        .published();
    Ok(format!("{}/{published}:{tag}", namespace(root)?))
}

fn origin(root: &Path) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["config", "--get", "remote.origin.url"])
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .filter(|url| !url.is_empty())
}

fn owner(url: &str) -> Option<&str> {
    let rest = GITHUB.iter().find_map(|prefix| url.strip_prefix(prefix))?;
    let (owner, _) = rest.split_once('/')?;
    (!owner.is_empty()).then_some(owner)
}

#[cfg(test)]
mod tests {
    #[test]
    fn owner() {
        for url in [
            "git@github.com:Someone/example.git",
            "ssh://git@github.com/Someone/example",
            "https://github.com/Someone/example.git",
        ] {
            assert_eq!(super::owner(url), Some("Someone"));
        }
        for url in [
            "https://gitlab.com/someone/example.git",
            "https://github.com/someone",
            "https://github.com//example",
        ] {
            assert_eq!(super::owner(url), None);
        }
    }
}
