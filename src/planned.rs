use crate::diag::Issues;
use crate::layout;
use crate::model::image::Backend;
use common::json::{self, Json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub(crate) struct Planned {
    json: Json,
    pub generated: usize,
}

pub(crate) struct Target<'a> {
    pub name: String,
    target: &'a Json,
    build: &'a Json,
}

pub(crate) struct Cloned {
    pub named: String,
    pub url: String,
    pub version: String,
    pub commit: String,
}

pub(crate) struct Built {
    pub path: String,
    pub flavour: Option<String>,
    pub witnessed: Vec<String>,
}

impl Planned {
    pub(crate) fn checked(root: &Path) -> Result<Option<Self>, String> {
        let mut issues = Issues::default();
        let generated = crate::verify_snapshot(root, &mut issues);
        if issues.report(&layout::generated(root).display().to_string()) {
            return Ok(None);
        }
        let path = layout::generated(root).join("plan.json");
        let text = std::fs::read_to_string(&path)
            .map_err(|err| format!("{}: {err}; run `buildc generate`", path.display()))?;
        let json = Json::parse(&text).map_err(|err| format!("{}: {err}", path.display()))?;
        Ok(Some(Self { json, generated }))
    }

    pub(crate) fn backend(&self) -> Result<Backend, String> {
        let name = json::text(&self.json, "backend").unwrap_or_default();
        Backend::parse(&name).ok_or_else(|| {
            "generated/plan.json names no backend; run `buildc generate`, which writes it"
                .to_string()
        })
    }

    pub(crate) fn audit_enforce(&self) -> bool {
        json::field(&self.json, "audit").is_some_and(|audit| flag(audit, "enforce"))
    }

    pub(crate) fn manifest_label(&self) -> bool {
        flag(&self.json, "manifest_label")
    }

    pub(crate) fn cache_image(&self) -> Option<String> {
        json::text(&self.json, "cache_image")
    }

    pub(crate) fn target(&self, named: Option<&str>) -> Result<Target<'_>, String> {
        let name = match named {
            Some(name) => name.to_string(),
            None => json::text(&self.json, "default_target")
                .ok_or("no default image to build; name a target with `--target`")?,
        };
        let targets = crate::emit::why::targets(&self.json);
        let target = targets
            .iter()
            .copied()
            .find(|target| json::text(target, "name").as_deref() == Some(name.as_str()))
            .ok_or_else(|| {
                let known: Vec<String> = targets
                    .iter()
                    .filter_map(|target| json::text(target, "name"))
                    .collect();
                format!("`{name}` is not a build target (have: {})", known.join(" "))
            })?;
        let build = json::field(target, "build").ok_or_else(|| {
            format!("`{name}` in generated/plan.json has no build facts; run `buildc generate`")
        })?;
        Ok(Target {
            name,
            target,
            build,
        })
    }
}

impl Target<'_> {
    pub(crate) fn published(&self) -> String {
        json::text(self.target, "published").unwrap_or_default()
    }

    pub(crate) fn siblings(&self) -> Vec<String> {
        json::items(self.target, "siblings")
            .iter()
            .filter_map(|sibling| json::text(sibling, "published"))
            .collect()
    }

    pub(crate) fn containerfile(&self) -> PathBuf {
        PathBuf::from(json::text(self.build, "containerfile").unwrap_or_default())
    }

    pub(crate) fn uki(&self) -> bool {
        flag(self.build, "uki")
    }

    pub(crate) fn family(&self) -> String {
        json::text(self.build, "family").unwrap_or_default()
    }

    pub(crate) fn base(&self) -> Option<String> {
        json::text(self.build, "base")
    }

    pub(crate) fn args(&self) -> Vec<String> {
        json::strings(self.build, "args")
    }

    pub(crate) fn unresolved(&self) -> Vec<String> {
        json::strings(self.build, "unresolved")
    }

    pub(crate) fn cloned(&self) -> Result<Vec<Cloned>, String> {
        let resolved = self
            .args()
            .into_iter()
            .find_map(|arg| arg.strip_prefix("ASSET_RESOLUTIONS=").map(str::to_string))
            .unwrap_or_default();
        let assets = json::items(self.target, "assets");
        resolved
            .split_whitespace()
            .map(|field| {
                let unpaired = || {
                    format!(
                        "generated/plan.json resolves `{field}`, which names no cloned asset of `{}`; run `buildc generate`",
                        self.name
                    )
                };
                let [component, name, version, commit] = field.split('|').collect::<Vec<_>>()[..]
                else {
                    return Err(unpaired());
                };
                let url = assets
                    .iter()
                    .filter(|asset| json::text(asset, "component").as_deref() == Some(component))
                    .filter(|asset| json::text(asset, "name").as_deref() == Some(name))
                    .find_map(|asset| {
                        json::field(asset, "pin").and_then(|pin| json::text(pin, "locator"))
                    })
                    .ok_or_else(unpaired)?;
                Ok(Cloned {
                    named: format!("`{component}` asset `{name}`"),
                    url,
                    version: version.to_string(),
                    commit: commit.to_string(),
                })
            })
            .collect()
    }

    pub(crate) fn validate(&self) -> Vec<String> {
        json::strings(self.build, "validate")
    }

    pub(crate) fn components(&self) -> Vec<Built> {
        json::items(self.target, "components")
            .iter()
            .map(|component| Built {
                path: json::text(component, "path").unwrap_or_default(),
                flavour: json::text(component, "flavour"),
                witnessed: json::strings(component, "witnessed"),
            })
            .collect()
    }

    pub(crate) fn overlay_owners(&self) -> BTreeMap<String, String> {
        match json::field(self.target, "overlay_files") {
            Some(Json::Object(fields)) => fields
                .iter()
                .filter_map(|(path, owner)| match owner {
                    Json::String(owner) => Some((path.clone(), owner.clone())),
                    Json::Null
                    | Json::Bool(_)
                    | Json::Number(_)
                    | Json::Array(_)
                    | Json::Object(_) => None,
                })
                .collect(),
            Some(
                Json::Null | Json::Bool(_) | Json::Number(_) | Json::String(_) | Json::Array(_),
            )
            | None => BTreeMap::new(),
        }
    }

    pub(crate) fn datastream(&self) -> Option<String> {
        json::text(self.target, "datastream")
    }
}

