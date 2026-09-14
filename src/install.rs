use crate::copy;
use crate::emit::json::{self, Json};
use crate::prompt::Prompt;
use crate::ui::Choice;
use std::io::{BufRead as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const LOCK: &str = "/run/bootc-anywhere.lock";

fn lock() -> PathBuf {
    std::env::var_os("BUILDC_INSTALLER_LOCK")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(LOCK))
}

fn console(pid: libc::pid_t) -> String {
    name(std::fs::read_link(format!("/proc/{pid}/fd/0")).ok())
}

fn name(tty: Option<PathBuf>) -> String {
    let is_console = |tty: &Path| {
        tty.parent()
            .is_some_and(|parent| parent.as_os_str() == "/dev")
            && tty
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("tty") || name == "console")
    };
    match tty {
        Some(tty) if is_console(&tty) => tty.display().to_string(),
        Some(tty) if tty.starts_with("/dev/pts/") => {
            format!("another session ({})", tty.display())
        }
        _ => "another console".into(),
    }
}

pub fn hold() -> Result<std::fs::File, String> {
    hold_at(&lock())
}

fn hold_at(path: &Path) -> Result<std::fs::File, String> {
    use std::os::fd::AsRawFd as _;

    let file = std::fs::File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(|why| {
            format!(
                "{}: {why}\nset $BUILDC_INSTALLER_LOCK to somewhere writable to run this without root",
                path.display()
            )
        })?;
    let wrlck = || libc::flock {
        l_type: libc::F_WRLCK as libc::c_short,
        l_whence: libc::SEEK_SET as libc::c_short,
        l_start: 0,
        l_len: 0,
        l_pid: 0,
    };
    for _ in 0..2 {
        if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETLK, &wrlck()) } != -1 {
            return Ok(file);
        }
        let denied = std::io::Error::last_os_error().raw_os_error();
        if denied != Some(libc::EACCES) && denied != Some(libc::EAGAIN) {
            return Err(format!(
                "{}: {}",
                path.display(),
                std::io::Error::last_os_error()
            ));
        }
        let mut holder = wrlck();
        if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETLK, &mut holder) } == -1 {
            break;
        }
        if holder.l_type != libc::F_UNLCK as libc::c_short {
            return Err(format!(
                "the installer is running on {}; run `buildc installer` again once it finishes",
                console(holder.l_pid)
            ));
        }
    }
    Err(
        "the installer is running on another console; run `buildc installer` again once it finishes"
            .into(),
    )
}

pub const RECIPE: &str = "install-recipe.json";

pub fn media() -> PathBuf {
    Path::new(crate::provenance::build::MANIFEST)
        .parent()
        .unwrap_or(Path::new("/"))
        .to_path_buf()
}

#[derive(Debug)]
pub struct Payload {
    pub recipe: PathBuf,
    pub image: String,
    pub hostname: String,
    pub filesystem: String,
    pub bootloader: String,
}

#[derive(Debug)]
pub enum Found {
    Image(Payload),
    Repo(PathBuf),
    Nothing(PathBuf),
}

pub fn classify(root: &Path) -> Result<Found, String> {
    let recipe = root.join(RECIPE);
    if recipe.is_file() {
        let raw = std::fs::read_to_string(&recipe)
            .map_err(|err| format!("{}: {err}", recipe.display()))?;
        let doc = Json::parse(&raw).map_err(|err| format!("{}: {err}", recipe.display()))?;
        let field = |key: &str| {
            json::text(&doc, key)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| format!("{}: no `{key}`", recipe.display()))
        };
        let told = |key: &str| json::text(&doc, key).unwrap_or_default();
        return Ok(Found::Image(Payload {
            image: field("image")?,
            hostname: field("hostname")?,
            filesystem: told("filesystem"),
            bootloader: told("bootloader"),
            recipe,
        }));
    }
    Ok(match root.join(crate::layout::REPO_FILE).is_file() {
        true => Found::Repo(root.to_path_buf()),
        false => Found::Nothing(root.to_path_buf()),
    })
}

pub const LABEL: &str = "PAYLOAD";

const MOUNTPOINT: &str = "/run/buildc-payload";

pub fn root() -> Result<PathBuf, String> {
    let listed = Command::new("blkid")
        .args(["-t", &format!("LABEL={LABEL}"), "-o", "device"])
        .output();
    let devices = match &listed {
        Ok(out) => labelled(&String::from_utf8_lossy(&out.stdout)),
        Err(_) => Vec::new(),
    };
    match devices.as_slice() {
        [] => Ok(media()),
        [device] => mounted(device),
        many => Err(format!(
            "{} partitions are labelled {LABEL}, so which one to install from is not \
             clear: {}\n\nhelp: `buildc installer --from <root>` names one outright",
            many.len(),
            many.join(", ")
        )),
    }
}

