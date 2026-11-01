use crate::layout;
use crate::model::image::{Entry, Image};
use crate::resolve::collect::Collection;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

const HEADER: &str = "\
#!/usr/bin/env bash
# GENERATED FILE, do not edit.
set -euxo pipefail
";

pub fn path(image: &Image) -> PathBuf {
    layout::generated_image(&image.id).join("finalize.sh")
}

pub fn hooks<'a>(image: &'a Image, root: &Path) -> Vec<(&'a Entry, Vec<String>)> {
    let family = image.base.as_ref().map_or("", |base| base.family.as_str());
    image
        .entries
        .iter()
        .filter(|entry| entry.component.is_some())
        .filter_map(|entry| {
            let found = hook_scripts(root, entry, family);
            (!found.is_empty()).then_some((entry, found))
        })
        .collect()
}

fn hook_scripts(root: &Path, entry: &Entry, family: &str) -> Vec<String> {
    let dir = layout::components(root).join(&entry.path);
    layout::layered(&dir, family, layout::FINALIZE_SCRIPT)
}

pub fn runs_hook(root: &Path, entry: &Entry, family: &str) -> bool {
    !hook_scripts(root, entry, family).is_empty()
}

fn gated(entry: &Entry, body: String) -> String {
    match &entry.flavour {
        Some(flavour) => {
            let indented: String = body.lines().map(|line| format!("    {line}\n")).collect();
            format!("if [ \"${{FLAVOUR:-}}\" = \"{flavour}\" ]; then\n{indented}fi\n")
        }
        None => body,
    }
}

pub fn script(image: &Image, collection: &Collection, root: &Path) -> (PathBuf, String) {
    let hooks = hooks(image, root);
    let mut out = String::from(HEADER);
    for (dest, parts) in &collection.assembled {
        let _ = write!(
            out,
            r#"
# ---- {dest} ----
parts=()
for part in {}; do
    [ -f "$part" ] || continue
    parts+=("$part")
done
if [ ${{#parts[@]}} -gt 0 ]; then
    cat "${{parts[@]}}" > {dest}
fi
rm -rf {dest}.d
"#,
            parts.join(" ")
        );
    }

    for (entry, scripts) in &hooks {
        let dir = format!("/ctx/components/{}", entry.path);
        let sourced: String = scripts
            .iter()
            .map(|script| format!("source {dir}/{script}\n"))
            .collect();
        let body = gated(
            entry,
            format!("COMPONENT_DIR={dir}\nexport COMPONENT_DIR\n{sourced}"),
        );
        let _ = write!(out, "\n# ---- {} ----\n{body}", entry.path);
    }

    // Every hook runs before every witness check, because a hook can create the file a witness checks.
    for (entry, _) in &hooks {
        let Some(component) = entry.component.as_ref() else {
            continue;
        };
        let checks = super::component_build::witnesses(component);
        if !checks.is_empty() {
            let _ = write!(
                out,
                "\n# ---- {} witnesses ----\n{}",
                entry.path,
                gated(entry, checks)
            );
        }
    }

    out.push('\n');
    out.push_str(include_str!("finalize.sh"));

    (path(image), out)
}