fn flag(value: &Json, key: &str) -> bool {
    matches!(json::field(value, key), Some(Json::Bool(true)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAN: &str = r#"{
        "default_target": "box/dev",
        "cache_image": "box",
        "backend": "buildx",
        "manifest_label": true,
        "audit": {"enforce": true},
        "images": [{"targets": [
            {"name": "box", "published": "box", "siblings": [{"name": "box/dev", "published": "box-dev"}],
             "datastream": "generated/box/scap/ssg-fedora-ds.xml",
             "components": [{"path": "core/one", "flavour": null, "witnessed": ["/usr/bin/one"]}],
             "overlay_files": {"/etc/one": "core/one"},
             "assets": [{"component": "core/one", "name": "src", "pin": {"locator": "https://example.com/src.git"}}],
             "build": {"containerfile": "generated/box/Containerfile", "uki": true, "family": "fedora",
                       "base": "quay.io/fedora/fedora-bootc:44", "args": ["TARGET=box", "ASSET_RESOLUTIONS=core/one|src|v1|abc"],
                       "unresolved": ["`core/one` asset `x` names no `version` to resolve"],
                       "validate": ["LUKS_INITRAMFS=true"]}},
            {"name": "box/dev", "published": "box-dev", "build": {}}
        ]}]
    }"#;

    fn plan() -> Planned {
        Planned {
            json: Json::parse(PLAN).expect("the test plan parses"),
            generated: 0,
        }
    }

    #[test]
    fn the_plan_answers_every_repository_fact_the_build_reads() {
        let plan = plan();
        assert_eq!(plan.backend(), Ok(Backend::Buildx));
        assert!(plan.audit_enforce());
        assert!(plan.manifest_label());
        assert_eq!(plan.cache_image().as_deref(), Some("box"));
        assert_eq!(plan.target(None).map(|t| t.name), Ok("box/dev".to_string()));
        let unknown = plan.target(Some("other")).err().unwrap_or_default();
        assert!(unknown.contains("have: box box/dev"), "{unknown}");
    }

    #[test]
    fn a_target_answers_every_fact_its_build_reads() {
        let plan = plan();
        let target = plan.target(Some("box")).expect("box is planned");
        assert_eq!(target.published(), "box");
        assert_eq!(target.siblings(), vec!["box-dev"]);
        assert_eq!(
            target.containerfile(),
            PathBuf::from("generated/box/Containerfile")
        );
        assert!(target.uki());
        assert_eq!(target.family(), "fedora");
        assert_eq!(
            target.base().as_deref(),
            Some("quay.io/fedora/fedora-bootc:44")
        );
        assert_eq!(
            target.args(),
            vec!["TARGET=box", "ASSET_RESOLUTIONS=core/one|src|v1|abc"]
        );
        let cloned = target
            .cloned()
            .expect("the resolution pairs with its asset");
        assert_eq!(cloned.len(), 1);
        assert_eq!(cloned[0].named, "`core/one` asset `src`");
        assert_eq!(cloned[0].url, "https://example.com/src.git");
        assert_eq!(cloned[0].version, "v1");
        assert_eq!(cloned[0].commit, "abc");
        assert_eq!(
            target.unresolved(),
            vec!["`core/one` asset `x` names no `version` to resolve"]
        );
        assert_eq!(target.validate(), vec!["LUKS_INITRAMFS=true"]);
        let components = target.components();
        assert_eq!(components.len(), 1);
        assert_eq!(components[0].path, "core/one");
        assert_eq!(components[0].flavour, None);
        assert_eq!(components[0].witnessed, vec!["/usr/bin/one"]);
        assert_eq!(
            target.overlay_owners().get("/etc/one").map(String::as_str),
            Some("core/one")
        );
        assert_eq!(
            target.datastream().as_deref(),
            Some("generated/box/scap/ssg-fedora-ds.xml")
        );
    }

    #[test]
    fn a_target_without_build_facts_is_refused() {
        let plan = Planned {
            json: Json::parse(r#"{"images": [{"targets": [{"name": "old"}]}]}"#)
                .expect("the test plan parses"),
            generated: 0,
        };
        let unpaired = Planned {
            json: Json::parse(
                r#"{"images": [{"targets": [{"name": "box", "build": {"args": ["ASSET_RESOLUTIONS=core/one|src|v1|abc"]}}]}]}"#,
            )
            .expect("the test plan parses"),
            generated: 0,
        };
        let target = unpaired.target(Some("box")).expect("box is planned");
        let refused = target.cloned().err().unwrap_or_default();
        assert!(refused.contains("names no cloned asset"), "{refused}");
        let refused = plan.target(Some("old")).err().unwrap_or_default();
        assert!(refused.contains("run `buildc generate`"), "{refused}");
        assert!(plan.backend().is_err());
        assert!(!plan.audit_enforce());
        assert!(!plan.manifest_label());
    }
}
