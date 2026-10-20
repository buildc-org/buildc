use crate::layout;
use crate::model::image::{is_name, SCHEMA_VERSION};
use std::fs;
use std::path::{Path, PathBuf};

const INSTALLED: [&str; 2] = [
    "/usr/local/share/buildc/assets",
    "/usr/share/buildc/assets",
];

pub(crate) const GITIGNORE: &str = include_str!("../assets/.gitignore");

const SCAFFOLDED: [(&str, &str); 6] = [
    (".dockerignore", include_str!("../assets/.dockerignore")),
    (".gitattributes", include_str!("../assets/.gitattributes")),
    (".gitignore", GITIGNORE),
    (".shellcheckrc", include_str!("../assets/.shellcheckrc")),
    (
        ".github/renovate.json5",
        include_str!("../assets/.github/renovate.json5"),
    ),
    (
        "disk_config/disk.toml",
        include_str!("../assets/disk_config/disk.toml"),
    ),
];

pub(crate) const SKELETON: &str = include_str!("../assets/scripts/Containerfile.skeleton");

pub fn drifted(root: &Path) -> Vec<String> {
    let differs = |path: &str, shipped: &str| {
        fs::read_to_string(root.join(path)).is_ok_and(|held| held != shipped)
    };
    let version = env!("CARGO_PKG_VERSION");
    let scaffolded = SCAFFOLDED
        .iter()
        .filter(|(path, shipped)| differs(path, shipped))
        .map(|(path, _)| {
            format!(
                "`{path}` is not what buildc v{version} scaffolds, and nothing rewrites it; \
                 the current one is assets/{path} in that release"
            )
        });
    let skeleton = crate::emit::containerfile::SKELETON;
    let kept_skeleton = differs(skeleton, SKELETON).then(|| {
        format!(
            "`{skeleton}` is not what buildc v{version} supplies; if it is deleted, then \
             `generate` uses the one buildc supplies"
        )
    });
    let kept_scripts = crate::emit::SCRIPTS
        .iter()
        .map(|script| (format!("{}/{}", layout::SCRIPTS, script.name), script))
        .filter(|(path, script)| differs(path, &crate::emit::located(root, script.body)))
        .map(|(path, script)| {
            format!(
                "`{path}` is not what buildc v{version} supplies, and `generate` does not \
                 replace it; if it is deleted, then `generate` writes the current one to \
                 generated/scripts/{}",
                script.name
            )
        });
    scaffolded
        .chain(kept_skeleton)
        .chain(kept_scripts)
        .collect()
}

pub fn sources_block(chosen: &[&crate::base::DefaultLibrary]) -> String {
    if chosen.is_empty() {
        return String::new();
    }
    let mut out = String::from("sources {\n");
    for library in chosen {
        let _ = std::fmt::Write::write_fmt(
            &mut out,
            format_args!(
                "    {} \"{}\" {{\n        url \"{}\"\n        path \"{}\"\n    }}\n",
                library.kind.as_str(),
                library.alias,
                crate::base::LIBRARY_URL,
                library.path
            ),
        );
    }
    out.push('}');
    out
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

pub fn write(
    root: &Path,
    name: &str,
    assets: &Path,
    sources: &str,
) -> Result<Vec<PathBuf>, String> {
    if root.join(layout::REPO_FILE).exists() {
        return Err(format!("{} is already a repository", root.display()));
    }

    let mut wrote = copy_tree_except(
        assets,
        root,
        &[
            Path::new("lib"),
            Path::new(layout::SCRIPTS),
            Path::new(layout::WORKFLOW_DIR),
        ],
    )?;

    let sources = match sources.is_empty() {
        true => String::new(),
        false => format!("\n{sources}"),
    };
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

pub(crate) fn remove(path: &Path) -> Result<(), String> {
    let removed = match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
        Err(err) => Err(err),
    };
    match removed {
        Err(err) if err.kind() != std::io::ErrorKind::NotFound => {
            Err(format!("{}: {err}", path.display()))
        }
        Ok(()) | Err(_) => Ok(()),
    }
}

pub(crate) fn tidy(path: &Path) {
    if let Err(err) = remove(path) {
        eprintln!("buildc: {err}; remove it by hand");
    }
}

pub(crate) struct Scratch(pub(crate) PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        tidy(&self.0);
    }
}