fn labelled(listed: &str) -> Vec<String> {
    listed
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

fn mounted(device: &str) -> Result<PathBuf, String> {
    let at = PathBuf::from(MOUNTPOINT);
    if at.join(RECIPE).is_file() {
        return Ok(at);
    }
    std::fs::create_dir_all(&at).map_err(|err| format!("{MOUNTPOINT}: {err}"))?;
    let out = Command::new("mount")
        .args(["-o", "ro", device, MOUNTPOINT])
        .output()
        .map_err(|err| format!("mount: {err}, and it is what reads a {LABEL} partition"))?;
    match out.status.success() {
        true => Ok(at),
        false => Err(format!(
            "mounting {device} at {MOUNTPOINT}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )),
    }
}

const KINDS: [(&str, &str, &str); 4] = [
    (NONE, copy::ENC_NONE, ""),
    ("tpm2-luks", copy::ENC_TPM2, copy::ENC_ANY_HOLDER),
    ("luks-passphrase", copy::ENC_PASSPHRASE, copy::ENC_ONLY_YOU),
    ("tpm2-luks-passphrase", copy::ENC_BOTH, copy::ENC_ANY_HOLDER),
];

const NONE: &str = "none";

const TPM: &str = "/dev/tpmrm0";

fn tpm() -> PathBuf {
    std::env::var_os("BUILDC_TPM")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(TPM))
}

const SYS_BLOCK: &str = "/sys/block";

fn sys_block() -> PathBuf {
    std::env::var_os("BUILDC_SYS_BLOCK")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(SYS_BLOCK))
}

pub struct Encryption {
    pub kind: String,
    pub passphrase: String,
}

#[derive(Default)]
pub struct Data {
    pub disk: String,
    pub size: String,
    pub keep: bool,
}

impl Data {
    fn wanted(&self) -> bool {
        !self.disk.is_empty() || !self.size.is_empty()
    }
}

impl Encryption {
    fn wants_passphrase(kind: &str) -> bool {
        kind.ends_with("passphrase")
    }
}

pub struct Answers {
    pub disk: String,
    pub hostname: String,
    pub user: String,
    pub password: String,
    pub encryption: Encryption,
    pub data: Data,
}

#[derive(Clone, Default)]
pub struct Given {
    pub disk: Option<String>,
    pub hostname: Option<String>,
    pub user: Option<String>,
    pub password: Option<String>,
    pub encryption: Option<String>,
    pub passphrase: Option<String>,
}

enum Leave {
    Back,
    Over,
    Shell,
}

fn leaving(err: &str) -> bool {
    err == crate::ui::INTERRUPTED
}

fn leave(prompt: &Prompt) -> Result<Leave, String> {
    if !prompt.draws() {
        return Ok(Leave::Shell);
    }
    let options = [
        Choice::new(copy::LEAVE_BACK, ""),
        Choice::new(copy::LEAVE_OVER, ""),
        Choice::new(copy::LEAVE_SHELL, ""),
    ];
    match prompt.choose(copy::LEAVING, &options) {
        Ok(Some(1)) => Ok(Leave::Over),
        Ok(Some(2)) => Ok(Leave::Shell),
        Ok(_) => Ok(Leave::Back),
        Err(err) if leaving(&err) => Ok(Leave::Back),
        Err(err) => Err(err),
    }
}

impl Answers {
    pub fn collect(
        payload: &Payload,
        given: Given,
        prompt: &Prompt,
    ) -> Result<Option<Self>, String> {
        let seeded = Self::seeded(payload, given, prompt)?;
        if !prompt.draws() {
            return Ok(Some(seeded));
        }
        let mut fields = seeded.fields(&payload.filesystem, &payload.bootloader);
        loop {
            let actions = [copy::INSTALL, copy::SHUT_DOWN];
            let filled =
                crate::ui::form(&mut fields, &actions, short_of, asked, copy::INSTALL_KEYS);
            match filled {
                Ok(crate::ui::Filled::Took(0)) => {
                    let answers = Self::of(&fields);
                    if crate::ui::confirm_over(
                        &copy::erasing(&answers.disk),
                        &answers.summary(payload),
                        copy::CONTINUE,
                        copy::GO_BACK,
                    )? {
                        return Ok(Some(answers));
                    }
                }
                Ok(crate::ui::Filled::Took(_)) => {
                    power_off()?;
                    return Ok(None);
                }
                Ok(crate::ui::Filled::Left) => match leave(prompt)? {
                    Leave::Shell => return Ok(None),
                    Leave::Over => {
                        fields = Self::seeded(payload, Given::default(), prompt)?
                            .fields(&payload.filesystem, &payload.bootloader)
                    }
                    Leave::Back => {}
                },
                Err(err) if leaving(&err) => match leave(prompt)? {
                    Leave::Shell => return Ok(None),
                    Leave::Over => {
                        fields = Self::seeded(payload, Given::default(), prompt)?
                            .fields(&payload.filesystem, &payload.bootloader)
                    }
                    Leave::Back => {}
                },
                Err(err) => return Err(err),
            }
        }
    }

    fn seeded(payload: &Payload, given: Given, prompt: &Prompt) -> Result<Self, String> {
        if !prompt.draws() {
            return Ok(Self {
                disk: ask_disk(given.disk, None, prompt)?,
                hostname: prompt.text(
                    given.hostname,
                    copy::INSTALL_NAME,
                    "--hostname",
                    Some(&payload.hostname),
                )?,
                user: prompt.text(given.user, copy::INSTALL_USER, "--user", None)?,
                password: prompt.text(
                    given.password,
                    copy::INSTALL_PASSWORD,
                    "--password",
                    None,
                )?,
                encryption: ask_encryption(given.encryption, given.passphrase, prompt)?,
                data: Data::default(),
            });
        }
        Ok(Self {
            disk: given.disk.unwrap_or_default(),
            hostname: given
                .hostname
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| payload.hostname.clone()),
            user: given.user.unwrap_or_default(),
            password: given.password.unwrap_or_default(),
            encryption: Encryption {
                kind: match given.encryption {
                    Some(kind) => named(kind)?,
                    None => NONE.to_string(),
                },
                passphrase: given.passphrase.unwrap_or_default(),
            },
            data: Data::default(),
        })
    }

    fn fields(&self, filesystem: &str, bootloader: &str) -> Vec<crate::ui::Field> {
        use crate::ui::Field;
        let found = disks(&sys_block(), &in_use_now());
        let at = found.iter().position(|(disk, _)| *disk == self.disk);
        let disk = match found.is_empty() {
            true => Field::text(copy::ROW_DISK, &self.disk),
            false => Field::pick(
                copy::ROW_DISK,
                found
                    .iter()
                    .map(|(disk, detail)| Choice::new(disk, detail))
                    .collect(),
                at,
            ),
        };
        vec![
            disk,
            Field::fixed(copy::ROW_LAYOUT, &copy::layout(filesystem, bootloader)),
            Field::text(copy::ROW_HOSTNAME, &self.hostname),
            Field::text(copy::ROW_ACCOUNT, &self.user),
            Field::secret(copy::ROW_PASSWORD, &self.password),
            Field::secret(copy::ROW_CONFIRM, &self.password),
            Field::pick(
                copy::ROW_ENCRYPTION,
                kinds(tpm().exists()),
                KINDS
                    .iter()
                    .position(|(name, _, _)| *name == self.encryption.kind),
            ),
            Field::secret(copy::ROW_PASSPHRASE, &self.encryption.passphrase),
            Field::pick(
                copy::ROW_DATA,
                data_rows(&self.disk, &found, self.encryption.kind != NONE),
                Some(0),
            ),
            Field::text(copy::ROW_SIZE, &self.data.size),
        ]
    }

    fn of(fields: &[crate::ui::Field]) -> Self {
        let at = |row: usize| fields[row].value();
        Self {
            disk: at(ROW_DISK),
            hostname: at(ROW_HOSTNAME),
            user: at(ROW_ACCOUNT),
            password: at(ROW_PASSWORD),
            encryption: Encryption {
                kind: written(&at(ROW_ENCRYPTION)).to_string(),
                passphrase: at(ROW_PASSPHRASE),
            },
            data: chose(&at(ROW_DATA), &at(ROW_SIZE)),
        }
    }

    fn summary(&self, payload: &Payload) -> Vec<(String, String)> {
        let mut rows = vec![
            (copy::ROW_DISK.to_string(), self.disk.clone()),
            (copy::ROW_HOSTNAME.to_string(), self.hostname.clone()),
            (copy::ROW_ACCOUNT.to_string(), self.user.clone()),
            (
                copy::ROW_PASSWORD.to_string(),
                copy::PASSWORD_SET.to_string(),
            ),
            (
                copy::ROW_ENCRYPTION.to_string(),
                shown(&self.encryption.kind).to_string(),
            ),
            (copy::ROW_DATA.to_string(), data_said(&self.data)),
        ];
        rows.extend(copy::written_over(
            &payload.bootloader,
            &payload.filesystem,
            &self.data.size,
        ));
        rows
    }
}

const ROW_DISK: usize = 0;
const ROW_HOSTNAME: usize = 2;
const ROW_ACCOUNT: usize = 3;
const ROW_PASSWORD: usize = 4;
const ROW_CONFIRM: usize = 5;
const ROW_ENCRYPTION: usize = 6;
const ROW_PASSPHRASE: usize = 7;
const ROW_DATA: usize = 8;
const ROW_SIZE: usize = 9;

fn asked(fields: &[crate::ui::Field]) -> Vec<usize> {
    let wants = Encryption::wants_passphrase(written(&fields[ROW_ENCRYPTION].value()));
    let sized = fields[ROW_DATA].value() == copy::DATA_HERE;
    (0..fields.len())
        .filter(|row| *row != ROW_PASSPHRASE || wants)
        .filter(|row| *row != ROW_SIZE || sized)
        .collect()
}

