use crate::model::component::{Component, Requirement};
use crate::model::image::Image;
use std::collections::BTreeSet;

fn family(image: &Image) -> &str {
    image.base.as_ref().map_or("", |base| base.family.as_str())
}

// The callers pass a `BTreeSet` to keep the emitted lines unique and sorted. An unchanged repository writes the same Containerfile.
fn join(entries: BTreeSet<String>) -> String {
    entries.into_iter().collect::<Vec<String>>().join(" ")
}

fn base_provides(image: &Image) -> BTreeSet<String> {
    image
        .base
        .iter()
        .flat_map(|base| base.shipped())
        .filter_map(|decl| Some(format!("{}|{}", decl.name, decl.package.as_deref()?)))
        .collect()
}

fn component_provides(image: &Image) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for component in image.components() {
        for decl in &component.provides {
            let Some(package) = decl.package.as_deref() else {
                continue;
            };
            if component.builds_only(&decl.name) {
                continue;
            }
            out.insert(format!(
                "{}|{}|{}|{}",
                component.path,
                decl.name,
                package,
                component.flavour.as_deref().unwrap_or_default()
            ));
        }
    }
    out
}

fn floors(image: &Image) -> BTreeSet<String> {
    let Some(base) = &image.base else {
        return BTreeSet::new();
    };
    let mut out = BTreeSet::new();
    let mut add = |requirement: &Requirement, flavour: Option<&str>, owner: &str| {
        let Some(min) = &requirement.min else {
            return;
        };
        // Only a capability the base provides from a named package can be measured before the components. The early run leaves any other floor to the final-image check.
        if base.packages(&requirement.name).next().is_none() {
            return;
        }
        out.insert(format!(
            "{}|{}|{}|{}",
            requirement.name,
            min,
            flavour.unwrap_or_default(),
            owner
        ));
    };
    for requirement in &base.requires {
        add(requirement, None, &base.image);
    }
    for component in image.components() {
        for requirement in &component.requires {
            add(requirement, component.flavour.as_deref(), &component.path);
        }
    }
    out
}

pub fn early(image: &Image) -> String {
    let provides = join(base_provides(image));
    let floors = join(floors(image));
    if provides.is_empty() && floors.is_empty() {
        return String::new();
    }
    format!(
        "# ---- capability versions ----\n\
         RUN --mount=type=bind,from=ctx,source=/generated/lib,target=/ctx/lib \\\n    \
         FAMILY={family} FLAVOUR=\"${{FLAVOUR:-}}\" PROVIDES=\"{provides}\" FLOORS=\"{floors}\" \\\n    \
         bash /ctx/lib/measure-base.sh\n\n",
        family = family(image),
    )
}

pub fn record(image: &Image, reset: bool) -> String {
    let measured = join(component_provides(image));
    if measured.is_empty() {
        return String::new();
    }
    let reset = match reset {
        true => "RESET_VERSIONS=1 ",
        false => "",
    };
    format!(
        "# ---- capability versions ----\n\
         RUN --mount=type=bind,from=ctx,source=/generated/lib,target=/ctx/lib \\\n    \
         FAMILY={family} FLAVOUR=\"${{FLAVOUR:-}}\" MEASURED=\"{measured}\" {reset}\\\n    \
         bash /ctx/lib/measure-image.sh\n\n",
        family = family(image),
    )
}

pub fn versioned(components: &[&Component]) -> Vec<String> {
    let mut out = BTreeSet::new();
    for component in components {
        for decl in &component.provides {
            let Some(package) = decl.package.as_deref() else {
                continue;
            };
            if component.builds_only(&decl.name) {
                continue;
            }
            out.insert(format!("{}|{}|{}", component.path, decl.name, package));
        }
    }
    out.into_iter().collect()
}

pub fn provided<'a>(components: &'a [&'a Component]) -> BTreeSet<&'a str> {
    components
        .iter()
        .flat_map(|component| {
            component
                .provides
                .iter()
                .filter(|decl| !component.builds_only(&decl.name))
                .map(|decl| decl.name.as_str())
        })
        .collect()
}

pub fn required(
    image: &Image,
    components: &[&Component],
    provided: &BTreeSet<&str>,
) -> Vec<String> {
    let mut out = BTreeSet::new();
    // The early measurement settles a floor that only the base provides. This check covers the component providers.
    let mut add = |requirement: &Requirement, owner: &str| {
        let Some(min) = &requirement.min else {
            return;
        };
        if !provided
            .iter()
            .any(|name| crate::resolve::meets(name, &requirement.name))
        {
            return;
        }
        out.insert(format!("{}|{}|{}", requirement.name, min, owner));
    };
    if let Some(base) = &image.base {
        for requirement in &base.requires {
            add(requirement, &base.image);
        }
    }
    for component in components {
        for requirement in &component.requires {
            add(requirement, &component.path);
        }
    }
    out.into_iter().collect()
}

pub fn libraries(early: bool, record: bool) -> Vec<(&'static str, &'static str)> {
    let mut out = Vec::new();
    if early || record {
        out.push((
            "version-helpers.sh",
            include_str!("../../assets/lib/version-helpers.sh"),
        ));
    }
    if early {
        out.push((
            "measure-base.sh",
            include_str!("../../assets/lib/measure-base.sh"),
        ));
    }
    if record {
        out.push((
            "measure-image.sh",
            include_str!("../../assets/lib/measure-image.sh"),
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_version_helper_is_generated_for_either_measurement() {
        for (early, record) in [(true, false), (false, true), (true, true)] {
            let names: Vec<&str> = libraries(early, record)
                .into_iter()
                .map(|(name, _)| name)
                .collect();
            assert!(names.contains(&"version-helpers.sh"), "{early} {record}");
        }
        assert!(libraries(false, false).is_empty());
    }

    #[test]
    fn a_measured_child_capability_meets_a_parent_floor() {
        let helper =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/lib/version-helpers.sh");
        let dir = std::env::temp_dir().join(format!("buildc-versions-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        let file = dir.join("versions");
        std::fs::write(&file, "base|package-manager/dnf5|dnf5|5.4.7.0\n")
            .expect("the measured versions are written");
        let out = std::process::Command::new("bash")
            .arg("-c")
            .arg(format!(
                "source {}; measured_version {} package-manager",
                helper.display(),
                file.display()
            ))
            .output()
            .expect("bash runs");
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "5.4.7.0");
    }
}
