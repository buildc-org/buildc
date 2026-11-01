use crate::build::Stopped;
use crate::planned::{Built, Planned};

use common::json::{text, Json};
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

pub fn run(root: &Path, reference: &str, target: Option<&str>) -> Result<Stopped, String> {
    let Some(plan) = Planned::checked(root)? else {
        return Ok(Stopped::Repository);
    };
    let target = plan.target(target)?;
    let out = Command::new("podman")
        .args(["history", "--format", "json", reference])
        .output()
        .map_err(|err| format!("podman: {err}"))?;
    if !out.status.success() {
        return Err(format!(
            "podman history {reference}: {}{}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    let history = Json::parse(&String::from_utf8_lossy(&out.stdout))
        .map_err(|err| format!("podman history {reference}: {err}"))?;
    let document = document(
        &history,
        reference,
        &target.name,
        &target.components(),
        &target.overlay_owners(),
    );
    print!("{}", document.render());
    Ok(Stopped::Built)
}

pub(crate) fn document(
    history: &Json,
    reference: &str,
    target: &str,
    entries: &[Built],
    owners: &BTreeMap<String, String>,
) -> Json {
    let layers: &[Json] = match history {
        Json::Array(items) => items,
        _ => &[],
    };
    Json::object([
        ("schema_version", Json::number(1)),
        ("image", Json::string(reference)),
        ("target", Json::string(target)),
        (
            "layers",
            Json::array(layers.iter().map(|layer| {
                let created = text(layer, "CreatedBy")
                    .or_else(|| text(layer, "created_by"))
                    .unwrap_or_default();
                let component = module_of(&created, entries);
                let paths = component
                    .map(|component| paths(component, owners, entries))
                    .unwrap_or_default();
                Json::object([
                    ("created_by", Json::string(&created)),
                    ("component", Json::optional(component)),
                    ("paths", Json::strings(paths)),
                    (
                        "size",
                        match common::json::number(layer, "size") {
                            Some(size) => Json::number(size),
                            None => Json::Null,
                        },
                    ),
                ])
            })),
        ),
    ])
}

fn module_of<'a>(created: &str, entries: &'a [Built]) -> Option<&'a str> {
    for entry in entries {
        let name = match &entry.flavour {
            Some(flavour) => format!("{}@{flavour}", entry.path),
            None => entry.path.clone(),
        };
        for kind in ["components", "packages"] {
            for suffix in [".sh", ".sh.tail.sh", ".sh.witness.sh"] {
                let needle = format!("/{kind}/{name}{suffix}");
                if created
                    .split_whitespace()
                    .any(|word| word.ends_with(&needle))
                {
                    return Some(&entry.path);
                }
            }
        }
    }
    None
}

fn paths(component: &str, owners: &BTreeMap<String, String>, entries: &[Built]) -> Vec<String> {
    let mut out: Vec<String> = owners
        .iter()
        .filter(|(_, owner)| owner.as_str() == component)
        .map(|(path, _)| path.clone())
        .collect();
    if let Some(entry) = entries.iter().find(|entry| entry.path == component) {
        out.extend(entry.witnessed.iter().cloned());
    }
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, flavour: Option<&str>) -> Built {
        Built {
            path: path.to_string(),
            flavour: flavour.map(str::to_string),
            witnessed: Vec::new(),
        }
    }

    #[test]
    fn a_component_is_responsible_for_the_overlays_it_wins_and_its_witnesses() {
        let mut plain = entry("core/plain", None);
        plain.witnessed.push("/usr/bin/plain".to_string());
        let entries = vec![plain, entry("core/gated", Some("dev"))];
        let mut owners = BTreeMap::new();
        owners.insert("/etc/shared".to_string(), "core/gated".to_string());
        assert_eq!(
            paths("core/plain", &owners, &entries),
            vec!["/usr/bin/plain"]
        );
        assert_eq!(paths("core/gated", &owners, &entries), vec!["/etc/shared"]);
    }

    #[test]
    fn a_base_layer_has_no_component_and_an_overlay_names_its_winner() {
        let one = entry("core/one", None);
        let two = entry("core/two", None);
        let entries = vec![one, two];
        let mut owners = BTreeMap::new();
        owners.insert("/etc/shared".to_string(), "core/two".to_string());
        owners.insert("/etc/one".to_string(), "core/one".to_string());
        let history = Json::parse(
            r#"[
                {"CreatedBy": "FROM base", "size": 10},
                {"CreatedBy": "bash /generated/x/components/core/one.sh", "size": 20}
            ]"#,
        )
        .unwrap();
        let doc = document(&history, "localhost/x:raw", "x", &entries, &owners);
        let text = doc.render();
        assert!(text.contains("\"component\": null"), "{text}");
        assert!(text.contains("\"component\": \"core/one\""), "{text}");
        assert!(text.contains("\"/etc/one\""), "{text}");
        assert!(!text.contains("\"/etc/shared\""), "{text}");
    }

    #[test]
    fn a_created_by_line_names_the_component_that_ran() {
        let plain = entry("core/hello", None);
        let gated = entry("core/browser", Some("dev"));
        let entries = vec![plain, gated];
        let line = "/bin/sh -c |1 COMPONENT_DIR=/ctx/components/core/hello bash /generated/x/components/core/hello.sh";
        assert_eq!(module_of(line, &entries), Some("core/hello"));
        let gated_line = "bash /generated/x/packages/core/browser@dev.sh";
        assert_eq!(module_of(gated_line, &entries), Some("core/browser"));
        let tail = "bash /generated/x/components/core/hello.sh.tail.sh";
        assert_eq!(module_of(tail, &entries), Some("core/hello"));
        assert_eq!(module_of("FROM base", &entries), None);
        let entries = vec![entry("core/run.sh", None), entry("core/hello", None)];
        assert_eq!(
            module_of("bash /generated/x/components/core/run.sh.sh", &entries),
            Some("core/run.sh")
        );
        let entries = vec![entry("core/run", None), entry("core/run.sh", None)];
        assert_eq!(
            module_of("bash /generated/x/components/core/run.sh.sh", &entries),
            Some("core/run.sh")
        );
    }
}
