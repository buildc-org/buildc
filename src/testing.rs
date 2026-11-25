use crate::layout;
use std::path::{Path, PathBuf};

pub(crate) fn image_file(root: &Path, build: &str) -> PathBuf {
    layout::manifest_file(root, build)
}

// repo.kdl must not hold a builds block yet because this helper appends one.
pub(crate) fn images(root: &Path, images: &[(&str, &str)]) {
    let mut builds = String::new();
    for (build, body) in images {
        let body = match body.is_empty() || body.contains("component \"") {
            true => body.to_string(),
            false => {
                let tree: String = body.lines().map(|line| format!("    {line}\n")).collect();
                format!("component \"/\" {{\n{tree}}}\n")
            }
        };
        crate::init::put(
            &image_file(root, build),
            &format!("schema-version 1\n\n{body}"),
        )
        .unwrap();
        builds.push_str(&format!("    {build}\n"));
    }
    let repo = root.join(layout::REPO_FILE);
    let head = std::fs::read_to_string(&repo).unwrap_or_else(|_| "schema-version 1\n".to_string());
    crate::init::put(&repo, &format!("{head}\nbuilds {{\n{builds}}}\n")).unwrap();
}

pub(crate) fn image_text(root: &Path, build: &str) -> String {
    std::fs::read_to_string(image_file(root, build)).unwrap()
}
