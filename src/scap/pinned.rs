use crate::layout;
use crate::model::image::Image;
use std::io::Read as _;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

pub(crate) const MOUNT: &str = "/ctx/scap";

const DOWNLOADS: &str = ".cache/scap";

const LIMIT: u64 = 256 * 1024 * 1024;

const ARCHIVE_LIMIT: u64 = 1024 * 1024 * 1024;

pub(crate) struct Pinned {
    pub file: String,
    pub bytes: Vec<u8>,
    pub url: String,
}

pub struct Measured {
    pub file: String,
    pub claimed: Vec<String>,
}

pub(crate) fn resolve(
    image: &Image,
    root: &Path,
) -> Result<Option<(Measured, Vec<crate::emit::resolved::CopyFile>)>, String> {
    let Some(pinned) = fetch(image, root)? else {
        return Ok(None);
    };
    let content = super::Content::read(&String::from_utf8_lossy(&pinned.bytes))
        .map_err(|err| format!("{}: {err}", pinned.file))?;
    let (claims, _) = super::exclusions(image);
    let mut claimed = Vec::new();
    let mut unreached = Vec::new();
    for number in &claims {
        match content.rules.get(number) {
            Some(rule) => claimed.push(super::rule_name(rule).to_string()),
            None => unreached.push(number.as_str()),
        }
    }
    if !unreached.is_empty() {
        return Err(format!(
            "{} reaches no rule in {}, which is the content `{}` is remediated and measured with",
            unreached.join(", "),
            pinned.file,
            image.id
        ));
    }
    let copies = vec![
        crate::emit::resolved::CopyFile {
            path: dir(image).join(&pinned.file),
            bytes: pinned.bytes,
            mode: 0o644,
            link: None,
        },
        crate::emit::resolved::CopyFile {
            path: dir(image).join(source(&pinned.file)),
            bytes: format!("{}\n", pinned.url).into_bytes(),
            mode: 0o644,
            link: None,
        },
    ];
    Ok(Some((
        Measured {
            file: pinned.file,
            claimed,
        },
        copies,
    )))
}

fn source(file: &str) -> String {
    format!("{file}.source")
}

pub(crate) fn dir(image: &Image) -> PathBuf {
    layout::generated_image(&image.id).join("scap")
}

pub(crate) fn fetch(image: &Image, root: &Path) -> Result<Option<Pinned>, String> {
    if !image.declares_conforms() {
        return Ok(None);
    }
    let Some(base) = &image.base else {
        return Ok(None);
    };
    let Some(pin) = &base.scap_pin else {
        return Err(unpinned(image));
    };
    let file = base.scap_content.clone();
    let named = Path::new(&file);
    if named
        .file_name()
        .is_none_or(|name| name != named.as_os_str())
    {
        return Err(format!(
            "`{}` names `{file}` as its SCAP content, which is not a bare filename",
            base.path
        ));
    }
    let (Some(sha256), Some(url)) = (pin.sha256.as_deref(), pin.url_resolved()) else {
        return Err(format!(
            "the `scap-content` pin of `{}` needs a `url` and a `sha256`",
            base.path
        ));
    };

    let committed = root.join(dir(image));
    let recorded = std::fs::read_to_string(committed.join(source(&file))).unwrap_or_default();
    if recorded.trim_end() == url {
        if let Ok(bytes) = std::fs::read(committed.join(&file)) {
            if crate::runtime::match_sha256_bytes(&bytes, sha256, "").is_ok() {
                return Ok(Some(Pinned { file, bytes, url }));
            }
        }
    }

    let bytes = match (url.starts_with("https://"), url.contains("://")) {
        (true, _) => downloaded(root, &url, &file)?,
        (false, false) => read(&inside(root, &url)?, &url, &file).map_err(|err| {
            format!(
                "{err}, which the `scap-content` pin of `{}` names",
                base.path
            )
        })?,
        (false, true) => {
            return Err(format!(
                "the `scap-content` pin of `{}` names {url}; a datastream downloads over https, or names a path inside the repository",
                base.path
            ))
        }
    };
    crate::runtime::match_sha256_bytes(
        &bytes,
        sha256,
        &format!(
            "{file} from {url} is not the datastream `{}` pins",
            base.path
        ),
    )?;
    Ok(Some(Pinned { file, bytes, url }))
}

pub(crate) fn unpinned(image: &Image) -> String {
    format!(
        "`{}` conforms, and its base pins no SCAP content to remediate and measure it with",
        image.id
    )
}

fn inside(root: &Path, url: &str) -> Result<PathBuf, String> {
    let leaves =
        || format!("`{url}` leaves the repository; a pin without a scheme names a path inside it");
    let path = Path::new(url);
    if !path
        .components()
        .all(|part| matches!(part, Component::Normal(_)))
    {
        return Err(leaves());
    }
    let at = root
        .join(path)
        .canonicalize()
        .map_err(|err| format!("{url}: {err}"))?;
    let repository = root
        .canonicalize()
        .map_err(|err| format!("{}: {err}", root.display()))?;
    match at.starts_with(&repository) {
        true => Ok(at),
        false => Err(leaves()),
    }
}

