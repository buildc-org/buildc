use crate::layout;
use crate::model::component::Component;
use crate::model::image::Image;
use crate::resolve::collect::Collection;
use std::collections::{BTreeMap, BTreeSet};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

pub struct CopyFile {
    pub path: PathBuf,
    pub bytes: Vec<u8>,
    pub mode: u32,
    pub link: Option<PathBuf>,
}

pub struct Content {
    pub copies: Vec<CopyFile>,
    pub hashes: BTreeMap<String, String>,
}

pub fn dir(image: &Image, path: &str) -> PathBuf {
    layout::generated_image(&image.id)
        .join(layout::COMPONENTS)
        .join(path)
}

pub fn of(image: &Image, collection: &Collection, root: &Path) -> Content {
    let family = image.base.as_ref().map_or("", |base| base.family.as_str());
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let mut content = Content {
        copies: Vec::new(),
        hashes: BTreeMap::new(),
    };

    for entry in &image.entries {
        let Some(component) = &entry.component else {
            continue;
        };
        if !seen.insert(entry.path.as_str()) {
            continue;
        }
        let mut files = Vec::new();
        resolve_one(&mut files, image, component, collection, family, root);
        if !files.is_empty() {
            if let Some(hash) = hash(&files) {
                content.hashes.insert(entry.path.clone(), hash);
            }
        }
        content.copies.extend(files);
    }

    content
}

fn resolve_one(
    copies: &mut Vec<CopyFile>,
    image: &Image,
    component: &Component,
    collection: &Collection,
    family: &str,
    root: &Path,
) {
    let on_disk = layout::component(root, &component.path);
    let at = dir(image, &component.path);

    for file in [
        layout::SCRIPT,
        layout::PACKAGES_SCRIPT,
        layout::FINALIZE_SCRIPT,
    ] {
        for script in layout::layered(&on_disk, family, file) {
            copy(&at, &script, &on_disk.join(&script), copies);
        }
    }
    if let Some(script) = layout::shipped(&on_disk, family, layout::REPO_SCRIPT) {
        copy(&at, &script, &on_disk.join(&script), copies);
    }

    if image.answers(layout::SELINUX.capability) {
        for file in layout::SELINUX.files(&on_disk) {
            let rel = format!("{}/{file}", layout::SELINUX.dir);
            copy(&at, &rel, &on_disk.join(&rel), copies);
        }
    }
    if image.answers(layout::APPARMOR.capability) {
        for file in layout::APPARMOR.files(&on_disk) {
            let rel = format!("{}/{file}", layout::APPARMOR.dir);
            copy(&at, &rel, &on_disk.join(&rel), copies);
        }
    }

    let roots = std::iter::once((String::new(), on_disk.clone())).chain(
        layout::family_dirs(&on_disk, family)
            .into_iter()
            .map(|gated| (gated.to_string(), on_disk.join(gated))),
    );
    for (gated, one) in roots {
        let from = one.join(layout::OVERLAY);
        let Ok(entries) = std::fs::read_dir(&from) else {
            continue;
        };
        let mut overlay = BTreeMap::new();
        walk(&from, entries, &mut overlay);
        for (rel, (bytes, mode, link)) in overlay {
            let path = at.join(&gated).join(layout::OVERLAY).join(rel);
            copies.push(CopyFile {
                path,
                bytes,
                mode,
                link,
            });
        }
    }

    if let Some(collected) = collection.by_component.get(&component.path) {
        for one in collected {
            copy(&at, &one.file, &on_disk.join(&one.file), copies);
        }
    }

    for helper in &component.helpers {
        copy(&at, &helper.name, &on_disk.join(&helper.name), copies);
    }

    if component.fragment.is_some() || !component.standard_layer {
        copy_whole(&at, &on_disk, family, copies);
    }

    let mut by_path: BTreeMap<PathBuf, usize> = BTreeMap::new();
    let mut kept: Vec<CopyFile> = Vec::with_capacity(copies.len());
    for file in copies.drain(..) {
        match by_path.get(&file.path) {
            Some(&index) => kept[index] = file,
            None => {
                by_path.insert(file.path.clone(), kept.len());
                kept.push(file);
            }
        }
    }
    *copies = kept;
}

fn copy_whole(at: &Path, on_disk: &Path, family: &str, copies: &mut Vec<CopyFile>) {
    let held = layout::held_components(on_disk);
    let gated = layout::family_dirs(on_disk, family);
    walk_whole(at, on_disk, on_disk, &gated, &held, copies);
}

