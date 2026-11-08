use crate::model::version::Version;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

type Matches = fn(&str) -> bool;

pub const VERIFY_CLASSES: [(&str, Matches); 2] = [
    ("mount-not-found", mount_not_found),
    ("man-page-missing", man_page_missing),
];

fn mount_not_found(line: &str) -> bool {
    line.contains("Failed to create ")
        && line.contains(": Unit ")
        && (line.ends_with(".mount not found.") || line.ends_with(".swap not found."))
}

fn man_page_missing(line: &str) -> bool {
    let digits = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
    if let Some(status) = line
        .strip_prefix("'(man)' failed with exit status ")
        .and_then(|rest| rest.strip_suffix('.'))
    {
        return digits(status);
    }
    if line.contains(": Can't show ") {
        return true;
    }
    let Some((_, rest)) = line.split_once("Command 'man ") else {
        return false;
    };
    let Some((page, code)) = rest.split_once("' failed with code ") else {
        return false;
    };
    !page.contains('\'') && digits(code)
}

fn about_another_unit(line: &str, unit: &str) -> bool {
    const TYPES: [&str; 11] = [
        ".service",
        ".socket",
        ".target",
        ".mount",
        ".automount",
        ".swap",
        ".timer",
        ".path",
        ".slice",
        ".scope",
        ".device",
    ];
    let mut fields = line.splitn(3, ':');
    let (Some(path), Some(number), Some(_)) = (fields.next(), fields.next(), fields.next()) else {
        return false;
    };
    let name = path.rsplit('/').next().unwrap_or_default();
    // A unit instance carries its template's faults, so a line naming the template is this unit's own.
    let template = match (unit.split_once('@'), unit.rfind('.')) {
        (Some((prefix, _)), Some(dot)) => format!("{prefix}@{}", &unit[dot..]),
        _ => unit.to_string(),
    };
    path.starts_with('/')
        && !number.is_empty()
        && number.chars().all(|c| c.is_ascii_digit())
        && TYPES.iter().any(|t| name.ends_with(t))
        && name != unit
        && name != template
}

fn unexpected<'a>(text: &'a str, unit: &str, allowed: &[&str]) -> Vec<&'a str> {
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let own: Vec<&str> = lines
        .iter()
        .copied()
        .filter(|line| !about_another_unit(line, unit))
        .collect();
    // A failed verify cannot read as a pass, so the unfiltered text stands when no line names this unit.
    if own.is_empty() { lines } else { own }
        .into_iter()
        .filter(|line| !allowed.iter().any(|class| classify(line) == Some(*class)))
        .collect()
}

fn found<'a>(token: &'a str, links: &[Link]) -> Option<&'a str> {
    token
        .split('|')
        .find(|path| resolve(Path::new(path), links).is_some_and(|at| at.exists()))
}

const RELOCATIONS: [&str; 2] = [
    "/usr/lib/tmpfiles.d/zz-opt-symlinks.conf",
    "/usr/lib/tmpfiles.d/zz-usrlocal-symlinks.conf",
];

struct Link {
    path: PathBuf,
    target: PathBuf,
    replaces: bool,
}

fn links(text: &str) -> Vec<Link> {
    text.lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let kind = fields.next()?;
            let path = fields.next()?;
            // tmpfiles.d puts the link target in the seventh field.
            let target = fields.nth(4)?;
            (kind == "L" || kind == "L+").then(|| Link {
                path: unescape(path),
                target: unescape(target),
                replaces: kind == "L+",
            })
        })
        .collect()
}

fn unescape(field: &str) -> PathBuf {
    use std::os::unix::ffi::OsStringExt as _;
    let bytes = field.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&byte) = bytes.get(i) {
        let hex = bytes
            .get(i + 2..i + 4)
            .and_then(|digits| std::str::from_utf8(digits).ok())
            .and_then(|digits| u8::from_str_radix(digits, 16).ok());
        match (byte, bytes.get(i + 1), hex) {
            (b'\\', Some(b'x'), Some(decoded)) => {
                out.push(decoded);
                i += 4;
            }
            (b'%', Some(b'%'), _) => {
                out.push(b'%');
                i += 2;
            }
            _ => {
                out.push(byte);
                i += 1;
            }
        }
    }
    PathBuf::from(std::ffi::OsString::from_vec(out))
}

fn resolve(path: &Path, links: &[Link]) -> Option<PathBuf> {
    const MAX_HOPS: usize = 40;
    let parts = |path: &Path| -> Vec<PathBuf> {
        path.components()
            .rev()
            .map(|part| PathBuf::from(part.as_os_str()))
            .collect()
    };
    let mut done = PathBuf::from("/");
    let mut rest = parts(path);
    let mut hops = 0;
    while let Some(part) = rest.pop() {
        if part == Path::new("..") {
            done.pop();
            continue;
        }
        if part == Path::new(".") {
            continue;
        }
        let next = done.join(&part);
        let mapped = links.iter().find(|link| link.path == next);
        let target = match mapped {
            Some(link) if link.replaces || fs::symlink_metadata(&next).is_err() => {
                Some(link.target.clone())
            }
            _ => fs::read_link(&next).ok(),
        };
        match target {
            Some(target) => {
                hops += 1;
                if hops > MAX_HOPS {
                    return None;
                }
                rest.extend(parts(&target));
            }
            None => done = next,
        }
    }
    Some(done)
}

pub fn class_names() -> String {
    VERIFY_CLASSES
        .iter()
        .map(|(name, _)| *name)
        .collect::<Vec<_>>()
        .join(", ")
}

fn classify(line: &str) -> Option<&'static str> {
    VERIFY_CLASSES
        .iter()
        .find(|(_, matches)| matches(line))
        .map(|(name, _)| *name)
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

