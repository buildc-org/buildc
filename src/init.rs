use crate::model::image::{is_name, REPO_FILE, SCHEMA_VERSION, BUILDC_VERSION};
use std::fs;
use std::path::{Path, PathBuf};

const INSTALLED: [&str; 2] = [
    "/usr/local/share/buildc/assets",
    "/usr/share/buildc/assets",
];

pub const SOURCES: &str = "\
sources {
    buildc-org \"https://github.com/buildc-org/modules/archive/refs/heads/{ref}.tar.gz\" {
        unpinned \"the collection this tool is published alongside, followed at its branch head\"
        ref \"main\"
    }
}
";

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

fn data_home() -> Option<PathBuf> {
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

pub fn write(root: &Path, name: &str, assets: &Path) -> Result<(), String> {
    if root.join(REPO_FILE).exists() {
        return Err(format!("{} is already a repository", root.display()));
    }

    copy_tree(assets, root)?;
    put(
        &root.join(REPO_FILE),
        &format!(
            "schema-version {SCHEMA_VERSION}\n\n\
             // renovate: datasource=github-releases depName=buildc-org/buildc\n\
             buildc-version \"{BUILDC_VERSION}\"\n\n\
             {SOURCES}"
        ),
    )?;
    put(&root.join("README.md"), &format!("# {name}\n"))?;
    put(&root.join("modules/.gitkeep"), "")?;
    Ok(())
}

pub(crate) fn put(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|err| format!("{}: {err}", dir.display()))?;
    }
    fs::write(path, text).map_err(|err| format!("{}: {err}", path.display()))
}

pub(crate) fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    fs::create_dir_all(to).map_err(|err| format!("{}: {err}", to.display()))?;
    let entries = fs::read_dir(from).map_err(|err| format!("{}: {err}", from.display()))?;
    for entry in entries {
        let entry = entry.map_err(|err| format!("{}: {err}", from.display()))?;
        let source = entry.path();
        let dest = to.join(entry.file_name());
        if source.is_dir() {
            copy_tree(&source, &dest)?;
        } else {
            fs::copy(&source, &dest).map_err(|err| format!("{}: {err}", dest.display()))?;
        }
    }
    Ok(())
}
