use crate::copy;
use crate::emit::json::{self, Json};
use crate::prompt::Prompt;
use crate::ui::Choice;
use std::io::{BufRead as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

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
        return Ok(Found::Image(Payload {
            image: field("image")?,
            hostname: field("hostname")?,
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
             clear: {}\n\nhelp: `buildc install --from <root>` names one outright",
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

const KINDS: [(&str, &str); 4] = [
    (NONE, copy::ENC_NONE),
    ("tpm2-luks", copy::ENC_TPM2),
    ("luks-passphrase", copy::ENC_PASSPHRASE),
    ("tpm2-luks-passphrase", copy::ENC_BOTH),
];

const NONE: &str = "none";

const TPM: &str = "/dev/tpmrm0";

fn tpm() -> PathBuf {
    std::env::var_os("BUILDC_TPM")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(TPM))
}

pub struct Encryption {
    pub kind: String,
    pub passphrase: String,
}

impl Encryption {
    fn none() -> Self {
        Self {
            kind: NONE.to_string(),
            passphrase: String::new(),
        }
    }

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
}

#[derive(Default)]
pub struct Given {
    pub disk: Option<String>,
    pub hostname: Option<String>,
    pub user: Option<String>,
    pub password: Option<String>,
    pub encryption: Option<String>,
    pub passphrase: Option<String>,
}

impl Answers {
    pub fn collect(
        payload: &Payload,
        given: Given,
        prompt: &Prompt,
    ) -> Result<Option<Self>, String> {
        let mut answers = Self {
            disk: ask_disk(given.disk, None, prompt)?,
            hostname: prompt.text(
                given.hostname,
                copy::INSTALL_NAME,
                "--hostname",
                Some(&payload.hostname),
            )?,
            user: prompt.text(given.user, copy::INSTALL_USER, "--user", None)?,
            password: prompt.secret(given.password, copy::INSTALL_PASSWORD, "--password")?,
            encryption: ask_encryption(
                given.encryption,
                given.passphrase,
                &Encryption::none(),
                prompt,
            )?,
        };
        while prompt.draws() {
            let rows = answers.rows();
            match crate::ui::review(
                &copy::erasing(&answers.disk),
                &rows,
                copy::INSTALL,
                copy::INSTALL_KEYS,
            )? {
                None => return Ok(None),
                Some(at) if at == rows.len() => break,
                Some(at) => answers.ask(at, prompt)?,
            }
        }
        Ok(Some(answers))
    }

    fn rows(&self) -> Vec<(String, String)> {
        vec![
            (copy::ROW_DISK.to_string(), self.disk.clone()),
            (copy::ROW_HOSTNAME.to_string(), self.hostname.clone()),
            (copy::ROW_ACCOUNT.to_string(), self.user.clone()),
            (
                copy::ROW_PASSWORD.to_string(),
                copy::PASSWORD_SET.to_string(),
            ),
            (
                copy::ROW_ENCRYPTION.to_string(),
                self.encryption.kind.clone(),
            ),
        ]
    }

    fn ask(&mut self, row: usize, prompt: &Prompt) -> Result<(), String> {
        match row {
            0 => self.disk = ask_disk(None, Some(&self.disk), prompt)?,
            1 => {
                self.hostname =
                    prompt.text(None, copy::INSTALL_NAME, "--hostname", Some(&self.hostname))?
            }
            2 => self.user = prompt.text(None, copy::INSTALL_USER, "--user", Some(&self.user))?,
            3 => self.password = prompt.secret(None, copy::INSTALL_PASSWORD, "--password")?,
            _ => self.encryption = ask_encryption(None, None, &self.encryption, prompt)?,
        }
        Ok(())
    }
}

const SECTOR: u64 = 512;

const VIRTUAL: [&str; 7] = ["loop", "ram", "zram", "sr", "fd", "dm-", "md"];

pub fn disks(sys: &Path) -> Vec<(String, String)> {
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
    let found = disks(Path::new("/sys/block"));
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
        None => Err(format!(
            "give --disk, since nothing was chosen: {}",
            copy::INSTALL_DISK.trim_end_matches(':')
        )),
    }
}

fn kinds(tpm: bool) -> Vec<Choice> {
    KINDS
        .iter()
        .map(|(name, detail)| match tpm || !name.starts_with("tpm2") {
            true => Choice::new(*name, *detail),
            false => Choice::new(*name, copy::NO_TPM).unavailable(),
        })
        .collect()
}

fn ask_encryption(
    given: Option<String>,
    passphrase: Option<String>,
    current: &Encryption,
    prompt: &Prompt,
) -> Result<Encryption, String> {
    let names = || {
        KINDS
            .iter()
            .map(|(name, _)| *name)
            .collect::<Vec<_>>()
            .join(", ")
    };
    let kind = match given {
        Some(kind) if !KINDS.iter().any(|(name, _)| *name == kind) => {
            return Err(format!("`{kind}` is not one of {}", names()))
        }
        Some(kind) => kind,
        None if !prompt.asks() => current.kind.clone(),
        None => {
            let options = kinds(tpm().exists());
            let at = KINDS
                .iter()
                .position(|(name, _)| *name == current.kind)
                .unwrap_or(0);
            match prompt.choose_current(copy::INSTALL_ENCRYPTION, &options, at)? {
                Some(at) => KINDS[at].0.to_string(),
                None => current.kind.clone(),
            }
        }
    };
    Ok(Encryption {
        passphrase: match Encryption::wants_passphrase(&kind) {
            false => String::new(),
            true => prompt.secret(passphrase, copy::LUKS_PASSPHRASE, "--passphrase")?,
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

impl Found {
    pub fn payload(&self) -> Result<&Payload, String> {
        match self {
            Self::Image(payload) => Ok(payload),
            Self::Repo(root) => Err(format!(
                "{} is a repository and not a built image, so there is nothing here to install \
                 yet\n\nhelp: `buildc build` in it, then `buildc install --from <the built payload>`",
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

fn say(line: &str) -> String {
    let Ok(event) = Json::parse(line) else {
        return line.to_string();
    };
    let text = |key: &str| json::text(&event, key).unwrap_or_default();
    let count = |key: &str| json::number(&event, key).unwrap_or(0);
    match json::text(&event, "type").as_deref() {
        Some("step") => format!(
            "[{:>3}%] {}/{} {}",
            count("cumulative_pct"),
            count("step"),
            count("total_steps"),
            text("step_name")
        ),
        Some("info" | "substep") => format!("       {}", text("message")),
        Some("complete") => format!("[100%] {}", text("message")),
        Some("recovery_key") => format!(
            "\nwrite this down, it is the recovery key: {}\n",
            text("key")
        ),
        _ => line.to_string(),
    }
}

pub fn run(payload: &Payload, answers: &Answers) -> Result<(), String> {
    let path = stage(&complete(&payload.recipe, answers)?)?;
    eprintln!(
        "buildc: installing {} as {} onto {}",
        payload.image, answers.hostname, answers.disk
    );
    let mut child = Command::new(BACKEND)
        .arg(&path)
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|err| format!("{BACKEND}: {err}"))?;
    if let Some(events) = child.stdout.take() {
        for line in std::io::BufReader::new(events)
            .lines()
            .map_while(Result::ok)
        {
            println!("{}", say(&line));
        }
    }
    let status = child.wait().map_err(|err| format!("{BACKEND}: {err}"))?;
    let _ = std::fs::remove_file(&path);
    match status.success() {
        true => Ok(()),
        false => Err(format!("{BACKEND} did not finish: {status}")),
    }
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
  "bootloader": "systemd",
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
        };
        let done = complete(&recipe, &answers).expect("the person's half goes in");

        for (key, value) in [
            ("image", "\"ghcr.io/buildc-org/deb2:latest\""),
            ("composeFsBackend", "true"),
            ("bootloader", "\"systemd\""),
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
            .map(|(name, _)| *name)
            .filter(|name| Encryption::wants_passphrase(name))
            .collect();
        assert_eq!(wants, ["luks-passphrase", "tpm2-luks-passphrase"]);
    }

    #[test]
    fn the_tpm_forms_are_shown_and_unpickable_where_there_is_no_tpm() {
        let shown = kinds(false);
        let without: Vec<(&str, bool)> = shown
            .iter()
            .map(|choice| (choice.detail.as_str(), choice.available))
            .collect();
        assert_eq!(
            without,
            vec![
                (copy::ENC_NONE, true),
                (copy::NO_TPM, false),
                (copy::ENC_PASSPHRASE, true),
                (copy::NO_TPM, false),
            ]
        );
        assert!(kinds(true).iter().all(|choice| choice.available));
    }

    #[test]
    fn an_encryption_no_backend_takes_is_refused_by_name() {
        let refused = ask_encryption(
            Some("luks".to_string()),
            None,
            &Encryption::none(),
            &Prompt::silent(),
        )
        .err()
        .expect("a refusal");
        assert!(refused.contains("tpm2-luks-passphrase"), "{refused}");
        let kept = ask_encryption(
            Some("tpm2-luks".to_string()),
            None,
            &Encryption::none(),
            &Prompt::silent(),
        )
        .expect("one of the four");
        assert_eq!(kept.kind, "tpm2-luks");
        assert!(kept.passphrase.is_empty());
    }

    #[test]
    fn only_the_disks_a_person_could_install_onto_are_offered() {
        let sys = scratch("sys");
        let block = |name: &str, sectors: &str, model: Option<&str>, removable: &str| {
            let at = sys.join(name);
            std::fs::create_dir_all(at.join("device")).expect("a block device");
            std::fs::write(at.join("size"), sectors).expect("a size");
            std::fs::write(at.join("removable"), removable).expect("a removable");
            if let Some(model) = model {
                std::fs::write(at.join("device/model"), model).expect("a model");
            }
        };
        block("sda", "937703088\n", Some("Samsung SSD 980\n"), "0\n");
        block("sdb", "60088320\n", None, "1\n");
        block("loop0", "204800\n", None, "0\n");
        block("sdc", "0\n", None, "1\n");

        assert_eq!(
            disks(&sys),
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
        let said = |line: &str| say(line);
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
    fn the_confirm_screen_shows_what_is_about_to_be_erased() {
        let answers = Answers {
            disk: "/dev/vda".to_string(),
            hostname: "deb2".to_string(),
            user: "buildc".to_string(),
            password: "hunter2".to_string(),
            encryption: Encryption::none(),
        };
        assert_eq!(
            answers.rows(),
            vec![
                (copy::ROW_DISK.to_string(), "/dev/vda".to_string()),
                (copy::ROW_HOSTNAME.to_string(), "deb2".to_string()),
                (copy::ROW_ACCOUNT.to_string(), "buildc".to_string()),
                (
                    copy::ROW_PASSWORD.to_string(),
                    copy::PASSWORD_SET.to_string()
                ),
                (copy::ROW_ENCRYPTION.to_string(), NONE.to_string()),
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
    fn a_root_with_nothing_built_on_it_refuses_by_name() {
        let nothing = Found::Nothing("/mnt/buildc".into()).payload().unwrap_err();
        assert!(nothing.contains(RECIPE) && nothing.contains(crate::layout::REPO_FILE));
        assert!(nothing.contains("/mnt/buildc"), "{nothing}");
        let repo = Found::Repo("/mnt/buildc".into()).payload().unwrap_err();
        assert!(repo.contains("buildc build"), "{repo}");
    }
}