fn walk_whole(
    at: &Path,
    root: &Path,
    here: &Path,
    gated: &[&str],
    held: &[PathBuf],
    copies: &mut Vec<CopyFile>,
) {
    let Ok(entries) = std::fs::read_dir(here) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(rel) = path.strip_prefix(root) else {
            continue;
        };
        if held.iter().any(|component| rel.starts_with(component)) {
            continue;
        }
        let is_dir = entry.file_type().is_ok_and(|kind| kind.is_dir());
        if is_dir {
            let first = rel.components().next().map(|part| part.as_os_str());
            let other_family = layout::FAMILY_DIRS
                .iter()
                .any(|(name, _)| first.is_some_and(|part| part == *name) && !gated.contains(name));
            if !other_family {
                walk_whole(at, root, &path, gated, held, copies);
            }
        } else if rel == Path::new(layout::MANIFEST_FILE) || rel == Path::new(layout::FRAGMENT) {
        } else if let Ok(meta) = std::fs::symlink_metadata(&path) {
            let mode = meta.permissions().mode();
            let link = meta
                .file_type()
                .is_symlink()
                .then(|| std::fs::read_link(&path).ok())
                .flatten();
            let bytes = match link {
                Some(_) => Vec::new(),
                None => match std::fs::read(&path) {
                    Ok(bytes) => bytes,
                    Err(_) => continue,
                },
            };
            copies.push(CopyFile {
                path: at.join(rel),
                bytes,
                mode,
                link,
            });
        }
    }
}

fn walk(
    from: &Path,
    entries: std::fs::ReadDir,
    out: &mut BTreeMap<String, (Vec<u8>, u32, Option<PathBuf>)>,
) {
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(rel) = path.strip_prefix(from) else {
            continue;
        };
        let rel = rel.to_string_lossy().into_owned();
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            let mut inner = BTreeMap::new();
            if let Ok(nested) = std::fs::read_dir(&path) {
                walk(&path, nested, &mut inner);
            }
            for (name, value) in inner {
                out.insert(format!("{rel}/{name}"), value);
            }
        } else if let Ok(meta) = std::fs::symlink_metadata(&path) {
            let mode = meta.permissions().mode();
            let link = meta
                .file_type()
                .is_symlink()
                .then(|| std::fs::read_link(&path).ok())
                .flatten();
            let bytes = match link {
                Some(_) => Vec::new(),
                None => match std::fs::read(&path) {
                    Ok(bytes) => bytes,
                    Err(_) => continue,
                },
            };
            out.insert(rel, (bytes, mode, link));
        }
    }
}

fn copy(at: &Path, rel: &str, from: &Path, copies: &mut Vec<CopyFile>) {
    let Ok(meta) = std::fs::symlink_metadata(from) else {
        return;
    };
    let mode = meta.permissions().mode();
    let link = meta
        .file_type()
        .is_symlink()
        .then(|| std::fs::read_link(from).ok())
        .flatten();
    let bytes = match link {
        Some(_) => Vec::new(),
        None => match std::fs::read(from) {
            Ok(bytes) => bytes,
            Err(_) => return,
        },
    };
    copies.push(CopyFile {
        path: at.join(rel),
        bytes,
        mode,
        link,
    });
}

fn hash(files: &[CopyFile]) -> Option<String> {
    let mut sorted: Vec<&CopyFile> = files.iter().collect();
    sorted.sort_by(|a, b| a.path.cmp(&b.path));
    let mut bytes = Vec::new();
    for file in sorted {
        let rel = file.path.to_string_lossy();
        bytes.extend_from_slice(rel.as_bytes());
        bytes.push(0);
        bytes.extend_from_slice(&(file.bytes.len() as u64).to_le_bytes());
        bytes.extend_from_slice(&file.mode.to_le_bytes());
        bytes.extend_from_slice(&file.bytes);
    }
    crate::runtime::sha256_stdin(&bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, bytes: &[u8]) -> CopyFile {
        CopyFile {
            path: PathBuf::from(path),
            bytes: bytes.to_vec(),
            mode: 0o644,
            link: None,
        }
    }

    #[test]
    fn a_hash_changes_with_a_byte_and_with_a_path() {
        let one = hash(&[file("a", b"one")]);
        assert_eq!(one, hash(&[file("a", b"one")]));
        assert_ne!(one, hash(&[file("a", b"two")]));
        assert_ne!(one, hash(&[file("b", b"one")]));
    }
}
