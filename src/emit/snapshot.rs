use crate::emit::resolved::CopyFile;
use crate::layout;
use common::json::Json;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

pub fn path() -> PathBuf {
    PathBuf::from(layout::GENERATED).join(FILE)
}

const FILE: &str = "snapshot.json";

pub fn file(files: &[(PathBuf, String)], copies: &[CopyFile]) -> (PathBuf, String) {
    let root = root(files, copies).unwrap_or_default();
    let count = generated(files, copies).count() + 1;
    let text = Json::object([
        ("root", Json::string(root)),
        (
            "files",
            Json::number(u32::try_from(count).unwrap_or(u32::MAX)),
        ),
    ])
    .render();
    (path(), text)
}

pub fn root(files: &[(PathBuf, String)], copies: &[CopyFile]) -> Option<String> {
    hash(generated(files, copies).collect())
}

pub fn root_on_disk(root_dir: &Path, paths: &[PathBuf]) -> Option<String> {
    let manifest = Path::new(layout::GENERATED).join(FILE);
    let mut listed: Vec<Entry> = Vec::new();
    for path in paths {
        let rel = path.strip_prefix(root_dir).ok()?;
        if rel == manifest {
            continue;
        }
        let meta = std::fs::symlink_metadata(path).ok()?;
        let mode = meta.permissions().mode() & 0o7777;
        if meta.file_type().is_symlink() {
            listed.push(Entry {
                path: rel.to_path_buf(),
                mode,
                link: Some(std::fs::read_link(path).ok()?),
                bytes: Vec::new(),
            });
        } else {
            listed.push(Entry {
                path: rel.to_path_buf(),
                mode,
                link: None,
                bytes: std::fs::read(path).ok()?,
            });
        }
    }
    hash(listed)
}

struct Entry {
    path: PathBuf,
    mode: u32,
    link: Option<PathBuf>,
    bytes: Vec<u8>,
}

fn generated<'a>(
    files: &'a [(PathBuf, String)],
    copies: &'a [CopyFile],
) -> impl Iterator<Item = Entry> + 'a {
    let manifest = path();
    let scripts = Path::new(layout::GENERATED).join(layout::SCRIPTS);
    files
        .iter()
        .filter(move |(at, _)| at.starts_with(layout::GENERATED) && at != &manifest)
        .map(move |(at, text)| Entry {
            path: at.clone(),
            mode: if at.starts_with(&scripts) {
                0o755
            } else {
                0o644
            },
            link: None,
            bytes: text.as_bytes().to_vec(),
        })
        .chain(copies.iter().map(|copy| Entry {
            path: copy.path.clone(),
            mode: copy.mode & 0o7777,
            link: copy.link.clone(),
            bytes: copy.bytes.clone(),
        }))
}

fn hash(mut listed: Vec<Entry>) -> Option<String> {
    listed.sort_by(|a, b| a.path.cmp(&b.path));
    let mut bytes = Vec::new();
    for file in listed {
        bytes.extend_from_slice(file.path.to_string_lossy().as_bytes());
        bytes.push(0);
        bytes.extend_from_slice(&file.mode.to_le_bytes());
        match &file.link {
            Some(target) => {
                bytes.push(b'L');
                bytes.extend_from_slice(target.to_string_lossy().as_bytes());
            }
            None => {
                bytes.push(b'F');
                bytes.extend_from_slice(&(file.bytes.len() as u64).to_le_bytes());
                bytes.extend_from_slice(&file.bytes);
            }
        }
    }
    crate::runtime::sha256_stdin(&bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn copy(path: &str, body: &str) -> CopyFile {
        CopyFile {
            path: PathBuf::from(path),
            bytes: body.as_bytes().to_vec(),
            mode: 0o644,
            link: None,
        }
    }

    #[test]
    fn the_root_covers_a_mode_and_a_link() {
        let one = vec![copy("generated/one/component.sh", "true\n")];
        assert_eq!(root(&[], &one), root(&[], &one));
        let mut other = vec![copy("generated/one/component.sh", "true\n")];
        other[0].mode = 0o755;
        assert_ne!(root(&[], &one), root(&[], &other));
        let mut link = vec![copy("generated/one/link", "")];
        link[0].link = Some(PathBuf::from("component.sh"));
        assert_ne!(root(&[], &one), root(&[], &link));
    }

    #[test]
    fn the_root_changes_with_a_byte_and_with_a_path() {
        let files = vec![(
            PathBuf::from("generated/one/Containerfile"),
            String::from("FROM scratch\n"),
        )];
        let one = root(&files, &[]);
        assert_eq!(one, root(&files, &[]));
        let changed = vec![(
            PathBuf::from("generated/one/Containerfile"),
            String::from("FROM other\n"),
        )];
        assert_ne!(one, root(&changed, &[]));
        let moved = vec![(
            PathBuf::from("generated/two/Containerfile"),
            String::from("FROM scratch\n"),
        )];
        assert_ne!(one, root(&moved, &[]));
        let copied = vec![copy("generated/one/components/a/component.sh", "true\n")];
        assert_ne!(one, root(&files, &copied));
    }
}