const FETCH_USAGE: &str = "\
usage: buildc fetch <what> <url> <sha256> [target] [extra...]

  file    <url> <sha256> <path>          verify and keep the download
  tree    <url> <sha256> <dir> [args..]  unpack it, extra args reaching tar
  bin     <url> <sha256> <name> [inner]  install one executable to /usr/bin
  rpm     <url> <sha256>                 install the package, on a rpm family
  deb     <url> <sha256>                 install the package, on a deb family";

pub fn fetch(args: &[&str]) -> Result<(), String> {
    let (what, url, sha256, rest) = match args {
        [what, url, sha256, rest @ ..] => (*what, *url, *sha256, rest),
        _ => return Err(FETCH_USAGE.to_string()),
    };

    match (what, rest) {
        ("file", [path]) => verified(url, Some(sha256), Path::new(path), None),
        ("tree", [dir, extra @ ..]) => extract(url, Some(sha256), Path::new(dir), extra),
        ("bin", [name]) => install_bin(url, sha256, name, name),
        ("bin", [name, inner]) => install_bin(url, sha256, name, inner),
        ("rpm", []) => {
            let rpm = crate::init::Scratch(scratch(url));
            verified(url, Some(sha256), &rpm.0, None)?;
            run("dnf", &["install", "-y", &rpm.0.to_string_lossy()])?
        }
        ("deb", []) => {
            let deb = crate::init::Scratch(scratch(url));
            verified(url, Some(sha256), &deb.0, None)?;
            run(
                "apt-get",
                &[
                    "install",
                    "-y",
                    "--no-install-recommends",
                    &deb.0.to_string_lossy(),
                ],
            )?
        }
        _ => Err(FETCH_USAGE.to_string()),
    }
}

fn scratch(url: &str) -> PathBuf {
    let name = url.rsplit('/').next().unwrap_or("download");
    PathBuf::from(format!("/tmp/fetch.{}.{name}", std::process::id()))
}

fn run(program: &str, args: &[&str]) -> Result<Result<(), String>, String> {
    let status = Command::new(program)
        .args(args)
        .status()
        .map_err(|err| format!("{program}: {err}"))?;
    Ok(if status.success() {
        Ok(())
    } else {
        Err(format!("{program} {}: {status}", args.join(" ")))
    })
}

pub(crate) fn verified(
    url: &str,
    sha256: Option<&str>,
    dest: &Path,
    max: Option<u64>,
) -> Result<(), String> {
    if let Some(dir) = dest.parent().filter(|d| !d.as_os_str().is_empty()) {
        fs::create_dir_all(dir).map_err(|err| format!("{}: {err}", dir.display()))?;
    }
    let mut curl = Command::new("curl");
    if let Some(max) = max {
        curl.args(["--max-filesize", &max.to_string()]);
    }
    let said = curl
        .args(["--retry", "3", "-fsSLo", &dest.to_string_lossy(), url])
        .output()
        .map_err(|err| format!("curl: {err}"))?;
    if !said.status.success() {
        let why = String::from_utf8_lossy(&said.stderr);
        let why = why.trim();
        return Err(match why.is_empty() {
            true => format!("cannot fetch {url}: curl {}", said.status),
            false => format!("cannot fetch {url}: {why}"),
        });
    }

    let Some(sha256) = sha256 else {
        return Ok(());
    };
    // A payload that does not match its hash is removed, so no later step can mistake it for verified content.
    match_sha256(dest, sha256, url).inspect_err(|_| crate::init::tidy(dest))
}

pub(crate) fn match_sha256(path: &Path, expected: &str, what: &str) -> Result<(), String> {
    agree(&sha256_file(path)?, expected, what)
}

pub(crate) fn match_sha256_bytes(bytes: &[u8], expected: &str, what: &str) -> Result<(), String> {
    agree(&sha256_stdin(bytes)?, expected, what)
}

fn agree(got: &str, expected: &str, what: &str) -> Result<(), String> {
    match got == expected.to_lowercase() {
        true => Ok(()),
        false => Err(format!(
            "{what}\n  expected sha256 {expected}\n  got      sha256 {got}"
        )),
    }
}

pub(crate) fn extract(
    url: &str,
    sha256: Option<&str>,
    dir: &Path,
    extra: &[&str],
) -> Result<(), String> {
    let archive = crate::init::Scratch(scratch(url));
    let archive = &archive.0;
    verified(url, sha256, archive, None)?;
    fs::create_dir_all(dir).map_err(|err| format!("{}: {err}", dir.display()))?;

    let path = archive.to_string_lossy().into_owned();
    let target = dir.to_string_lossy().into_owned();
    let (program, mut args) = if url.ends_with(".zip") || url.ends_with(".plasmoid") {
        ("unzip", vec!["-q", &path, "-d", &target])
    } else if url.ends_with(".tar.zst") {
        (
            "tar",
            vec!["--use-compress-program=zstd", "-xf", &path, "-C", &target],
        )
    } else {
        ("tar", vec!["-xf", &path, "-C", &target])
    };
    args.extend_from_slice(extra);

    run(program, &args)?
}

fn install_bin(url: &str, sha256: &str, name: &str, inner: &str) -> Result<(), String> {
    let dest = PathBuf::from("/usr/bin").join(name);
    let tmp = crate::init::Scratch(PathBuf::from(format!(
        "/tmp/fetch-bin.{}.{name}",
        std::process::id()
    )));
    let tmp = &tmp.0;

    if [".zip", ".tar.gz", ".tgz", ".tar.xz", ".tar.zst"]
        .iter()
        .any(|ext| url.ends_with(ext))
    {
        extract(url, Some(sha256), tmp, &[])?;
        install(&tmp.join(inner), &dest)?;
    } else {
        verified(url, Some(sha256), tmp, None)?;
        install(tmp, &dest)?;
    }
    Ok(())
}