fn short_of(fields: &[crate::ui::Field]) -> Option<String> {
    let at = |row: usize| fields[row].value();
    if !at(ROW_PASSWORD).is_empty() && at(ROW_PASSWORD) != at(ROW_CONFIRM) {
        return Some(copy::NO_MATCH_ROW.to_string());
    }
    let data = chose(&at(ROW_DATA), &at(ROW_SIZE));
    if written(&at(ROW_ENCRYPTION)) != NONE && data.wanted() {
        return Some(copy::DATA_UNENCRYPTED.to_string());
    }
    if !data.disk.is_empty() && data.disk == at(ROW_DISK) {
        return Some(copy::DATA_SAME_DISK.to_string());
    }
    let wants = Encryption::wants_passphrase(written(&at(ROW_ENCRYPTION)));
    let missing: Vec<&str> = [
        (at(ROW_DISK).is_empty(), copy::ROW_DISK),
        (at(ROW_ACCOUNT).is_empty(), copy::ROW_ACCOUNT),
        (at(ROW_PASSWORD).is_empty(), copy::ROW_PASSWORD),
        (wants && at(ROW_PASSPHRASE).is_empty(), copy::ROW_PASSPHRASE),
        (
            at(ROW_DATA) == copy::DATA_HERE && at(ROW_SIZE).is_empty(),
            copy::ROW_SIZE,
        ),
    ]
    .into_iter()
    .filter(|(missing, _)| *missing)
    .map(|(_, name)| name)
    .collect();
    match missing.is_empty() {
        true => None,
        false => Some(copy::still_needs(&missing)),
    }
}

