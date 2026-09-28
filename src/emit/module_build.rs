use crate::layout;
use crate::model::image::{Entry, Image};
use crate::model::module::{Copr, Module};
use crate::model::options::{env_name, OptType};
use crate::resolve::collect::Collection;
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

const HEADER: &str = "\
#!/usr/bin/env bash
# GENERATED FILE, do not edit.
set -euxo pipefail
";

pub fn dir(image: &Image) -> PathBuf {
    layout::generated_image(&image.id).join(layout::MODULES)
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

pub fn installs_packages(image: &Image, entry: &Entry, module: &Module, root: &Path) -> bool {
    let base_family = image.base.as_ref().map_or("", |b| b.family.as_str());
    !split_packages(entry, module, base_family, &provided(image), root).is_empty()
}

fn split_packages(
    entry: &Entry,
    module: &Module,
    base_family: &str,
    has: &BTreeSet<&str>,
    root: &Path,
) -> String {
    let mut out = package_step(entry, module, base_family, root);
    let on_disk = layout::module(root, entry.dir());
    if has.contains(layout::SELINUX.capability) && !layout::SELINUX.files(&on_disk).is_empty() {
        out.push_str("\nsource /ctx/lib/selinux-helpers.sh\nensure_checkpolicy\n");
    }
    out
}

fn carried(module: &Module, base_family: &str) -> String {
    let mut out =
        String::from("\nif [ -f /ctx/lib/family.sh ]; then\n    source /ctx/lib/family.sh\nfi\n");
    for copr in coprs(module, base_family) {
        let _ = writeln!(
            out,
            "export {}={}",
            copr.env_name(),
            shell(&copr.selector())
        );
    }
    out
}

pub fn splits(image: &Image, entry: &Entry, module: &Module, root: &Path) -> bool {
    module.access.packages != module.access.scripts && installs_packages(image, entry, module, root)
}

fn provided(image: &Image) -> BTreeSet<&str> {
    image
        .base
        .iter()
        .flat_map(|base| base.provides.iter())
        .map(|decl| decl.name.as_str())
        .chain(
            image
                .modules()
                .flat_map(|m| m.provides.iter())
                .map(|decl| decl.name.as_str()),
        )
        .collect()
}

pub fn scripts(image: &Image, collection: &Collection, root: &Path) -> Vec<(PathBuf, String)> {
    let base_family = image.base.as_ref().map_or("", |b| b.family.as_str());
    let has = provided(image);
    let mut out = Vec::new();
    for entry in &image.entries {
        let Some(module) = entry.module.as_ref().filter(|m| m.standard_layer) else {
            continue;
        };
        let prelude = prelude(entry, module, &image.boot);
        let rest = rest(entry, module, collection, base_family, &has, root);
        match splits(image, entry, module, root) {
            true => {
                let packages = split_packages(entry, module, base_family, &has, root);
                let carried = carried(module, base_family);
                out.push((packages_path(image, entry), format!("{prelude}{packages}")));
                out.push((path(image, entry), format!("{prelude}{carried}{rest}")));
            }
            false => {
                let packages = package_step(entry, module, base_family, root);
                out.push((path(image, entry), format!("{prelude}{packages}{rest}")));
            }
        }
    }
    out
}

fn prelude(entry: &Entry, module: &Module, boot: &str) -> String {
    let dir = format!("/ctx/modules/{}", entry.dir());
    let mut out = String::from(HEADER);
    let _ = write!(out, "\nMODDIR={dir}\nexport MODDIR\n");
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

    let lists: Vec<String> = module
        .options
        .iter()
        .filter(|o| o.ty == OptType::List)
        .map(|o| env_name(&o.name))
        .collect();
    if !module.resolved.is_empty() || !module.assets.is_empty() {
        out.push('\n');
    }
    for (name, value) in &module.resolved {
        if lists.contains(name) {
            let _ = writeln!(out, "{name}=({value})");
        } else {
            let _ = writeln!(out, "export {name}=\"{value}\"");
        }
    }
    for asset in &module.assets {
        for (name, value) in asset.env() {
            let _ = writeln!(out, "export {name}=\"{value}\"");
        }
    }
    out
}

fn family_helpers(module: &Module, base_family: &str) -> &'static str {
    match !coprs(module, base_family).is_empty() || installs(module, base_family) {
        true => "\nsource /ctx/lib/family.sh\n",
        false => "",
    }
}

fn coprs<'a>(module: &'a Module, base_family: &str) -> Vec<&'a Copr> {
    match base_family {
        "fedora" => module.coprs.iter().collect(),
        _ => Vec::new(),
    }
}

fn installs(module: &Module, base_family: &str) -> bool {
    module
        .packages
        .iter()
        .chain(&module.groups)
        .any(|group| group.family == base_family)
}

fn package_step(entry: &Entry, module: &Module, base_family: &str, root: &Path) -> String {
    let dir = format!("/ctx/modules/{}", entry.dir());
    let mut out = String::new();

    let on_disk = layout::module(root, entry.dir());
    if let Some(at) = layout::shipped(&on_disk, base_family, "repo") {
        match repo_id(&on_disk.join(&at)).filter(|_| crate::parse::module::rpm(base_family)) {
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

    out.push_str(family_helpers(module, base_family));
    for copr in coprs(module, base_family) {
        let _ = write!(
            out,
            "\nexport {}={}\nenable_copr {}\n",
            copr.env_name(),
            shell(&copr.selector()),
            shell(&copr.name())
        );
    }
    for (helper, declared) in [
        ("install_packages", &module.packages),
        ("install_groups", &module.groups),
    ] {
        for batch in declared.iter().filter(|g| g.family == base_family) {
            let names = batch
                .packages
                .iter()
                .map(|name| shell(name))
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
    out
}

fn rest(
    entry: &Entry,
    module: &Module,
    collection: &Collection,
    base_family: &str,
    has: &BTreeSet<&str>,
    root: &Path,
) -> String {
    let dir = format!("/ctx/modules/{}", entry.dir());
    let on_disk = layout::module(root, entry.dir());
    let mut out = String::new();

    let mut roots = vec![(on_disk.clone(), dir.clone())];
    for gated in layout::family_dirs(&on_disk, base_family) {
        roots.push((on_disk.join(gated), format!("{dir}/{gated}")));
    }

    for key in &module.keys {
        let from = shell(&format!("/ctx/keys{}", key.public));
        let to = shell(&key.public);
        let _ = writeln!(out, "\ninstall -D -m 0644 -- {from} {to}",);
    }

    for (at, ctx) in &roots {
        if at.join("module.sh").is_file() {
            let _ = write!(out, "\nsource {ctx}/module.sh\n");
        }
    }

    let policy = match has.contains(layout::SELINUX.capability) {
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

    let profiles = match has.contains(layout::APPARMOR.capability) {
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
        for declared in &module.modes {
            let _ = writeln!(
                out,
                "chmod '{}' -- {}",
                format_args!("{:04o}", declared.mode),
                shell(&declared.path)
            );
        }
    }

    if let Some(collected) = collection.by_module.get(&entry.path) {
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

fn shell(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn repo_id(file: &Path) -> Option<String> {
    let text = std::fs::read_to_string(file).ok()?;
    text.lines()
        .find_map(|line| line.trim().strip_prefix("REPO_ID=").map(str::trim))
        .map(|value| value.trim_matches('"').to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    #[test]
    fn shell_paths_survive_a_single_quote() {
        assert_eq!(super::shell("a'b"), "'a'\"'\"'b'");
    }
}
