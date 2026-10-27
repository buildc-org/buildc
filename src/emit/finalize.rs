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
            let dir = layout::components(root).join(&entry.path);
            let found = layout::layered(&dir, family, layout::FINALIZE_SCRIPT);
            (!found.is_empty()).then_some((entry, found))
        })
        .collect()
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

    for (entry, scripts) in hooks {
        let dir = format!("/ctx/components/{}", entry.path);
        let sourced: String = scripts
            .iter()
            .map(|script| format!("source {dir}/{script}\n"))
            .collect();
        let mut body = format!("COMPONENT_DIR={dir}\nexport COMPONENT_DIR\n{sourced}");
        if let Some(flavour) = &entry.flavour {
            body = body
                .lines()
                .map(|line| format!("    {line}\n"))
                .collect::<String>();
            body = format!("if [ \"${{FLAVOUR:-}}\" = \"{flavour}\" ]; then\n{body}fi\n");
        }
        let _ = write!(out, "\n# ---- {} ----\n{body}", entry.path);
    }

    for entry in &image.entries {
        if entry
            .component
            .as_ref()
            .is_some_and(|component| super::containerfile::renders_os_release(image, component))
        {
            let _ = write!(
                out,
                "\n# ---- os-release ----\n{}\n",
                super::containerfile::flavoured(entry.flavour.as_deref(), "/ctx/buildc os-release")
            );
        }
    }

    out.push('\n');
    out.push_str(include_str!("finalize.sh"));

    (path(image), out)
}
