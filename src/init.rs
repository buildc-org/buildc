use crate::layout;
use crate::model::image::{is_name, SCHEMA_VERSION};
use std::fs;
use std::path::{Path, PathBuf};

const INSTALLED: [&str; 2] = ["/usr/local/share/buildc/assets", "/usr/share/buildc/assets"];

pub(crate) const GITIGNORE: &str = include_str!("../assets/.gitignore");

const SCAFFOLDED: [(&str, &str); 5] = [
    (".dockerignore", include_str!("../assets/.dockerignore")),
    (".gitattributes", include_str!("../assets/.gitattributes")),
    (".gitignore", GITIGNORE),
    (".shellcheckrc", include_str!("../assets/.shellcheckrc")),
    (
        "disk_config/disk.toml",
        include_str!("../assets/disk_config/disk.toml"),
    ),
];

const RENOVATE: &str = r#"// Renovate opens its pull requests in this window.
// The pull request build and the automerge finish before the 12:30 UTC image build.
renovate rebaseWhen="never" timezone="Etc/UTC" {
    extends "config:best-practices"
    schedule "* 0-5 * * *"
    packageRules {
        - groupName="scheduled updates" automerge=#true {
            matchPackageNames "*"
        }
        // quay.io/fedora prunes an untagged manifest within days.
        // A pinned digest then fails to pull before the next bump lands.
        - pinDigests=#false {
            matchPackageNames "quay.io/fedora/fedora-bootc"
        }
    }
}
"#;

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