fn downloaded(root: &Path, url: &str, file: &str) -> Result<Vec<u8>, String> {
    let name = url.rsplit('/').next().unwrap_or("download");
    let at = root
        .join(DOWNLOADS)
        .join(format!(".{}.{name}", std::process::id()));
    let archive = crate::init::Scratch(at);
    crate::runtime::verified(url, None, &archive.0, Some(ARCHIVE_LIMIT))?;
    read(&archive.0, url, file)
}

fn read(at: &Path, url: &str, file: &str) -> Result<Vec<u8>, String> {
    if url.ends_with(".zip") {
        return member(at, file);
    }
    let size = std::fs::metadata(at)
        .map_err(|err| format!("{url}: {err}"))?
        .len();
    if size > LIMIT {
        return Err(too_large(url, size));
    }
    std::fs::read(at).map_err(|err| format!("{url}: {err}"))
}

fn too_large(what: &str, size: u64) -> String {
    format!("{what} is {size} bytes, which is larger than any SCAP datastream")
}

fn member(archive: &Path, file: &str) -> Result<Vec<u8>, String> {
    member_within(archive, file, LIMIT)
}

fn member_within(archive: &Path, file: &str, limit: u64) -> Result<Vec<u8>, String> {
    let listed = Command::new("unzip")
        .arg("-Z1")
        .arg(archive)
        .output()
        .map_err(|err| format!("unzip: {err}; install unzip to read a zipped datastream"))?;
    if !listed.status.success() {
        return Err(format!(
            "unzip -Z1 {}: {}",
            archive.display(),
            listed.status
        ));
    }
    let names = String::from_utf8_lossy(&listed.stdout);
    let found: Vec<&str> = names
        .lines()
        .filter(|entry| {
            Path::new(entry)
                .file_name()
                .is_some_and(|name| name == file)
        })
        .collect();
    let entry = match found.as_slice() {
        [entry] => *entry,
        [] => return Err(format!("{} holds no {file}", archive.display())),
        _ => return Err(format!("{} holds {file} more than once", archive.display())),
    };
    let mut child = Command::new("unzip")
        .arg("-p")
        .arg(archive)
        .arg(entry)
        .stdout(std::process::Stdio::piped())
        .spawn()
        .map_err(|err| format!("unzip: {err}"))?;
    let mut bytes = Vec::new();
    let read = child
        .stdout
        .take()
        .ok_or("unzip: no stdout")?
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|err| format!("unzip -p {} {entry}: {err}", archive.display()));
    if bytes.len() as u64 > limit {
        if let Err(err) = child.kill() {
            eprintln!(
                "buildc: unzip -p {} {entry} did not stop: {err}",
                archive.display()
            );
        }
        child
            .wait()
            .map_err(|err| format!("unzip -p {} {entry}: {err}", archive.display()))?;
        return Err(format!("{entry} is larger than any SCAP datastream"));
    }
    let status = child
        .wait()
        .map_err(|err| format!("unzip -p {} {entry}: {err}", archive.display()))?;
    read?;
    match status.success() {
        true => Ok(bytes),
        false => Err(format!("unzip -p {} {entry}: {status}", archive.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_member_larger_than_the_cap_is_refused_while_it_is_read() {
        let archive = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/scap/datastream.zip");
        let whole = member_within(&archive, "ssg-fedora-ds.xml", LIMIT).expect("the member reads");
        let capped = member_within(&archive, "ssg-fedora-ds.xml", 10);
        assert!(whole.len() > 10);
        assert!(capped.is_err_and(|err| err.contains("larger than any SCAP datastream")));
    }

    #[test]
    fn a_path_pin_cannot_leave_the_repository() {
        let root = std::env::temp_dir().join(format!("buildc-inside-{}", std::process::id()));
        std::fs::create_dir_all(root.join("scap")).expect("the scratch repository is made");
        std::fs::write(root.join("scap/ds.xml"), "").expect("the datastream is written");
        let outside = root.with_extension("outside.xml");
        std::fs::write(&outside, "").expect("the file outside is written");
        std::os::unix::fs::symlink(&outside, root.join("scap/host.xml")).expect("the link is made");
        let inside_answer = inside(&root, "scap/ds.xml");
        let linked = inside(&root, "scap/host.xml");
        let up = inside(&root, "../ds.xml");
        let absolute = inside(&root, "/etc/ds.xml");
        std::fs::remove_dir_all(&root).expect("the scratch repository goes");
        std::fs::remove_file(&outside).expect("the file outside goes");
        assert!(inside_answer.is_ok_and(|at| at.ends_with("scap/ds.xml")));
        assert!(linked.is_err());
        assert!(up.is_err());
        assert!(absolute.is_err());
    }
}
