use crate::copy;
use crate::emit::json::{self, Json};
use crate::prompt::Prompt;
use crate::ui::Choice;
use std::io::{BufRead as _, Read as _, Write as _};
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
    pub boot: String,
    pub luks_initramfs: bool,
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
            boot: told("boot"),
            luks_initramfs: matches!(json::field(&doc, "luksInitramfs"), Some(Json::Bool(true))),
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

#[derive(Clone, Debug, PartialEq)]
struct CustomMount {
    partition: String,
    target: String,
    fstype: String,
}

#[derive(Clone, PartialEq)]
enum Key {
    Passphrase(String),
    File(PathBuf),
    Data(Vec<u8>),
}

impl Key {
    fn bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Passphrase(passphrase) => Some(passphrase.as_bytes()),
            Self::Data(bytes) => Some(bytes),
            Self::File(_) => None,
        }
    }
}

impl std::fmt::Debug for Key {
    fn fmt(&self, form: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Passphrase(_) => form.write_str("Passphrase(***)"),
            Self::Data(_) => form.write_str("Data(***)"),
            Self::File(path) => form.debug_tuple("File").field(path).finish(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct LuksOpen {
    partition: String,
    target: String,
    key: Key,
}

#[derive(Clone, Debug, PartialEq)]
struct Opening {
    partition: String,
    target: String,
}

#[derive(Clone, Debug, PartialEq)]
struct Mounted {
    target: String,
    fstype: String,
}

const OPEN: &str = "open";

#[derive(Clone, Debug, PartialEq)]
struct CustomLayout {
    disk: String,
    mounts: Vec<CustomMount>,
    opens: Vec<LuksOpen>,
}

#[derive(Debug, PartialEq)]
struct Partition {
    device: String,
    size: String,
    fstype: String,
    label: String,
    parttype: String,
    uuid: String,
}

impl CustomLayout {
    fn summary(&self) -> String {
        copy::custom_layout(
            &self.disk,
            self.mounts.len(),
            self.mounts
                .iter()
                .filter(|mount| mount.fstype == "unformatted")
                .count(),
            self.opens.len(),
        )
    }

    fn answer(&self, partition: &Partition) -> Option<Mounted> {
        self.mounts
            .iter()
            .find(|mount| mount.partition == partition.device)
            .map(|mount| Mounted {
                target: mount.target.clone(),
                fstype: mount.fstype.clone(),
            })
            .or_else(|| {
                self.opens
                    .iter()
                    .find(|open| open.partition == partition.device)
                    .map(|open| Mounted {
                        target: open.target.clone(),
                        fstype: OPEN.to_string(),
                    })
            })
    }

    fn mappers(&self) -> Vec<(String, &LuksOpen)> {
        self.opens
            .iter()
            .enumerate()
            .map(|(at, open)| (format!("buildc-{}", at + 1), open))
            .collect()
    }

    fn opens_the_root(&self) -> bool {
        self.opens.iter().any(|open| open.target == "/")
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Slots {
    keys: Vec<u32>,
    tokens: Vec<String>,
}

impl Slots {
    fn has_tpm2(&self) -> bool {
        self.tokens.iter().any(|kind| kind == "systemd-tpm2")
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Opened {
    Keep,
    Tpm2,
    AddKey,
}

impl Opened {
    fn of(shown: &str) -> Self {
        match shown {
            copy::OPENED_TPM2 => Self::Tpm2,
            copy::OPENED_ADD_KEY => Self::AddKey,
            _ => Self::Keep,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Keep => copy::OPENED_KEEP,
            Self::Tpm2 => copy::OPENED_TPM2,
            Self::AddKey => copy::OPENED_ADD_KEY,
        }
    }
}

fn at_boot(open: &LuksOpen, opened: Opened) -> &'static str {
    if opened == Opened::Tpm2 {
        return copy::BOOT_TPM2;
    }
    if open.target == "/" {
        return match open.key {
            Key::Passphrase(_) => copy::BOOT_PASSPHRASE,
            _ => copy::OPENED_ROOT_KEYFILE,
        };
    }
    match (&open.key, opened) {
        (Key::Passphrase(_), Opened::AddKey) => copy::BOOT_ADDED_KEY,
        (Key::Passphrase(_), _) => copy::BOOT_PASSPHRASE,
        _ => copy::BOOT_KEYFILE,
    }
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
    layout: Option<CustomLayout>,
    opened: Opened,
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
        let mut layout = seeded.layout.clone();
        let mut fields = seeded.fields(
            &payload.filesystem,
            &payload.bootloader,
            &payload.boot,
            payload.luks_initramfs,
        );
        loop {
            let actions = [copy::INSTALL, copy::SHUT_DOWN];
            let filled = crate::ui::form(
                &mut fields,
                &actions,
                |fields| short_of(fields, layout.as_ref(), payload.luks_initramfs),
                |fields| asked(fields, layout.is_some()),
                copy::INSTALL_KEYS,
            );
            match filled {
                Ok(crate::ui::Filled::Took(0)) => {
                    let answers = Self::of(&fields, layout.clone());
                    if crate::ui::confirm_over(
                        &match answers.layout.is_some() {
                            true => copy::changing_partitions(&answers.disk),
                            false => copy::erasing(&answers.disk),
                        },
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
                Ok(crate::ui::Filled::Opened(ROW_LAYOUT)) => {
                    let disk = fields[ROW_DISK].value();
                    let automatic =
                        || copy::layout(&payload.filesystem, &payload.bootloader, &payload.boot);
                    let summary = if disk.is_empty() {
                        format!("{}; {}", automatic(), copy::CUSTOM_NEEDS_DISK)
                    } else {
                        if layout.as_ref().is_some_and(|held| held.disk != disk) {
                            layout = None;
                        }
                        let discovered =
                            partitions(&disk).and_then(|partitions| match partitions.is_empty() {
                                true => Err(format!(
                                    "{disk} has no partitions to use in a custom layout"
                                )),
                                false => Ok(partitions),
                            });
                        match discovered {
                            Ok(partitions) => {
                                let found = old_keys(&disk, &partitions);
                                match edit_layout(
                                    &disk,
                                    &payload.filesystem,
                                    &payload.bootloader,
                                    payload.luks_initramfs,
                                    &partitions,
                                    &mut layout,
                                    &found,
                                ) {
                                    Ok(()) => {}
                                    Err(err) if leaving(&err) => match leave(prompt)? {
                                        Leave::Shell => return Ok(None),
                                        Leave::Over => {
                                            layout = None;
                                            fields =
                                                Self::seeded(payload, Given::default(), prompt)?
                                                    .fields(
                                                        &payload.filesystem,
                                                        &payload.bootloader,
                                                        &payload.boot,
                                                        payload.luks_initramfs,
                                                    );
                                            continue;
                                        }
                                        Leave::Back => continue,
                                    },
                                    Err(err) => return Err(err),
                                }
                                layout
                                    .as_ref()
                                    .map_or_else(automatic, CustomLayout::summary)
                            }
                            Err(err) => format!(
                                "{}; {err}",
                                layout
                                    .as_ref()
                                    .map_or_else(automatic, CustomLayout::summary)
                            ),
                        }
                    };
                    fields[ROW_LAYOUT] = crate::ui::Field::action(copy::ROW_LAYOUT, &summary);
                    fields[ROW_ENCRYPTION] = encryption_row(
                        layout.as_ref(),
                        &fields[ROW_ENCRYPTION],
                        tpm().exists(),
                        payload.luks_initramfs,
                    );
                }
                Ok(crate::ui::Filled::Opened(_)) => {}
                Ok(crate::ui::Filled::Left) => match leave(prompt)? {
                    Leave::Shell => return Ok(None),
                    Leave::Over => {
                        layout = None;
                        fields = Self::seeded(payload, Given::default(), prompt)?.fields(
                            &payload.filesystem,
                            &payload.bootloader,
                            &payload.boot,
                            payload.luks_initramfs,
                        )
                    }
                    Leave::Back => {}
                },
                Err(err) if leaving(&err) => match leave(prompt)? {
                    Leave::Shell => return Ok(None),
                    Leave::Over => {
                        layout = None;
                        fields = Self::seeded(payload, Given::default(), prompt)?.fields(
                            &payload.filesystem,
                            &payload.bootloader,
                            &payload.boot,
                            payload.luks_initramfs,
                        )
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
                encryption: ask_encryption(
                    given.encryption,
                    given.passphrase,
                    prompt,
                    payload.luks_initramfs,
                )?,
                data: Data::default(),
                layout: None,
                opened: Opened::Keep,
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
            layout: None,
            opened: Opened::Keep,
        })
    }

    fn fields(
        &self,
        filesystem: &str,
        bootloader: &str,
        boot: &str,
        luks_initramfs: bool,
    ) -> Vec<crate::ui::Field> {
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
            Field::action(
                copy::ROW_LAYOUT,
                &self.layout.as_ref().map_or_else(
                    || copy::layout(filesystem, bootloader, boot),
                    CustomLayout::summary,
                ),
            ),
            Field::text(copy::ROW_HOSTNAME, &self.hostname),
            Field::text(copy::ROW_ACCOUNT, &self.user),
            Field::secret(copy::ROW_PASSWORD, &self.password),
            Field::secret(copy::ROW_CONFIRM, &self.password),
            Field::pick(
                copy::ROW_ENCRYPTION,
                kinds(tpm().exists(), luks_initramfs),
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

    fn of(fields: &[crate::ui::Field], layout: Option<CustomLayout>) -> Self {
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
            data: match layout {
                Some(_) => Data::default(),
                None => chose(&at(ROW_DATA), &at(ROW_SIZE)),
            },
            opened: match layout.as_ref().filter(|layout| !layout.opens.is_empty()) {
                Some(_) => Opened::of(&at(ROW_ENCRYPTION)),
                None => Opened::Keep,
            },
            layout,
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
        ];
        if self.layout.is_none() {
            rows.push((
                copy::ROW_DATA.to_string(),
                data_said(&self.data, self.encryption.kind != NONE),
            ));
        }
        match &self.layout {
            Some(layout) => {
                rows.extend(layout.mounts.iter().map(|mount| {
                    let how = match mount.fstype.as_str() {
                        "unformatted" => format!("{}  kept", mount.partition),
                        fstype => format!("{}  format as {fstype}", mount.partition),
                    };
                    (mount.target.clone(), how)
                }));
                rows.extend(layout.opens.iter().map(|open| {
                    (
                        open.target.clone(),
                        copy::opened(&open.partition, at_boot(open, self.opened)),
                    )
                }));
                if let Some(chain) = copy::boot_chain(&payload.boot) {
                    rows.push(("boot chain".to_string(), chain.to_string()));
                }
            }
            None => rows.extend(copy::written_over(
                &payload.bootloader,
                &payload.filesystem,
                &self.data.size,
                &payload.boot,
                self.encryption.kind != NONE,
            )),
        }
        rows
    }
}

const ROW_DISK: usize = 0;
const ROW_LAYOUT: usize = 1;
const ROW_HOSTNAME: usize = 2;
const ROW_ACCOUNT: usize = 3;
const ROW_PASSWORD: usize = 4;
const ROW_CONFIRM: usize = 5;
const ROW_ENCRYPTION: usize = 6;
const ROW_PASSPHRASE: usize = 7;
const ROW_DATA: usize = 8;
const ROW_SIZE: usize = 9;

fn asked(fields: &[crate::ui::Field], custom: bool) -> Vec<usize> {
    let wants = Encryption::wants_passphrase(written(&fields[ROW_ENCRYPTION].value()));
    let sized = fields[ROW_DATA].value() == copy::DATA_HERE;
    (0..fields.len())
        .filter(|row| *row != ROW_PASSPHRASE || wants)
        .filter(|row| !custom || (*row != ROW_DATA && *row != ROW_SIZE))
        .filter(|row| custom || *row != ROW_SIZE || sized)
        .collect()
}

fn short_of(
    fields: &[crate::ui::Field],
    layout: Option<&CustomLayout>,
    luks_initramfs: bool,
) -> Option<String> {
    let at = |row: usize| fields[row].value();
    if !at(ROW_PASSWORD).is_empty() && at(ROW_PASSWORD) != at(ROW_CONFIRM) {
        return Some(copy::NO_MATCH_ROW.to_string());
    }
    if let Some(layout) = layout {
        if layout.disk != at(ROW_DISK) {
            return Some(copy::CUSTOM_OTHER_DISK.to_string());
        }
        if layout.opens_the_root() && !luks_initramfs {
            return Some(copy::NO_LUKS_INITRAMFS.to_string());
        }
        if written(&at(ROW_ENCRYPTION)) != NONE {
            return Some(copy::CUSTOM_ENCRYPTION.to_string());
        }
        if layout
            .opens
            .iter()
            .any(|open| open.target == "/" && !matches!(open.key, Key::Passphrase(_)))
        {
            return Some(copy::OPENED_ROOT_KEYFILE.to_string());
        }
        if !layout.opens_the_root() && Opened::of(&at(ROW_ENCRYPTION)) != Opened::Keep {
            return Some(copy::OPENED_KEYFILE_PLAIN.to_string());
        }
    }
    if written(&at(ROW_ENCRYPTION)) != NONE && !luks_initramfs {
        return Some(copy::NO_LUKS_INITRAMFS.to_string());
    }
    let data = match layout {
        Some(_) => Data::default(),
        None => chose(&at(ROW_DATA), &at(ROW_SIZE)),
    };
    if written(&at(ROW_ENCRYPTION)) != NONE && data.wanted() && data.keep {
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

fn edit_layout(
    disk: &str,
    filesystem: &str,
    bootloader: &str,
    luks_initramfs: bool,
    partitions: &[Partition],
    held: &mut Option<CustomLayout>,
    found: &Discovered,
) -> Result<(), String> {
    loop {
        let mut fields: Vec<crate::ui::Field> = partitions
            .iter()
            .map(|partition| {
                let options = mount_choices(partition, filesystem, bootloader, luks_initramfs);
                let previous = held.as_ref().and_then(|layout| layout.answer(partition));
                let at = previous
                    .and_then(|previous| {
                        options.iter().position(|option| {
                            option.available
                                && mounted_answer(&option.label) == Some(previous.clone())
                        })
                    })
                    .or_else(|| found.default_at(partition, &options))
                    .or_else(|| {
                        is_esp(partition)
                            .then(|| copy::keep_as("/boot/efi"))
                            .and_then(|default| {
                                options.iter().position(|option| option.label == default)
                            })
                    })
                    .unwrap_or(0);
                crate::ui::Field::pick(&partition_label(partition), options, Some(at))
            })
            .collect();
        let actions = [copy::USE_LAYOUT, copy::AUTOMATIC_LAYOUT, copy::GO_BACK];
        match crate::ui::form(
            &mut fields,
            &actions,
            layout_short_of,
            |fields| (0..fields.len()).collect(),
            copy::INSTALL_KEYS,
        )? {
            crate::ui::Filled::Took(0) => {
                let (mounts, openings) = layout_from(partitions, &fields);
                match opens_from(&openings, held.as_ref(), found) {
                    Ok(opens) => {
                        *held = Some(CustomLayout {
                            disk: disk.to_string(),
                            mounts,
                            opens,
                        });
                        return Ok(());
                    }
                    Err(err) if leaving(&err) => continue,
                    Err(err) => return Err(err),
                }
            }
            crate::ui::Filled::Took(1) => {
                *held = None;
                return Ok(());
            }
            crate::ui::Filled::Took(_) | crate::ui::Filled::Opened(_) => {}
            crate::ui::Filled::Left => return Ok(()),
        }
    }
}

fn partition_label(partition: &Partition) -> String {
    [
        partition.device.as_str(),
        partition.size.as_str(),
        partition.fstype.as_str(),
        partition.label.as_str(),
    ]
    .into_iter()
    .filter(|part| !part.is_empty())
    .collect::<Vec<_>>()
    .join("  ")
}

const LINUX_FILESYSTEMS: [&str; 4] = ["ext3", "ext4", "xfs", "btrfs"];

fn is_fat(fstype: &str) -> bool {
    ["vfat", "fat", "fat32"].contains(&fstype)
}

fn is_esp(partition: &Partition) -> bool {
    is_fat(&partition.fstype)
        && (partition
            .parttype
            .eq_ignore_ascii_case("c12a7328-f81f-11d2-ba4b-00a0c93ec93b")
            || partition.parttype.eq_ignore_ascii_case("0xef"))
}

fn mount_choices(
    partition: &Partition,
    filesystem: &str,
    bootloader: &str,
    luks_initramfs: bool,
) -> Vec<Choice> {
    let mut choices = vec![Choice::new(copy::LEAVE_PARTITION, "")];
    for (target, format) in [
        ("/", filesystem),
        ("/boot", "ext4"),
        ("/boot/efi", "fat32"),
        ("/var", filesystem),
    ] {
        if partition.fstype == "crypto_LUKS" && target != "/boot/efi" {
            let choice = Choice::new(copy::open_at(target), copy::open_cost(target));
            choices.push(match target {
                "/" if !luks_initramfs => {
                    Choice::new(copy::open_at(target), copy::NO_LUKS_INITRAMFS).unavailable()
                }
                "/boot" if bootloader == "systemd" => {
                    Choice::new(copy::open_at(target), copy::custom_keep_boot(bootloader))
                        .unavailable()
                }
                "/boot" => Choice::new(copy::open_at(target), copy::CUSTOM_OPEN_BOOT).unavailable(),
                _ => choice,
            });
        }
        if !partition.fstype.is_empty() && partition.fstype != "crypto_LUKS" {
            let choice = Choice::new(copy::keep_as(target), "preserve its filesystem");
            choices.push(match target {
                "/boot" if bootloader == "systemd" || partition.fstype != "ext4" => {
                    Choice::new(copy::keep_as(target), copy::custom_keep_boot(bootloader))
                        .unavailable()
                }
                "/boot/efi" if !is_fat(&partition.fstype) => {
                    Choice::new(copy::keep_as(target), copy::CUSTOM_KEEP_ESP).unavailable()
                }
                "/" | "/var" if !LINUX_FILESYSTEMS.contains(&partition.fstype.as_str()) => {
                    Choice::new(copy::keep_as(target), copy::CUSTOM_KEEP_FILESYSTEM).unavailable()
                }
                _ => choice,
            });
        }
        let choice = Choice::new(copy::format_at(format, target), "erase this partition");
        choices.push(match target {
            "/boot" if bootloader == "systemd" => Choice::new(
                copy::format_at(format, target),
                copy::custom_keep_boot(bootloader),
            )
            .unavailable(),
            "/" | "/var" if !LINUX_FILESYSTEMS.contains(&format) => Choice::new(
                copy::format_at(format, target),
                copy::CUSTOM_FORMAT_FILESYSTEM,
            )
            .unavailable(),
            _ => choice,
        });
    }
    choices
}

fn mounted_answer(answer: &str) -> Option<Mounted> {
    if let Some(target) = answer.strip_prefix("open it as ") {
        return Some(Mounted {
            target: target.to_string(),
            fstype: OPEN.to_string(),
        });
    }
    if let Some(target) = answer.strip_prefix("keep as ") {
        return Some(Mounted {
            target: target.to_string(),
            fstype: "unformatted".to_string(),
        });
    }
    let (fstype, target) = answer.strip_prefix("format as ")?.split_once(" at ")?;
    Some(Mounted {
        target: target.to_string(),
        fstype: fstype.to_string(),
    })
}

fn layout_from(
    partitions: &[Partition],
    fields: &[crate::ui::Field],
) -> (Vec<CustomMount>, Vec<Opening>) {
    let mut mounts = Vec::new();
    let mut openings = Vec::new();
    for (partition, field) in partitions.iter().zip(fields) {
        let Some(answer) = mounted_answer(&field.value()) else {
            continue;
        };
        match answer.fstype.as_str() {
            OPEN => openings.push(Opening {
                partition: partition.device.clone(),
                target: answer.target,
            }),
            fstype => mounts.push(CustomMount {
                partition: partition.device.clone(),
                target: answer.target,
                fstype: fstype.to_string(),
            }),
        }
    }
    (mounts, openings)
}

fn usable_key(key: &Key, encrypted_root: bool) -> bool {
    encrypted_root || matches!(key, Key::Passphrase(_))
}

fn opens_from(
    openings: &[Opening],
    held: Option<&CustomLayout>,
    found: &Discovered,
) -> Result<Vec<LuksOpen>, String> {
    let encrypted_root = openings.iter().any(|opening| opening.target == "/");
    let mut opens = Vec::new();
    for opening in openings {
        let usable = |key: &Key| opening.target == "/" || usable_key(key, encrypted_root);
        let carried = held
            .and_then(|layout| {
                layout.opens.iter().find(|open| {
                    open.partition == opening.partition && open.target == opening.target
                })
            })
            .map(|open| open.key.clone())
            .filter(usable);
        let found_here = found.key(&opening.partition).cloned().filter(usable);
        let key = match carried.or(found_here) {
            Some(key) => key,
            None => ask_key(opening, found.why(&opening.partition), !encrypted_root)?,
        };
        opens.push(LuksOpen {
            partition: opening.partition.clone(),
            target: opening.target.clone(),
            key,
        });
    }
    Ok(opens)
}

fn ask_key(opening: &Opening, why: Option<&str>, plain_root: bool) -> Result<Key, String> {
    let refused = plain_root.then_some(copy::OPENED_KEYFILE_PLAIN);
    let mut fields = vec![
        crate::ui::Field::pick(
            &copy::open_question(&opening.partition, &opening.target),
            key_methods(refused),
            Some(0),
        ),
        crate::ui::Field::secret(copy::ROW_PASSPHRASE, ""),
        crate::ui::Field::text(copy::KEY_FILE_PATH, ""),
    ];
    if let Some(why) = why {
        fields.push(crate::ui::Field::fixed(copy::OLD_SYSTEM, why));
    }
    let reason = fields.len() > 3;
    let actions = [copy::USE_KEY];
    let filled = crate::ui::form(
        &mut fields,
        &actions,
        |fields| key_short_of(&fields[0].value(), &fields[1].value(), &fields[2].value()),
        |fields| {
            let mut rows = key_asked(&fields[0].value());
            if reason {
                rows.push(3);
            }
            rows
        },
        copy::INSTALL_KEYS,
    )?;
    match filled {
        crate::ui::Filled::Took(0) => Ok(key_of(
            &fields[0].value(),
            &fields[1].value(),
            &fields[2].value(),
        )),
        _ => Err(crate::ui::INTERRUPTED.to_string()),
    }
}

fn key_methods(refused: Option<&str>) -> Vec<Choice> {
    vec![
        Choice::new(copy::KEY_PASSPHRASE, copy::KEY_PASSPHRASE_COST),
        match refused {
            Some(why) => Choice::new(copy::KEY_FILE, why).unavailable(),
            None => Choice::new(copy::KEY_FILE, copy::KEY_FILE_COST),
        },
    ]
}

fn key_asked(method: &str) -> Vec<usize> {
    match method == copy::KEY_FILE {
        true => vec![0, 2],
        false => vec![0, 1],
    }
}

fn key_short_of(method: &str, passphrase: &str, path: &str) -> Option<String> {
    match method == copy::KEY_FILE {
        true if path.is_empty() => Some(copy::still_needs(&[copy::KEY_FILE_PATH])),
        true if !Path::new(path).is_file() => Some(copy::KEY_FILE_MISSING.to_string()),
        true => None,
        false if passphrase.is_empty() => Some(copy::still_needs(&[copy::ROW_PASSPHRASE])),
        false => None,
    }
}

fn key_of(method: &str, passphrase: &str, path: &str) -> Key {
    match method == copy::KEY_FILE {
        true => Key::File(PathBuf::from(path)),
        false => Key::Passphrase(passphrase.to_string()),
    }
}

fn layout_short_of(fields: &[crate::ui::Field]) -> Option<String> {
    let targets: Vec<String> = fields
        .iter()
        .filter_map(|field| mounted_answer(&field.value()).map(|mount| mount.target))
        .collect();
    if !targets.iter().any(|target| target == "/") {
        return Some(copy::CUSTOM_ROOT.to_string());
    }
    for (at, target) in targets.iter().enumerate() {
        if targets[..at].contains(target) {
            return Some(copy::CUSTOM_DUPLICATE.to_string());
        }
    }
    None
}

fn partitions(disk: &str) -> Result<Vec<Partition>, String> {
    let out = Command::new("lsblk")
        .args([
            "--json",
            "--paths",
            "--output",
            "NAME,SIZE,FSTYPE,LABEL,TYPE,PARTTYPE,UUID",
            disk,
        ])
        .output()
        .map_err(|err| format!("lsblk {disk}: {err}"))?;
    if !out.status.success() {
        return Err(format!(
            "lsblk {disk}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    partition_rows(&String::from_utf8_lossy(&out.stdout))
}

fn partition_rows(raw: &str) -> Result<Vec<Partition>, String> {
    fn visit(node: &Json, found: &mut Vec<Partition>) {
        if json::text(node, "type").as_deref() == Some("part") {
            found.push(Partition {
                device: json::text(node, "name").unwrap_or_default(),
                size: json::text(node, "size").unwrap_or_default(),
                fstype: json::text(node, "fstype").unwrap_or_default(),
                label: json::text(node, "label").unwrap_or_default(),
                parttype: json::text(node, "parttype").unwrap_or_default(),
                uuid: json::text(node, "uuid").unwrap_or_default(),
            });
        }
        for child in json::items(node, "children") {
            visit(child, found);
        }
    }

    let doc = Json::parse(raw).map_err(|err| format!("lsblk wrote invalid JSON: {err}"))?;
    let mut found = Vec::new();
    for block in json::items(&doc, "blockdevices") {
        visit(block, &mut found);
    }
    Ok(found)
}

#[derive(Debug)]
struct OldSystem {
    at: PathBuf,
    crypttab: String,
    fstab: String,
}

#[derive(Clone, Debug, PartialEq)]
enum KeySource {
    Inside(PathBuf),
    Default,
    OnDevice { device: String, path: PathBuf },
    Waited,
    Unreadable,
}

#[derive(Clone, Debug, PartialEq)]
struct Crypttab {
    name: String,
    device: String,
    key: KeySource,
}

fn device_spec(said: &str) -> bool {
    said.starts_with("/dev/")
        || ["UUID=", "PARTUUID=", "LABEL="]
            .iter()
            .any(|prefix| said.starts_with(prefix))
}

fn on_device(said: &str, passdev: bool) -> Option<KeySource> {
    let (first, rest) = said.split_once(':')?;
    let (device, path, timed) = match (device_spec(first), passdev) {
        (true, _) => (first, rest, true),
        (false, false) if device_spec(rest) => (rest, first, false),
        _ => return None,
    };
    let path = match timed {
        true => path.split_once(':').map_or(path, |(path, _timeout)| path),
        false => path,
    };
    Some(KeySource::OnDevice {
        device: device.to_string(),
        path: PathBuf::from(path),
    })
}

fn crypttabs(raw: &str) -> Vec<Crypttab> {
    let mut found = Vec::new();
    for line in raw.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let mut words = line.split_whitespace();
        let (Some(name), Some(device)) = (words.next(), words.next()) else {
            continue;
        };
        let field = words.next().unwrap_or("none");
        let options = words.next().unwrap_or("");
        let keyscript = options
            .split(',')
            .find_map(|option| option.strip_prefix("keyscript="));
        let timed = options
            .split(',')
            .any(|option| option.starts_with("keyfile-timeout="));
        let key = match keyscript {
            Some(script) if script.rsplit('/').next() == Some("passdev") => {
                on_device(field, true).unwrap_or(KeySource::Unreadable)
            }
            Some(_) => KeySource::Unreadable,
            None if field == "none" || field == "-" => KeySource::Default,
            None => match on_device(field, false) {
                Some(device) => device,
                None if timed => KeySource::Waited,
                None => KeySource::Inside(PathBuf::from(field)),
            },
        };
        found.push(Crypttab {
            name: name.to_string(),
            device: device.to_string(),
            key,
        });
    }
    found
}

fn names(entry: &Crypttab, container: &Partition) -> bool {
    let said = entry.device.as_str();
    if let Some(uuid) = said.strip_prefix("UUID=") {
        return !container.uuid.is_empty() && uuid.eq_ignore_ascii_case(&container.uuid);
    }
    if let Some(uuid) = said.strip_prefix("/dev/disk/by-uuid/") {
        return !container.uuid.is_empty() && uuid.eq_ignore_ascii_case(&container.uuid);
    }
    said == container.device
}

fn fstab_target(raw: &str, name: &str) -> String {
    let mapper = format!("/dev/mapper/{name}");
    for line in raw.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let mut words = line.split_whitespace();
        let (Some(device), Some(target)) = (words.next(), words.next()) else {
            continue;
        };
        if device == mapper {
            return target.to_string();
        }
    }
    String::new()
}

const MOUNTS: &str = "/run/buildc-old";

fn mounts_root() -> PathBuf {
    std::env::var_os("BUILDC_MOUNT_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(MOUNTS))
}

#[derive(Default)]
struct Mounts(Vec<PathBuf>);

impl Drop for Mounts {
    fn drop(&mut self) {
        for at in &self.0 {
            let _ = Command::new("umount").arg(at).status();
        }
    }
}

fn mount_options(fstype: &str) -> &'static str {
    match LINUX_FILESYSTEMS.contains(&fstype) {
        true => "ro,norecovery",
        false => "ro",
    }
}

fn mount_ro(device: &Path, fstype: &str, mounts: &mut Mounts) -> Result<PathBuf, String> {
    let at = mounts_root().join(format!("old-{}", mounts.0.len() + 1));
    std::fs::create_dir_all(&at).map_err(|err| format!("{}: {err}", at.display()))?;
    let options = mount_options(fstype);
    let out = Command::new("mount")
        .args(["-o", options])
        .arg(device)
        .arg(&at)
        .output()
        .map_err(|err| format!("mount: {err}, and it is what reads {}", device.display()))?;
    if !out.status.success() {
        return Err(format!(
            "mounting {} read-only: {}",
            device.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    mounts.0.push(at.clone());
    Ok(at)
}

fn mountable_rows(raw: &str) -> Result<Vec<(String, String)>, String> {
    fn visit(node: &Json, found: &mut Vec<(String, String)>) {
        let kind = json::text(node, "type").unwrap_or_default();
        let fstype = json::text(node, "fstype").unwrap_or_default();
        if ["part", "crypt", "lvm"].contains(&kind.as_str())
            && !fstype.is_empty()
            && fstype != "crypto_LUKS"
        {
            found.push((json::text(node, "name").unwrap_or_default(), fstype));
        }
        for child in json::items(node, "children") {
            visit(child, found);
        }
    }
    let doc = Json::parse(raw).map_err(|err| format!("lsblk wrote invalid JSON: {err}"))?;
    let mut found = Vec::new();
    for block in json::items(&doc, "blockdevices") {
        visit(block, &mut found);
    }
    Ok(found)
}

fn mountables(disk: &str) -> Result<Vec<(String, String)>, String> {
    let out = Command::new("lsblk")
        .args(["--json", "--paths", "--output", "NAME,FSTYPE,TYPE", disk])
        .output()
        .map_err(|err| format!("lsblk {disk}: {err}"))?;
    if !out.status.success() {
        return Err(format!(
            "lsblk {disk}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    mountable_rows(&String::from_utf8_lossy(&out.stdout))
}

fn old_systems(disk: &str, mounts: &mut Mounts) -> (Vec<OldSystem>, Vec<String>) {
    let mut systems = Vec::new();
    let mut unread = Vec::new();
    match mountables(disk) {
        Ok(devices) => {
            for (device, fstype) in devices {
                let at = match mount_ro(Path::new(&device), &fstype, mounts) {
                    Ok(at) => at,
                    Err(err) => {
                        unread.push(err);
                        continue;
                    }
                };
                let fstab = at.join("etc/fstab");
                if !fstab.is_file() {
                    continue;
                }
                let crypttab = at.join("etc/crypttab");
                systems.push(OldSystem {
                    crypttab: match crypttab.is_file() {
                        true => std::fs::read_to_string(&crypttab).unwrap_or_default(),
                        false => String::new(),
                    },
                    fstab: std::fs::read_to_string(&fstab).unwrap_or_default(),
                    at,
                });
            }
        }
        Err(err) => unread.push(err),
    }
    (systems, unread)
}

#[derive(Debug, PartialEq)]
struct OldKey {
    target: String,
    key: Key,
}

#[derive(Default)]
struct Discovered {
    found: Vec<(String, OldKey)>,
    why: Vec<(String, String)>,
}

impl Discovered {
    fn default_at(&self, partition: &Partition, options: &[Choice]) -> Option<usize> {
        let (_, old) = self
            .found
            .iter()
            .find(|(device, _)| device == &partition.device)?;
        if old.target.is_empty() {
            return None;
        }
        options
            .iter()
            .position(|option| option.available && option.label == copy::open_at(&old.target))
    }

    fn key(&self, partition: &str) -> Option<&Key> {
        self.found
            .iter()
            .find(|(device, _)| device == partition)
            .map(|(_, old)| &old.key)
    }

    fn why(&self, partition: &str) -> Option<&str> {
        self.why
            .iter()
            .find(|(device, _)| device == partition)
            .map(|(_, why)| why.as_str())
    }
}

fn key_file(volume: &str, root: &Path, path: &Path) -> Result<Vec<u8>, String> {
    let said = root.join(path.strip_prefix("/").unwrap_or(path));
    let file = std::fs::canonicalize(&said).map_err(|err| {
        copy::old_root_key_missing(volume, &path.display().to_string(), &err.to_string())
    })?;
    let at = std::fs::canonicalize(root).map_err(|err| format!("{}: {err}", root.display()))?;
    if !file.starts_with(&at) || !file.is_file() {
        return Err(copy::old_root_key_outside(&path.display().to_string()));
    }
    std::fs::read(&file).map_err(|err| {
        copy::old_root_key_missing(volume, &path.display().to_string(), &err.to_string())
    })
}

fn key_bytes(system: &OldSystem, entry: &Crypttab, mounts: &mut Mounts) -> Result<Vec<u8>, String> {
    match &entry.key {
        KeySource::Inside(path) => key_file(&entry.name, &system.at, path),
        KeySource::Default => key_file(
            &entry.name,
            &system.at,
            &PathBuf::from(format!("/etc/cryptsetup-keys.d/{}.key", entry.name)),
        ),
        KeySource::OnDevice { device, path } => {
            let resolved = resolve_device(device)?;
            if removable(&resolved) {
                return Err(copy::old_root_key_removable(&entry.name));
            }
            let fstype = fstype_of(&resolved);
            let at = mount_ro(&resolved, &fstype, mounts)?;
            key_file(&entry.name, &at, path)
        }
        KeySource::Waited => Err(copy::old_root_key_waited(&entry.name)),
        KeySource::Unreadable => Err(copy::old_root_key_unreadable(&entry.name)),
    }
}

fn resolve_device(said: &str) -> Result<PathBuf, String> {
    if said.starts_with('/') {
        return Ok(PathBuf::from(said));
    }
    let Some((tag, value)) = ["UUID", "LABEL", "PARTUUID"].iter().find_map(|tag| {
        said.strip_prefix(&format!("{tag}="))
            .map(|value| (*tag, value))
    }) else {
        return Err(format!("{said} is not a device"));
    };
    let out = Command::new("blkid")
        .args(["-t", &format!("{tag}={value}"), "-o", "device"])
        .output()
        .map_err(|err| format!("blkid: {err}, and it is what resolves {said}"))?;
    labelled(&String::from_utf8_lossy(&out.stdout))
        .into_iter()
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| format!("{said} is not present"))
}

fn fstype_of(device: &Path) -> String {
    Command::new("blkid")
        .args(["-s", "TYPE", "-o", "value"])
        .arg(device)
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .unwrap_or_default()
}

const SYS_CLASS_BLOCK: &str = "/sys/class/block";

fn removable_at(class: &Path, device: &Path) -> bool {
    let device = std::fs::canonicalize(device).unwrap_or_else(|_| device.to_path_buf());
    let Some(name) = device.file_name() else {
        return false;
    };
    let Ok(at) = std::fs::canonicalize(class.join(name)) else {
        return false;
    };
    let disk = match at.join("partition").exists() {
        true => match at.parent() {
            Some(parent) => parent.to_path_buf(),
            None => at,
        },
        false => at,
    };
    std::fs::read_to_string(disk.join("removable")).is_ok_and(|flag| flag.trim() == "1")
}

fn removable(device: &Path) -> bool {
    removable_at(Path::new(SYS_CLASS_BLOCK), device)
}

fn test_key(container: &str, key: &[u8]) -> Result<bool, String> {
    let mut command = Command::new("cryptsetup");
    command
        .args(["-q", "luksOpen", "--test-passphrase", "--key-file", "-"])
        .arg(container)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = command
        .spawn()
        .map_err(|err| format!("cryptsetup: {err}, and it is what tests a key"))?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(key);
    }
    let status = child.wait().map_err(|err| format!("cryptsetup: {err}"))?;
    Ok(status.success())
}

fn discover(
    partitions: &[Partition],
    systems: &[OldSystem],
    unread: &[String],
    mounts: &mut Mounts,
    opens: &dyn Fn(&Partition, &[u8]) -> Result<bool, String>,
) -> Discovered {
    let mut found = Discovered::default();
    for partition in partitions
        .iter()
        .filter(|part| part.fstype == "crypto_LUKS")
    {
        let mut named = false;
        let mut why = None;
        let mut key = None;
        'systems: for system in systems {
            for entry in crypttabs(&system.crypttab) {
                if !names(&entry, partition) {
                    continue;
                }
                named = true;
                match key_bytes(system, &entry, mounts)
                    .and_then(|bytes| opens(partition, &bytes).map(|opens| (bytes, opens)))
                {
                    Ok((bytes, true)) => {
                        key = Some(OldKey {
                            target: fstab_target(&system.fstab, &entry.name),
                            key: Key::Data(bytes),
                        });
                        break 'systems;
                    }
                    Ok((_, false)) => why = Some(copy::old_root_key_wrong(&partition.device)),
                    Err(err) => why = Some(err),
                }
            }
        }
        match key {
            Some(key) => found.found.push((partition.device.clone(), key)),
            None => {
                let why = why.unwrap_or_else(|| match named {
                    true => copy::old_root_key_wrong(&partition.device),
                    false if !unread.is_empty() => copy::old_root_partly(&unread[0]),
                    false if !systems.is_empty() => copy::old_root_unnamed(&partition.device),
                    false => copy::OLD_ROOT_NONE.to_string(),
                });
                found.why.push((partition.device.clone(), why));
            }
        }
    }
    found
}

fn old_keys(disk: &str, partitions: &[Partition]) -> Discovered {
    if !partitions.iter().any(|part| part.fstype == "crypto_LUKS") {
        return Discovered::default();
    }
    let mut mounts = Mounts::default();
    let (systems, unread) = old_systems(disk, &mut mounts);
    discover(
        partitions,
        &systems,
        &unread,
        &mut mounts,
        &|container, key| test_key(&container.device, key),
    )
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

fn kinds(tpm: bool, luks_initramfs: bool) -> Vec<Choice> {
    KINDS
        .iter()
        .map(|(name, shown, detail)| match () {
            _ if *name != NONE && !luks_initramfs => {
                Choice::new(*shown, copy::NO_LUKS_INITRAMFS).unavailable()
            }
            _ if !tpm && name.starts_with("tpm2") => {
                Choice::new(*shown, copy::NO_TPM).unavailable()
            }
            _ => Choice::new(*shown, *detail),
        })
        .collect()
}

fn encryption_row(
    layout: Option<&CustomLayout>,
    held: &crate::ui::Field,
    tpm: bool,
    luks_initramfs: bool,
) -> crate::ui::Field {
    match layout.filter(|layout| !layout.opens.is_empty()) {
        Some(layout) => {
            let chosen = Opened::of(&held.value());
            let rows = opened_rows(layout, tpm, luks_initramfs);
            let at = rows
                .iter()
                .position(|row| row.label == chosen.label())
                .or_else(|| rows.iter().position(|row| row.label == copy::OPENED_KEEP));
            crate::ui::Field::pick(copy::ROW_ENCRYPTION, rows, at)
        }
        None => {
            let kind = written(&held.value());
            crate::ui::Field::pick(
                copy::ROW_ENCRYPTION,
                kinds(tpm, luks_initramfs),
                KINDS.iter().position(|(name, _, _)| *name == kind),
            )
        }
    }
}

fn opened_rows(layout: &CustomLayout, tpm: bool, luks_initramfs: bool) -> Vec<Choice> {
    let read: Vec<Result<Slots, String>> = layout
        .opens
        .iter()
        .map(|open| slots(&open.partition))
        .collect();
    let said = match read.is_empty() {
        true => String::new(),
        false => read
            .iter()
            .map(|slots| match slots {
                Ok(slots) => copy::slots_said(&slots.keys, &slots.tokens),
                Err(why) => copy::slots_unknown(why),
            })
            .collect::<Vec<_>>()
            .join("; "),
    };
    let mut rows = vec![Choice::new(copy::OPENED_KEEP, said)];
    let keyfile_root = layout
        .opens
        .iter()
        .any(|open| open.target == "/" && !matches!(open.key, Key::Passphrase(_)));
    if keyfile_root {
        rows[0] = Choice::new(copy::OPENED_KEEP, copy::OPENED_ROOT_KEYFILE).unavailable();
    }
    let unread = read.iter().find_map(|slots| slots.as_ref().err());
    let enrolled = read
        .iter()
        .any(|slots| slots.as_ref().is_ok_and(Slots::has_tpm2));
    let encrypted_root = layout.opens_the_root();
    rows.push(match (tpm, unread, enrolled) {
        (false, _, _) => Choice::new(copy::OPENED_TPM2, copy::NO_TPM).unavailable(),
        _ if !encrypted_root => {
            Choice::new(copy::OPENED_TPM2, copy::OPENED_KEYFILE_PLAIN).unavailable()
        }
        (_, Some(why), _) => Choice::new(copy::OPENED_TPM2, why.clone()).unavailable(),
        (_, _, true) => Choice::new(copy::OPENED_TPM2, copy::OPENED_TPM2_HAS).unavailable(),
        _ if keyfile_root => {
            Choice::new(copy::OPENED_TPM2, copy::OPENED_ROOT_KEYFILE).unavailable()
        }
        _ => Choice::new(copy::OPENED_TPM2, copy::OPENED_TPM2_COST),
    });
    if let Some((_, slots)) = layout
        .opens
        .iter()
        .zip(read.iter())
        .find(|(open, _)| open.target != "/" && matches!(open.key, Key::Passphrase(_)))
    {
        let detail = match slots {
            Ok(slots) => format!(
                "{}; {} slots now",
                copy::OPENED_ADD_KEY_COST,
                slots.keys.len()
            ),
            Err(_) => copy::OPENED_ADD_KEY_COST.to_string(),
        };
        rows.push(match encrypted_root {
            true => Choice::new(copy::OPENED_ADD_KEY, detail),
            false => Choice::new(copy::OPENED_ADD_KEY, copy::OPENED_KEYFILE_PLAIN).unavailable(),
        });
    }
    if layout.opens_the_root() && !luks_initramfs {
        for row in &mut rows {
            *row = Choice::new(row.label.clone(), copy::NO_LUKS_INITRAMFS).unavailable();
        }
    }
    rows
}

fn slots(container: &str) -> Result<Slots, String> {
    let out = Command::new("cryptsetup")
        .args(["luksDump", "--dump-json-metadata"])
        .arg(container)
        .output()
        .map_err(|err| format!("cryptsetup: {err}, and it is what reads a header"))?;
    match out.status.success() {
        true => slots_from(&String::from_utf8_lossy(&out.stdout)),
        false => Err(String::from_utf8_lossy(&out.stderr).trim().to_string()),
    }
}

fn slots_from(raw: &str) -> Result<Slots, String> {
    const SLOTS: u32 = 32;
    let doc = Json::parse(raw).map_err(|err| format!("luksDump wrote invalid JSON: {err}"))?;
    let mut keys = Vec::new();
    if let Some(keyslots) = json::field(&doc, "keyslots") {
        for at in 0..SLOTS {
            if json::field(keyslots, &at.to_string()).is_some() {
                keys.push(at);
            }
        }
    }
    let mut tokens = Vec::new();
    if let Some(listed) = json::field(&doc, "tokens") {
        let Json::Object(entries) = listed else {
            return Err("luksDump wrote tokens that are not an object".to_string());
        };
        for (_, token) in entries {
            if let Some(kind) = json::text(token, "type") {
                tokens.push(kind);
            }
        }
    }
    tokens.sort();
    Ok(Slots { keys, tokens })
}

fn data_rows(disk: &str, found: &[(String, String)], encrypted: bool) -> Vec<Choice> {
    let mut rows = vec![Choice::new(copy::NONE, "")];
    let mut offered = vec![(Choice::new(copy::DATA_HERE, ""), false)];
    for (other, detail) in found.iter().filter(|(other, _)| other != disk) {
        offered.push((
            Choice::new(copy::on_disk(other, copy::DATA_ERASED), detail),
            false,
        ));
        offered.push((
            Choice::new(copy::on_disk(other, copy::DATA_KEPT), detail),
            true,
        ));
    }
    rows.extend(
        offered
            .into_iter()
            .map(|(row, kept)| match encrypted && kept {
                false => row,
                true => Choice::new(row.label, copy::DATA_UNENCRYPTED).unavailable(),
            }),
    );
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

fn data_said(data: &Data, encrypted: bool) -> String {
    let said = match (data.disk.is_empty(), data.size.is_empty()) {
        (true, true) => copy::NONE.to_string(),
        (true, false) => copy::DATA_HERE.to_string(),
        _ => copy::on_disk(
            &data.disk,
            match data.keep {
                true => copy::DATA_KEPT,
                false => copy::DATA_ERASED,
            },
        ),
    };
    match encrypted && data.wanted() {
        true => format!("{said}, {}", copy::DATA_ENCRYPTED),
        false => said,
    }
}

fn ask_encryption(
    given: Option<String>,
    passphrase: Option<String>,
    prompt: &Prompt,
    luks_initramfs: bool,
) -> Result<Encryption, String> {
    let kind = match given {
        Some(kind) => named(kind)?,
        None if !prompt.asks() => NONE.to_string(),
        None => {
            let options = kinds(tpm().exists(), luks_initramfs);
            match prompt.choose_current(copy::INSTALL_ENCRYPTION, &options, 0)? {
                Some(at) => KINDS[at].0.to_string(),
                None => NONE.to_string(),
            }
        }
    };
    if kind != NONE && !luks_initramfs {
        return Err(copy::NO_LUKS_INITRAMFS.to_string());
    }
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

fn unset(value: &mut Json, key: &str) {
    let Json::Object(fields) = value else { return };
    fields.retain(|(name, _)| name != key);
}

pub fn complete(recipe: &Path, answers: &Answers) -> Result<Json, String> {
    if answers.layout.is_some() && answers.encryption.kind != NONE {
        return Err(copy::CUSTOM_ENCRYPTION.to_string());
    }
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
    if let Some(layout) = &answers.layout {
        unset(&mut doc, "varDisk");
        let mut mounts: Vec<Json> = layout
            .mounts
            .iter()
            .map(|mount| {
                Json::object([
                    ("partition", Json::string(&mount.partition)),
                    ("target", Json::string(&mount.target)),
                    ("fstype", Json::string(&mount.fstype)),
                ])
            })
            .collect();
        for (name, open) in layout.mappers() {
            mounts.push(Json::object([
                ("partition", Json::string(&mapper_path(&name))),
                ("target", Json::string(&open.target)),
                ("fstype", Json::string("unformatted")),
            ]));
        }
        set(&mut doc, "customMounts", Json::array(mounts));
        return Ok(doc);
    }
    let encrypted = answers.encryption.kind != NONE;
    if !answers.data.disk.is_empty() {
        let mut var = Json::object([
            ("disk", Json::string(&answers.data.disk)),
            ("keepExisting", Json::Bool(answers.data.keep)),
        ]);
        if encrypted {
            set(&mut var, "encrypt", Json::Bool(true));
        }
        set(&mut doc, "varDisk", var);
    } else if !answers.data.size.is_empty() {
        let mut var = Json::object([("size", Json::string(&answers.data.size))]);
        if encrypted {
            set(&mut var, "encrypt", Json::Bool(true));
        }
        set(&mut doc, "varDisk", var);
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

fn configure_boot_chain(image: &str, disk: &str, chain: &str) -> Result<(), String> {
    if chain.is_empty() {
        return Ok(());
    }
    let at = PathBuf::from(TARGET);
    let boot = at.join("boot");
    std::fs::create_dir_all(&boot).map_err(|err| format!("{TARGET}: {err}"))?;
    let Some((device, root)) = boot_partition(disk, &boot)? else {
        return Err(format!(
            "no partition of {disk} carries `loader/entries`, so the {chain} chain cannot be configured"
        ));
    };
    let esp = match root {
        "/target" => boot.clone(),
        _ => boot.join("boot"),
    };

    let configured = match chain {
        "uki-db" => {
            let missing = ["PK", "KEK", "db"]
                .into_iter()
                .map(|name| esp.join(format!("loader/keys/auto/{name}.auth")))
                .find(|key| !key.is_file());
            match missing {
                Some(key) => Err(format!(
                    "{}: bootc did not install the {chain} enrollment key",
                    key.display()
                )),
                None => {
                    let loader = esp.join("loader/loader.conf");
                    let current = std::fs::read_to_string(&loader).unwrap_or_default();
                    let mut lines: Vec<&str> = current
                        .lines()
                        .filter(|line| !line.trim_start().starts_with("secure-boot-enroll "))
                        .collect();
                    lines.push("secure-boot-enroll force");
                    std::fs::write(&loader, format!("{}\n", lines.join("\n")))
                        .map_err(|err| format!("{}: {err}", loader.display()))
                }
            }
        }
        "uki-shim" => run_enrolment(image, &esp),
        _ => Err(format!("unknown boot chain `{chain}`")),
    };
    let unmounted = Command::new("umount").arg(&boot).output();
    configured?;
    match unmounted {
        Ok(out) if out.status.success() => {
            eprintln!("buildc: configured {chain} on {device}");
            Ok(())
        }
        Ok(out) => Err(format!(
            "unmounting {device}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )),
        Err(err) => Err(format!("unmounting {device}: {err}")),
    }
}

fn require_signed_boot_chain(image: &str, chain: &str) -> Result<(), String> {
    if chain.is_empty() {
        return Ok(());
    }
    let out = Command::new("podman")
        .args([
            "run",
            "--rm",
            "--pull=never",
            "--net=none",
            "--security-opt",
            "label=disable",
            "--entrypoint",
            "/usr/bin/test",
            image,
            "-e",
            "/usr/share/buildc/secureboot-signed",
        ])
        .output()
        .map_err(|err| format!("podman: {err}, and it is what verifies the {chain} chain"))?;
    if out.status.success() {
        return Ok(());
    }
    Err(format!(
        "{image} is not signed for its declared {chain} chain; rebuild it with the Secure Boot private key before installing"
    ))
}

fn require_tpm2_enrolment(image: &str) -> Result<(), String> {
    let out = Command::new("podman")
        .args([
            "run",
            "--rm",
            "--pull=never",
            "--net=none",
            "--security-opt",
            "label=disable",
            "--entrypoint",
            "",
            image,
            "/usr/bin/systemd-cryptenroll",
            "--version",
        ])
        .output()
        .map_err(|err| format!("podman: {err}, and it is what checks {image} for TPM2"))?;
    if out.status.success() {
        return Ok(());
    }
    Err(format!(
        "{image} has no /usr/bin/systemd-cryptenroll, so it cannot enrol the TPM2 token this answer asks for"
    ))
}

fn pcr_policy_in(image: &str) -> bool {
    Command::new("podman")
        .args([
            "run",
            "--rm",
            "--pull=never",
            "--net=none",
            "--security-opt",
            "label=disable",
            "--entrypoint",
            "",
            image,
            "/usr/bin/test",
            "-s",
            "/usr/share/buildc/pcr-policy.pem",
        ])
        .output()
        .is_ok_and(|out| out.status.success())
}

fn run_enrolment(image: &str, esp: &Path) -> Result<(), String> {
    let target = format!("{}:/esp", esp.display());
    let out = Command::new("podman")
        .args([
            "run",
            "--rm",
            "--pull=never",
            "--net=none",
            "--security-opt",
            "label=disable",
            "--entrypoint",
            "",
            "-v",
            &target,
            image,
            "/usr/libexec/secureboot-enrolment",
            "/esp",
        ])
        .output()
        .map_err(|err| format!("podman: {err}, and it is what installs the shim chain"))?;
    match out.status.success() {
        true => Ok(()),
        false => Err(format!(
            "/usr/libexec/secureboot-enrolment in {image}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )),
    }
}

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
        if boot.join("loader/entries").is_dir() || boot.join("EFI/Linux").is_dir() {
            return Ok(Some((device, "/target")));
        }
        if boot.join("boot/loader/entries").is_dir() || boot.join("boot/EFI/Linux").is_dir() {
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

fn mapper_path(name: &str) -> String {
    format!("/dev/mapper/{name}")
}

#[derive(Default)]
struct Mappers(Vec<String>);

impl Drop for Mappers {
    fn drop(&mut self) {
        for name in &self.0 {
            if let Err(why) = close_volume(name) {
                eprintln!("buildc: {why}");
            }
        }
    }
}

fn open_volumes(layout: Option<&CustomLayout>) -> Result<Mappers, String> {
    let mut opened = Mappers::default();
    let Some(layout) = layout else {
        return Ok(opened);
    };
    for (name, open) in layout.mappers() {
        open_volume(open, &name)?;
        opened.0.push(name);
    }
    Ok(opened)
}

fn open_command(open: &LuksOpen, name: &str) -> Command {
    let mut command = Command::new("cryptsetup");
    command.args(["-q", "luksOpen"]);
    match &open.key {
        Key::Passphrase(_) | Key::Data(_) => {
            command.args(["--key-file", "-"]);
        }
        Key::File(path) => {
            command.args(["--key-file"]).arg(path);
        }
    }
    command.arg(&open.partition).arg(name);
    command
}

fn open_volume(open: &LuksOpen, name: &str) -> Result<(), String> {
    if Path::new(&mapper_path(name)).exists() {
        let _ = close_volume(name);
    }
    let mut command = open_command(open, name);
    command.stdout(Stdio::null()).stderr(Stdio::piped());
    if open.key.bytes().is_some() {
        command.stdin(Stdio::piped());
    }
    let mut child = command
        .spawn()
        .map_err(|err| format!("cryptsetup: {err}, and it is what opens {}", open.partition))?;
    if let Some(bytes) = open.key.bytes() {
        child
            .stdin
            .take()
            .ok_or("cryptsetup: no stdin")?
            .write_all(bytes)
            .map_err(|err| format!("cryptsetup: {err}"))?;
    }
    let out = child
        .wait_with_output()
        .map_err(|err| format!("cryptsetup: {err}"))?;
    match out.status.success() {
        true => Ok(()),
        false => Err(format!(
            "cryptsetup could not open {}: {}",
            open.partition,
            String::from_utf8_lossy(&out.stderr).trim()
        )),
    }
}

fn close_volume(name: &str) -> Result<(), String> {
    let out = Command::new("cryptsetup")
        .args(["-q", "close", name])
        .output()
        .map_err(|err| format!("cryptsetup: {err}, and it is what closes a container"))?;
    match out.status.success() {
        true => Ok(()),
        false => Err(format!(
            "{} is still open: {}",
            mapper_path(name),
            String::from_utf8_lossy(&out.stderr).trim()
        )),
    }
}

const ROOT_MOUNT: &str = "/run/buildc-root";

const ROOT_GUID: &str = "4f68bce3-e8cd-4db1-96e7-fbcaf984b709";

struct EtcWrite {
    at: String,
    bytes: Vec<u8>,
    mode: u32,
}

fn arrange(payload: &Payload, answers: &Answers) -> Result<Vec<String>, String> {
    let Some(layout) = &answers.layout else {
        return Ok(Vec::new());
    };
    if layout.opens.is_empty() {
        return Ok(Vec::new());
    }
    let mut writes: Vec<EtcWrite> = Vec::new();
    let mut crypttab: Vec<(String, String)> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    let mut added: Vec<String> = Vec::new();
    let mut root: Option<(String, String)> = None;
    let encrypted_root = layout.opens_the_root();
    let pcr_policy = answers.opened == Opened::Tpm2 && pcr_policy_in(&payload.image);
    let planned = (|| {
        for (mapper, open) in layout.mappers() {
            let uuid = luks_uuid(&open.partition)?;
            if open.target == "/" {
                match payload.boot.is_empty() {
                    true => root = Some((uuid.clone(), "root".to_string())),
                    false => retag_root(&answers.disk, open, &mapper)?,
                }
            } else {
                if !encrypted_root
                    && (answers.opened != Opened::Keep || !matches!(open.key, Key::Passphrase(_)))
                {
                    return Err(copy::OPENED_KEYFILE_PLAIN.to_string());
                }
                let name = crypttab_name(&open.target);
                let (line, note) =
                    data_volume(&name, open, &uuid, answers.opened, &mut writes, &mut added)?;
                if let Some(note) = note {
                    notes.extend(note);
                }
                crypttab.push((name, line));
            }
            if answers.opened == Opened::Tpm2 {
                let name = match open.target.as_str() {
                    "/" => "root".to_string(),
                    target => crypttab_name(target),
                };
                stage_tpm2(&name, open, &uuid, pcr_policy, &mut writes)?;
            }
        }
        if !writes.is_empty() || !crypttab.is_empty() {
            write_etc(layout, &writes, &crypttab)?;
        }
        if let Some((uuid, name)) = root {
            inject_luks_args(&answers.disk, &uuid, &name)?;
        }
        Ok(())
    })();
    if let Err(err) = planned {
        return Err(naming_added(err, &added));
    }
    Ok(notes)
}

fn naming_added(err: String, added: &[String]) -> String {
    match added.is_empty() {
        true => err,
        false => format!(
            "{err}; a key was added to {} and the install did not finish",
            added.join(", ")
        ),
    }
}

fn crypttab_name(target: &str) -> String {
    target.trim_start_matches('/').replace('/', "-")
}

fn data_volume(
    name: &str,
    open: &LuksOpen,
    uuid: &str,
    opened: Opened,
    writes: &mut Vec<EtcWrite>,
    added: &mut Vec<String>,
) -> Result<(String, Option<Vec<String>>), String> {
    let path = format!("/etc/cryptsetup-keys.d/{name}.key");
    match (&open.key, opened) {
        (Key::Passphrase(_), Opened::AddKey) => {
            let before = slots(&open.partition)?.keys;
            let key = random_key()?;
            add_key(open, &key)?;
            if !test_key(&open.partition, &key)? {
                return Err(copy::added_key_wrong(&open.partition));
            }
            added.push(open.partition.clone());
            writes.push(EtcWrite {
                at: path.clone(),
                bytes: key,
                mode: 0o600,
            });
            let note = redundant_slot_note(&open.partition, &before)?;
            Ok((copy::crypttab_line(name, uuid, Some(&path)), Some(note)))
        }
        (Key::Passphrase(_), _) => Ok((copy::crypttab_line(name, uuid, None), None)),
        (key, _) => {
            writes.push(EtcWrite {
                at: path.clone(),
                bytes: key_bytes_of(key)?,
                mode: 0o600,
            });
            Ok((copy::crypttab_line(name, uuid, Some(&path)), None))
        }
    }
}

fn key_bytes_of(key: &Key) -> Result<Vec<u8>, String> {
    match key {
        Key::Passphrase(passphrase) => Ok(passphrase.as_bytes().to_vec()),
        Key::Data(bytes) => Ok(bytes.clone()),
        Key::File(path) => std::fs::read(path).map_err(|err| format!("{}: {err}", path.display())),
    }
}

fn redundant_slot_note(partition: &str, before: &[u32]) -> Result<Vec<String>, String> {
    let after = slots(partition)?;
    let count = after.keys.len();
    Ok(match before {
        [only] => copy::kill_slot(partition, *only, count),
        _ => copy::kill_a_slot(partition, count),
    })
}

fn stage_tpm2(
    name: &str,
    open: &LuksOpen,
    uuid: &str,
    pcr_policy: bool,
    writes: &mut Vec<EtcWrite>,
) -> Result<(), String> {
    let key = format!("/etc/buildc/tpm2-enroll-{name}.key");
    writes.push(EtcWrite {
        at: key.clone(),
        bytes: key_bytes_of(&open.key)?,
        mode: 0o600,
    });
    writes.push(EtcWrite {
        at: format!("/etc/systemd/system/buildc-tpm2-enroll-{name}.service"),
        bytes: copy::tpm2_unit(name, &key, uuid, pcr_policy).into_bytes(),
        mode: 0o644,
    });
    Ok(())
}

fn write_etc(
    layout: &CustomLayout,
    writes: &[EtcWrite],
    crypttab: &[(String, String)],
) -> Result<(), String> {
    let Some(device) = root_device(layout) else {
        return Err("the layout names no / to write into".to_string());
    };
    let at = PathBuf::from(ROOT_MOUNT);
    std::fs::create_dir_all(&at).map_err(|err| format!("{ROOT_MOUNT}: {err}"))?;
    let mounted = Command::new("mount")
        .arg(&device)
        .arg(&at)
        .output()
        .map_err(|err| format!("mount: {err}, and it is what holds the installed root"))?;
    if !mounted.status.success() {
        return Err(format!(
            "mounting {device} at {ROOT_MOUNT}: {}",
            String::from_utf8_lossy(&mounted.stderr).trim()
        ));
    }
    let written = (|| {
        let etc = deployment_etc(&at)?;
        for write in writes {
            let path = etc.join(write.at.trim_start_matches('/'));
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|err| format!("{}: {err}", parent.display()))?;
            }
            std::fs::write(&path, &write.bytes)
                .map_err(|err| format!("{}: {err}", path.display()))?;
            set_mode(&path, write.mode)?;
        }
        merge_crypttab(&etc, crypttab)?;
        for write in writes {
            let Some(unit) = write.at.rsplit('/').next() else {
                continue;
            };
            if !unit.ends_with(".service") {
                continue;
            }
            let wants = etc
                .join("systemd/system/multi-user.target.wants")
                .join(unit);
            if let Some(parent) = wants.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|err| format!("{}: {err}", parent.display()))?;
            }
            let _ = std::fs::remove_file(&wants);
            std::os::unix::fs::symlink(format!("/etc/systemd/system/{unit}"), &wants)
                .map_err(|err| format!("{}: {err}", wants.display()))?;
        }
        Ok(())
    })();
    let _ = Command::new("umount").arg(&at).output();
    written
}

fn set_mode(path: &Path, mode: u32) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .map_err(|err| format!("{}: {err}", path.display()))
}

fn deployment_etc(root: &Path) -> Result<PathBuf, String> {
    let mut found = Vec::new();
    if let Ok(stateroots) = std::fs::read_dir(root.join("ostree/deploy")) {
        for stateroot in stateroots.flatten() {
            if let Ok(deployments) = std::fs::read_dir(stateroot.path().join("deploy")) {
                for deployment in deployments.flatten() {
                    let etc = deployment.path().join("etc");
                    if etc.is_dir() {
                        found.push(etc);
                    }
                }
            }
        }
    }
    if let Ok(deployments) = std::fs::read_dir(root.join("state/deploy")) {
        for deployment in deployments.flatten() {
            let etc = deployment.path().join("etc");
            if etc.is_dir() {
                found.push(etc);
            }
        }
    }
    match found.as_slice() {
        [one] => Ok(one.clone()),
        [] => Err(format!(
            "{}: no installed deployment carries an /etc",
            root.display()
        )),
        many => Err(format!(
            "{}: {} deployments carry an /etc, so which one to write is not clear",
            root.display(),
            many.len()
        )),
    }
}

fn merge_crypttab(etc: &Path, lines: &[(String, String)]) -> Result<(), String> {
    let at = etc.join("crypttab");
    let held = std::fs::read_to_string(&at).unwrap_or_default();
    let mut out: Vec<&str> = held
        .lines()
        .filter(|line| {
            let name = line.split_whitespace().next().unwrap_or("");
            !lines.iter().any(|(ours, _)| ours == name)
        })
        .collect();
    let ours: Vec<&str> = lines.iter().map(|(_, line)| line.as_str()).collect();
    out.extend(ours);
    if out.is_empty() {
        return Ok(());
    }
    std::fs::write(&at, format!("{}\n", out.join("\n")))
        .map_err(|err| format!("{}: {err}", at.display()))
}

fn root_device(layout: &CustomLayout) -> Option<String> {
    if let Some((name, _)) = layout
        .mappers()
        .into_iter()
        .find(|(_, open)| open.target == "/")
    {
        return Some(mapper_path(&name));
    }
    layout
        .mounts
        .iter()
        .find(|mount| mount.target == "/")
        .map(|mount| mount.partition.clone())
}

fn luks_uuid(partition: &str) -> Result<String, String> {
    let out = Command::new("cryptsetup")
        .args(["luksUUID", partition])
        .output()
        .map_err(|err| format!("cryptsetup: {err}, and it is what names a container"))?;
    let uuid = String::from_utf8_lossy(&out.stdout).trim().to_string();
    match out.status.success() && !uuid.is_empty() {
        true => Ok(uuid),
        false => Err(format!(
            "cryptsetup luksUUID {partition}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )),
    }
}

fn add_key(open: &LuksOpen, key: &[u8]) -> Result<(), String> {
    use std::os::unix::fs::OpenOptionsExt as _;
    let at = std::env::temp_dir().join(format!(
        "buildc-luks-key.{}.{:x}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0)
    ));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&at)
        .map_err(|err| format!("{}: {err}", at.display()))?;
    file.write_all(key)
        .map_err(|err| format!("{}: {err}", at.display()))?;
    drop(file);
    let mut command = Command::new("cryptsetup");
    command.args(["-q", "luksAddKey"]);
    match &open.key {
        Key::File(path) => {
            command.arg("--key-file").arg(path);
        }
        _ => {
            command.args(["--key-file", "-"]);
        }
    }
    command.arg(&open.partition).arg(&at);
    command.stdout(Stdio::null()).stderr(Stdio::piped());
    if open.key.bytes().is_some() {
        command.stdin(Stdio::piped());
    }
    let added = (|| {
        let mut child = command
            .spawn()
            .map_err(|err| format!("cryptsetup: {err}, and it is what adds a key"))?;
        if let Some(bytes) = open.key.bytes() {
            child
                .stdin
                .take()
                .ok_or("cryptsetup: no stdin")?
                .write_all(bytes)
                .map_err(|err| format!("cryptsetup: {err}"))?;
        }
        let out = child
            .wait_with_output()
            .map_err(|err| format!("cryptsetup: {err}"))?;
        match out.status.success() {
            true => Ok(()),
            false => Err(format!(
                "adding a key to {}: {}",
                open.partition,
                String::from_utf8_lossy(&out.stderr).trim()
            )),
        }
    })();
    let _ = std::fs::remove_file(&at);
    added
}

fn random_key() -> Result<Vec<u8>, String> {
    let mut bytes = [0u8; 32];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut source| source.read_exact(&mut bytes))
        .map_err(|err| format!("/dev/urandom: {err}"))?;
    let mut said = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        said.push_str(&format!("{byte:02x}"));
    }
    Ok(said.into_bytes())
}

fn retag_root(disk: &str, open: &LuksOpen, mapper: &str) -> Result<(), String> {
    let number = partition_number(&open.partition)?;
    close_volume(mapper)?;
    let out = Command::new("sfdisk")
        .args(["--part-type", disk, &number, ROOT_GUID])
        .output()
        .map_err(|err| format!("sfdisk: {err}, and it is what retags a root"))?;
    if !out.status.success() {
        return Err(format!(
            "retagging {} as the root partition: {}",
            open.partition,
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    open_volume(open, mapper)
}

fn partition_number(device: &str) -> Result<String, String> {
    let number: String = device
        .chars()
        .rev()
        .take_while(char::is_ascii_digit)
        .collect::<Vec<char>>()
        .into_iter()
        .rev()
        .collect();
    match number.is_empty() {
        true => Err(format!("{device} names no partition number")),
        false => Ok(number),
    }
}

fn inject_luks_args(disk: &str, uuid: &str, name: &str) -> Result<usize, String> {
    let at = PathBuf::from(TARGET);
    let boot = at.join("boot");
    std::fs::create_dir_all(&boot).map_err(|err| format!("{TARGET}: {err}"))?;
    let Some((device, root)) = boot_partition(disk, &boot)? else {
        return Err(format!(
            "no partition of {disk} carries `loader/entries`, so the containers \
             the layout opened have no boot entry to name"
        ));
    };
    let entries_dir = match root {
        "/target" => boot.clone(),
        _ => boot.join("boot"),
    };
    let arg = format!("rd.luks.name={uuid}={name}");
    let mut entries = 0;
    let mut named = 0;
    let mut patched = 0;
    let listed = std::fs::read_dir(entries_dir.join("loader/entries"));
    if let Ok(listed) = listed {
        for entry in listed.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|kind| kind != "conf") {
                continue;
            }
            entries += 1;
            let raw = std::fs::read_to_string(&path)
                .map_err(|err| format!("{}: {err}", path.display()))?;
            let carried = boot_arg_named(&raw, &arg);
            let (text, changed) = add_boot_arg(&raw, &arg);
            if changed {
                std::fs::write(&path, text).map_err(|err| format!("{}: {err}", path.display()))?;
                patched += 1;
            }
            if changed || carried {
                named += 1;
            }
        }
    }
    let unmounted = Command::new("umount").arg(&boot).output();
    match unmounted {
        Ok(out) if out.status.success() => {}
        Ok(out) => {
            return Err(format!(
                "unmounting {device}: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ))
        }
        Err(err) => return Err(format!("unmounting {device}: {err}")),
    }
    if named == 0 {
        return Err(format!(
            "{device} carries no boot entry naming the {name} container, so \
             the machine would not find its root"
        ));
    }
    eprintln!(
        "buildc: named the {name} container in {patched} of {entries} boot entries on {device}"
    );
    Ok(patched)
}

fn boot_arg_named(raw: &str, arg: &str) -> bool {
    raw.lines()
        .any(|line| line.starts_with("options ") && line.split_whitespace().any(|word| word == arg))
}

fn add_boot_arg(raw: &str, arg: &str) -> (String, bool) {
    let mut changed = false;
    let mut lines: Vec<String> = raw
        .lines()
        .map(|line| {
            if line.starts_with("options ") && !line.split_whitespace().any(|word| word == arg) {
                changed = true;
                format!("{line} {arg}")
            } else {
                line.to_string()
            }
        })
        .collect();
    if raw.ends_with('\n') {
        lines.push(String::new());
    }
    (lines.join("\n"), changed)
}

pub fn run(payload: &Payload, answers: &Answers, prompt: &Prompt) -> Result<(), String> {
    require_signed_boot_chain(&payload.image, &payload.boot)?;
    if answers.opened == Opened::Tpm2 {
        require_tpm2_enrolment(&payload.image)?;
    }
    let _opened = open_volumes(answers.layout.as_ref())?;
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
    let notes = arrange(payload, answers)?;
    configure_boot_chain(&payload.image, &answers.disk, &payload.boot)?;
    render_menu(&payload.image, &answers.disk)?;
    finish(
        recovery.as_deref(),
        at.as_deref(),
        &payload.boot,
        &notes,
        prompt,
    )
}

fn finish(
    recovery: Option<&str>,
    log: Option<&Path>,
    boot: &str,
    notes: &[String],
    prompt: &Prompt,
) -> Result<(), String> {
    if !prompt.draws() {
        if let Some(key) = recovery {
            println!("\n{}", copy::recovery(key));
            println!("{}\n", copy::KEY_NOT_LOGGED);
        }
        if let Some(enrollment) = copy::enrollment(boot) {
            println!("{enrollment}");
        }
        for note in notes {
            println!("{note}");
        }
        eprintln!("buildc: {}", copy::logging(log));
        return Ok(());
    }
    match crate::ui::offer_over(
        copy::INSTALL_DONE,
        done_rows(recovery, log, boot, notes),
        copy::RESTART,
        copy::DONE_KEYS,
    )? {
        false => Ok(()),
        true => restart(),
    }
}

pub(crate) fn done_rows(
    recovery: Option<&str>,
    log: Option<&Path>,
    boot: &str,
    notes: &[String],
) -> Vec<Choice> {
    let mut rows = Vec::new();
    if let Some(key) = recovery {
        rows.push(Choice::new(copy::WRITE_DOWN, "").content());
        rows.push(Choice::new(key, "").content().tinted());
        rows.push(Choice::new(copy::KEY_NOT_LOGGED, "").content());
    }
    if let Some(enrollment) = copy::enrollment(boot) {
        rows.push(Choice::new(enrollment, "").content());
    }
    for note in notes {
        rows.push(Choice::new(note.clone(), "").content());
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
        assert!(payload.boot.is_empty());
        assert!(!payload.luks_initramfs, "an old recipe proves nothing");

        let uki = EMITTED.replace(
            "\"bootloader\": \"grub2\",",
            "\"bootloader\": \"systemd\",\n  \"boot\": \"uki-db\",\n  \"luksInitramfs\": true,",
        );
        std::fs::write(root.join(RECIPE), uki).expect("a UKI recipe");
        let Ok(Found::Image(payload)) = classify(&root) else {
            panic!("a UKI recipe is a payload");
        };
        assert_eq!(payload.boot, "uki-db");
        assert!(payload.luks_initramfs);

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
            opened: Opened::Keep,
            encryption: Encryption {
                kind: "luks-passphrase".to_string(),
                passphrase: "opensesame".to_string(),
            },
            data: Data::default(),
            layout: None,
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
    fn lsblk_partitions_keep_the_details_the_editor_shows() {
        let rows = partition_rows(
            r#"{
  "blockdevices": [{
    "name": "/dev/vda", "size": "64G", "fstype": null, "label": null, "type": "disk",
    "children": [
      {"name": "/dev/vda1", "size": "512M", "fstype": "vfat", "label": "EFI", "type": "part", "parttype": "C12A7328-F81F-11D2-BA4B-00A0C93EC93B"},
      {"name": "/dev/vda2", "size": "63.5G", "fstype": "crypto_LUKS", "label": null, "type": "part", "parttype": null, "uuid": "a1b2c3d4-0000-0000-0000-000000000000"}
    ]
  }]
}"#,
        )
        .expect("lsblk JSON");
        assert_eq!(
            rows,
            [
                Partition {
                    device: "/dev/vda1".to_string(),
                    size: "512M".to_string(),
                    fstype: "vfat".to_string(),
                    label: "EFI".to_string(),
                    parttype: "C12A7328-F81F-11D2-BA4B-00A0C93EC93B".to_string(),
                    uuid: String::new(),
                },
                Partition {
                    device: "/dev/vda2".to_string(),
                    size: "63.5G".to_string(),
                    fstype: "crypto_LUKS".to_string(),
                    label: String::new(),
                    parttype: String::new(),
                    uuid: "a1b2c3d4-0000-0000-0000-000000000000".to_string(),
                },
            ]
        );
        assert!(is_esp(&rows[0]));
        assert!(mount_choices(&rows[0], "ext4", "grub2", true)
            .iter()
            .any(|choice| choice.label == copy::keep_as("/boot/efi")));
        let luks = mount_choices(&rows[1], "ext4", "grub2", true);
        assert!(!luks
            .iter()
            .any(|choice| choice.label.starts_with("keep as ")));
        for target in ["/", "/var"] {
            assert!(
                luks.iter()
                    .any(|choice| choice.label == copy::open_at(target) && choice.available),
                "{target} is not offered"
            );
        }
        assert!(!luks
            .iter()
            .any(|choice| choice.label == copy::open_at("/boot/efi")));
        for bootloader in ["grub2", "systemd"] {
            let choices = mount_choices(&rows[1], "ext4", bootloader, true);
            let boot = choices
                .iter()
                .find(|choice| choice.label == copy::open_at("/boot"))
                .expect("the opened /boot refusal");
            assert!(!boot.available, "{bootloader} opens an encrypted /boot");
        }

        let choices = mount_choices(&rows[0], "zfs", "grub2", true);
        assert!(
            !choices
                .iter()
                .find(|choice| choice.label == copy::keep_as("/boot"))
                .expect("the explicit /boot refusal")
                .available
        );
        assert!(
            !choices
                .iter()
                .find(|choice| choice.label == copy::format_at("zfs", "/"))
                .expect("the unsupported format")
                .available
        );

        let ext4 = Partition {
            device: "/dev/vda3".to_string(),
            size: "1G".to_string(),
            fstype: "ext4".to_string(),
            label: String::new(),
            parttype: String::new(),
            uuid: String::new(),
        };
        let choices = mount_choices(&ext4, "ext4", "systemd", true);
        assert!(
            !choices
                .iter()
                .find(|choice| choice.label == copy::keep_as("/boot"))
                .expect("the kept systemd-boot refusal")
                .available
        );
        assert!(
            mount_choices(&ext4, "ext4", "grub2", true)
                .iter()
                .find(|choice| choice.label == copy::keep_as("/boot"))
                .expect("the kept grub /boot")
                .available
        );
        assert!(
            !choices
                .iter()
                .find(|choice| choice.label == copy::format_at("ext4", "/boot"))
                .expect("the formatted systemd-boot refusal")
                .available
        );
    }

    #[test]
    fn custom_layouts_need_one_root_and_unique_mount_points() {
        let field = |target| {
            crate::ui::Field::pick(
                "partition",
                vec![Choice::new(copy::format_at("ext4", target), "")],
                Some(0),
            )
        };
        assert_eq!(
            layout_short_of(&[field("/var")]).as_deref(),
            Some(copy::CUSTOM_ROOT)
        );
        assert_eq!(
            layout_short_of(&[field("/"), field("/")]).as_deref(),
            Some(copy::CUSTOM_DUPLICATE)
        );
        assert!(layout_short_of(&[field("/"), field("/var")]).is_none());
    }

    #[test]
    fn every_answer_the_editor_takes_reads_back_as_a_mount() {
        assert_eq!(
            mounted_answer(&copy::open_at("/")),
            Some(Mounted {
                target: "/".to_string(),
                fstype: OPEN.to_string(),
            })
        );
        assert_eq!(
            mounted_answer(&copy::keep_as("/boot/efi")),
            Some(Mounted {
                target: "/boot/efi".to_string(),
                fstype: "unformatted".to_string(),
            })
        );
        assert_eq!(
            mounted_answer(&copy::format_at("btrfs", "/var")),
            Some(Mounted {
                target: "/var".to_string(),
                fstype: "btrfs".to_string(),
            })
        );
        assert_eq!(mounted_answer(copy::LEAVE_PARTITION), None);
    }

    #[test]
    fn an_open_container_is_an_opening_and_not_a_format() {
        let partition = |device: &str, fstype: &str| Partition {
            device: device.to_string(),
            size: "4G".to_string(),
            fstype: fstype.to_string(),
            label: String::new(),
            parttype: String::new(),
            uuid: String::new(),
        };
        let rows = [
            partition("/dev/vda1", "vfat"),
            partition("/dev/vda2", "crypto_LUKS"),
            partition("/dev/vda3", "xfs"),
        ];
        let field = |answer: &str| {
            crate::ui::Field::pick("partition", vec![Choice::new(answer, "")], Some(0))
        };
        let fields = [
            field(&copy::keep_as("/boot/efi")),
            field(&copy::open_at("/")),
            field(&copy::format_at("ext4", "/var")),
        ];
        let (mounts, openings) = layout_from(&rows, &fields);
        assert_eq!(
            mounts,
            [
                CustomMount {
                    partition: "/dev/vda1".to_string(),
                    target: "/boot/efi".to_string(),
                    fstype: "unformatted".to_string(),
                },
                CustomMount {
                    partition: "/dev/vda3".to_string(),
                    target: "/var".to_string(),
                    fstype: "ext4".to_string(),
                },
            ]
        );
        assert_eq!(
            openings,
            [Opening {
                partition: "/dev/vda2".to_string(),
                target: "/".to_string(),
            }]
        );
    }

    #[test]
    fn a_key_file_is_usable_only_where_a_container_encrypts_the_root() {
        assert!(usable_key(&Key::Passphrase("x".to_string()), false));
        assert!(!usable_key(&Key::File(PathBuf::from("/run/key")), false));
        assert!(!usable_key(&Key::Data(b"key".to_vec()), false));
        assert!(usable_key(&Key::File(PathBuf::from("/run/key")), true));
        assert!(usable_key(&Key::Data(b"key".to_vec()), true));
    }

    #[test]
    fn an_edited_layout_keeps_the_key_it_already_has() {
        let opening = Opening {
            partition: "/dev/vda2".to_string(),
            target: "/".to_string(),
        };
        let held = CustomLayout {
            disk: "/dev/vda".to_string(),
            mounts: Vec::new(),
            opens: vec![LuksOpen {
                partition: opening.partition.clone(),
                target: opening.target.clone(),
                key: Key::Passphrase("opensesame".to_string()),
            }],
        };
        let opens = opens_from(
            std::slice::from_ref(&opening),
            Some(&held),
            &Discovered::default(),
        )
        .expect("the carried key");
        assert_eq!(opens, held.opens);
    }

    #[test]
    fn an_opened_container_reaches_fisherman_as_its_mapper() {
        let root = scratch("opened-layout");
        let recipe = root.join(RECIPE);
        std::fs::write(&recipe, EMITTED).expect("a recipe");
        let answers = Answers {
            disk: "/dev/vda".to_string(),
            hostname: "deb2".to_string(),
            user: "buildc".to_string(),
            password: "hunter2".to_string(),
            opened: Opened::Keep,
            encryption: Encryption {
                kind: NONE.to_string(),
                passphrase: String::new(),
            },
            data: Data::default(),
            layout: Some(CustomLayout {
                disk: "/dev/vda".to_string(),
                mounts: vec![CustomMount {
                    partition: "/dev/vda1".to_string(),
                    target: "/boot/efi".to_string(),
                    fstype: "unformatted".to_string(),
                }],
                opens: vec![
                    LuksOpen {
                        partition: "/dev/vda2".to_string(),
                        target: "/".to_string(),
                        key: Key::Passphrase("opensesame".to_string()),
                    },
                    LuksOpen {
                        partition: "/dev/vda3".to_string(),
                        target: "/var".to_string(),
                        key: Key::File(PathBuf::from("/run/keyfile")),
                    },
                ],
            }),
        };
        let done = complete(&recipe, &answers).expect("a custom recipe");
        let mounts = json::items(&done, "customMounts");
        assert_eq!(mounts.len(), 3);
        assert_eq!(
            json::text(&mounts[1], "partition").as_deref(),
            Some("/dev/mapper/buildc-1")
        );
        assert_eq!(json::text(&mounts[1], "target").as_deref(), Some("/"));
        assert_eq!(
            json::text(&mounts[1], "fstype").as_deref(),
            Some("unformatted")
        );
        assert_eq!(
            json::text(&mounts[2], "partition").as_deref(),
            Some("/dev/mapper/buildc-2")
        );
        assert_eq!(json::text(&mounts[2], "target").as_deref(), Some("/var"));
        let said = done.render();
        for secret in ["/dev/vda2", "/dev/vda3", "opensesame", "/run/keyfile"] {
            assert!(!said.contains(secret), "{secret} reached the recipe");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn opening_a_container_names_cryptsetup_and_the_mapper() {
        let open = |key| LuksOpen {
            partition: "/dev/vda2".to_string(),
            target: "/".to_string(),
            key,
        };
        let args = |open: &LuksOpen| -> Vec<String> {
            let command = open_command(open, "buildc-1");
            command
                .get_args()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect()
        };
        assert_eq!(
            args(&open(Key::File(PathBuf::from("/run/keyfile")))),
            [
                "-q",
                "luksOpen",
                "--key-file",
                "/run/keyfile",
                "/dev/vda2",
                "buildc-1"
            ]
        );
        assert_eq!(
            args(&open(Key::Passphrase("opensesame".to_string()))),
            ["-q", "luksOpen", "--key-file", "-", "/dev/vda2", "buildc-1"]
        );
        assert_eq!(
            args(&open(Key::Data(b"opensesame".to_vec()))),
            ["-q", "luksOpen", "--key-file", "-", "/dev/vda2", "buildc-1"]
        );
    }

    #[test]
    fn the_key_file_method_is_refused_where_the_root_is_not_encrypted() {
        let both = key_methods(None);
        assert!(both.iter().all(|method| method.available));
        assert_eq!(both[1].detail, copy::KEY_FILE_COST);

        let refused = key_methods(Some(copy::OPENED_KEYFILE_PLAIN));
        assert!(refused[0].available);
        assert!(refused[0].detail == copy::KEY_PASSPHRASE_COST);
        assert!(!refused[1].available);
        assert_eq!(refused[1].detail, copy::OPENED_KEYFILE_PLAIN);
    }

    #[test]
    fn a_passphrase_never_reads_back_out_of_a_key() {
        let said = format!("{:?}", Key::Passphrase("opensesame".to_string()));
        assert!(!said.contains("opensesame"), "{said}");
        let said = format!("{:?}", Key::Data(b"opensesame".to_vec()));
        assert!(!said.contains("opensesame"), "{said}");
    }

    #[test]
    fn an_old_crypttab_says_where_each_key_is() {
        let entries = crypttabs(
            "\
# a comment
root UUID=aa11 none luks
var UUID=bb22 /etc/luks/var.key luks,discard
data UUID=cc33 /key:UUID=dd44 luks
stick UUID=ee55 UUID=ff66:/key:10 luks,keyscript=/lib/cryptsetup/scripts/passdev
other UUID=gg77 none luks,keyscript=/lib/cryptsetup/scripts/decrypt_derived
passphrase UUID=hh88
timed UUID=ii99 /media/stick/var.key luks,keyfile-timeout=30s
",
        );
        assert_eq!(entries.len(), 7);
        assert_eq!(entries[0].key, KeySource::Default);
        assert_eq!(
            entries[1].key,
            KeySource::Inside(PathBuf::from("/etc/luks/var.key"))
        );
        assert_eq!(
            entries[2].key,
            KeySource::OnDevice {
                device: "UUID=dd44".to_string(),
                path: PathBuf::from("/key"),
            }
        );
        assert_eq!(
            entries[3].key,
            KeySource::OnDevice {
                device: "UUID=ff66".to_string(),
                path: PathBuf::from("/key"),
            }
        );
        assert_eq!(entries[4].key, KeySource::Unreadable);
        assert_eq!(entries[5].key, KeySource::Default);
        assert_eq!(entries[6].key, KeySource::Waited);
    }

    #[test]
    fn an_old_system_names_a_container_by_its_uuid_or_its_device() {
        let container = Partition {
            device: "/dev/vda3".to_string(),
            size: String::new(),
            fstype: "crypto_LUKS".to_string(),
            label: String::new(),
            parttype: String::new(),
            uuid: "A1B2".to_string(),
        };
        let entry = |device: &str| Crypttab {
            name: "var".to_string(),
            device: device.to_string(),
            key: KeySource::Default,
        };
        assert!(names(&entry("UUID=a1b2"), &container));
        assert!(names(&entry("/dev/disk/by-uuid/A1B2"), &container));
        assert!(names(&entry("/dev/vda3"), &container));
        assert!(!names(&entry("UUID=ffff"), &container));
        assert!(!names(&entry("/dev/vda2"), &container));
    }

    #[test]
    fn a_key_the_old_system_names_is_read_and_proved_against_the_container() {
        let root = scratch("old-system");
        let etc = root.join("etc");
        std::fs::create_dir_all(etc.join("cryptsetup-keys.d")).expect("a scratch /etc");
        std::fs::write(etc.join("crypttab"), "var UUID=aa11 none luks\n").expect("a crypttab");
        std::fs::write(
            etc.join("fstab"),
            "/dev/mapper/var /var ext4 defaults 0 0\n",
        )
        .expect("an fstab");
        std::fs::write(etc.join("cryptsetup-keys.d/var.key"), b"opensesame").expect("a key");
        let container = Partition {
            device: "/dev/vda3".to_string(),
            size: "60G".to_string(),
            fstype: "crypto_LUKS".to_string(),
            label: String::new(),
            parttype: String::new(),
            uuid: "AA11".to_string(),
        };
        let system = OldSystem {
            crypttab: std::fs::read_to_string(etc.join("crypttab")).unwrap(),
            fstab: std::fs::read_to_string(etc.join("fstab")).unwrap(),
            at: root.clone(),
        };
        let choices = mount_choices(&container, "ext4", "grub2", true);
        let mut mounts = Mounts::default();
        let found = discover(
            std::slice::from_ref(&container),
            std::slice::from_ref(&system),
            &[],
            &mut mounts,
            &|_, key| Ok(key == b"opensesame"),
        );
        assert!(found.why.is_empty(), "{:?}", found.why);
        let (partition, old) = &found.found[0];
        assert_eq!(partition, "/dev/vda3");
        assert_eq!(old.target, "/var");
        assert!(matches!(&old.key, Key::Data(bytes) if bytes == b"opensesame"));
        assert_eq!(
            found.default_at(&container, &choices),
            choices
                .iter()
                .position(|option| option.label == copy::open_at("/var"))
        );
        assert_eq!(Discovered::default().default_at(&container, &choices), None);
        let unsaid = Discovered {
            found: vec![(
                container.device.clone(),
                OldKey {
                    target: String::new(),
                    key: Key::Data(b"opensesame".to_vec()),
                },
            )],
            why: Vec::new(),
        };
        assert_eq!(unsaid.default_at(&container, &choices), None);

        let refused = discover(
            std::slice::from_ref(&container),
            std::slice::from_ref(&system),
            &[],
            &mut mounts,
            &|_, _| Ok(false),
        );
        assert!(refused.found.is_empty());
        assert_eq!(refused.why[0].1, copy::old_root_key_wrong("/dev/vda3"));

        let unnamed = OldSystem {
            crypttab: "root UUID=ffff none luks\n".to_string(),
            fstab: String::new(),
            at: root.clone(),
        };
        let missed = discover(
            std::slice::from_ref(&container),
            std::slice::from_ref(&unnamed),
            &[],
            &mut mounts,
            &|_, _| Ok(true),
        );
        assert_eq!(missed.why[0].1, copy::old_root_unnamed("/dev/vda3"));
        let unread = ["mounting /dev/vda2 read-only: permission denied".to_string()];
        let partly = discover(
            std::slice::from_ref(&container),
            std::slice::from_ref(&unnamed),
            &unread,
            &mut mounts,
            &|_, _| Ok(true),
        );
        assert_eq!(partly.why[0].1, copy::old_root_partly(&unread[0]));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_key_file_waited_for_at_boot_is_refused_rather_than_read() {
        let container = Partition {
            device: "/dev/vda3".to_string(),
            size: String::new(),
            fstype: "crypto_LUKS".to_string(),
            label: String::new(),
            parttype: String::new(),
            uuid: "AA11".to_string(),
        };
        let system = OldSystem {
            crypttab: "var UUID=aa11 /media/stick/var.key luks,keyfile-timeout=30s\n".to_string(),
            fstab: String::new(),
            at: PathBuf::from("/nonexistent-old-system"),
        };
        let mut mounts = Mounts::default();
        let found = discover(
            std::slice::from_ref(&container),
            std::slice::from_ref(&system),
            &[],
            &mut mounts,
            &|_, _| Ok(true),
        );
        assert!(found.found.is_empty());
        assert_eq!(found.why[0].1, copy::old_root_key_waited("var"));
    }

    #[test]
    fn a_key_mediums_devices_are_told_apart_by_the_kernels_own_flag() {
        let root = scratch("removable");
        let class = root.join("class");
        let block = root.join("block");
        std::fs::create_dir_all(&class).expect("a class directory");
        for (disk, flag) in [("vdb", "1"), ("nvme0n1", "0")] {
            std::fs::create_dir_all(block.join(disk)).expect("a disk");
            std::fs::write(block.join(disk).join("removable"), flag).expect("a flag");
            std::os::unix::fs::symlink(block.join(disk), class.join(disk)).expect("a class entry");
        }
        std::fs::create_dir_all(block.join("vdb/vdb1")).expect("a partition");
        std::fs::write(block.join("vdb/vdb1/partition"), "1").expect("a partition number");
        std::os::unix::fs::symlink(block.join("vdb/vdb1"), class.join("vdb1"))
            .expect("a partition class entry");

        let by_uuid = root.join("by-uuid");
        std::fs::create_dir_all(&by_uuid).expect("a by-uuid directory");
        std::os::unix::fs::symlink(block.join("vdb/vdb1"), by_uuid.join("AAAA"))
            .expect("a udev name");

        assert!(removable_at(&class, Path::new("/dev/vdb")));
        assert!(removable_at(&class, Path::new("/dev/vdb1")));
        assert!(removable_at(&class, &by_uuid.join("AAAA")));
        assert!(!removable_at(&class, Path::new("/dev/nvme0n1")));
        assert!(!removable_at(&class, Path::new("/dev/mapper/vg-data")));
        assert!(!removable_at(&class, Path::new("/dev")));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_key_path_cannot_leave_the_system_that_names_it() {
        let root = scratch("key-confine");
        std::fs::create_dir_all(root.join("etc")).expect("a scratch /etc");
        std::fs::write(root.join("etc/real.key"), b"opensesame").expect("a key");
        std::os::unix::fs::symlink("/etc/hostname", root.join("etc/escape.key")).expect("a link");
        let fifo = root.join("etc/slow.key");
        let path = std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
        assert!(key_file("var", &root, Path::new("/etc/real.key")).is_ok());
        assert_eq!(
            key_file("var", &root, Path::new("/etc/escape.key")).unwrap_err(),
            copy::old_root_key_outside("/etc/escape.key")
        );
        assert_eq!(
            key_file("var", &root, Path::new("/../../../etc/hostname")).unwrap_err(),
            copy::old_root_key_outside("/../../../etc/hostname")
        );
        assert_eq!(
            key_file("var", &root, Path::new("/etc/slow.key")).unwrap_err(),
            copy::old_root_key_outside("/etc/slow.key")
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_walk_reads_every_mountable_node_and_no_container() {
        let rows = mountable_rows(
            r#"{
  "blockdevices": [
    {"name": "/dev/vda1", "fstype": "vfat", "type": "part"},
    {"name": "/dev/vda2", "fstype": "crypto_LUKS", "type": "part", "children": [
      {"name": "/dev/mapper/vda2", "fstype": "ext4", "type": "crypt", "children": [
        {"name": "/dev/mapper/vg-root", "fstype": "xfs", "type": "lvm"}
      ]}
    ]},
    {"name": "/dev/vda3", "fstype": null, "type": "part"}
  ]
}"#,
        )
        .expect("lsblk JSON");
        assert_eq!(
            rows,
            [
                ("/dev/vda1".to_string(), "vfat".to_string()),
                ("/dev/mapper/vda2".to_string(), "ext4".to_string()),
                ("/dev/mapper/vg-root".to_string(), "xfs".to_string()),
            ]
        );
        for fstype in ["ext3", "ext4", "xfs", "btrfs"] {
            assert_eq!(mount_options(fstype), "ro,norecovery", "{fstype}");
        }
        for fstype in ["vfat", "exfat", "iso9660", ""] {
            assert_eq!(mount_options(fstype), "ro", "{fstype}");
        }
    }

    #[test]
    fn a_custom_layout_is_the_recipe_fisherman_takes() {
        let root = scratch("custom-layout");
        let recipe = root.join(RECIPE);
        let emitted = EMITTED.replace(
            "\"user\": { \"groups\": [\"sudo\"] },",
            "\"user\": { \"groups\": [\"sudo\"] },\n  \"varDisk\": { \"size\": \"20 GB\" },",
        );
        std::fs::write(&recipe, emitted).expect("a recipe");
        let answers = Answers {
            disk: "/dev/vda".to_string(),
            hostname: "deb2".to_string(),
            user: "buildc".to_string(),
            password: "hunter2".to_string(),
            opened: Opened::Keep,
            encryption: Encryption {
                kind: NONE.to_string(),
                passphrase: String::new(),
            },
            data: Data::default(),
            layout: Some(CustomLayout {
                disk: "/dev/vda".to_string(),
                mounts: vec![
                    CustomMount {
                        partition: "/dev/vda1".to_string(),
                        target: "/boot/efi".to_string(),
                        fstype: "unformatted".to_string(),
                    },
                    CustomMount {
                        partition: "/dev/vda2".to_string(),
                        target: "/".to_string(),
                        fstype: "ext4".to_string(),
                    },
                ],
                opens: Vec::new(),
            }),
        };
        let done = complete(&recipe, &answers).expect("a custom recipe");
        let mounts = json::items(&done, "customMounts");
        assert_eq!(mounts.len(), 2);
        assert_eq!(
            json::text(&mounts[0], "partition").as_deref(),
            Some("/dev/vda1")
        );
        assert_eq!(
            json::text(&mounts[0], "fstype").as_deref(),
            Some("unformatted")
        );
        assert_eq!(json::text(&mounts[1], "target").as_deref(), Some("/"));
        assert!(json::field(&done, "varDisk").is_none());

        let mut encrypted = answers;
        encrypted.encryption.kind = "luks-passphrase".to_string();
        let Err(refused) = complete(&recipe, &encrypted) else {
            panic!("encrypted custom layout was accepted")
        };
        assert_eq!(refused, copy::CUSTOM_ENCRYPTION);
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
                opened: Opened::Keep,
                encryption: Encryption {
                    kind: name.to_string(),
                    passphrase: "opensesame".to_string(),
                },
                data: Data::default(),
                layout: None,
            };
            let fields = answers.fields("ext4", "grub2", "", true);
            assert_eq!(fields[ROW_ENCRYPTION].value(), label);
            let read = Answers::of(&fields, None);
            assert_eq!(read.encryption.kind, name);
            let done = complete(&recipe, &read).expect("a completed recipe");
            let encryption = json::field(&done, "encryption").expect("an encryption");
            assert_eq!(json::text(encryption, "type").as_deref(), Some(name));
            assert_eq!(shown(name), label);
            let flagged = ask_encryption(
                Some(name.to_string()),
                Some("opensesame".to_string()),
                &Prompt::silent(),
                true,
            )
            .expect("one of the four");
            assert_eq!(flagged.kind, name);
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_var_rows_an_encrypted_root_can_take_are_pickable() {
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
        let at = |label: &str| gated.iter().find(|row| row.label == label).expect("a row");
        assert!(at(copy::NONE).available);
        assert!(at(copy::DATA_HERE).available);
        assert!(at(&copy::on_disk("/dev/sdb", copy::DATA_ERASED)).available);
        let kept = at(&copy::on_disk("/dev/sdb", copy::DATA_KEPT));
        assert!(!kept.available);
        assert_eq!(kept.detail, copy::DATA_UNENCRYPTED);
    }

    #[test]
    fn a_sized_var_writes_a_size_and_another_disk_writes_that_disk() {
        let root = scratch("var");
        let recipe = root.join(RECIPE);
        std::fs::write(&recipe, EMITTED).expect("a recipe");
        let written = |data: Data, kind: &str| {
            let answers = Answers {
                disk: "/dev/vda".to_string(),
                hostname: "deb2".to_string(),
                user: "buildc".to_string(),
                password: "hunter2".to_string(),
                opened: Opened::Keep,
                encryption: Encryption {
                    kind: kind.to_string(),
                    passphrase: String::new(),
                },
                data,
                layout: None,
            };
            complete(&recipe, &answers).expect("a completed recipe")
        };
        let held = |doc: &Json, key: &str| {
            json::field(doc, "varDisk")
                .and_then(|var| json::field(var, key))
                .map(|value| value.render().trim().to_string())
        };

        let sized = written(chose(copy::DATA_HERE, "200 GB"), NONE);
        assert_eq!(held(&sized, "size").as_deref(), Some("\"200 GB\""));
        assert_eq!(held(&sized, "disk"), None);
        assert_eq!(held(&sized, "encrypt"), None);

        let other = written(
            chose(&copy::on_disk("/dev/sdb", copy::DATA_ERASED), ""),
            NONE,
        );
        assert_eq!(held(&other, "disk").as_deref(), Some("\"/dev/sdb\""));
        assert_eq!(held(&other, "size"), None);
        assert_eq!(held(&other, "keepExisting").as_deref(), Some("false"));

        let kept = written(chose(&copy::on_disk("/dev/sdb", copy::DATA_KEPT), ""), NONE);
        assert_eq!(held(&kept, "disk").as_deref(), Some("\"/dev/sdb\""));
        assert_eq!(held(&kept, "keepExisting").as_deref(), Some("true"));

        for data in [
            chose(copy::DATA_HERE, "200 GB"),
            chose(&copy::on_disk("/dev/sdb", copy::DATA_ERASED), ""),
        ] {
            let encrypted = written(data, "luks-passphrase");
            assert_eq!(
                held(&encrypted, "encrypt").as_deref(),
                Some("true"),
                "{}",
                json::field(&encrypted, "varDisk")
                    .map(|var| var.render())
                    .unwrap_or_default()
            );
        }

        assert!(json::field(&written(Data::default(), NONE), "varDisk").is_none());
        assert!(json::field(&written(Data::default(), "tpm2-luks"), "varDisk").is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_tpm_forms_are_shown_and_unpickable_where_there_is_no_tpm() {
        let rows = kinds(false, true);
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
        let with = kinds(true, true);
        assert!(with.iter().all(|choice| choice.available));
        assert_eq!(with[1].detail, copy::ENC_ANY_HOLDER);
        assert_eq!(with[3].detail, copy::ENC_ANY_HOLDER);
    }

    #[test]
    fn root_encryption_is_refused_without_an_initramfs_witness() {
        let rows = kinds(true, false);
        assert!(rows[0].available);
        for row in &rows[1..] {
            assert!(!row.available, "{}", row.label);
            assert_eq!(row.detail, copy::NO_LUKS_INITRAMFS);
        }
        assert_eq!(
            ask_encryption(
                Some("luks-passphrase".to_string()),
                Some("opensesame".to_string()),
                &Prompt::silent(),
                false,
            )
            .err()
            .as_deref(),
            Some(copy::NO_LUKS_INITRAMFS)
        );

        let container = Partition {
            device: "/dev/vda2".to_string(),
            size: "60G".to_string(),
            fstype: "crypto_LUKS".to_string(),
            label: String::new(),
            parttype: String::new(),
            uuid: String::new(),
        };
        let choices = mount_choices(&container, "ext4", "grub2", false);
        let at = |target| {
            choices
                .iter()
                .find(|choice| choice.label == copy::open_at(target))
                .expect("an open row")
        };
        assert!(!at("/").available);
        assert_eq!(at("/").detail, copy::NO_LUKS_INITRAMFS);
        assert!(at("/var").available, "a data volume opens after root boots");
    }

    #[test]
    fn an_encryption_no_backend_takes_is_refused_by_name() {
        let refused = ask_encryption(Some("luks".to_string()), None, &Prompt::silent(), true)
            .err()
            .expect("a refusal");
        assert!(refused.contains("tpm2-luks-passphrase"), "{refused}");
        let kept = ask_encryption(Some("tpm2-luks".to_string()), None, &Prompt::silent(), true)
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
            opened: Opened::Keep,
            encryption: Encryption {
                kind: NONE.to_string(),
                passphrase: String::new(),
            },
            data: Data::default(),
            layout: None,
        };
        assert_eq!(
            answers.summary(&Payload {
                recipe: "/mnt/buildc/install-recipe.json".into(),
                image: "ghcr.io/buildc-org/deb2:latest".to_string(),
                hostname: "deb2".to_string(),
                filesystem: "ext4".to_string(),
                bootloader: "grub2".to_string(),
                boot: String::new(),
                luks_initramfs: true,
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
                Field::action(copy::ROW_LAYOUT, "esp + ext4 /boot + ext4 root"),
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
        assert!(!asked(&form(NONE), false).contains(&ROW_PASSPHRASE));
        assert!(!asked(&form("tpm2-luks"), false).contains(&ROW_PASSPHRASE));
        assert!(asked(&form("luks-passphrase"), false).contains(&ROW_PASSPHRASE));
        assert!(asked(&form("tpm2-luks-passphrase"), false).contains(&ROW_PASSPHRASE));
        let mut sized = form(NONE);
        assert!(!asked(&sized, false).contains(&ROW_SIZE));
        sized[ROW_DATA] = Field::pick(
            copy::ROW_DATA,
            vec![Choice::new(copy::DATA_HERE, "")],
            Some(0),
        );
        assert!(asked(&sized, false).contains(&ROW_SIZE));
        assert_eq!(asked(&form(NONE), false).len(), 8);
    }

    #[test]
    fn the_action_says_what_the_form_is_short_of() {
        use crate::ui::{Choice, Field};
        let form = |password: &str, confirm: &str, kind: &str, passphrase: &str| {
            vec![
                Field::text(copy::ROW_DISK, "/dev/vda"),
                Field::action(copy::ROW_LAYOUT, "esp + ext4 /boot + ext4 root"),
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
        assert!(short_of(&form("hunter2", "hunter2", NONE, ""), None, true).is_none());
        let differ = short_of(&form("hunter2", "hunter3", NONE, ""), None, true).unwrap();
        assert_eq!(differ, copy::NO_MATCH_ROW);
        assert!(short_of(
            &form("hunter2", "hunter2", "luks-passphrase", ""),
            None,
            true
        )
        .is_some());
        assert!(short_of(
            &form("hunter2", "hunter2", "luks-passphrase", "x"),
            None,
            true
        )
        .is_none());
        assert_eq!(
            short_of(
                &form("hunter2", "hunter2", "luks-passphrase", "x"),
                None,
                false
            )
            .as_deref(),
            Some(copy::NO_LUKS_INITRAMFS)
        );
        let mut bare = form("", "", NONE, "");
        bare[ROW_ACCOUNT] = Field::text(copy::ROW_ACCOUNT, "");
        let short = short_of(&bare, None, true).unwrap();
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
        assert!(short_of(&sized, None, true)
            .unwrap()
            .contains(copy::ROW_SIZE));
        data(&mut sized, copy::DATA_HERE.to_string(), "200 GB");
        assert!(short_of(&sized, None, true).is_none());

        let mut encrypted = form("hunter2", "hunter2", "tpm2-luks", "");
        data(&mut encrypted, copy::DATA_HERE.to_string(), "200 GB");
        assert!(short_of(&encrypted, None, true).is_none());

        let mut kept = form("hunter2", "hunter2", "tpm2-luks", "");
        data(&mut kept, copy::on_disk("/dev/sdb", copy::DATA_KEPT), "");
        assert_eq!(
            short_of(&kept, None, true).as_deref(),
            Some(copy::DATA_UNENCRYPTED)
        );

        let mut same = form("hunter2", "hunter2", NONE, "");
        data(&mut same, copy::on_disk("/dev/vda", copy::DATA_ERASED), "");
        assert_eq!(
            short_of(&same, None, true).as_deref(),
            Some(copy::DATA_SAME_DISK)
        );

        let root_keyfile = CustomLayout {
            disk: "/dev/vda".to_string(),
            mounts: Vec::new(),
            opens: vec![LuksOpen {
                partition: "/dev/vda3".to_string(),
                target: "/".to_string(),
                key: Key::Data(b"key".to_vec()),
            }],
        };
        let opens_row = |label: &str| {
            let mut fields = form("hunter2", "hunter2", NONE, "");
            fields[ROW_ENCRYPTION] =
                Field::pick(copy::ROW_ENCRYPTION, vec![Choice::new(label, "")], Some(0));
            fields
        };
        assert_eq!(
            short_of(&opens_row(copy::OPENED_KEEP), Some(&root_keyfile), true).as_deref(),
            Some(copy::OPENED_ROOT_KEYFILE)
        );
        assert_eq!(
            short_of(&opens_row(copy::OPENED_TPM2), Some(&root_keyfile), true).as_deref(),
            Some(copy::OPENED_ROOT_KEYFILE)
        );

        let plain_root = CustomLayout {
            disk: "/dev/vda".to_string(),
            mounts: vec![CustomMount {
                partition: "/dev/vda2".to_string(),
                target: "/".to_string(),
                fstype: "ext4".to_string(),
            }],
            opens: vec![LuksOpen {
                partition: "/dev/vda3".to_string(),
                target: "/var".to_string(),
                key: Key::Passphrase("opensesame".to_string()),
            }],
        };
        assert!(short_of(&opens_row(copy::OPENED_KEEP), Some(&plain_root), true).is_none());
        for answer in [copy::OPENED_ADD_KEY, copy::OPENED_TPM2] {
            assert_eq!(
                short_of(&opens_row(answer), Some(&plain_root), true).as_deref(),
                Some(copy::OPENED_KEYFILE_PLAIN),
                "{answer}"
            );
        }
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

    #[test]
    fn a_headers_slots_are_read_by_index_and_the_tokens_beside_them() {
        let raw = r#"{"keyslots":{"0":{"type":"luks2"},"2":{"type":"luks2"}},
                      "tokens":{"0":{"type":"systemd-tpm2"}}}"#;
        let slots = slots_from(raw).expect("a luksDump document");
        assert_eq!(slots.keys, vec![0, 2]);
        assert_eq!(slots.tokens, vec!["systemd-tpm2".to_string()]);
        assert!(slots.has_tpm2());
        let said = copy::slots_said(&slots.keys, &slots.tokens);
        assert!(said.contains("slot 2"), "{said}");
        assert!(said.contains("systemd-tpm2"), "{said}");
    }

    #[test]
    fn a_boot_argument_lands_on_the_options_line_once() {
        let raw = "title Fedora\nlinux /vmlinuz\noptions root=UUID=aa quiet\n";
        let (text, changed) = add_boot_arg(raw, "rd.luks.name=bb=root");
        assert!(changed);
        assert!(text.contains("options root=UUID=aa quiet rd.luks.name=bb=root"));
        assert!(text.ends_with('\n'));
        let (again, changed) = add_boot_arg(&text, "rd.luks.name=bb=root");
        assert!(!changed);
        assert_eq!(again, text);
        assert!(boot_arg_named(&text, "rd.luks.name=bb=root"));
        assert!(!boot_arg_named(raw, "rd.luks.name=bb=root"));
        assert!(!boot_arg_named("title nothing\n", "rd.luks.name=bb=root"));
    }

    #[test]
    fn the_crypttab_keeps_other_entries_and_replaces_its_own() {
        let root = scratch("crypttab");
        let etc = root.join("etc");
        std::fs::create_dir_all(&etc).expect("an etc");
        std::fs::write(
            etc.join("crypttab"),
            "var UUID=old none luks\nhome UUID=bb none luks\n",
        )
        .expect("a crypttab");
        let lines = vec![(
            "var".to_string(),
            "var UUID=new /etc/cryptsetup-keys.d/var.key luks".to_string(),
        )];
        merge_crypttab(&etc, &lines).expect("a merged crypttab");
        let said = std::fs::read_to_string(etc.join("crypttab")).expect("a crypttab");
        assert!(said.contains("home UUID=bb"), "{said}");
        assert!(said.contains("UUID=new"), "{said}");
        assert!(!said.contains("UUID=old"), "{said}");
        let other = scratch("crypttab-empty");
        let empty = other.join("etc");
        std::fs::create_dir_all(&empty).expect("an etc");
        merge_crypttab(&empty, &[]).expect("nothing to merge");
        assert!(!empty.join("crypttab").exists());
    }

    #[test]
    fn the_deployment_etc_is_the_single_one_installed() {
        let root = scratch("deployments");
        let ostree = root.join("ostree/deploy/default/deploy/deadbeef.0/etc");
        std::fs::create_dir_all(&ostree).expect("an ostree deployment");
        assert_eq!(deployment_etc(&root).expect("one"), ostree);
        let composefs = root.join("state/deploy/deadbeef/etc");
        std::fs::create_dir_all(&composefs).expect("a composefs deployment");
        assert!(deployment_etc(&root).is_err());
        std::fs::remove_dir_all(&ostree).expect("one deployment");
        assert_eq!(deployment_etc(&root).expect("one"), composefs);
    }

    #[test]
    fn the_root_device_is_where_the_layout_put_it() {
        let opened = CustomLayout {
            disk: "/dev/vda".to_string(),
            mounts: Vec::new(),
            opens: vec![LuksOpen {
                partition: "/dev/vda3".to_string(),
                target: "/".to_string(),
                key: Key::Passphrase("opensesame".to_string()),
            }],
        };
        assert_eq!(root_device(&opened).as_deref(), Some("/dev/mapper/buildc-1"));
        let mounted = CustomLayout {
            disk: "/dev/vda".to_string(),
            mounts: vec![CustomMount {
                partition: "/dev/vda2".to_string(),
                target: "/".to_string(),
                fstype: "ext4".to_string(),
            }],
            opens: Vec::new(),
        };
        assert_eq!(root_device(&mounted).as_deref(), Some("/dev/vda2"));
    }

    #[test]
    fn the_ladder_says_how_each_container_opens_at_boot() {
        let data = |key: Key| LuksOpen {
            partition: "/dev/vda3".to_string(),
            target: "/var".to_string(),
            key,
        };
        let root = LuksOpen {
            partition: "/dev/vda3".to_string(),
            target: "/".to_string(),
            key: Key::Data(b"opensesame".to_vec()),
        };
        assert_eq!(at_boot(&root, Opened::Keep), copy::OPENED_ROOT_KEYFILE);
        assert_eq!(at_boot(&root, Opened::Tpm2), copy::BOOT_TPM2);
        let root_passphrase = LuksOpen {
            partition: "/dev/vda3".to_string(),
            target: "/".to_string(),
            key: Key::Passphrase("opensesame".to_string()),
        };
        assert_eq!(
            at_boot(&root_passphrase, Opened::Keep),
            copy::BOOT_PASSPHRASE
        );
        assert_eq!(
            at_boot(&data(Key::Data(b"key".to_vec())), Opened::Keep),
            copy::BOOT_KEYFILE
        );
        assert_eq!(
            at_boot(&data(Key::Passphrase("x".into())), Opened::Keep),
            copy::BOOT_PASSPHRASE
        );
        assert_eq!(
            at_boot(&data(Key::Passphrase("x".into())), Opened::AddKey),
            copy::BOOT_ADDED_KEY
        );
        assert_eq!(
            at_boot(&data(Key::Passphrase("x".into())), Opened::Tpm2),
            copy::BOOT_TPM2
        );
    }

    #[test]
    fn a_partition_number_is_the_trailing_digits() {
        assert_eq!(partition_number("/dev/vda3").expect("a number"), "3");
        assert_eq!(partition_number("/dev/nvme0n1p12").expect("a number"), "12");
        assert_eq!(partition_number("/dev/mmcblk0p2").expect("a number"), "2");
        assert!(partition_number("/dev/vda").is_err());
    }

    #[test]
    fn the_opened_rows_answer_reads_back() {
        assert_eq!(Opened::of(copy::OPENED_TPM2), Opened::Tpm2);
        assert_eq!(Opened::of(copy::OPENED_ADD_KEY), Opened::AddKey);
        assert_eq!(Opened::of(copy::OPENED_KEEP), Opened::Keep);
        assert_eq!(Opened::of("anything else"), Opened::Keep);
    }

    #[test]
    fn a_key_on_the_root_needs_an_encrypted_root() {
        let volume = |target: &str, partition: &str| LuksOpen {
            partition: partition.to_string(),
            target: target.to_string(),
            key: Key::Passphrase("opensesame".to_string()),
        };
        let layout = |encrypted_root: bool| {
            let mounts = match encrypted_root {
                true => Vec::new(),
                false => vec![CustomMount {
                    partition: "/dev/vda2".to_string(),
                    target: "/".to_string(),
                    fstype: "ext4".to_string(),
                }],
            };
            let mut opens = vec![volume("/var", "/dev/buildc-test-no-luks")];
            if encrypted_root {
                opens.insert(0, volume("/", "/dev/buildc-test-no-luks-root"));
            }
            CustomLayout {
                disk: "/dev/vda".to_string(),
                mounts,
                opens,
            }
        };
        fn row<'a>(rows: &'a [Choice], label: &str) -> &'a Choice {
            rows.iter().find(|row| row.label == label).expect("a row")
        }
        let plain = opened_rows(&layout(false), true, true);
        for label in [copy::OPENED_ADD_KEY, copy::OPENED_TPM2] {
            let row = row(&plain, label);
            assert!(!row.available, "{label}");
            assert_eq!(row.detail, copy::OPENED_KEYFILE_PLAIN, "{label}");
        }
        let encrypted = opened_rows(&layout(true), true, true);
        assert!(row(&encrypted, copy::OPENED_ADD_KEY).available);
        assert_eq!(
            row(&encrypted, copy::OPENED_ADD_KEY).detail,
            copy::OPENED_ADD_KEY_COST
        );
        assert!(row(&encrypted, copy::OPENED_KEEP)
            .detail
            .starts_with("its slots could not be read"));
    }

    #[test]
    fn the_summary_says_a_created_var_is_encrypted() {
        let sized = chose(copy::DATA_HERE, "200 GB");
        assert!(data_said(&sized, true).contains(copy::DATA_ENCRYPTED));
        assert!(!data_said(&sized, false).contains(copy::DATA_ENCRYPTED));
        let disk = chose(&copy::on_disk("/dev/sdb", copy::DATA_ERASED), "");
        assert!(data_said(&disk, true).contains(copy::DATA_ENCRYPTED));
        assert!(!data_said(&Data::default(), true).contains(copy::DATA_ENCRYPTED));
    }
}
