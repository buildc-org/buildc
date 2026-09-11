use crate::layout;
use crate::model::image::{is_name, SCHEMA_VERSION};
use std::fs;
use std::path::{Path, PathBuf};

const INSTALLED: [&str; 2] = [
    "/usr/local/share/buildc/assets",
    "/usr/share/buildc/assets",
];

const SCAFFOLDED: [(&str, &str); 7] = [
    (".dockerignore", include_str!("../assets/.dockerignore")),
    (".gitattributes", include_str!("../assets/.gitattributes")),
    (".gitignore", include_str!("../assets/.gitignore")),
    (".shellcheckrc", include_str!("../assets/.shellcheckrc")),
    (
        ".github/renovate.json5",
        include_str!("../assets/.github/renovate.json5"),
    ),
    (
        "disk_config/disk.toml",
        include_str!("../assets/disk_config/disk.toml"),
    ),
    (
        "scripts/Containerfile.skeleton",
        include_str!("../assets/scripts/Containerfile.skeleton"),
    ),
];

pub fn drifted(root: &Path) -> Vec<String> {
    SCAFFOLDED
        .iter()
        .filter(|(path, shipped)| {
            fs::read_to_string(root.join(path)).is_ok_and(|held| held != *shipped)
        })
        .map(|(path, _)| {
            format!(
                "`{path}` is not what buildc v{} scaffolds, and nothing rewrites it; the \
                 current one is assets/{path} in that release",
                env!("CARGO_PKG_VERSION")
            )
        })
        .collect()
}

pub const SOURCES_FILE: &str = "repo.sources.kdl";

pub fn sources(assets: &Path) -> String {
    fs::read_to_string(assets.join(SOURCES_FILE)).unwrap_or_default()
}

pub fn assets() -> Result<PathBuf, String> {
    let mut tried: Vec<PathBuf> = Vec::new();
    if let Some(dir) = std::env::var_os("BUILDC_ASSETS") {
        tried.push(PathBuf::from(dir));
    }
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_path_buf))
    {
        tried.push(dir.join("assets"));
    }
    if let Some(dir) = data_home() {
        tried.push(dir.join("buildc/assets"));
    }
    tried.extend(INSTALLED.iter().map(PathBuf::from));

    match tried.iter().find(|dir| dir.is_dir()) {
        Some(dir) => Ok(dir.clone()),
        None => Err(format!(
            "no assets directory; looked in {}",
            tried
                .iter()
                .map(|d| d.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

pub(crate) fn data_home() -> Option<PathBuf> {
    // The XDG base directory specification counts an empty XDG_DATA_HOME as unset.
    std::env::var_os("XDG_DATA_HOME")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| Path::new(&home).join(".local/share")))
}

pub fn id(name: &str) -> Result<String, String> {
    let id = name.to_lowercase().replace(' ', "-");
    if !is_name(&id) {
        return Err(format!(
            "`{name}` does not derive a usable name: lowercase letters, digits and \
             dashes, starting with a letter"
        ));
    }
    Ok(id)
}

pub fn write(root: &Path, name: &str, assets: &Path) -> Result<Vec<PathBuf>, String> {
    if root.join(layout::REPO_FILE).exists() {
        return Err(format!("{} is already a repository", root.display()));
    }

    let mut wrote = copy_tree_except(assets, root, Some("lib"))?;
    for (path, _) in crate::emit::SCRIPTS {
        wrote.retain(|written| written != Path::new(path));
        let at = root.join(path);
        if at.exists() {
            fs::remove_file(&at).map_err(|err| format!("{}: {err}", at.display()))?;
        }
    }
    let shipped = root.join(layout::WORKFLOW_DIR);
    if shipped.is_dir() {
        fs::remove_dir_all(&shipped).map_err(|err| format!("{}: {err}", shipped.display()))?;
    }
    wrote.retain(|path| !path.starts_with(layout::WORKFLOW_DIR));

    let scaffold = root.join(SOURCES_FILE);
    let sources = match sources(assets) {
        block if block.is_empty() => block,
        block => format!("\n{block}"),
    };
    for control in [scaffold.as_path(), &root.join(crate::base::BASES_FILE)] {
        if control.is_file() {
            fs::remove_file(control).map_err(|err| format!("{}: {err}", control.display()))?;
        }
        wrote.retain(|path| root.join(path) != control);
    }
    put(
        &root.join(layout::REPO_FILE),
        &format!(
            "schema-version {SCHEMA_VERSION}\n\
             name \"{name}\"\n\
             {sources}"
        ),
    )?;
    put(&root.join("README.md"), &format!("# {name}\n"))?;
    put(&root.join("modules/.gitkeep"), "")?;
    wrote.extend([layout::REPO_FILE, "README.md", "modules/.gitkeep"].map(PathBuf::from));
    Ok(wrote)
}

pub(crate) fn put(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|err| format!("{}: {err}", dir.display()))?;
    }
    fs::write(path, text).map_err(|err| format!("{}: {err}", path.display()))
}

pub(crate) fn copy_tree(from: &Path, to: &Path) -> Result<Vec<PathBuf>, String> {
    copy_tree_except(from, to, None)
}

fn copy_tree_except(from: &Path, to: &Path, skip: Option<&str>) -> Result<Vec<PathBuf>, String> {
    fs::create_dir_all(to).map_err(|err| format!("{}: {err}", to.display()))?;
    let entries = fs::read_dir(from).map_err(|err| format!("{}: {err}", from.display()))?;
    let mut wrote = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|err| format!("{}: {err}", from.display()))?;
        let source = entry.path();
        let name = entry.file_name();
        if skip.is_some_and(|skip| name == skip) {
            continue;
        }
        let dest = to.join(&name);
        if source.is_dir() {
            wrote.extend(
                copy_tree(&source, &dest)?
                    .into_iter()
                    .map(|under| Path::new(&name).join(under)),
            );
        } else {
            fs::copy(&source, &dest).map_err(|err| format!("{}: {err}", dest.display()))?;
            wrote.push(PathBuf::from(name));
        }
    }
    Ok(wrote)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scaffolded_table_is_what_create_repo_copies() {
        let root = std::env::temp_dir().join(format!("buildc-scaffold-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
        let mut wrote: Vec<String> = write(&root, "Example", &assets)
            .unwrap()
            .iter()
            .map(|path| path.display().to_string())
            .filter(|path| {
                ![layout::REPO_FILE, "README.md", "modules/.gitkeep"].contains(&path.as_str())
            })
            .collect();
        wrote.sort();
        let mut table: Vec<&str> = SCAFFOLDED.iter().map(|(path, _)| *path).collect();
        table.sort();
        assert_eq!(wrote, table);
        assert!(drifted(&root).is_empty());

        fs::write(root.join(".gitignore"), "mine\n").unwrap();
        fs::remove_file(root.join(".shellcheckrc")).unwrap();
        let said = drifted(&root);
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(said[0].starts_with("`.gitignore`"), "{said:?}");
        fs::remove_dir_all(root).unwrap();
    }
}