fn install(from: &Path, to: &Path) -> Result<(), String> {
    fs::copy(from, to).map_err(|err| format!("{} to {}: {err}", from.display(), to.display()))?;
    fs::set_permissions(to, fs::Permissions::from_mode(0o755))
        .map_err(|err| format!("{}: {err}", to.display()))
}

pub fn sha256_tree(dir: &Path, files: &[String]) -> Result<String, String> {
    let out = Command::new("sha256sum")
        .args(files)
        .current_dir(dir)
        .output()
        .map_err(|err| format!("sha256sum: {err}"))?;
    if !out.status.success() {
        return Err(format!(
            "sha256sum in {}: {}{}",
            dir.display(),
            out.status,
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    sha256_stdin(&out.stdout)
}

pub(crate) fn sha256_stdin(bytes: &[u8]) -> Result<String, String> {
    use std::io::Write as _;

    let mut child = Command::new("sha256sum")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .map_err(|err| format!("sha256sum: {err}"))?;
    child
        .stdin
        .take()
        .ok_or("sha256sum: no stdin")?
        .write_all(bytes)
        .map_err(|err| format!("sha256sum: {err}"))?;
    let out = child
        .wait_with_output()
        .map_err(|err| format!("sha256sum: {err}"))?;
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .next()
        .filter(|hex| hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit()))
        .map(str::to_lowercase)
        .ok_or_else(|| "sha256sum: no hash in its output".to_string())
}

pub(crate) fn sha256_file(path: &Path) -> Result<String, String> {
    let out = Command::new("sha256sum")
        .arg(path)
        .output()
        .map_err(|err| format!("sha256sum: {err}"))?;
    if !out.status.success() {
        return Err(format!(
            "sha256sum {}: {}{}",
            path.display(),
            out.status,
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .next()
        .filter(|hex| hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit()))
        .map(str::to_lowercase)
        .ok_or_else(|| format!("sha256sum {}: no hash in its output", path.display()))
}

fn kernel_version() -> Option<String> {
    match env("FAMILY").as_deref() {
        Some("debian" | "ubuntu") => modules_dir(),
        _ => {
            let package = fs::read_to_string("/usr/lib/kernel-build/kernel-package").map_or_else(
                |_| "kernel-core".to_string(),
                |text| text.trim().to_string(),
            );
            match output(
                "rpm",
                &["-q", "--qf", "%{VERSION}-%{RELEASE}.%{ARCH}", &package],
            ) {
                Ok((true, version)) if !version.trim().is_empty() => Some(version.trim().into()),
                _ => modules_dir(),
            }
        }
    }
}

fn modules_dir() -> Option<String> {
    let mut found: Vec<String> = fs::read_dir("/usr/lib/modules")
        .ok()?
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    found.sort();
    match found.as_slice() {
        [one] => Some(one.clone()),
        _ => None,
    }
}

fn initramfs() -> Option<PathBuf> {
    let version = kernel_version();
    version
        .iter()
        .flat_map(|version| {
            [
                PathBuf::from(format!("/usr/lib/modules/{version}/initramfs.img")),
                PathBuf::from(format!("/kernel/{version}/initramfs.img")),
            ]
        })
        .find(|path| path.is_file())
}

fn opens_luks(listing: &str) -> bool {
    const BINARIES: [&str; 4] = [
        "usr/bin/systemd-cryptsetup",
        "usr/lib/systemd/systemd-cryptsetup",
        "usr/bin/cryptsetup",
        "usr/sbin/cryptsetup",
    ];
    // A symlink can dangle, so only a regular archive entry proves the binary is present.
    listing
        .lines()
        .filter(|line| !line.contains(" -> "))
        .any(|line| BINARIES.iter().any(|path| line.ends_with(path)))
}

pub(crate) const COMPONENT_PRESET: &str = "45-component-";

struct Report {
    failures: usize,
}

impl Report {
    fn fail(&mut self, message: impl AsRef<str>) {
        eprintln!("FAIL: {}", message.as_ref());
        self.failures += 1;
    }
}

fn output(program: &str, args: &[&str]) -> Result<(bool, String), String> {
    let out = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|err| format!("{program}: {err}"))?;
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    Ok((out.status.success(), text))
}

fn section_bytes(uki: &str, name: &str) -> Option<Vec<u8>> {
    let at = crate::init::Scratch(std::env::temp_dir().join(format!("buildc-{name}")));
    let at = &at.0;
    let out = Command::new("objcopy")
        .args(["-O", "binary", "--only-section", name])
        .arg(uki)
        .arg(at)
        .stdin(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    fs::read(at).ok()
}

fn signed_by(path: &Path, cert: &str, report: &mut Report) {
    let name = path.display().to_string();
    match output("sbverify", &["--cert", cert, &name]) {
        Ok((true, _)) => println!("    {name} signed by the owner certificate"),
        Ok((_, text)) => report.fail(format!(
            "{name}: owner signature is invalid: {}",
            text.trim()
        )),
        Err(message) => report.fail(message),
    }
}

fn validate_boot_chain(report: &mut Report) {
    println!("==> owner-signed UKI boot chain");
    let uki_dir = Path::new("/boot/EFI/Linux");
    let mut ukis: Vec<PathBuf> = fs::read_dir(uki_dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && path.extension().is_some_and(|ext| ext == "efi"))
        .collect();
    ukis.sort();
    if ukis.is_empty() {
        report.fail(format!("{} carries no UKI", uki_dir.display()));
    }
    if Path::new("/usr/lib/efi/grub2").exists() {
        report.fail("/usr/lib/efi/grub2: GRUB payload remains in a UKI image");
    }
    let pcr_key = fs::read(PCR_POLICY).ok().filter(|bytes| !bytes.is_empty());
    for uki in &ukis {
        let name = uki.display().to_string();
        match output("objdump", &["--section-headers", &name]) {
            Ok((true, text)) => {
                if text.split_whitespace().any(|field| field == ".initrd") {
                    println!("    {name} carries .initrd");
                } else {
                    report.fail(format!("{name}: UKI carries no .initrd section"));
                }
                if let Some(expected) = &pcr_key {
                    for section in [".pcrpkey", ".pcrsig"] {
                        if !text.split_whitespace().any(|field| field == section) {
                            report.fail(format!(
                                "{name}: carries no {section}, and this image declares a PCR policy"
                            ));
                        }
                    }
                    match section_bytes(&name, ".pcrpkey") {
                        Some(bytes) if &bytes == expected => {
                            println!("    {name} embeds the declared PCR policy key")
                        }
                        Some(_) => report.fail(format!("{name}: .pcrpkey is not {PCR_POLICY}")),
                        None => report.fail(format!("{name}: .pcrpkey could not be read")),
                    }
                }
            }
            Ok((_, _)) => report.fail(format!("{name}: UKI carries no .initrd section")),
            Err(message) => report.fail(message),
        }
    }

    let recorded =
        fs::read_to_string("/usr/share/buildc/composefs-sealer-version").unwrap_or_default();
    match output("bootc", &["--version"]) {
        Ok((true, actual)) if actual.trim() == recorded.trim() => {
            println!("    sealed with {}", recorded.trim())
        }
        Ok((_, actual)) => report.fail(format!(
            "composefs sealer version moved: recorded `{}`, image carries `{}`",
            recorded.trim(),
            actual.trim()
        )),
        Err(message) => report.fail(message),
    }
    let signed = Path::new(SIGNED_MARKER).exists();
    match fs::read_to_string("/usr/share/buildc/boot-chain")
        .unwrap_or_default()
        .trim()
    {
        "uki-shim" if !Path::new("/usr/libexec/secureboot-enrolment").is_file() => {
            report.fail("/usr/libexec/secureboot-enrolment: shim installer is missing")
        }
        "uki-db" if signed => {
            for name in ["PK", "KEK", "db"] {
                let path = format!("/usr/lib/bootc/install/secureboot-keys/auto/{name}.auth");
                if !Path::new(&path).is_file() {
                    report.fail(format!("{path}: firmware enrollment file is missing"));
                }
            }
        }
        "uki-shim" | "uki-db" => {}
        chain => report.fail(format!("unknown recorded UKI boot chain `{chain}`")),
    }

    if !signed {
        println!("    no {SIGNED_MARKER}: signature checks skipped");
        return;
    }
    let cert = "/usr/share/secureboot/sb_cert.pem";
    signed_by(
        Path::new("/usr/lib/systemd/boot/efi/systemd-bootx64.efi"),
        cert,
        report,
    );
    for uki in ukis {
        signed_by(&uki, cert, report);
    }
}

const SIGNED_MARKER: &str = "/usr/share/secureboot/signed";
const PCR_POLICY: &str = "/usr/share/secureboot/pcr-policy.pem";

fn package_version(family: &str, package: &str) -> Option<String> {
    let (program, args): (&str, Vec<&str>) = match family {
        "fedora" | "rhel" => ("rpm", vec!["-q", "--qf", "%{VERSION}", "--", package]),
        "debian" | "ubuntu" => ("dpkg-query", vec!["-W", "-f=${Version}", "--", package]),
        _ => return None,
    };
    let (true, text) = output(program, &args).ok()? else {
        return None;
    };
    let version = upstream_version(family, &text);
    (!version.is_empty()).then_some(version)
}

fn upstream_version(family: &str, text: &str) -> String {
    // A Debian package version carries an epoch and a revision that the upstream version does not. The comparison drops both.
    let version = text.trim();
    match family {
        "debian" | "ubuntu" => {
            let version = version.split_once(':').map_or(version, |(_, rest)| rest);
            version
                .rsplit_once('-')
                .map_or(version, |(upstream, _)| upstream)
                .to_string()
        }
        _ => version.to_string(),
    }
}

fn check_versions(report: &mut Report) {
    let versioned = env("VERSIONED").unwrap_or_default();
    let floors = env("FLOORS").unwrap_or_default();
    if versioned.is_empty() && floors.is_empty() {
        return;
    }
    println!("==> capability versions");
    let family = env("FAMILY").unwrap_or_default();
    let mut measured: Vec<(String, String, String)> = Vec::new();
    for entry in versioned.split_whitespace() {
        let fields: Vec<&str> = entry.split('|').collect();
        let [component, name, package] = fields[..] else {
            report.fail(format!("`{entry}` is not a capability version entry"));
            continue;
        };
        match package_version(&family, package) {
            Some(version) => {
                println!("    {component} {name} {version}");
                measured.push((component.to_string(), name.to_string(), version));
            }
            None => report.fail(format!(
                "`{component}` provides `{name}` from `{package}`. The package database has no such package."
            )),
        }
    }
    for entry in floors.split_whitespace() {
        let fields: Vec<&str> = entry.split('|').collect();
        let [name, minimum, owner] = fields[..] else {
            report.fail(format!("`{entry}` is not a version floor"));
            continue;
        };
        let Some(minimum) = Version::parse(minimum) else {
            report.fail(format!("`{minimum}` is not a dotted version"));
            continue;
        };
        if let Err(message) = check_floor(&measured, name, &minimum, owner) {
            report.fail(message);
        }
    }
}

fn check_floor(
    measured: &[(String, String, String)],
    name: &str,
    minimum: &Version,
    owner: &str,
) -> Result<(), String> {
    let mut best: Option<(&str, &str, Version)> = None;
    let mut found = false;
    for (component, provided, version) in measured {
        if !crate::resolve::meets(provided, name) {
            continue;
        }
        found = true;
        let version = Version::parse(version).ok_or_else(|| {
            format!(
                "`{component}` provides `{provided}` at {version}. That is not a dotted number. The floor {minimum} that `{owner}` requires cannot be compared."
            )
        })?;
        if best.as_ref().is_none_or(|(_, _, held)| version > *held) {
            best = Some((component, provided, version));
        }
    }
    match (found, best) {
        (false, _) => Err(format!(
            "no measured provider of `{name}` exists. `{owner}` requires {minimum} or later."
        )),
        (true, Some((component, provided, version))) if version < *minimum => Err(format!(
            "`{component}` provides `{provided}` at {version}. `{owner}` requires {minimum} or later."
        )),
        _ => Ok(()),
    }
}

fn check_contracts(contracts: &str, relocated: &[Link], report: &mut Report) {
    if contracts.trim().is_empty() {
        println!("    (none declared)");
        return;
    }
    for token in contracts.split_whitespace() {
        if let Some(removed) = token.strip_prefix('!') {
            match found(removed, relocated) {
                Some(path) => report.fail(format!(
                    "{path}: a component removes this capability, and the image still holds its witness"
                )),
                None => println!("    {} absent ok", removed.replace('|', " and ")),
            }
            continue;
        }
        match found(token, relocated) {
            Some(path) => println!("    {path} ok"),
            None => report.fail(format!(
                "{}: the manifest declares it, the image does not have it",
                token.replace('|', " or ")
            )),
        }
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "the checks run and print in the order the build log shows them"
)]
pub fn validate_image() -> Result<(), String> {
    let mut report = Report { failures: 0 };

    println!("==> bootc install print-configuration");
    match output("bootc", &["install", "print-configuration"]) {
        Ok((true, _)) => println!("    ok"),
        _ => report.fail("bootc install print-configuration failed to parse"),
    }

    println!("==> initramfs");
    let initramfs = initramfs();
    if Path::new("/usr/share/buildc/composefs-sealer-version").is_file() {
        println!("    embedded in the UKI; checked below");
    } else {
        match &initramfs {
            Some(path) => println!("    {} present", path.display()),
            None => report.fail("cannot find the kernel's initramfs"),
        }
    }
    if env("LUKS_INITRAMFS").as_deref() == Some("true") {
        match initramfs.as_ref() {
            Some(path) => match output("lsinitrd", &[&path.display().to_string()]) {
                Ok((true, listing)) if opens_luks(&listing) => {
                    println!("    {} can open LUKS", path.display())
                }
                Ok((_, _)) => report.fail(format!(
                    "{}: the image declares luks-initramfs, but the archive carries no cryptsetup binary",
                    path.display()
                )),
                Err(message) => report.fail(message),
            },
            None => report.fail("luks-initramfs is declared, but there is no initramfs to inspect"),
        }
    }

    println!("==> relocation symlinks");
    let mut relocated: Vec<Link> = Vec::new();
    for map in RELOCATIONS {
        match fs::read_to_string(map) {
            Ok(text) => relocated.extend(links(&text)),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                println!("    (no {map})");
            }
            Err(err) => report.fail(format!("{map}: {err}")),
        }
    }
    for link in &relocated {
        let shown = format!("{} -> {}", link.path.display(), link.target.display());
        if resolve(&link.target, &relocated).is_some_and(|at| at.exists()) {
            println!("    {shown} ok");
        } else {
            report.fail(format!("{shown}: target does not exist"));
        }
    }

    println!("==> contract files");
    check_contracts(
        &env("CONTRACT_FILES").unwrap_or_default(),
        &relocated,
        &mut report,
    );

    check_versions(&mut report);

    let mut exceptions: Vec<(String, String)> = Vec::new();
    for token in env("VERIFY_EXCEPTIONS")
        .unwrap_or_default()
        .split_whitespace()
    {
        let (class, unit) = token.split_once('|').unwrap_or((token, ""));
        if VERIFY_CLASSES.iter().any(|(name, _)| *name == class) {
            exceptions.push((class.to_string(), unit.to_string()));
        } else {
            report.fail(format!(
                "allow-verify \"{class}\": not a diagnostic class this image knows; known: {}",
                class_names()
            ));
        }
    }

    println!("==> systemd unit verification");
    let mut arrived: Vec<String> = Vec::new();
    for scope in ["system", "user"] {
        let contradicts = |unit: &str, verb: &str| -> Vec<String> {
            let opposite = match verb {
                "enable" => "disable",
                _ => "enable",
            };
            presets(scope)
                .iter()
                .filter_map(|other| {
                    let text = fs::read_to_string(other).ok()?;
                    names(&text, opposite, unit).then(|| {
                        other
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned()
                    })
                })
                .collect()
        };
        let unit_dirs = [
            PathBuf::from(format!("/usr/lib/systemd/{scope}")),
            PathBuf::from(format!("/etc/systemd/{scope}")),
        ];
        for preset in presets(scope) {
            println!("    {}", preset.display());
            arrived.push(preset.display().to_string());
            let Ok(text) = fs::read_to_string(&preset) else {
                continue;
            };
            for line in text.lines() {
                let mut fields = line.split_whitespace();
                let (Some(verb), Some(unit)) = (fields.next(), fields.next()) else {
                    continue;
                };
                if verb != "enable" && verb != "disable" {
                    continue;
                }

                if !unit_dirs.iter().any(|dir| find_unit(dir, unit)) {
                    if verb == "enable" {
                        report.fail(format!(
                            "{unit}: unit file not found in {} {}",
                            unit_dirs[0].display(),
                            unit_dirs[1].display()
                        ));
                    } else {
                        println!("        {unit} (not present, {verb}d)");
                    }
                    continue;
                }

                let config_root = PathBuf::from(format!("/etc/systemd/{scope}"));
                let links = enablement_links(&config_root, unit);
                let overruled = match contradicts(unit, verb).as_slice() {
                    [] => String::new(),
                    named => format!(
                        "; {} asks the opposite and the glob order decided it",
                        named.join(" and ")
                    ),
                };
                if verb == "enable" && links.is_empty() {
                    report.fail(format!(
                        "{unit}: preset enables it, but nothing under {} does{overruled}",
                        config_root.display()
                    ));
                } else if verb == "disable" && !links.is_empty() {
                    report.fail(format!(
                        "{unit}: preset disables it, but {} still enables it: {}{overruled}",
                        config_root.display(),
                        links.join(" ")
                    ));
                }

                if scope != "system" || verb != "enable" {
                    println!("        {unit} {verb}d");
                    continue;
                }

                let verified = output("systemd-analyze", &["verify", "--no-pager", unit]);
                let (ok, text) = match verified {
                    Ok(result) => result,
                    Err(message) => {
                        report.fail(message);
                        continue;
                    }
                };
                if ok {
                    println!("        {unit} enabled");
                    continue;
                }
                if text.trim().is_empty() {
                    report.fail(format!(
                        "{unit}: systemd-analyze verify failed without saying why"
                    ));
                    continue;
                }

                let allowed: Vec<&str> = exceptions
                    .iter()
                    .filter(|(_, for_unit)| for_unit == unit)
                    .map(|(class, _)| class.as_str())
                    .collect();
                let unexpected = unexpected(&text, unit, &allowed);

                if unexpected.is_empty() {
                    println!("        {unit} enabled (verify: declared exceptions only)");
                    continue;
                }
                report.fail(format!("{unit}: systemd-analyze verify"));
                for line in unexpected {
                    eprintln!("          {line}");
                    if let Some(class) = classify(line) {
                        eprintln!(
                            "            this is the known class '{class}'. If it is expected here,"
                        );
                        eprintln!(
                            "            declare it in the component shipping {}:",
                            preset.file_name().unwrap_or_default().to_string_lossy()
                        );
                        eprintln!("              allow-verify \"{class}\" unit=\"{unit}\"");
                    }
                }
            }
        }
    }

    for message in missing_presets(&env("COMPONENT_PRESETS").unwrap_or_default(), &arrived) {
        report.fail(message);
    }

    clear_run_systemd();

    if Path::new("/usr/share/buildc/composefs-sealer-version").is_file() {
        validate_boot_chain(&mut report);
    }

    println!();
    if report.failures == 0 {
        println!("All validation checks passed.");
        Ok(())
    } else {
        Err(format!("{} validation check(s) failed.", report.failures))
    }
}

fn clear_run_systemd() {
    let path = Path::new("/run/systemd");
    if path.is_dir() {
        if let Err(err) = fs::remove_dir_all(path) {
            eprintln!("    could not remove {}: {err}", path.display());
        }
    }
}

fn missing_presets(expected: &str, arrived: &[String]) -> Vec<String> {
    expected
        .split_whitespace()
        .filter(|path| !arrived.iter().any(|have| have == path))
        .map(|path| format!("{path}: a component ships it, the image does not have it"))
        .collect()
}

fn presets(scope: &str) -> Vec<PathBuf> {
    let dir = PathBuf::from(format!("/usr/lib/systemd/{scope}-preset"));
    let mut found: Vec<PathBuf> = fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            path.is_file() && name.starts_with(COMPONENT_PRESET) && name.ends_with(".preset")
        })
        .collect();
    found.sort();
    found
}

fn names(preset: &str, verb: &str, unit: &str) -> bool {
    preset.lines().any(|line| {
        let mut fields = line.split_whitespace();
        (fields.next(), fields.next()) == (Some(verb), Some(unit))
    })
}

#[cfg(feature = "fuzzing")]
pub fn fuzz_readers(text: &str) {
    for line in text.lines() {
        let _ = mount_not_found(line);
        let _ = man_page_missing(line);
        let _ = about_another_unit(line, "fuzz.service");
        let _ = classify(line);
    }
    let _ = unexpected(text, "fuzz.service", &["mount-not-found"]);
    let _ = found(text, &links(text));
    let _ = opens_luks(text);
    let _ = missing_presets(text, &["fuzz.service".to_string()]);
    let _ = names(text, "enable", "fuzz.service");
}

fn find_unit(dir: &Path, unit: &str) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            if find_unit(&path, unit) {
                return true;
            }
        } else if path.file_name().is_some_and(|name| name == unit) && path.is_file() {
            return true;
        }
    }
    false
}