pub(crate) fn put(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|err| format!("{}: {err}", dir.display()))?;
    }
    fs::write(path, text).map_err(|err| format!("{}: {err}", path.display()))
}

pub(crate) fn copy_module(from: &Path, to: &Path) -> Result<Vec<PathBuf>, String> {
    let held = layout::held_modules(from);
    let skip: Vec<&Path> = held.iter().map(PathBuf::as_path).collect();
    copy_tree_except(from, to, &skip)
}

fn copy_tree_except(from: &Path, to: &Path, skip: &[&Path]) -> Result<Vec<PathBuf>, String> {
    copy_below(from, to, Path::new(""), skip)
}

fn copy_below(
    from: &Path,
    to: &Path,
    under: &Path,
    skip: &[&Path],
) -> Result<Vec<PathBuf>, String> {
    let dest_dir = to.join(under);
    fs::create_dir_all(&dest_dir).map_err(|err| format!("{}: {err}", dest_dir.display()))?;
    let dir = from.join(under);
    let entries = fs::read_dir(&dir).map_err(|err| format!("{}: {err}", dir.display()))?;
    let mut wrote = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|err| format!("{}: {err}", dir.display()))?;
        let rel = under.join(entry.file_name());
        if skip.iter().any(|skip| rel == *skip) {
            continue;
        }
        let source = entry.path();
        if source.is_dir() {
            wrote.extend(copy_below(from, to, &rel, skip)?);
        } else {
            let dest = to.join(&rel);
            fs::copy(&source, &dest).map_err(|err| format!("{}: {err}", dest.display()))?;
            wrote.push(rel);
        }
    }
    Ok(wrote)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_absent_path_is_already_removed() {
        let at = std::env::temp_dir().join(format!("buildc-absent-{}", std::process::id()));
        assert_eq!(remove(&at), Ok(()));
    }

    #[test]
    fn a_removal_that_fails_names_the_path() {
        use std::os::unix::fs::PermissionsExt as _;
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        let root = std::env::temp_dir().join(format!("buildc-held-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let file = root.join("private.key");
        put(&file, "secret").unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o500)).unwrap();
        let said = remove(&file);
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let _ = fs::remove_dir_all(&root);
        let said = said.expect_err("a read-only directory keeps its file");
        assert!(said.contains("private.key"), "{said}");
    }

    #[test]
    fn a_module_copy_leaves_out_the_modules_it_holds() {
        let root = std::env::temp_dir().join(format!("buildc-copy-held-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let from = root.join("from");
        put(&from.join(layout::MODULE_FILE), "").unwrap();
        put(&from.join("module.sh"), "true\n").unwrap();
        put(&from.join("cuda").join(layout::MODULE_FILE), "").unwrap();
        put(&from.join("cuda/module.sh"), "true\n").unwrap();

        let mut wrote = copy_module(&from, &root.join("to")).unwrap();
        wrote.sort();
        let _ = fs::remove_dir_all(&root);
        assert_eq!(
            wrote,
            [
                PathBuf::from(layout::MODULE_FILE),
                PathBuf::from("module.sh")
            ]
        );
    }

    #[test]
    fn a_scratch_tree_is_gone_once_its_guard_drops() {
        let root = std::env::temp_dir().join(format!("buildc-scratch-{}", std::process::id()));
        put(&root.join("inner/private.key"), "secret").unwrap();
        drop(Scratch(root.clone()));
        assert!(!root.exists());
    }

    #[test]
    fn the_scaffolded_table_is_what_create_repo_copies() {
        let root = std::env::temp_dir().join(format!("buildc-scaffold-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
        let mut wrote: Vec<String> = write(&root, "Example", &assets, "")
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

    #[test]
    fn a_repository_written_into_a_full_directory_keeps_the_users_files() {
        let root = std::env::temp_dir().join(format!("buildc-keep-{}", std::process::id()));
        if root.exists() {
            fs::remove_dir_all(&root).unwrap();
        }
        let mine = [
            "scripts/deploy.sh",
            ".github/workflows/mine.yml",
            "base-images/mine.base.kdl",
        ];
        for path in mine {
            put(&root.join(path), "mine\n").unwrap();
        }
        let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
        write(&root, "Example", &assets, "").unwrap();
        for path in mine {
            assert_eq!(
                fs::read_to_string(root.join(path)).unwrap(),
                "mine\n",
                "{path}"
            );
        }
        fs::remove_dir_all(root).unwrap();
    }
}
