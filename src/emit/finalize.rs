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

pub fn hooks<'a>(image: &'a Image, root: &Path) -> Vec<&'a Entry> {
    image
        .entries
        .iter()
        .filter(|entry| entry.module.is_some())
        .filter(|entry| {
            layout::modules(root)
                .join(entry.dir())
                .join("finalize.sh")
                .is_file()
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

    for entry in hooks {
        let dir = format!("/ctx/modules/{}", entry.dir());
        let mut body = format!("MODDIR={dir}\nexport MODDIR\nsource {dir}/finalize.sh\n");
        if let Some(flavour) = &entry.flavour {
            body = body
                .lines()
                .map(|line| format!("    {line}\n"))
                .collect::<String>();
            body = format!("if [ \"${{FLAVOUR:-}}\" = \"{flavour}\" ]; then\n{body}fi\n");
        }
        let _ = write!(out, "\n# ---- {} ----\n{body}", entry.path);
    }

    out.push('\n');
    out.push_str(include_str!("finalize.sh"));

    (path(image), out)
}
