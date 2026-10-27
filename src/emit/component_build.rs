use crate::layout;
use crate::model::component::{Component, Copr};
use crate::model::image::{Entry, Image};
use crate::model::options::{env_name, OptType};
use crate::resolve::collect::Collection;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

const HEADER: &str = "\
#!/usr/bin/env bash
# GENERATED FILE, do not edit.
set -euxo pipefail
";

pub fn dir(image: &Image) -> PathBuf {
    layout::generated_image(&image.id).join(layout::COMPONENTS)
}

pub fn path(image: &Image, entry: &Entry) -> PathBuf {
    dir(image).join(file_name(entry))
}

pub fn packages_path(image: &Image, entry: &Entry) -> PathBuf {
    layout::generated_image(&image.id)
        .join("packages")
        .join(file_name(entry))
}

fn file_name(entry: &Entry) -> String {
    match &entry.flavour {
        Some(flavour) => format!("{}@{flavour}.sh", entry.path),
        None => format!("{}.sh", entry.path),
    }
}

pub fn installs_packages(image: &Image, entry: &Entry, component: &Component, root: &Path) -> bool {
    let base_family = image.base.as_ref().map_or("", |b| b.family.as_str());
    !split_packages(entry, component, base_family, image, root).is_empty()
}

fn split_packages(
    entry: &Entry,
    component: &Component,
    base_family: &str,
    image: &Image,
    root: &Path,
) -> String {
    let mut out = package_step(entry, component, base_family, root);
    let on_disk = layout::component(root, &entry.path);
    if image.answers(layout::SELINUX.capability) && !layout::SELINUX.files(&on_disk).is_empty() {
        out.push_str("\nsource /ctx/lib/selinux-helpers.sh\nensure_checkpolicy\n");
    }
    out
}

// A RUN layer inherits no shell from the previous one, so the script layer sources the family helper and repeats the COPR exports its own scripts read.
fn carried(component: &Component, base_family: &str) -> String {
    let mut out =
        String::from("\nif [ -f /ctx/lib/family.sh ]; then\n    source /ctx/lib/family.sh\nfi\n");
    for copr in coprs(component, base_family) {
        let _ = writeln!(
            out,
            "export {}={}",
            copr.env_name(),
            shell(&copr.selector())
        );
    }
    out
}

pub fn splits(image: &Image, entry: &Entry, component: &Component, root: &Path) -> bool {
    component.access.packages != component.access.scripts
        && installs_packages(image, entry, component, root)
}

pub fn scripts(image: &Image, collection: &Collection, root: &Path) -> Vec<(PathBuf, String)> {
    let base_family = image.base.as_ref().map_or("", |b| b.family.as_str());
    let mut out = Vec::new();
    for entry in &image.entries {
        let Some(component) = entry.component.as_ref().filter(|m| m.standard_layer) else {
            continue;
        };
        let prelude = prelude(entry, component, &image.boot);
        let rest = rest(entry, component, collection, base_family, image, root);
        match splits(image, entry, component, root) {
            true => {
                let packages = split_packages(entry, component, base_family, image, root);
                let carried = carried(component, base_family);
                out.push((packages_path(image, entry), format!("{prelude}{packages}")));
                out.push((path(image, entry), format!("{prelude}{carried}{rest}")));
            }
            false => {
                let packages = package_step(entry, component, base_family, root);
                out.push((path(image, entry), format!("{prelude}{packages}{rest}")));
            }
        }
    }
    out
}

fn prelude(entry: &Entry, component: &Component, boot: &str) -> String {
    let dir = format!("/ctx/components/{}", entry.path);
    let mut out = String::from(HEADER);
    let _ = write!(out, "\nCOMPONENT_DIR={dir}\nexport COMPONENT_DIR\n");
    if !boot.is_empty() {
        let _ = writeln!(out, "BOOT_CHAIN={}", shell(boot));
        out.push_str("export BOOT_CHAIN\n");
    }

    if let Some(flavour) = &entry.flavour {
        let _ = write!(
            out,
            "\nif [ \"${{FLAVOUR:-}}\" != \"{flavour}\" ]; then\n\
             \x20   echo \"skipping {}: not built for '${{FLAVOUR:-the ungated build}}'\"\n\
             \x20   exit 0\n\
             fi\n",
            entry.path
        );
    }

    let lists: Vec<String> = component
        .options
        .iter()
        .filter(|o| o.ty == OptType::List)
        .map(|o| env_name(&o.name))
        .collect();
    if !component.resolved.is_empty() || !component.assets.is_empty() {
        out.push('\n');
    }
    for (name, value) in &component.resolved {
        if lists.contains(name) {
            let _ = writeln!(out, "{name}=({value})");
        } else {
            let _ = writeln!(out, "export {name}=\"{value}\"");
        }
    }
    for asset in &component.assets {
        for (name, value) in asset.env() {
            let _ = writeln!(out, "export {name}=\"{value}\"");
        }
    }
    out
}

fn family_helpers(component: &Component, base_family: &str, scripts: bool) -> &'static str {
    match scripts || !coprs(component, base_family).is_empty() || installs(component, base_family) {
        true => "\nsource /ctx/lib/family.sh\n",
        false => "",
    }
}

fn coprs<'a>(component: &'a Component, base_family: &str) -> Vec<&'a Copr> {
    match base_family {
        "fedora" => component.coprs.iter().collect(),
        _ => Vec::new(),
    }
}

fn installs(component: &Component, base_family: &str) -> bool {
    component
        .packages
        .iter()
        .chain(&component.groups)
        .any(|group| group.family == base_family)
}