fn power_off() -> Result<(), String> {
    let out = Command::new("systemctl")
        .arg("poweroff")
        .output()
        .map_err(|err| format!("systemctl: {err}, and it is what stops a machine"))?;
    match out.status.success() {
        true => Ok(()),
        false => Err(format!(
            "systemctl poweroff: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )),
    }
}

const SECTOR: u64 = 512;

const VIRTUAL: [&str; 7] = ["loop", "ram", "zram", "sr", "fd", "dm-", "md"];

fn in_use_now() -> String {
    std::fs::read_to_string("/proc/mounts").unwrap_or_default()
}

fn in_use(disk: &Path, name: &str, mounts: &str) -> bool {
    let is_source = |dev: &str| {
        mounts
            .lines()
            .filter_map(|line| line.split_whitespace().next())
            .any(|source| source == dev)
    };
    if is_source(&format!("/dev/{name}")) {
        return true;
    }
    let Ok(parts) = std::fs::read_dir(disk) else {
        return false;
    };
    parts.flatten().any(|part| {
        part.path().join("partition").is_file()
            && is_source(&format!("/dev/{}", part.file_name().to_string_lossy()))
    })
}

pub fn disks(sys: &Path, mounts: &str) -> Vec<(String, String)> {
    let Ok(entries) = std::fs::read_dir(sys) else {
        return Vec::new();
    };
    let mut found: Vec<(String, String)> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if VIRTUAL.iter().any(|prefix| name.starts_with(prefix)) {
            continue;
        }
        let read = |leaf: &str| {
            std::fs::read_to_string(entry.path().join(leaf))
                .map(|text| text.trim().to_string())
                .unwrap_or_default()
        };
        let sectors: u64 = read("size").parse().unwrap_or(0);
        if sectors == 0 {
            continue;
        }
        if read("ro") == "1" || in_use(&entry.path(), &name, mounts) {
            continue;
        }
        let removable = match read("removable").as_str() {
            "1" => copy::REMOVABLE.to_string(),
            _ => String::new(),
        };
        let detail = [
            format!("{} GB", sectors * SECTOR / 1_000_000_000),
            read("device/model"),
            removable,
        ];
        found.push((
            format!("/dev/{name}"),
            detail
                .iter()
                .filter(|part| !part.is_empty())
                .cloned()
                .collect::<Vec<_>>()
                .join("  "),
        ));
    }
    found.sort();
    found
}

fn ask_disk(
    given: Option<String>,
    current: Option<&str>,
    prompt: &Prompt,
) -> Result<String, String> {
    if let Some(disk) = given.filter(|disk| !disk.is_empty()) {
        return Ok(disk);
    }
    let found = disks(&sys_block(), &in_use_now());
    if !prompt.asks() || found.is_empty() {
        return prompt.text(None, copy::INSTALL_DISK, "--disk", current);
    }
    let options: Vec<Choice> = found
        .iter()
        .map(|(disk, detail)| Choice::new(disk, detail))
        .collect();
    let at = current
        .and_then(|held| found.iter().position(|(disk, _)| disk == held))
        .unwrap_or(0);
    match prompt.choose_current(copy::INSTALL_DISK, &options, at)? {
        Some(at) => Ok(found[at].0.clone()),
        None => Ok(current.unwrap_or_default().to_string()),
    }
}

fn named(kind: String) -> Result<String, String> {
    if KINDS.iter().any(|(name, _, _)| *name == kind) {
        return Ok(kind);
    }
    Err(format!(
        "`{kind}` is not one of {}",
        KINDS
            .iter()
            .map(|(name, _, _)| *name)
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

fn written(shown: &str) -> &'static str {
    KINDS
        .iter()
        .find(|(_, label, _)| *label == shown)
        .map_or(NONE, |(name, _, _)| *name)
}

fn shown(kind: &str) -> &'static str {
    KINDS
        .iter()
        .find(|(name, _, _)| *name == kind)
        .map_or(copy::ENC_NONE, |(_, label, _)| *label)
}

fn kinds(tpm: bool) -> Vec<Choice> {
    KINDS
        .iter()
        .map(
            |(name, shown, detail)| match tpm || !name.starts_with("tpm2") {
                true => Choice::new(*shown, *detail),
                false => Choice::new(*shown, copy::NO_TPM).unavailable(),
            },
        )
        .collect()
}

fn data_rows(disk: &str, found: &[(String, String)], encrypted: bool) -> Vec<Choice> {
    let mut rows = vec![Choice::new(copy::NONE, "")];
    let mut offered = vec![Choice::new(copy::DATA_HERE, "")];
    for (other, detail) in found.iter().filter(|(other, _)| other != disk) {
        offered.push(Choice::new(copy::on_disk(other, copy::DATA_ERASED), detail));
        offered.push(Choice::new(copy::on_disk(other, copy::DATA_KEPT), detail));
    }
    rows.extend(offered.into_iter().map(|row| match encrypted {
        false => row,
        true => Choice::new(row.label, copy::DATA_UNENCRYPTED).unavailable(),
    }));
    rows
}

fn chose(shown: &str, size: &str) -> Data {
    if shown == copy::DATA_HERE {
        return Data {
            size: size.to_string(),
            ..Data::default()
        };
    }
    for (how, keep) in [(copy::DATA_KEPT, true), (copy::DATA_ERASED, false)] {
        if let Some(disk) = shown.strip_suffix(&format!(", {how}")) {
            return Data {
                disk: disk.to_string(),
                keep,
                ..Data::default()
            };
        }
    }
    Data::default()
}

fn data_said(data: &Data) -> String {
    match (data.disk.is_empty(), data.size.is_empty()) {
        (true, true) => copy::NONE.to_string(),
        (true, false) => copy::DATA_HERE.to_string(),
        _ => copy::on_disk(
            &data.disk,
            match data.keep {
                true => copy::DATA_KEPT,
                false => copy::DATA_ERASED,
            },
        ),
    }
}

fn ask_encryption(
    given: Option<String>,
    passphrase: Option<String>,
    prompt: &Prompt,
) -> Result<Encryption, String> {
    let kind = match given {
        Some(kind) => named(kind)?,
        None if !prompt.asks() => NONE.to_string(),
        None => {
            let options = kinds(tpm().exists());
            match prompt.choose_current(copy::INSTALL_ENCRYPTION, &options, 0)? {
                Some(at) => KINDS[at].0.to_string(),
                None => NONE.to_string(),
            }
        }
    };
    Ok(Encryption {
        passphrase: match Encryption::wants_passphrase(&kind) {
            true => prompt.text(passphrase, copy::LUKS_PASSPHRASE, "--passphrase", None)?,
            false => String::new(),
        },
        kind,
    })
}

fn set(value: &mut Json, key: &str, field: Json) {
    let Json::Object(fields) = value else { return };
    match fields.iter_mut().find(|(name, _)| name == key) {
        Some((_, held)) => *held = field,
        None => fields.push((key.to_string(), field)),
    }
}

pub fn complete(recipe: &Path, answers: &Answers) -> Result<Json, String> {
    let raw =
        std::fs::read_to_string(recipe).map_err(|err| format!("{}: {err}", recipe.display()))?;
    let mut doc = Json::parse(&raw).map_err(|err| format!("{}: {err}", recipe.display()))?;
    set(&mut doc, "disk", Json::string(&answers.disk));
    set(&mut doc, "hostname", Json::string(&answers.hostname));
    set(
        &mut doc,
        "encryption",
        Json::object([
            ("type", Json::string(&answers.encryption.kind)),
            ("passphrase", Json::string(&answers.encryption.passphrase)),
        ]),
    );
    let mut user = match doc {
        Json::Object(ref mut fields) => match fields.iter().position(|(name, _)| name == "user") {
            Some(at) => fields.remove(at).1,
            None => Json::object([]),
        },
        _ => return Err(format!("{}: not an object", recipe.display())),
    };
    set(&mut user, "username", Json::string(&answers.user));
    set(
        &mut user,
        "password",
        Json::string(hashed(&answers.password)?),
    );
    set(&mut doc, "user", user);
    if !answers.data.disk.is_empty() {
        set(
            &mut doc,
            "varDisk",
            Json::object([
                ("disk", Json::string(&answers.data.disk)),
                ("keepExisting", Json::Bool(answers.data.keep)),
            ]),
        );
    } else if !answers.data.size.is_empty() {
        set(
            &mut doc,
            "varDisk",
            Json::object([("size", Json::string(&answers.data.size))]),
        );
    }
    Ok(doc)
}

fn hashed(password: &str) -> Result<String, String> {
    let mut child = Command::new("openssl")
        .args(["passwd", "-6", "-stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|err| format!("openssl: {err}, and it is what hashes the password"))?;
    child
        .stdin
        .take()
        .ok_or("openssl: no stdin")?
        .write_all(format!("{password}\n").as_bytes())
        .map_err(|err| format!("openssl: {err}"))?;
    let out = child
        .wait_with_output()
        .map_err(|err| format!("openssl: {err}"))?;
    let hash = String::from_utf8_lossy(&out.stdout).trim().to_string();
    match out.status.success() && hash.starts_with('$') {
        true => Ok(hash),
        false => Err(format!(
            "openssl passwd wrote no crypt string, and a plaintext password loses the install at \
             its last step: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )),
    }
}

fn stage(recipe: &Json) -> Result<PathBuf, String> {
    use std::os::unix::fs::OpenOptionsExt as _;
    let path = std::env::temp_dir().join(format!("bootc-anywhere.{}.json", std::process::id()));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&path)
        .map_err(|err| format!("{}: {err}", path.display()))?;
    file.write_all(recipe.render().as_bytes())
        .map_err(|err| format!("{}: {err}", path.display()))?;
    Ok(path)
}

const BACKEND: &str = "fisherman";

const TICK: std::time::Duration = std::time::Duration::from_millis(120);

impl Found {
    pub fn payload(&self) -> Result<&Payload, String> {
        match self {
            Self::Image(payload) => Ok(payload),
            Self::Repo(root) => Err(format!(
                "{} is a repository, so there is nothing here to install yet\n\nhelp: \
                 `buildc build` in it, then `buildc installer --from <the built payload>`",
                root.display()
            )),
            Self::Nothing(root) => Err(format!(
                "{} holds no {RECIPE} and no {}, so it is neither a payload nor a repository",
                root.display(),
                crate::layout::REPO_FILE
            )),
        }
    }
}

enum Event {
    Step(u16, u16, String),
    Note(String),
    Done(String),
    Recovery(String),
    Other(String),
}

impl Event {
    fn of(line: &str) -> Self {
        let Ok(event) = Json::parse(line) else {
            return Self::Other(line.to_string());
        };
        let text = |key: &str| json::text(&event, key).unwrap_or_default();
        let count = |key: &str| json::number(&event, key).unwrap_or(0);
        match json::text(&event, "type").as_deref() {
            Some("step") => Self::Step(
                count("cumulative_pct") as u16,
                count("weight_pct") as u16,
                format!(
                    "{}/{} {}",
                    count("step"),
                    count("total_steps"),
                    text("step_name")
                ),
            ),
            Some("info" | "substep") => Self::Note(text("message")),
            Some("complete") => Self::Done(text("message")),
            Some("recovery_key") => Self::Recovery(text("key")),
            _ => Self::Other(line.to_string()),
        }
    }

    fn logged(&self) -> Option<String> {
        match self {
            Self::Recovery(_) => None,
            said => Some(said.say()),
        }
    }

    fn say(&self) -> String {
        match self {
            Self::Step(pct, _, what) => format!("[{pct:>3}%] {what}"),
            Self::Note(message) => format!("       {message}"),
            Self::Done(message) => format!("[100%] {message}"),
            Self::Recovery(key) => format!("\n{}\n", copy::recovery(key)),
            Self::Other(line) => line.clone(),
        }
    }
}

const RENDERER: &str = "/usr/libexec/grub-menu-from-bls";

const TARGET: &str = "/run/buildc-target";

fn render_menu(image: &str, disk: &str) -> Result<(), String> {
    let at = PathBuf::from(TARGET);
    let boot = at.join("boot");
    std::fs::create_dir_all(&boot).map_err(|err| format!("{TARGET}: {err}"))?;

    let Some((device, root)) = boot_partition(disk, &boot)? else {
        return Err(format!(
            "no partition of {disk} carries `loader/entries`, so there is no menu to render"
        ));
    };
    let rendered = run_renderer(image, root);
    let _ = Command::new("umount").arg(&boot).output();
    match rendered {
        Ok(true) => {
            eprintln!("buildc: wrote the boot menu {device} needs, since its GRUB reads no BLS");
            Ok(())
        }
        Ok(false) => Ok(()),
        Err(err) => Err(err),
    }
}

fn boot_partition(disk: &str, boot: &Path) -> Result<Option<(String, &'static str)>, String> {
    let listed = Command::new("lsblk")
        .args(["-nrpo", "NAME", disk])
        .output()
        .map_err(|err| format!("lsblk: {err}, and it is what lists a disk's partitions"))?;
    for device in labelled(&String::from_utf8_lossy(&listed.stdout)) {
        if device == disk {
            continue;
        }
        let mounted = Command::new("mount")
            .args([&device, &boot.to_string_lossy().to_string()])
            .output();
        if !matches!(&mounted, Ok(out) if out.status.success()) {
            continue;
        }
        if boot.join("loader/entries").is_dir() {
            return Ok(Some((device, "/target")));
        }
        if boot.join("boot/loader/entries").is_dir() {
            return Ok(Some((device, "/target/boot")));
        }
        let _ = Command::new("umount").arg(boot).output();
    }
    Ok(None)
}

fn run_renderer(image: &str, root: &str) -> Result<bool, String> {
    let out = Command::new("podman")
        .args([
            "run",
            "--rm",
            "--net=none",
            "--security-opt",
            "label=disable",
            "--entrypoint",
            "",
            "-v",
            &format!("{TARGET}:/target"),
            image,
            "/bin/sh",
            "-c",
            &format!("test -x {RENDERER} || exit 3; exec {RENDERER} {root}"),
        ])
        .output()
        .map_err(|err| format!("podman: {err}, and it is what runs the image's renderer"))?;
    match out.status.code() {
        Some(0) => Ok(true),
        Some(3) => Ok(false),
        _ => Err(format!(
            "{RENDERER} in {image}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )),
    }
}

const LOG: &str = "bootc-anywhere.log";

const IN_RAM: &str = "/run";

fn open_log(payload: &Payload) -> (Option<std::fs::File>, Option<PathBuf>) {
    if payload.recipe.parent() == Some(Path::new(MOUNTPOINT)) && remounted_rw(MOUNTPOINT) {
        let path = Path::new(MOUNTPOINT).join(LOG);
        if let Ok(file) = std::fs::File::create(&path) {
            return (Some(file), Some(path));
        }
    }
    let path = Path::new(IN_RAM).join(LOG);
    match std::fs::File::create(&path) {
        Ok(file) => (Some(file), Some(path)),
        Err(_) => (None, None),
    }
}

fn remounted_rw(at: &str) -> bool {
    Command::new("mount")
        .args(["-o", "remount,rw", at])
        .output()
        .is_ok_and(|out| out.status.success())
}

pub fn run(payload: &Payload, answers: &Answers, prompt: &Prompt) -> Result<(), String> {
    let path = stage(&complete(&payload.recipe, answers)?)?;
    let (mut log, at) = open_log(payload);
    if !prompt.draws() {
        eprintln!(
            "buildc: installing {} as {} onto {}, {}",
            payload.image,
            answers.hostname,
            answers.disk,
            copy::logging(at.as_deref())
        );
    }
    let (events, writer) = std::io::pipe().map_err(|err| format!("{BACKEND}: {err}"))?;
    let errors = writer
        .try_clone()
        .map_err(|err| format!("{BACKEND}: {err}"))?;
    let mut child = Command::new(BACKEND)
        .arg(&path)
        .stdout(Stdio::from(writer))
        .stderr(Stdio::from(errors))
        .spawn()
        .map_err(|err| format!("{BACKEND}: {err}"))?;
    let mut region = match prompt.draws() {
        true => crate::ui::Progress::open(&copy::writing(at.as_deref())).ok(),
        false => None,
    };
    let mut recovery = None;
    {
        let (lines, arriving) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for line in std::io::BufReader::new(events)
                .lines()
                .map_while(Result::ok)
            {
                if lines.send(line).is_err() {
                    return;
                }
            }
        });
        loop {
            let line = match arriving.recv_timeout(TICK) {
                Ok(line) => line,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    if let Some(region) = &mut region {
                        let _ = region.tick();
                    }
                    continue;
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            };
            let event = Event::of(&line);
            if let Event::Recovery(key) = &event {
                recovery = Some(key.clone());
            }
            if let (Some(said), Some(log)) = (event.logged(), &mut log) {
                let _ = writeln!(log, "{said}");
            }
            match &mut region {
                None => println!("{}", event.say()),
                Some(region) => {
                    let _ = match &event {
                        Event::Step(pct, flight, what) => region.step(*pct, *flight, what),
                        Event::Done(message) => region.step(100, 0, message),
                        Event::Recovery(_) => Ok(()),
                        Event::Note(message) => region.note(message),
                        Event::Other(line) => region.note(line),
                    };
                }
            }
        }
    }
    let finished = child.wait().map_err(|err| format!("{BACKEND}: {err}"));
    if let Some(region) = region {
        region.close();
    }
    let _ = std::fs::remove_file(&path);
    let status = finished?;
    if !status.success() {
        return Err(format!(
            "{BACKEND} did not finish: {status}, and {}",
            copy::logging(at.as_deref())
        ));
    }
    render_menu(&payload.image, &answers.disk)?;
    finish(recovery.as_deref(), at.as_deref(), prompt)
}

fn finish(recovery: Option<&str>, log: Option<&Path>, prompt: &Prompt) -> Result<(), String> {
    if !prompt.draws() {
        if let Some(key) = recovery {
            println!("\n{}", copy::recovery(key));
            println!("{}\n", copy::KEY_NOT_LOGGED);
        }
        eprintln!("buildc: {}", copy::logging(log));
        return Ok(());
    }
    match crate::ui::offer_over(
        copy::INSTALL_DONE,
        done_rows(recovery, log),
        copy::RESTART,
        copy::DONE_KEYS,
    )? {
        false => Ok(()),
        true => restart(),
    }
}

pub(crate) fn done_rows(recovery: Option<&str>, log: Option<&Path>) -> Vec<Choice> {
    let mut rows = Vec::new();
    if let Some(key) = recovery {
        rows.push(Choice::new(copy::WRITE_DOWN, "").content());
        rows.push(Choice::new(key, "").content().tinted());
        rows.push(Choice::new(copy::KEY_NOT_LOGGED, "").content());
    }
    rows.push(Choice::new(copy::logging(log), "").content());
    rows
}

fn restart() -> Result<(), String> {
    let out = Command::new("systemctl")
        .arg("reboot")
        .output()
        .map_err(|err| format!("systemctl: {err}, and it is what restarts a machine"))?;
    match out.status.success() {
        true => Ok(()),
        false => Err(format!(
            "systemctl reboot: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )),
    }
}

pub fn own_screen(payload: &Payload, prompt: &Prompt) {
    if !prompt.draws() {
        return;
    }
    print!("\u{1b}[2J\u{1b}[H");
    let _ = std::io::stdout().flush();
    crate::ui::own_screen(copy::installing(&payload.image));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("bootc-anywhere-{name}.{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("a scratch tree");
        root
    }

    const EMITTED: &str = r#"{
  "image": "ghcr.io/buildc-org/deb2:latest",
  "targetImgref": "ghcr.io/buildc-org/deb2:latest",
  "composeFsBackend": true,
  "genericImage": true,
  "bootloader": "grub2",
  "filesystem": "ext4",
  "hostname": "deb2",
  "user": { "groups": ["sudo"] },
  "additionalImageStores": ["/var/lib/buildc/store"]
}"#;

    #[test]
    fn a_payload_wins_over_the_repository_that_would_have_to_be_built() {
        let root = scratch("cases");
        assert!(matches!(classify(&root), Ok(Found::Nothing(_))));

        std::fs::write(root.join(crate::layout::REPO_FILE), "repo {\n}\n").expect("a repo.kdl");
        assert!(matches!(classify(&root), Ok(Found::Repo(_))));

        std::fs::write(root.join(RECIPE), EMITTED).expect("a recipe");
        let Ok(Found::Image(payload)) = classify(&root) else {
            panic!("a payload beside a repository is still a payload");
        };
        assert_eq!(payload.image, "ghcr.io/buildc-org/deb2:latest");
        assert_eq!(payload.hostname, "deb2");

        std::fs::write(root.join(RECIPE), "{").expect("a broken recipe");
        assert!(classify(&root).is_err());
        std::fs::write(root.join(RECIPE), "{}").expect("an empty recipe");
        assert!(classify(&root)
            .unwrap_err()
            .contains(&format!("{RECIPE}: no `image`")));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_completed_recipe_keeps_every_derived_field_and_hashes_the_password() {
        let root = scratch("complete");
        let recipe = root.join(RECIPE);
        std::fs::write(&recipe, EMITTED).expect("a recipe");
        let answers = Answers {
            disk: "/dev/vda".to_string(),
            hostname: "deb2".to_string(),
            user: "buildc".to_string(),
            password: "hunter2".to_string(),
            encryption: Encryption {
                kind: "luks-passphrase".to_string(),
                passphrase: "opensesame".to_string(),
            },
            data: Data::default(),
        };
        let done = complete(&recipe, &answers).expect("the person's half goes in");

        for (key, value) in [
            ("image", "\"ghcr.io/buildc-org/deb2:latest\""),
            ("composeFsBackend", "true"),
            ("bootloader", "\"grub2\""),
            ("filesystem", "\"ext4\""),
            ("hostname", "\"deb2\""),
            ("disk", "\"/dev/vda\""),
        ] {
            let held = json::field(&done, key).map(|v| v.render().trim().to_string());
            assert_eq!(held.as_deref(), Some(value), "{key}");
        }

        let user = json::field(&done, "user").expect("an account");
        assert_eq!(json::strings(user, "groups"), ["sudo"]);
        assert_eq!(json::text(user, "username").as_deref(), Some("buildc"));
        let hash = json::text(user, "password").expect("a password");
        assert!(hash.starts_with("$6$"), "{hash}");
        assert!(!hash.contains("hunter2"), "{hash}");

        let encryption = json::field(&done, "encryption").expect("an encryption");
        assert_eq!(
            json::text(encryption, "type").as_deref(),
            Some("luks-passphrase")
        );
        assert_eq!(
            json::text(encryption, "passphrase").as_deref(),
            Some("opensesame")
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn only_the_forms_named_for_a_passphrase_are_asked_for_one() {
        let wants: Vec<&str> = KINDS
            .iter()
            .map(|(name, _, _)| *name)
            .filter(|name| Encryption::wants_passphrase(name))
            .collect();
        assert_eq!(wants, ["luks-passphrase", "tpm2-luks-passphrase"]);
    }

    #[test]
    fn every_shown_description_is_written_as_the_name_fisherman_takes() {
        let root = scratch("kinds");
        let recipe = root.join(RECIPE);
        std::fs::write(&recipe, EMITTED).expect("a recipe");
        for (name, label, _) in KINDS {
            let answers = Answers {
                disk: "/dev/vda".to_string(),
                hostname: "deb2".to_string(),
                user: "buildc".to_string(),
                password: "hunter2".to_string(),
                encryption: Encryption {
                    kind: name.to_string(),
                    passphrase: "opensesame".to_string(),
                },
                data: Data::default(),
            };
            let fields = answers.fields("ext4", "grub2");
            assert_eq!(fields[ROW_ENCRYPTION].value(), label);
            let read = Answers::of(&fields);
            assert_eq!(read.encryption.kind, name);
            let done = complete(&recipe, &read).expect("a completed recipe");
            let encryption = json::field(&done, "encryption").expect("an encryption");
            assert_eq!(json::text(encryption, "type").as_deref(), Some(name));
            assert_eq!(shown(name), label);
            let flagged = ask_encryption(
                Some(name.to_string()),
                Some("opensesame".to_string()),
                &Prompt::silent(),
            )
            .expect("one of the four");
            assert_eq!(flagged.kind, name);
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_var_rows_are_shown_and_unpickable_under_an_encrypted_root() {
        let found = [
            ("/dev/vda".to_string(), "68 GB".to_string()),
            ("/dev/sdb".to_string(), "30 GB".to_string()),
        ];
        let open = data_rows("/dev/vda", &found, false);
        let labels: Vec<&str> = open.iter().map(|row| row.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                copy::NONE,
                copy::DATA_HERE,
                &copy::on_disk("/dev/sdb", copy::DATA_ERASED),
                &copy::on_disk("/dev/sdb", copy::DATA_KEPT),
            ]
        );
        assert!(open.iter().all(|row| row.available));

        let gated = data_rows("/dev/vda", &found, true);
        assert!(gated[0].available);
        for row in &gated[1..] {
            assert!(!row.available, "{} is pickable", row.label);
            assert_eq!(row.detail, copy::DATA_UNENCRYPTED);
        }
    }

    #[test]
    fn a_sized_var_writes_a_size_and_another_disk_writes_that_disk() {
        let root = scratch("var");
        let recipe = root.join(RECIPE);
        std::fs::write(&recipe, EMITTED).expect("a recipe");
        let written = |data: Data| {
            let answers = Answers {
                disk: "/dev/vda".to_string(),
                hostname: "deb2".to_string(),
                user: "buildc".to_string(),
                password: "hunter2".to_string(),
                encryption: Encryption {
                    kind: NONE.to_string(),
                    passphrase: String::new(),
                },
                data,
            };
            complete(&recipe, &answers).expect("a completed recipe")
        };
        let held = |doc: &Json, key: &str| {
            json::field(doc, "varDisk")
                .and_then(|var| json::field(var, key))
                .map(|value| value.render().trim().to_string())
        };

        let sized = written(chose(copy::DATA_HERE, "200 GB"));
        assert_eq!(held(&sized, "size").as_deref(), Some("\"200 GB\""));
        assert_eq!(held(&sized, "disk"), None);

        let other = written(chose(&copy::on_disk("/dev/sdb", copy::DATA_ERASED), ""));
        assert_eq!(held(&other, "disk").as_deref(), Some("\"/dev/sdb\""));
        assert_eq!(held(&other, "size"), None);
        assert_eq!(held(&other, "keepExisting").as_deref(), Some("false"));

        let kept = written(chose(&copy::on_disk("/dev/sdb", copy::DATA_KEPT), ""));
        assert_eq!(held(&kept, "disk").as_deref(), Some("\"/dev/sdb\""));
        assert_eq!(held(&kept, "keepExisting").as_deref(), Some("true"));

        assert!(json::field(&written(Data::default()), "varDisk").is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_tpm_forms_are_shown_and_unpickable_where_there_is_no_tpm() {
        let rows = kinds(false);
        let without: Vec<(&str, &str, bool)> = rows
            .iter()
            .map(|choice| {
                (
                    choice.label.as_str(),
                    choice.detail.as_str(),
                    choice.available,
                )
            })
            .collect();
        assert_eq!(
            without,
            vec![
                (copy::ENC_NONE, "", true),
                (copy::ENC_TPM2, copy::NO_TPM, false),
                (copy::ENC_PASSPHRASE, copy::ENC_ONLY_YOU, true),
                (copy::ENC_BOTH, copy::NO_TPM, false),
            ]
        );
        let with = kinds(true);
        assert!(with.iter().all(|choice| choice.available));
        assert_eq!(with[1].detail, copy::ENC_ANY_HOLDER);
        assert_eq!(with[3].detail, copy::ENC_ANY_HOLDER);
    }

    #[test]
    fn an_encryption_no_backend_takes_is_refused_by_name() {
        let refused = ask_encryption(Some("luks".to_string()), None, &Prompt::silent())
            .err()
            .expect("a refusal");
        assert!(refused.contains("tpm2-luks-passphrase"), "{refused}");
        let kept = ask_encryption(Some("tpm2-luks".to_string()), None, &Prompt::silent())
            .expect("one of the four");
        assert_eq!(kept.kind, "tpm2-luks");
        assert!(kept.passphrase.is_empty());
    }

    #[test]
    fn only_the_disks_a_person_could_install_onto_are_offered() {
        let sys = scratch("sys");
        let block = |name: &str, sectors: &str, model: Option<&str>, removable: &str, ro: &str| {
            let at = sys.join(name);
            std::fs::create_dir_all(at.join("device")).expect("a block device");
            std::fs::write(at.join("size"), sectors).expect("a size");
            std::fs::write(at.join("removable"), removable).expect("a removable");
            std::fs::write(at.join("ro"), ro).expect("a read-only flag");
            if let Some(model) = model {
                std::fs::write(at.join("device/model"), model).expect("a model");
            }
        };
        block(
            "sda",
            "937703088\n",
            Some("Samsung SSD 980\n"),
            "0\n",
            "0\n",
        );
        block("sdb", "60088320\n", None, "1\n", "0\n");
        block("loop0", "204800\n", None, "0\n", "0\n");
        block("sdc", "0\n", None, "1\n", "0\n");
        block(
            "sdd",
            "6291456\n",
            Some("QEMU USB HARDDRIVE\n"),
            "1\n",
            "1\n",
        );
        block("sde", "60088320\n", Some("Cruzer Blade\n"), "1\n", "0\n");
        let part = sys.join("sde/sde1");
        std::fs::create_dir_all(&part).expect("a partition");
        std::fs::write(part.join("partition"), "1\n").expect("a partition number");
        let mounts = "\
/dev/sde1 /run/initramfs/live iso9660 ro,relatime 0 0
tmpfs /run tmpfs rw,nosuid,nodev 0 0
";

        assert_eq!(
            disks(&sys, mounts),
            vec![
                (
                    "/dev/sda".to_string(),
                    "480 GB  Samsung SSD 980".to_string()
                ),
                (
                    "/dev/sdb".to_string(),
                    format!("30 GB  {}", copy::REMOVABLE)
                ),
            ]
        );
        assert!(disks(&sys, "").iter().any(|(disk, _)| disk == "/dev/sde"));
        let _ = std::fs::remove_dir_all(&sys);
    }

    #[test]
    fn more_than_one_labelled_partition_is_named_rather_than_chosen() {
        assert!(labelled("\n").is_empty());
        assert_eq!(labelled("/dev/sdb2\n"), ["/dev/sdb2"]);
        assert_eq!(
            labelled("/dev/sdb2\n/dev/sdc1\n"),
            ["/dev/sdb2", "/dev/sdc1"]
        );
    }

    #[test]
    fn every_event_reads_as_a_line_and_anything_else_passes_through() {
        let said = |line: &str| Event::of(line).say();
        assert_eq!(
            said(
                r#"{"type":"step","step":7,"total_steps":12,"step_name":"install OS","cumulative_pct":9,"weight_pct":87,"elapsed_ms":4210}"#
            ),
            "[  9%] 7/12 install OS"
        );
        assert_eq!(
            said(r#"{"type":"info","message":"Live environment detected"}"#),
            "       Live environment detected"
        );
        assert_eq!(
            said(r#"{"type":"complete","message":"Installation complete"}"#),
            "[100%] Installation complete"
        );
        let key = said(r#"{"type":"recovery_key","key":"abcd-efgh"}"#);
        assert!(
            key.contains("abcd-efgh") && key.contains("write this down"),
            "{key}"
        );
        assert_eq!(said("bootc: pulling layer 3/9"), "bootc: pulling layer 3/9");
    }

    #[test]
    fn the_log_holds_every_event_but_the_key() {
        let logged = |line: &str| Event::of(line).logged();
        assert!(logged(r#"{"type":"recovery_key","key":"abcd-efgh"}"#).is_none());
        assert!(logged(r#"{"type":"info","message":"Live environment"}"#).is_some());
        assert!(logged("bootc: pulling layer 3/9").is_some());
    }

    #[test]
    fn the_confirm_screen_shows_what_is_about_to_be_erased() {
        let answers = Answers {
            disk: "/dev/vda".to_string(),
            hostname: "deb2".to_string(),
            user: "buildc".to_string(),
            password: "hunter2".to_string(),
            encryption: Encryption {
                kind: NONE.to_string(),
                passphrase: String::new(),
            },
            data: Data::default(),
        };
        assert_eq!(
            answers.summary(&Payload {
                recipe: "/mnt/buildc/install-recipe.json".into(),
                image: "ghcr.io/buildc-org/deb2:latest".to_string(),
                hostname: "deb2".to_string(),
                filesystem: "ext4".to_string(),
                bootloader: "grub2".to_string(),
            }),
            vec![
                (copy::ROW_DISK.to_string(), "/dev/vda".to_string()),
                (copy::ROW_HOSTNAME.to_string(), "deb2".to_string()),
                (copy::ROW_ACCOUNT.to_string(), "buildc".to_string()),
                (
                    copy::ROW_PASSWORD.to_string(),
                    copy::PASSWORD_SET.to_string()
                ),
                (copy::ROW_ENCRYPTION.to_string(), copy::ENC_NONE.to_string()),
                (copy::ROW_DATA.to_string(), copy::NONE.to_string()),
                ("esp".to_string(), "2 GB  fat32".to_string()),
                ("/boot".to_string(), "2 GB  ext4".to_string()),
                ("root".to_string(), "the rest  ext4".to_string()),
            ]
        );
        let question = copy::erasing(&answers.disk);
        assert!(
            question.contains("/dev/vda") && question.contains("erased"),
            "{question}"
        );
        assert!(!question.contains("hunter2"));
    }

    #[test]
    fn the_passphrase_is_a_row_only_where_it_is_owed() {
        use crate::ui::{Choice, Field};
        let form = |kind: &str| {
            vec![
                Field::text(copy::ROW_DISK, "/dev/vda"),
                Field::fixed(copy::ROW_LAYOUT, "esp + ext4 /boot + ext4 root"),
                Field::text(copy::ROW_HOSTNAME, "deb2"),
                Field::text(copy::ROW_ACCOUNT, "buildc"),
                Field::secret(copy::ROW_PASSWORD, "hunter2"),
                Field::secret(copy::ROW_CONFIRM, "hunter2"),
                Field::pick(
                    copy::ROW_ENCRYPTION,
                    vec![Choice::new(shown(kind), "")],
                    Some(0),
                ),
                Field::secret(copy::ROW_PASSPHRASE, ""),
                Field::pick(copy::ROW_DATA, vec![Choice::new(copy::NONE, "")], Some(0)),
                Field::text(copy::ROW_SIZE, ""),
            ]
        };
        assert!(!asked(&form(NONE)).contains(&ROW_PASSPHRASE));
        assert!(!asked(&form("tpm2-luks")).contains(&ROW_PASSPHRASE));
        assert!(asked(&form("luks-passphrase")).contains(&ROW_PASSPHRASE));
        assert!(asked(&form("tpm2-luks-passphrase")).contains(&ROW_PASSPHRASE));
        let mut sized = form(NONE);
        assert!(!asked(&sized).contains(&ROW_SIZE));
        sized[ROW_DATA] = Field::pick(
            copy::ROW_DATA,
            vec![Choice::new(copy::DATA_HERE, "")],
            Some(0),
        );
        assert!(asked(&sized).contains(&ROW_SIZE));
        assert_eq!(asked(&form(NONE)).len(), 8);
    }

    #[test]
    fn the_action_says_what_the_form_is_short_of() {
        use crate::ui::{Choice, Field};
        let form = |password: &str, confirm: &str, kind: &str, passphrase: &str| {
            vec![
                Field::text(copy::ROW_DISK, "/dev/vda"),
                Field::fixed(copy::ROW_LAYOUT, "esp + ext4 /boot + ext4 root"),
                Field::text(copy::ROW_HOSTNAME, "deb2"),
                Field::text(copy::ROW_ACCOUNT, "buildc"),
                Field::secret(copy::ROW_PASSWORD, password),
                Field::secret(copy::ROW_CONFIRM, confirm),
                Field::pick(
                    copy::ROW_ENCRYPTION,
                    vec![Choice::new(shown(kind), "")],
                    Some(0),
                ),
                Field::secret(copy::ROW_PASSPHRASE, passphrase),
                Field::pick(copy::ROW_DATA, vec![Choice::new(copy::NONE, "")], Some(0)),
                Field::text(copy::ROW_SIZE, ""),
            ]
        };
        assert!(short_of(&form("hunter2", "hunter2", NONE, "")).is_none());
        let differ = short_of(&form("hunter2", "hunter3", NONE, "")).unwrap();
        assert_eq!(differ, copy::NO_MATCH_ROW);
        assert!(short_of(&form("hunter2", "hunter2", "luks-passphrase", "")).is_some());
        assert!(short_of(&form("hunter2", "hunter2", "luks-passphrase", "x")).is_none());
        let mut bare = form("", "", NONE, "");
        bare[ROW_ACCOUNT] = Field::text(copy::ROW_ACCOUNT, "");
        let short = short_of(&bare).unwrap();
        assert!(
            short.contains(copy::ROW_ACCOUNT) && short.contains(copy::ROW_PASSWORD),
            "{short}"
        );

        let data = |form: &mut Vec<crate::ui::Field>, label: String, size: &str| {
            form[ROW_DATA] = Field::pick(copy::ROW_DATA, vec![Choice::new(label, "")], Some(0));
            form[ROW_SIZE] = Field::text(copy::ROW_SIZE, size);
        };
        let mut sized = form("hunter2", "hunter2", NONE, "");
        data(&mut sized, copy::DATA_HERE.to_string(), "");
        assert!(short_of(&sized).unwrap().contains(copy::ROW_SIZE));
        data(&mut sized, copy::DATA_HERE.to_string(), "200 GB");
        assert!(short_of(&sized).is_none());

        let mut encrypted = form("hunter2", "hunter2", "tpm2-luks", "");
        data(&mut encrypted, copy::DATA_HERE.to_string(), "200 GB");
        assert_eq!(
            short_of(&encrypted).as_deref(),
            Some(copy::DATA_UNENCRYPTED)
        );

        let mut same = form("hunter2", "hunter2", NONE, "");
        data(&mut same, copy::on_disk("/dev/vda", copy::DATA_ERASED), "");
        assert_eq!(short_of(&same).as_deref(), Some(copy::DATA_SAME_DISK));
    }

    #[test]
    fn a_root_with_nothing_built_on_it_refuses_by_name() {
        let nothing = Found::Nothing("/mnt/buildc".into()).payload().unwrap_err();
        assert!(nothing.contains(RECIPE) && nothing.contains(crate::layout::REPO_FILE));
        assert!(nothing.contains("/mnt/buildc"), "{nothing}");
        let repo = Found::Repo("/mnt/buildc".into()).payload().unwrap_err();
        assert!(repo.contains("buildc build"), "{repo}");
    }

    #[test]
    #[ignore]
    fn holds_a_lock_for_another_process_to_find() {
        let Ok(path) = std::env::var("BUILDC_TEST_LOCK") else {
            return;
        };
        let _held = hold_at(Path::new(&path)).expect("the child takes the lock");
        std::fs::write(format!("{path}.taken"), "").expect("the child reports it");
        std::thread::sleep(std::time::Duration::from_secs(30));
    }

    #[test]
    fn a_second_installer_is_refused_and_told_where_the_first_one_is() {
        let path = std::env::temp_dir().join(format!("buildc-lock-{}", std::process::id()));
        let taken = PathBuf::from(format!("{}.taken", path.display()));
        let _ = std::fs::remove_file(&taken);

        let mut child = Command::new(std::env::current_exe().expect("the test binary"))
            .args([
                "--ignored",
                "--exact",
                "install::tests::holds_a_lock_for_another_process_to_find",
            ])
            .env("BUILDC_TEST_LOCK", &path)
            .stdout(Stdio::null())
            .spawn()
            .expect("the test binary re-runs itself as the holder");
        for _ in 0..200 {
            if taken.exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        assert!(taken.exists(), "the holder never took the lock");

        let refused = hold_at(&path).expect_err("a second installer is refused");
        assert!(
            refused.starts_with("the installer is running on "),
            "{refused}"
        );
        assert!(refused.contains("again once it finishes"), "{refused}");
        child.kill().expect("the holder is killed");
        child.wait().expect("the holder is reaped");
        assert!(
            hold_at(&path).is_ok(),
            "the lock outlived the process holding it"
        );

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&taken);
    }

    #[test]
    fn a_pty_holder_is_named_as_the_graphical_console() {
        assert_eq!(name(Some("/dev/ttyS0".into())), "/dev/ttyS0");
        assert_eq!(name(Some("/dev/tty1".into())), "/dev/tty1");
        assert_eq!(name(Some("/dev/console".into())), "/dev/console");
        assert_eq!(
            name(Some("/dev/pts/0".into())),
            "another session (/dev/pts/0)"
        );
        assert_eq!(name(Some("/dev/null".into())), "another console");
        assert_eq!(name(Some("/proc/1/fd/0".into())), "another console");
        assert_eq!(name(None), "another console");
        assert_eq!(console(-1), "another console");
    }
}
