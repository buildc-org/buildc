use crate::layout;
use crate::model::component::{Component, Copr, PackageGroup, Position};
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
    format!("{}.sh", entry.path)
}

/// The selected variant's directory comes first so its file wins over the component's file of the same name.
fn content_dirs(entry: &Entry, root: &Path) -> Vec<PathBuf> {
    let home = entry.dir(root);
    let variant = entry
        .component
        .as_ref()
        .and_then(|component| component.variant.as_deref());
    let mut out = Vec::new();
    if let Some(at) = layout::variant_home(&home, variant) {
        out.push(at);
    }
    out.push(home);
    out
}

fn shipped_path(entry: &Entry, root: &Path, family: &str, file: &str) -> Option<(PathBuf, String)> {
    layout::shipped_across(&content_dirs(entry, root), family, file)
}

pub(crate) fn script_paths(entry: &Entry, root: &Path, family: &str, file: &str) -> Vec<String> {
    let dirs = content_dirs(entry, root);
    let mut out = Vec::new();
    if dirs.iter().any(|dir| dir.join(file).is_file()) {
        out.push(file.to_string());
    }
    for (name, taken_on) in layout::FAMILY_DIRS.iter() {
        if !taken_on.contains(&family) {
            continue;
        }
        let script = format!("{name}/{file}");
        if dirs.iter().any(|dir| dir.join(&script).is_file()) {
            out.push(script);
        }
    }
    out
}

fn files_of(entry: &Entry, root: &Path, files: impl Fn(&Path) -> Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for dir in content_dirs(entry, root) {
        for file in files(&dir) {
            if !out.contains(&file) {
                out.push(file);
            }
        }
    }
    out
}

pub fn installs_packages(image: &Image, entry: &Entry, component: &Component, root: &Path) -> bool {
    let base_family = image.family();
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
    let policies = files_of(entry, root, |dir| layout::SELINUX.files(dir));
    if image.answers(layout::SELINUX.capability) && !policies.is_empty() {
        out.push_str("\nsource /ctx/lib/selinux-helpers.sh\nensure_checkpolicy\n");
    }
    out
}

// A RUN layer starts a new shell. The script layer sources the family helper and repeats the COPR exports its own scripts read.
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
    let base_family = image.family();
    let mut out = Vec::new();
    for entry in &image.entries {
        let Some(component) = entry.component.as_ref() else {
            continue;
        };
        let prelude = prelude(entry, component);
        if witness_step(root, entry, base_family, component) {
            let body = witnesses(component);
            out.push((witness_path(image, entry), format!("{prelude}{body}")));
        }
        if !component.standard_layer {
            continue;
        }
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
                if !packages.is_empty() || !rest.is_empty() {
                    out.push((path(image, entry), format!("{prelude}{packages}{rest}")));
                }
            }
        }
        if let Some(tail) = tail(entry, component, collection, base_family, root) {
            out.push((tail_path(image, entry), format!("{prelude}{tail}")));
        }
    }
    out
}

pub fn tail_path(image: &Image, entry: &Entry) -> PathBuf {
    dir(image).join(format!("{}.tail.sh", file_name(entry)))
}

pub fn tail(
    entry: &Entry,
    component: &Component,
    collection: &Collection,
    base_family: &str,
    root: &Path,
) -> Option<String> {
    if overlay_prefixes(root, entry, base_family).is_empty() {
        return None;
    }
    let body = after_overlays(entry, component, collection, base_family, root);
    (!body.is_empty()).then_some(body)
}

pub fn witness_path(image: &Image, entry: &Entry) -> PathBuf {
    dir(image).join(format!("{}.witness.sh", file_name(entry)))
}

pub(super) fn witnesses(component: &Component) -> String {
    let mut out = String::new();
    for file in component.files.iter().filter(|file| !file.build_only) {
        let path = shell(&file.file);
        let _ = write!(
            out,
            "\n# ---- witness for {} ----\n\
             if [ ! -e {path} ]; then\n    \
             echo \"component {} provides {}, and its witness {} is missing\" >&2\n    \
             exit 1\n\
             fi\n",
            file.name, component.path, file.name, file.file
        );
    }
    out
}