fn enablement_links(root: &Path, unit: &str) -> Vec<String> {
    let mut links = Vec::new();
    let mut dirs = vec![(root.to_path_buf(), 1u32)];
    while let Some((dir, depth)) = dirs.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            if kind.is_dir() {
                if depth < 2 {
                    dirs.push((path, depth + 1));
                }
                continue;
            }
            if !kind.is_symlink() {
                continue;
            }
            let Ok(target) = fs::read_link(&path) else {
                continue;
            };
            // A symlink to /dev/null masks the unit, so it does not count as enabled.
            if target == Path::new("/dev/null") {
                continue;
            }
            let named = |p: &Path| p.file_name().is_some_and(|name| name == unit);
            if named(&path) || named(&target) {
                links.push(path.display().to_string());
            }
        }
    }
    links.sort();
    links
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(text: &str) -> Version {
        Version::parse(text).expect("a dotted version")
    }

    fn measured(entries: &[(&str, &str, &str)]) -> Vec<(String, String, String)> {
        entries
            .iter()
            .map(|(component, name, version)| {
                (component.to_string(), name.to_string(), version.to_string())
            })
            .collect()
    }

    #[test]
    fn a_floor_passes_when_any_provider_reaches_it() {
        let measured = measured(&[
            ("core/one", "runtime", "1.0"),
            ("core/two", "runtime", "2.5"),
        ]);
        assert_eq!(
            check_floor(&measured, "runtime", &version("2.0"), "core/app"),
            Ok(())
        );
    }

    #[test]
    fn a_floor_names_the_highest_provider_when_every_one_is_below() {
        let measured = measured(&[
            ("core/one", "runtime", "1.0"),
            ("core/two", "runtime", "1.5"),
        ]);
        let error = check_floor(&measured, "runtime", &version("2.0"), "core/app")
            .expect_err("below the floor");
        assert!(
            error.contains("core/two") && error.contains("1.5"),
            "{error}"
        );
    }

    #[test]
    fn a_floor_with_no_provider_is_refused() {
        let error =
            check_floor(&[], "runtime", &version("2.0"), "core/app").expect_err("no provider");
        assert!(
            error.contains("no measured provider of `runtime`"),
            "{error}"
        );
    }

    #[test]
    fn a_floor_with_a_non_dotted_version_is_refused() {
        let measured = measured(&[("core/one", "runtime", "1.0-rc1")]);
        let error =
            check_floor(&measured, "runtime", &version("2.0"), "core/app").expect_err("not dotted");
        assert!(error.contains("not a dotted number"), "{error}");
    }

    #[test]
    fn a_debian_version_is_its_upstream_part() {
        assert_eq!(upstream_version("debian", "1:2025.2-1"), "2025.2");
        assert_eq!(upstream_version("fedora", "2025.3"), "2025.3");
        assert_eq!(upstream_version("debian", "2:1.45.1-3~deb13"), "1.45.1");
    }

    #[test]
    fn a_removed_name_whose_witness_is_still_present_fails() {
        let dir = std::env::temp_dir().join(format!("buildc-removed.{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let kept = dir.join("bootupctl");
        fs::write(&kept, "").unwrap();
        let gone = dir.join("gone");
        let mut report = Report { failures: 0 };
        check_contracts(
            &format!("!{}|{}", gone.display(), kept.display()),
            &[],
            &mut report,
        );
        assert_eq!(report.failures, 1);
        let mut report = Report { failures: 0 };
        check_contracts(&format!("!{}", gone.display()), &[], &mut report);
        assert_eq!(report.failures, 0);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_file_hashes_to_what_the_vector_says_or_fails() {
        let path = std::env::temp_dir().join(format!("buildc-sha256.{}", std::process::id()));
        fs::write(&path, "abc").unwrap();
        assert_eq!(
            sha256_file(&path).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        fs::remove_file(&path).unwrap();
        assert!(sha256_file(&path).is_err());
    }

    #[test]
    fn a_preset_names_a_unit_only_on_the_verb_it_says() {
        let policy = "# no sshd\ndisable sshd.service\nenable faillock.service\n";
        assert!(names(policy, "disable", "sshd.service"));
        assert!(!names(policy, "enable", "sshd.service"));
        assert!(!names(
            "disable other.service sshd.service\n",
            "disable",
            "sshd.service"
        ));
        assert!(!names(policy, "disable", "sshd.service.d"));
    }

    #[test]
    fn verify_classes_match_what_they_name() {
        assert_eq!(
            classify("Failed to create var-home.mount: Unit var-home.mount not found."),
            Some("mount-not-found")
        );
        assert_eq!(
            classify("systemd-analyze[1]: Command 'man foo.service(8)' failed with code 16"),
            Some("man-page-missing")
        );
        assert_eq!(
            classify("'(man)' failed with exit status 16."),
            Some("man-page-missing")
        );
        assert_eq!(
            classify("probe.service: Can't show no-such-page(8): Protocol error"),
            Some("man-page-missing")
        );
        assert_eq!(classify("'(man)' failed with exit status ."), None);
        assert_eq!(classify("Unit is bad in some other way"), None);
    }

    #[test]
    fn a_contract_is_met_by_any_of_its_paths() {
        let here = std::env::temp_dir().join(format!("buildc-found-{}", std::process::id()));
        fs::write(&here, "").unwrap();
        let there = here.display().to_string();
        assert_eq!(
            found(&format!("/nonexistent|{there}"), &[]),
            Some(there.as_str())
        );
        assert_eq!(found("/nonexistent|/nonexistent/too", &[]), None);
        fs::remove_file(here).unwrap();
    }

    fn relocated(name: &str) -> (PathBuf, Vec<Link>) {
        let root = std::env::temp_dir().join(format!("buildc {name}-{}", std::process::id()));
        if root.exists() {
            fs::remove_dir_all(&root).unwrap();
        }
        let store = root.join("usr/lib/usrlocal");
        fs::create_dir_all(store.join("cuda-13.4/bin")).unwrap();
        fs::write(store.join("cuda-13.4/bin/nvcc"), "").unwrap();
        std::os::unix::fs::symlink("cuda-13.4", store.join("cuda")).unwrap();
        fs::create_dir_all(store.join("bin")).unwrap();
        fs::write(store.join("bin/mytool"), "").unwrap();
        std::os::unix::fs::symlink("../var/usrlocal", root.join("usr/local")).unwrap();
        let at = root.display().to_string().replace(' ', "\\x20");
        let map = format!(
            "d {at}/var/usrlocal 0755 root root -\n\
             d {at}/var/usrlocal/cuda-13.4 0755 root root -\n\
             L {at}/var/usrlocal/cuda-13.4/bin - - - - {at}/usr/lib/usrlocal/cuda-13.4/bin\n\
             L {at}/var/usrlocal/cuda - - - - {at}/usr/lib/usrlocal/cuda\n\
             d {at}/var/usrlocal/bin 0755 root root -\n\
             L {at}/var/usrlocal/bin/mytool - - - - {at}/usr/lib/usrlocal/bin/mytool\n"
        );
        (root, links(&map))
    }

    #[test]
    fn a_witness_under_a_whole_entry_link_is_found() {
        let (root, links) = relocated("whole-entry");
        let witness = root.join("usr/local/cuda/bin/nvcc").display().to_string();
        assert_eq!(found(&witness, &links), Some(witness.as_str()));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_witness_under_a_child_link_in_bin_is_found() {
        let (root, links) = relocated("child-link");
        let witness = root.join("usr/local/bin/mytool").display().to_string();
        assert_eq!(found(&witness, &links), Some(witness.as_str()));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_witness_missing_from_the_relocated_tree_is_not_found() {
        let (root, links) = relocated("missing");
        let witness = root.join("usr/local/bin/absent").display().to_string();
        assert_eq!(found(&witness, &links), None);
        fs::remove_dir_all(root).unwrap();
    }

    fn shadowed(name: &str, verb: &str) -> (PathBuf, Vec<Link>) {
        let root = std::env::temp_dir().join(format!("buildc {name}-{}", std::process::id()));
        if root.exists() {
            fs::remove_dir_all(&root).unwrap();
        }
        fs::create_dir_all(root.join("var/opt/vendor")).unwrap();
        fs::create_dir_all(root.join("usr/lib/opt/vendor/bin")).unwrap();
        fs::write(root.join("usr/lib/opt/vendor/bin/tool"), "").unwrap();
        let at = root.display().to_string().replace(' ', "\\x20");
        let links = links(&format!(
            "{verb} {at}/var/opt/vendor - - - - {at}/usr/lib/opt/vendor\n"
        ));
        (root, links)
    }

    #[test]
    fn an_l_plus_link_wins_over_a_path_on_disk() {
        let (root, links) = shadowed("replaces", "L+");
        let witness = root.join("var/opt/vendor/bin/tool").display().to_string();
        assert_eq!(found(&witness, &links), Some(witness.as_str()));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn an_l_link_leaves_a_path_on_disk_alone() {
        let (root, links) = shadowed("leaves", "L");
        let witness = root.join("var/opt/vendor/bin/tool").display().to_string();
        assert_eq!(found(&witness, &links), None);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_symlink_loop_resolves_to_nothing() {
        let root = std::env::temp_dir().join(format!("buildc loop-{}", std::process::id()));
        if root.exists() {
            fs::remove_dir_all(&root).unwrap();
        }
        fs::create_dir_all(&root).unwrap();
        std::os::unix::fs::symlink(root.join("b"), root.join("a")).unwrap();
        std::os::unix::fs::symlink(root.join("a"), root.join("b")).unwrap();
        assert_eq!(resolve(&root.join("a/x"), &[]), None);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_map_field_decodes_every_escape_finalize_writes() {
        assert_eq!(
            unescape("/a\\x20b%%c\\x5cd\\x0a"),
            PathBuf::from("/a b%c\\d\n")
        );
    }

    #[test]
    fn luks_needs_a_binary_in_the_initramfs() {
        assert!(opens_luks(
            "-rwxr-xr-x root root usr/bin/systemd-cryptsetup\n"
        ));
        assert!(opens_luks("-rwxr-xr-x root root usr/sbin/cryptsetup\n"));
        assert!(!opens_luks(
            "dracut module: crypt\nusr/lib/dm-crypt.ko.xz\n"
        ));
        assert!(!opens_luks(
            "lrwxrwxrwx root root usr/lib/systemd/systemd-cryptsetup -> ../../../bin/missing\n"
        ));
        assert!(!opens_luks(
            "lrwxrwxrwx root root usr/bin/cryptsetup -> /usr/bin/systemd-cryptsetup\n"
        ));
    }

    #[test]
    fn another_unit_s_diagnostics_do_not_fail_this_one() {
        let xfs = "/usr/lib/systemd/system/xfs_scrub_all.service:26: Support for option \
                   CPUAccounting= has been removed and it is ignored\n\
                   /usr/lib/systemd/system/system-xfs_scrub.slice:15: Support for option \
                   CPUAccounting= has been removed and it is ignored\n";
        let own = "'(man)' failed with exit status 16.\n\
                   probe.service: Can't show no-such-page(8): Protocol error\n";
        let text = format!("{xfs}{own}");
        assert!(unexpected(&text, "probe.service", &["man-page-missing"]).is_empty());
        assert_eq!(unexpected(&text, "probe.service", &[]).len(), 2);
        assert_eq!(
            unexpected(xfs, "probe.service", &["man-page-missing"]).len(),
            2
        );
        let mine = "/usr/lib/systemd/system/getty@.service:3: bad\n\
                    /etc/systemd/system/probe.service.d/x.conf:1: bad\n";
        assert_eq!(unexpected(mine, "getty@tty1.service", &[]).len(), 2);
        assert!(!about_another_unit(
            "/usr/lib/systemd/system/probe.service:2: bad",
            "probe.service"
        ));
    }

    #[test]
    fn a_preset_the_plan_expects_and_the_image_lacks_is_named() {
        let one = "/usr/lib/systemd/system-preset/45-component-one.preset".to_string();
        let two = "/usr/lib/systemd/user-preset/45-component-two.preset".to_string();
        let arrived = [one.clone()];

        assert!(missing_presets("", &[]).is_empty());
        assert!(missing_presets(&one, &arrived).is_empty());
        assert!(missing_presets("", &arrived).is_empty());

        let missing = missing_presets(&format!("{one} {two}"), &arrived);
        assert_eq!(missing.len(), 1);
        assert!(missing[0].starts_with(&two));
    }
}