fn package_step(entry: &Entry, component: &Component, base_family: &str, root: &Path) -> String {
    let dir = format!("/ctx/components/{}", entry.path);
    let mut out = String::new();

    let on_disk = layout::component(root, &entry.path);
    if let Some(at) = layout::shipped(&on_disk, base_family, layout::REPO_SCRIPT) {
        match repo_id(&on_disk.join(&at)).filter(|_| crate::parse::component::rpm(base_family)) {
            Some(id) => {
                let _ = write!(
                    out,
                    "\nif [ -f /etc/yum.repos.d/{id}.repo ]; then\n\
                     \x20   echo \"repo {id} is already configured\"\n\
                     else\n\
                     \x20   source {dir}/{at}\n\
                     fi\n"
                );
            }
            None => {
                let _ = write!(out, "\nsource {dir}/{at}\n");
            }
        }
    }

    let scripts = layout::layered(&on_disk, base_family, layout::PACKAGES_SCRIPT);
    out.push_str(family_helpers(component, base_family, !scripts.is_empty()));
    for copr in coprs(component, base_family) {
        let _ = write!(
            out,
            "\nexport {}={}\nenable_copr {}\n",
            copr.env_name(),
            shell(&copr.selector()),
            shell(&copr.name())
        );
    }
    for (helper, declared) in [
        ("install_packages", &component.packages),
        ("install_groups", &component.groups),
    ] {
        for batch in declared.iter().filter(|g| g.family == base_family) {
            let names = batch
                .packages
                .iter()
                .map(|name| shell(&spelled(name, base_family)))
                .collect::<Vec<_>>()
                .join(" ");
            let repo = batch
                .enablerepo
                .as_ref()
                .map(|repo| format!("BUILDC_ENABLE_REPO={} ", shell(repo)))
                .unwrap_or_default();
            let _ = write!(out, "\n{repo}{helper} {names}\n");
        }
    }
    for script in scripts {
        let _ = write!(out, "\nsource {dir}/{script}\n");
    }
    out
}

fn rest(
    entry: &Entry,
    component: &Component,
    collection: &Collection,
    base_family: &str,
    image: &Image,
    root: &Path,
) -> String {
    let dir = format!("/ctx/components/{}", entry.path);
    let on_disk = layout::component(root, &entry.path);
    let mut out = String::new();

    let mut roots = vec![(on_disk.clone(), dir.clone())];
    for gated in layout::family_dirs(&on_disk, base_family) {
        roots.push((on_disk.join(gated), format!("{dir}/{gated}")));
    }

    for key in &component.keys {
        let from = shell(&format!("/ctx/keys{}", key.public));
        let to = shell(&key.public);
        let _ = writeln!(out, "\ninstall -D -m 0644 -- {from} {to}",);
    }

    for script in layout::layered(&on_disk, base_family, layout::SCRIPT) {
        let _ = write!(out, "\nsource {dir}/{script}\n");
    }

    let policy = match image.answers(layout::SELINUX.capability) {
        true => layout::SELINUX.files(&on_disk),
        false => Vec::new(),
    };
    if !policy.is_empty() {
        let _ = write!(out, "\nsource /ctx/lib/selinux-helpers.sh\n");
        for file in policy {
            let _ = write!(
                out,
                "cp {dir}/selinux/{file} /tmp/{file}\n\
                 install_selinux_module /tmp/{file}\n"
            );
        }
    }

    let profiles = match image.answers(layout::APPARMOR.capability) {
        true => layout::APPARMOR.files(&on_disk),
        false => Vec::new(),
    };
    if !profiles.is_empty() {
        let _ = write!(out, "\nsource /ctx/lib/apparmor-helpers.sh\n");
        for file in profiles {
            let _ = writeln!(out, "install_apparmor_profile {dir}/apparmor/{file}");
        }
    }

    let overlays: Vec<&String> = roots
        .iter()
        .filter(|(at, _)| at.join(layout::OVERLAY).is_dir())
        .map(|(_, ctx)| ctx)
        .collect();
    for ctx in &overlays {
        let _ = write!(out, "\ncp -rT {ctx}/files /\n");
    }
    if !overlays.is_empty() {
        for declared in &component.modes {
            let _ = writeln!(
                out,
                "chmod '{}' -- {}",
                format_args!("{:04o}", declared.mode),
                shell(&declared.path)
            );
        }
    }

    if let Some(collected) = collection.by_component.get(&entry.path) {
        for one in collected {
            let staged = Path::new(&one.staged);
            let _ = write!(
                out,
                "\nmkdir -p {}\ncat {dir}/{} > {}\n",
                staged.parent().unwrap_or(staged).display(),
                one.file,
                one.staged
            );
        }
    }

    out
}

fn spelled(name: &str, family: &str) -> String {
    match crate::parse::component::rpm(family) {
        true => name.replacen('=', "-", 1),
        false => name.to_string(),
    }
}

fn shell(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn repo_id(file: &Path) -> Option<String> {
    let text = std::fs::read_to_string(file).ok()?;
    repo_id_in(&text)
}

fn repo_id_in(text: &str) -> Option<String> {
    text.lines()
        .find_map(|line| line.trim().strip_prefix("REPO_ID=").map(str::trim))
        .map(|value| value.trim_matches('"').to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(feature = "fuzzing")]
pub fn fuzz_repo_id(text: &str) {
    let _ = repo_id_in(text);
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_version_is_spelled_per_family() {
        assert_eq!(super::spelled("firefox=130.0", "fedora"), "firefox-130.0");
        assert_eq!(super::spelled("firefox=130.0", "debian"), "firefox=130.0");
    }

    #[test]
    fn shell_paths_survive_a_single_quote() {
        assert_eq!(super::shell("a'b"), "'a'\"'\"'b'");
    }
}