pub fn witness_step(root: &Path, entry: &Entry, family: &str, component: &Component) -> bool {
    let after_fragment = match component.fragment_position {
        Position::After => true,
        Position::Before | Position::Tail => false,
    };
    !witnesses(component).is_empty()
        && !super::finalize::runs_hook(root, entry, family)
        && (!component.standard_layer || after_fragment)
}

fn witnesses_in_layer(root: &Path, entry: &Entry, family: &str, component: &Component) -> String {
    match witness_step(root, entry, family, component)
        || super::finalize::runs_hook(root, entry, family)
    {
        true => String::new(),
        false => witnesses(component),
    }
}

pub fn overlay_prefixes(root: &Path, entry: &Entry, family: &str) -> Vec<String> {
    let dirs = content_dirs(entry, root);
    let mut out = Vec::new();
    if dirs.iter().any(|dir| dir.join(layout::OVERLAY).is_dir()) {
        out.push(String::new());
    }
    for (name, taken_on) in layout::FAMILY_DIRS.iter() {
        if !taken_on.contains(&family) {
            continue;
        }
        if dirs
            .iter()
            .any(|dir| dir.join(name).join(layout::OVERLAY).is_dir())
        {
            out.push((*name).to_string());
        }
    }
    out
}

pub fn package_reads_dir(root: &Path, entry: &Entry, family: &str) -> bool {
    shipped_path(entry, root, family, layout::REPO_SCRIPT).is_some()
        || !script_paths(entry, root, family, layout::PACKAGES_SCRIPT).is_empty()
}

pub fn script_reads_dir(
    root: &Path,
    entry: &Entry,
    component: &Component,
    image: &Image,
    family: &str,
    collection: &Collection,
) -> bool {
    let packages_here =
        !splits(image, entry, component, root) && package_reads_dir(root, entry, family);
    let has_overlay = !overlay_prefixes(root, entry, family).is_empty();
    let collected_here = collection.by_component.contains_key(&entry.path) && !has_overlay;
    !script_paths(entry, root, family, layout::SCRIPT).is_empty()
        || (image.answers(layout::SELINUX.capability)
            && !files_of(entry, root, |dir| layout::SELINUX.files(dir)).is_empty())
        || (image.answers(layout::APPARMOR.capability)
            && !files_of(entry, root, |dir| layout::APPARMOR.files(dir)).is_empty())
        || collected_here
        || packages_here
}

pub fn tail_reads_dir(root: &Path, entry: &Entry, family: &str, collection: &Collection) -> bool {
    !overlay_prefixes(root, entry, family).is_empty()
        && collection.by_component.contains_key(&entry.path)
}

fn prelude(entry: &Entry, component: &Component) -> String {
    let dir = format!("/ctx/components/{}", entry.path);
    let mut out = String::from(HEADER);
    let _ = write!(out, "\nCOMPONENT_DIR={dir}\nexport COMPONENT_DIR\n");

    if !component.resolved.is_empty() || !component.assets.is_empty() {
        out.push('\n');
    }
    out.push_str(&options(component));
    for asset in &component.assets {
        for (name, value) in asset.env() {
            let _ = writeln!(out, "export {name}=\"{value}\"");
        }
    }
    out
}