pub fn libraries_block(chosen: &[crate::model::remote::DefaultLibrary]) -> String {
    let own = layout::ROOT_LIBRARY;
    let mut out = format!(
        "libraries default=\"{own}\" {{\n    local {{\n        {own} {{ source \".\" }}\n    }}\n"
    );
    if !chosen.is_empty() {
        out.push_str("    remote {\n");
        for library in chosen {
            let _ = std::fmt::Write::write_fmt(
                &mut out,
                format_args!(
                    "        {} {{ source {:?} }}\n",
                    library.name, library.source
                ),
            );
        }
        out.push_str("    }\n");
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

// The XDG base directory specification counts an empty `XDG_DATA_HOME` as unset.
pub(crate) fn data_home() -> Option<PathBuf> {
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
    base_url: Option<&str>,
    registry: Option<&str>,
    assets: &Path,
    libraries: &str,
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
            Path::new(crate::collection::DEFAULTS_FILE),
        ],
    )?;

    let base_url = base_url
        .map(|url| format!("base-url \"{url}\"\n"))
        .unwrap_or_default();
    let registry = registry
        .map(|registry| format!("registry \"{registry}\"\n"))
        .unwrap_or_default();
    let libraries = match libraries.is_empty() {
        true => String::new(),
        false => format!("\n{libraries}\n"),
    };
    put(
        &root.join(layout::REPO_FILE),
        &format!(
            "schema-version {SCHEMA_VERSION}\n\
             name \"{name}\"\n\
             {base_url}{registry}{libraries}\n{RENOVATE}"
        ),
    )?;
    put(&root.join("README.md"), &format!("# {name}\n"))?;
    put(&root.join("components/.gitkeep"), "")?;
    wrote.extend([layout::REPO_FILE, "README.md", "components/.gitkeep"].map(PathBuf::from));
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

pub(crate) fn undone(err: String, failed: &[String]) -> String {
    match failed.is_empty() {
        true => err,
        false => format!(
            "{err}\n\nThe undo left these changes in place, and each needs undoing by hand:\n{}",
            failed.join("\n")
        ),
    }
}

pub(crate) type Kept = Vec<(PathBuf, Option<String>)>;

pub(crate) fn restore(kept: &[(PathBuf, Option<String>)]) -> Vec<String> {
    kept.iter()
        .filter_map(|(file, text)| match text {
            Some(text) => put(file, text).err(),
            None => remove(file).err(),
        })
        .collect()
}

pub(crate) fn undoable<T>(
    root: &Path,
    files: &[PathBuf],
    write: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let mut kept: Kept = Vec::new();
    for file in files {
        let text = match file.symlink_metadata().is_ok() {
            true => {
                Some(fs::read_to_string(file).map_err(|err| format!("{}: {err}", file.display()))?)
            }
            false => None,
        };
        kept.push((file.clone(), text));
    }
    // The undo also removes the outermost folder that the write creates for a file. A dangling link counts as present and stays.
    for file in files {
        let created = file
            .ancestors()
            .skip(1)
            .take_while(|dir| dir.symlink_metadata().is_err())
            .last();
        if let Some(dir) = created {
            if !kept.iter().any(|(held, _)| held == dir) {
                kept.push((dir.to_path_buf(), None));
            }
        }
    }
    let blocking = || crate::load(root).issues.blocking_edits();
    // A load failure undoes the edit only if the builds loaded before it.
    let loaded = blocking().is_empty();
    write()
        .and_then(|done| {
            let issues = blocking();
            match loaded && !issues.is_empty() {
                true => Err(format!(
                    "the change is undone, because the builds no longer load with it:\n\n{}",
                    issues.plain()
                )),
                false => Ok(done),
            }
        })
        .map_err(|err| undone(err, &restore(&kept)))
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

/// The calling command must run `layout::contained` on the component first. This copy follows a link and a link can point back up the tree.
pub(crate) fn copy_component_tree(from: &Path, to: &Path) -> Result<Vec<PathBuf>, String> {
    copy_below(from, to, Path::new(""), &[], Links::Component)
}

fn copy_tree_except(from: &Path, to: &Path, skip: &[&Path]) -> Result<Vec<PathBuf>, String> {
    copy_below(from, to, Path::new(""), skip, Links::Refuse)
}

#[derive(Clone, Copy)]
enum Links {
    Refuse,
    Component,
}

fn copy_below(
    from: &Path,
    to: &Path,
    under: &Path,
    skip: &[&Path],
    links: Links,
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
        let dest = to.join(&rel);
        let kind = entry
            .file_type()
            .map_err(|err| format!("{}: {err}", source.display()))?;
        // An overlay link such as a masked unit must reach the image as a link. A component copy reads through any other link that stays inside the component.
        if kind.is_symlink() {
            if matches!(links, Links::Refuse) {
                return Err(format!(
                    "{} is a symbolic link, and a repository copy holds files and directories only",
                    source.display()
                ));
            }
            if layout::in_overlay(from, &rel) {
                let target =
                    fs::read_link(&source).map_err(|err| format!("{}: {err}", source.display()))?;
                std::os::unix::fs::symlink(&target, &dest)
                    .map_err(|err| format!("{}: {err}", dest.display()))?;
                wrote.push(rel);
                continue;
            }
            if let Some(escape) = layout::leaves(from, &source) {
                let (at, title) = layout::escaped(&escape, "the component copy");
                return Err(format!("{}: {title}", at.display()));
            }
        }
        if source.is_dir() {
            wrote.extend(copy_below(from, to, &rel, skip, links)?);
        } else {
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
    fn an_undo_keeps_a_link_to_a_missing_folder() {
        let at = std::env::temp_dir().join(format!("buildc-undo-link-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&at);
        fs::create_dir_all(&at).unwrap();
        std::os::unix::fs::symlink(at.join("unmounted"), at.join("components")).unwrap();
        let file = at.join("components/.gitkeep");
        let undone = undoable(&at, std::slice::from_ref(&file), || {
            Err::<(), _>("stop".to_string())
        });
        let kept = at.join("components").symlink_metadata().is_ok();
        fs::remove_dir_all(&at).unwrap();
        assert!(undone.is_err());
        assert!(kept);
    }

    #[test]
    fn a_component_copy_refuses_a_link_outside_the_component() {
        let at = std::env::temp_dir().join(format!("buildc-link-{}", std::process::id()));
        put(&at.join("library/shared/notes"), "shared\n").unwrap();
        put(&at.join("library/browser/manifest.kdl"), "").unwrap();
        std::os::unix::fs::symlink(
            at.join("library/shared/notes"),
            at.join("library/browser/notes"),
        )
        .unwrap();
        let refused =
            copy_component_tree(&at.join("library/browser"), &at.join("components/browser"))
                .unwrap_err();
        assert!(refused.contains("outside the component"), "{refused}");
        assert!(!at.join("components/browser/notes").exists());
        fs::remove_dir_all(at).unwrap();
    }

    #[test]
    fn a_component_copy_keeps_an_overlay_link_and_copies_through_its_own_link() {
        let at = std::env::temp_dir().join(format!("buildc-link-kept-{}", std::process::id()));
        let _ = fs::remove_dir_all(&at);
        let from = at.join("library/browser");
        put(&from.join("manifest.kdl"), "").unwrap();
        put(&from.join("lib/family.sh"), "true\n").unwrap();
        fs::create_dir_all(from.join("files/etc")).unwrap();
        std::os::unix::fs::symlink("/dev/null", from.join("files/etc/masked.service")).unwrap();
        std::os::unix::fs::symlink("lib", from.join("tools")).unwrap();
        copy_component_tree(&from, &at.join("to")).unwrap();
        let masked = fs::read_link(at.join("to/files/etc/masked.service")).ok();
        let tools = fs::symlink_metadata(at.join("to/tools")).map(|meta| meta.is_dir());
        let script = fs::read_to_string(at.join("to/tools/family.sh")).ok();
        fs::remove_dir_all(at).unwrap();
        assert_eq!(masked, Some(PathBuf::from("/dev/null")));
        assert!(matches!(tools, Ok(true)));
        assert_eq!(script.as_deref(), Some("true\n"));
    }

    #[test]
    fn an_absent_path_is_already_removed() {
        let at = std::env::temp_dir().join(format!("buildc-absent-{}", std::process::id()));
        assert_eq!(remove(&at), Ok(()));
    }

    #[test]
    fn a_removal_that_fails_names_the_path() {
        use std::os::unix::fs::PermissionsExt as _;
        if crate::sys::privileged() {
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
        let mut wrote: Vec<String> = write(&root, "Example", None, None, &assets, "")
            .unwrap()
            .iter()
            .map(|path| path.display().to_string())
            .filter(|path| {
                ![layout::REPO_FILE, "README.md", "components/.gitkeep"].contains(&path.as_str())
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
    fn create_repo_declares_this_repository_and_each_chosen_library() {
        let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
        let block = libraries_block(&crate::collection::defaults(&assets));
        assert_eq!(
            block,
            "libraries default=\"local-library\" {\n    local {\n        local-library { source \".\" }\n    }\n    remote {\n        buildc { source \"https://github.com/buildc-org/library\" }\n    }\n}"
        );
        let text = format!("schema-version 1\nname \"Example\"\n{block}\n");
        let issues = crate::parse::schema::check_text(&text, &crate::parse::repo::REPO, false)
            .unwrap_or_else(|err| panic!("{err}: {text}"));
        assert!(issues.is_empty(), "{}", issues.plain());
        assert_eq!(
            libraries_block(&[]),
            "libraries default=\"local-library\" {\n    local {\n        local-library { source \".\" }\n    }\n}"
        );
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
            "base-image/mine.base.kdl",
        ];
        for path in mine {
            put(&root.join(path), "mine\n").unwrap();
        }
        let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
        write(&root, "Example", None, None, &assets, "").unwrap();
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