pub(super) fn options(component: &Component) -> String {
    let lists: Vec<String> = component
        .options
        .iter()
        .filter(|o| o.ty == OptType::List)
        .map(|o| env_name(&o.name))
        .collect();
    let mut out = String::new();
    for (name, value) in &component.resolved {
        if lists.contains(name) {
            let _ = writeln!(out, "{name}=({value})");
        } else {
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

pub fn runs(
    entry: &Entry,
    component: &Component,
    collection: &Collection,
    base_family: &str,
    image: &Image,
    root: &Path,
) -> bool {
    !package_step(entry, component, base_family, root).is_empty()
        || !rest(entry, component, collection, base_family, image, root).is_empty()
}

fn package_step(entry: &Entry, component: &Component, base_family: &str, root: &Path) -> String {
    let dir = format!("/ctx/components/{}", entry.path);
    let mut out = String::new();

    if let Some((home, at)) = shipped_path(entry, root, base_family, layout::REPO_SCRIPT) {
        match repo_id(&home.join(&at)).filter(|_| crate::parse::component::rpm(base_family)) {
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

    let scripts = script_paths(entry, root, base_family, layout::PACKAGES_SCRIPT);
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
    for (kind, declared) in [
        (Kind::Packages, &component.packages),
        (Kind::Groups, &component.groups),
    ] {
        for batch in declared.iter().filter(|g| g.family == base_family) {
            let _ = write!(out, "\n{}", install(batch, kind, base_family));
        }
    }
    for script in scripts {
        let _ = write!(out, "\nsource {dir}/{script}\n");
    }
    out
}

fn install(batch: &PackageGroup, kind: Kind, family: &str) -> String {
    let names = batch
        .packages
        .iter()
        .map(|name| shell(&spelled(name, family)))
        .collect::<Vec<_>>()
        .join(" ");
    match crate::parse::component::rpm(family) {
        true => {
            let verb = match kind {
                Kind::Packages => "install",
                Kind::Groups => "group install",
            };
            let repo = batch
                .enablerepo
                .as_ref()
                .map(|repo| format!("--enablerepo={} ", shell(repo)))
                .unwrap_or_default();
            format!("dnf {verb} -y {repo}{names}\nstash_systemctl\n")
        }
        false => {
            format!(
                "mkdir -p /var/lib/apt/lists/partial /var/cache/apt/archives/partial /var/log/apt\n\
                 mkdir -p /var/lib/dpkg/updates /var/lib/dpkg/info /var/lib/dpkg/triggers\n\
                 [ -f /var/lib/dpkg/status ] || : > /var/lib/dpkg/status\n\
                 DEBIAN_FRONTEND=noninteractive apt-get update\n\
                 DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends {names}\n"
            )
        }
    }
}

#[derive(Clone, Copy)]
enum Kind {
    Packages,
    Groups,
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
    let mut out = String::new();

    for key in &component.keys {
        let from = shell(&format!("/ctx/keys{}", key.public));
        let to = shell(&key.public);
        let _ = writeln!(out, "\ninstall -D -m 0644 -- {from} {to}",);
    }

    for script in script_paths(entry, root, base_family, layout::SCRIPT) {
        let _ = write!(out, "\nsource {dir}/{script}\n");
    }

    let policy = match image.answers(layout::SELINUX.capability) {
        true => files_of(entry, root, |dir| layout::SELINUX.files(dir)),
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
        true => files_of(entry, root, |dir| layout::APPARMOR.files(dir)),
        false => Vec::new(),
    };
    if !profiles.is_empty() {
        let _ = write!(out, "\nsource /ctx/lib/apparmor-helpers.sh\n");
        for file in profiles {
            let _ = writeln!(out, "install_apparmor_profile {dir}/apparmor/{file}");
        }
    }

    if overlay_prefixes(root, entry, base_family).is_empty() {
        out.push_str(&collected(entry, collection, &dir));
        out.push_str(&witnesses_in_layer(root, entry, base_family, component));
    }

    out
}

fn after_overlays(
    entry: &Entry,
    component: &Component,
    collection: &Collection,
    base_family: &str,
    root: &Path,
) -> String {
    let dir = format!("/ctx/components/{}", entry.path);
    let mut out = String::new();

    if !overlay_prefixes(root, entry, base_family).is_empty() {
        for declared in &component.modes {
            let _ = writeln!(
                out,
                "chmod '{}' -- {}",
                format_args!("{:04o}", declared.mode),
                shell(&declared.path)
            );
        }
    }

    out.push_str(&collected(entry, collection, &dir));
    out.push_str(&witnesses_in_layer(root, entry, base_family, component));
    out
}

fn collected(entry: &Entry, collection: &Collection, dir: &str) -> String {
    let mut out = String::new();
    if let Some(parts) = collection.by_component.get(&entry.path) {
        for one in parts {
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
