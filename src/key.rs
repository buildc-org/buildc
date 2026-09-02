use crate::copy;
use crate::layout;
use crate::model::module::Key as Declared;
use crate::model::remote::REMOTE_DIR;
use crate::parse::disk::Disk;
use crate::prompt::Prompt;
use crate::ui::Choice;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct Key {
    declared: Declared,
    public: PathBuf,
    private: PathBuf,
    cn: Option<String>,
}

fn declared(
    command: &str,
    root: &Path,
    kind: Option<String>,
    module: Option<String>,
    prompt: &Prompt,
) -> Result<Declared, String> {
    let mut disk = Disk::scan(root);
    let kind = match kind {
        Some(kind) => kind,
        None => which(command, &disk, prompt)?,
    };

    let mut declaring = disk.keys.remove(&kind).unwrap_or_default();
    if declaring.is_empty() {
        return Err(absent(root, &kind, &disk));
    }
    let at = provider(&declaring, &kind, module, prompt)?;
    Ok(declaring.swap_remove(at).1)
}

impl Key {
    pub fn collect(
        root: &Path,
        kind: Option<String>,
        module: Option<String>,
        cn: Option<String>,
        prompt: &Prompt,
    ) -> Result<Self, String> {
        let declared = declared("create key", root, kind, module, prompt)?;
        let kind = declared.kind.clone();

        let public = layout::public_key(root, &declared.public);
        let private = layout::private_key(root, &declared.private);
        unwritten(&public)?;
        unwritten(&private)?;

        let cn = match (declared.generator.as_str(), declared.profile.as_deref()) {
            (generator @ ("cosign" | "ssh-keygen"), _) if cn.is_some() => {
                return Err(format!(
                    "`--cn` is a certificate's common name, and `{generator}` writes no certificate"
                ))
            }
            ("cosign" | "ssh-keygen", _) => None,
            ("openssl", Some("module-signing")) => Some(common_name(root, cn, prompt)?),
            (generator, _) => {
                return Err(format!(
                    "`{generator}` generates a key for a profile, and `key \"{kind}\"` names none; \
                     `generator \"{generator}\" profile=\"module-signing\"` is the one it has"
                ))
            }
        };

        Ok(Self {
            declared,
            public,
            private,
            cn,
        })
    }

    pub fn apply(&self, root: &Path) -> Result<(), String> {
        let work = workspace(&self.declared.kind)?;
        let (public, private) = match self.declared.generator.as_str() {
            "cosign" => cosign(&work),
            "ssh-keygen" => ssh(&work, &self.declared.kind),
            _ => openssl(
                &work,
                &self.declared,
                self.cn.as_deref().unwrap_or_default(),
            ),
        }?;

        install(&public, &self.public, 0o644)?;
        println!("wrote {}", shown(root, &self.public));
        install(&private, &self.private, 0o600)?;
        println!("wrote {}", shown(root, &self.private));
        let _ = std::fs::remove_dir_all(&work);

        print!("{}", self.next(root));
        warn_unignored(root);
        Ok(())
    }

    fn next(&self, root: &Path) -> String {
        let public = shown(root, &self.public);
        let private = shown(root, &self.private);
        match self.declared.generator.as_str() {
            "ssh-keygen" => format!(
                "\nthe private half is yours and is not the repository's: nothing in a build \
                 reads it,\nand the public half is what the image ships.\n\n\
                 next:\n\
                 \x20 commit {public}\n\
                 \x20 ssh -i {private} into a machine built from this image\n"
            ),
            "cosign" => format!(
                "\nthe key carries no password, which is what the build workflow decrypts it with.\n\n\
                 next:\n\
                 \x20 commit {public}\n\
                 \x20 gh secret set SIGNING_SECRET < {private}\n"
            ),
            _ => format!(
                "\nnext:\n\
                 \x20 commit {public}\n\
                 \x20 gh secret set MOK_PRIVKEY < {private}\n\
                 \x20 MOK_KEY_PATH={private} is what a local build reads it from\n\
                 \n\
                 every machine that boots this image enrols the certificate once, and\n\
                 until it does the modules signed with it will not load:\n\
                 \x20 sudo mokutil --import {public}\n\
                 \x20 it asks for a one-time password; reboot, choose Enroll MOK, and\n\
                 \x20 give the same password\n"
            ),
        }
    }
}

pub struct Recorded {
    from: PathBuf,
    public: PathBuf,
}

impl Recorded {
    pub fn collect(
        root: &Path,
        kind: Option<String>,
        module: Option<String>,
        from: Option<String>,
        prompt: &Prompt,
    ) -> Result<Self, String> {
        let declared = declared("set key", root, kind, module, prompt)?;
        let public = layout::public_key(root, &declared.public);
        unwritten(&public)?;
        let from = PathBuf::from(prompt.text(from, copy::KEY_FROM, "`--from`", None)?);
        let bytes = std::fs::read(&from).map_err(|err| format!("{}: {err}", from.display()))?;
        holds(&declared, &bytes)?;
        Ok(Self { from, public })
    }

    pub fn apply(&self, root: &Path) -> Result<(), String> {
        install(&self.from, &self.public, 0o644)?;
        let public = shown(root, &self.public);
        println!("wrote {public}");
        println!(
            "\nthe private half stays where it is; nothing here reads it.\n\n\
             next:\n\
             \x20 commit {public}\n"
        );
        Ok(())
    }
}

fn holds(declared: &Declared, bytes: &[u8]) -> Result<(), String> {
    let text = String::from_utf8_lossy(bytes);
    let (ok, wanted) = match declared.generator.as_str() {
        "cosign" => (
            text.starts_with("-----BEGIN PUBLIC KEY-----"),
            "a PEM public key".to_string(),
        ),
        "ssh-keygen" => (
            text.split_whitespace().next().is_some_and(|kind| {
                ["ssh-", "ecdsa-", "sk-"]
                    .iter()
                    .any(|p| kind.starts_with(p))
            }),
            "an OpenSSH public key line".to_string(),
        ),
        _ => match declared.format.as_str() {
            "der" => (
                bytes.first() == Some(&0x30),
                "a DER certificate".to_string(),
            ),
            format => (
                text.starts_with("-----BEGIN CERTIFICATE-----"),
                format!("a {} certificate", format.to_uppercase()),
            ),
        },
    };
    match ok {
        true => Ok(()),
        false => Err(format!(
            "`key \"{}\"` is written by `{}`, so the public half it records is {wanted}, and this \
             file is not one",
            declared.kind, declared.generator
        )),
    }
}

fn which(command: &str, disk: &Disk, prompt: &Prompt) -> Result<String, String> {
    let kinds: Vec<&String> = disk.keys.keys().collect();
    if kinds.is_empty() {
        return Err(undeclared("", disk));
    }
    let options: Vec<Choice> = kinds.iter().map(|kind| Choice::new(*kind, "")).collect();
    prompt
        .choose(copy::WHICH_KEY, &options)?
        .map(|at| kinds[at].clone())
        .ok_or_else(|| {
            format!(
                "`buildc {command} <kind>` names which key; this repository declares {}",
                listed(disk)
            )
        })
}

fn provider(
    declaring: &[(String, Declared)],
    kind: &str,
    given: Option<String>,
    prompt: &Prompt,
) -> Result<usize, String> {
    let dirs: Vec<&str> = declaring
        .iter()
        .map(|(dir, _)| dir.strip_prefix(&format!("{REMOTE_DIR}/")).unwrap_or(dir))
        .collect();
    if let Some(given) = given {
        return dirs
            .iter()
            .position(|dir| *dir == given)
            .ok_or_else(|| format!("`modules/{given}` does not declare `key \"{kind}\"`"));
    }
    match dirs.as_slice() {
        [_] => Ok(0),
        many => {
            let listed = many.join(", ");
            let options: Vec<Choice> = many.iter().map(|dir| Choice::new(*dir, "")).collect();
            prompt
                .choose(&copy::key_provider(kind), &options)?
                .ok_or_else(|| {
                    format!("the {kind} key is declared by {listed}; name one with `--module`")
                })
        }
    }
}

fn absent(root: &Path, kind: &str, disk: &Disk) -> String {
    let (list, ..) = crate::declarations(root);
    let index = crate::provider::Index::scan(root, &list.sources, disk, true);
    let found: Vec<String> = index
        .declaring_key(kind)
        .iter()
        .map(|module| module.qualified())
        .collect();
    match found.first() {
        Some(first) => format!(
            "The {} module must be imported before creating {kind} keys.\n\
             You can add it with `buildc import module {first}`",
            found.join(" or ")
        ),
        None => match index.unsearched() {
            said if said.is_empty() => undeclared(kind, disk),
            said => format!("{}\n\n{said}", undeclared(kind, disk)),
        },
    }
}

fn undeclared(kind: &str, disk: &Disk) -> String {
    let noun = match kind.is_empty() {
        true => "a signing key".to_string(),
        false => format!("a {kind} key"),
    };
    match disk.keys.is_empty() {
        true => format!(
            "no module in this repository declares {noun}.\n\n\
             Keys come from modules: image signing from a module carrying the cosign public \
             key, Secure Boot from a kernel module carrying a MOK certificate. Import one and \
             run this again."
        ),
        false => format!(
            "no module in this repository declares {noun}.\n\n\
             The keys declared here are {}. Import a module carrying one and run this again.",
            listed(disk)
        ),
    }
}

fn listed(disk: &Disk) -> String {
    disk.keys
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ")
}

fn common_name(root: &Path, given: Option<String>, prompt: &Prompt) -> Result<String, String> {
    let named = crate::create::named_after_root(root).unwrap_or_else(|| "buildc".to_string());
    let cn = prompt.text(
        given,
        copy::KEY_CN,
        "`--cn`",
        Some(&format!("{named} Secure Boot")),
    )?;
    usable(&cn)?;
    Ok(cn)
}

fn cosign(work: &Path) -> Result<(PathBuf, PathBuf), String> {
    let mut generate = Command::new("cosign");
    generate
        .arg("generate-key-pair")
        .current_dir(work)
        .env("COSIGN_PASSWORD", "");
    finish(
        generate,
        "cosign",
        "get it from https://github.com/sigstore/cosign/releases",
    )?;
    Ok((work.join("cosign.pub"), work.join("cosign.key")))
}

fn ssh(work: &Path, kind: &str) -> Result<(PathBuf, PathBuf), String> {
    let private = work.join(kind);
    let mut generate = Command::new("ssh-keygen");
    generate
        .args(["-t", "ed25519", "-N", "", "-C", kind, "-f"])
        .arg(&private);
    finish(
        generate,
        "ssh-keygen",
        "install it from your platform's openssh package",
    )?;
    Ok((work.join(format!("{kind}.pub")), private))
}

fn openssl(work: &Path, declared: &Declared, cn: &str) -> Result<(PathBuf, PathBuf), String> {
    let config = work.join("openssl.cnf");
    crate::init::put(&config, &module_signing(cn))?;
    let public = work.join("public");
    let private = work.join("private.pem");

    let mut generate = Command::new("openssl");
    generate
        .args([
            "req", "-x509", "-new", "-nodes", "-utf8", "-sha256", "-days", "36500", "-batch",
            "-newkey",
        ])
        .arg(format!("rsa:{}", declared.bits))
        .args(["-outform", &declared.format.to_uppercase()])
        .arg("-config")
        .arg(&config)
        .arg("-out")
        .arg(&public)
        .arg("-keyout")
        .arg(&private);
    finish(
        generate,
        "openssl",
        "install it from your platform's openssl package",
    )?;
    Ok((public, private))
}

fn unwritten(path: &Path) -> Result<(), String> {
    match std::fs::metadata(path) {
        Ok(meta) if meta.len() > 0 => Err(format!(
            "{} holds a key already, and a key is never overwritten; move it aside first",
            path.display()
        )),
        _ => Ok(()),
    }
}

fn workspace(what: &str) -> Result<PathBuf, String> {
    let dir = std::env::temp_dir().join(format!("buildc-{what}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|err| format!("{}: {err}", dir.display()))?;
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
        .map_err(|err| format!("{}: {err}", dir.display()))?;
    Ok(dir)
}

fn install(from: &Path, to: &Path, mode: u32) -> Result<(), String> {
    if let Some(dir) = to.parent() {
        std::fs::create_dir_all(dir).map_err(|err| format!("{}: {err}", dir.display()))?;
    }
    std::fs::copy(from, to).map_err(|err| format!("{}: {err}", to.display()))?;
    std::fs::set_permissions(to, std::fs::Permissions::from_mode(mode))
        .map_err(|err| format!("{}: {err}", to.display()))
}

fn finish(mut command: Command, tool: &str, install: &str) -> Result<(), String> {
    match command.status() {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!(
            "`{tool}` exited {}",
            status.code().unwrap_or_default()
        )),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Err(format!(
            "`{tool}` is not installed, and it is what generates this key: {install}"
        )),
        Err(err) => Err(format!("{tool}: {err}")),
    }
}

fn shown(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

fn usable(cn: &str) -> Result<&str, String> {
    let ok = !cn.is_empty()
        && cn.len() <= 64
        && cn.trim() == cn
        && cn
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || " -._".contains(c));
    match ok {
        true => Ok(cn),
        false => Err(format!(
            "`{cn}` is not a usable common name: up to 64 letters, digits, spaces, \
             dashes, dots and underscores"
        )),
    }
}

fn module_signing(cn: &str) -> String {
    format!(
        "[req]\n\
         distinguished_name = dn\n\
         prompt = no\n\
         x509_extensions = ext\n\
         \n\
         [dn]\n\
         CN = {cn}\n\
         \n\
         [ext]\n\
         basicConstraints = critical,CA:FALSE\n\
         keyUsage = digitalSignature\n\
         extendedKeyUsage = codeSigning\n\
         subjectKeyIdentifier = hash\n"
    )
}

fn warn_unignored(root: &Path) {
    let name = "keys/private/";
    let ignored =
        std::fs::read_to_string(root.join(".gitignore")).is_ok_and(|text| ignores_private(&text));
    if !ignored {
        eprintln!("buildc: nothing in .gitignore covers {name}; add that line before committing");
    }
}

fn ignores_private(text: &str) -> bool {
    text.lines()
        .any(|line| line.trim().trim_start_matches('/').trim_end_matches('/') == "keys/private")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Stdio;

    const SECUREBOOT: &str = r#"description "x"

supports "fedora"

key "secureboot" {
    generator "openssl" profile="module-signing" bits=4096
    public "/usr/share/secureboot/sb_cert.der" format="der"
    private "MOK.priv"
}
"#;

    const SSH: &str = r#"description "x"

supports "debian"

key "ssh" {
    generator "ssh-keygen"
    public "/usr/lib/buildc/authorized_keys"
    private "id_ed25519"
}
"#;

    fn have(tool: &str) -> bool {
        Command::new(tool)
            .arg("version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok()
    }

    fn repo(name: &str, manifest: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("{name}.{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        crate::init::put(&root.join("modules/keyholder/module.kdl"), manifest).unwrap();
        root
    }

    #[test]
    fn a_secure_boot_key_is_well_formed() {
        if !have("openssl") {
            return;
        }
        let root = repo("buildc-secureboot-key-test", SECUREBOOT);
        let ask = || {
            Key::collect(
                &root,
                Some("secureboot".into()),
                None,
                Some("Test Key".into()),
                &Prompt::silent(),
            )
        };
        ask().unwrap().apply(&root).unwrap();

        let openssl = |args: &[&str]| {
            let out = Command::new("openssl").args(args).output().unwrap();
            assert!(out.status.success(), "openssl {args:?}");
            out.stdout
        };
        let cert = layout::public_key(&root, "/usr/share/secureboot/sb_cert.der");
        let private = layout::private_key(&root, "MOK.priv");
        let from_cert = openssl(&[
            "x509",
            "-inform",
            "DER",
            "-in",
            &cert.display().to_string(),
            "-noout",
            "-pubkey",
        ]);
        let from_key = openssl(&["pkey", "-in", &private.display().to_string(), "-pubout"]);
        assert_eq!(from_cert, from_key);

        let again = ask().map(|_| ());
        assert!(again.is_err_and(|message| message.contains("never overwritten")));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_module_has_to_declare_the_key() {
        let root = repo("buildc-no-key-test", SECUREBOOT);
        let message = Key::collect(&root, Some("cosign".into()), None, None, &Prompt::silent())
            .map(|_| ())
            .unwrap_err();
        assert_eq!(
            message,
            "no module in this repository declares a cosign key.\n\n\
             The keys declared here are secureboot. Import a module carrying one and run this again."
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_referenced_module_can_declare_a_repository_key() {
        let root = std::env::temp_dir().join("buildc-remote-key-test");
        let _ = std::fs::remove_dir_all(&root);
        crate::init::put(
            &root.join("modules/.remote/one/keyholder/module.kdl"),
            SECUREBOOT,
        )
        .unwrap();
        let key = Key::collect(
            &root,
            Some("secureboot".into()),
            Some("one/keyholder".into()),
            Some("Test Key".into()),
            &Prompt::silent(),
        )
        .unwrap();
        assert_eq!(
            key.public,
            root.join("keys/public/usr/share/secureboot/sb_cert.der")
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn an_ssh_key_is_a_pair_openssh_reads() {
        if !have("ssh-keygen") {
            return;
        }
        let root = repo("buildc-ssh-key-test", SSH);
        let err = Key::collect(
            &root,
            Some("ssh".into()),
            None,
            Some("Nobody".into()),
            &Prompt::silent(),
        )
        .map(|_| ())
        .unwrap_err();
        assert!(err.contains("`ssh-keygen` writes no certificate"), "{err}");

        Key::collect(&root, Some("ssh".into()), None, None, &Prompt::silent())
            .unwrap()
            .apply(&root)
            .unwrap();
        let public = layout::public_key(&root, "/usr/lib/buildc/authorized_keys");
        let line = std::fs::read_to_string(&public).unwrap();
        assert!(line.starts_with("ssh-ed25519 "), "{line}");
        let out = Command::new("ssh-keygen")
            .args(["-y", "-P", "", "-f"])
            .arg(layout::private_key(&root, "id_ed25519"))
            .output()
            .unwrap();
        assert!(out.status.success());
        let derived = String::from_utf8_lossy(&out.stdout);
        assert_eq!(
            derived.split_whitespace().nth(1),
            line.split_whitespace().nth(1)
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_recorded_key_lands_where_the_module_says_and_is_read_before_it_does() {
        let root = repo("buildc-set-key-test", SSH);
        let held = root.join("held.pub");
        let record = |from: &Path| {
            Recorded::collect(
                &root,
                Some("ssh".into()),
                None,
                Some(from.display().to_string()),
                &Prompt::silent(),
            )
            .map(|_| ())
        };

        std::fs::write(&held, "-----BEGIN CERTIFICATE-----\n").unwrap();
        let err = record(&held).unwrap_err();
        assert!(err.contains("an OpenSSH public key line"), "{err}");

        std::fs::write(&held, "ssh-ed25519 AAAAC3Nz nobody@example\n").unwrap();
        Recorded::collect(
            &root,
            Some("ssh".into()),
            None,
            Some(held.display().to_string()),
            &Prompt::silent(),
        )
        .unwrap()
        .apply(&root)
        .unwrap();
        let public = layout::public_key(&root, "/usr/lib/buildc/authorized_keys");
        assert_eq!(
            std::fs::read_to_string(&public).unwrap(),
            "ssh-ed25519 AAAAC3Nz nobody@example\n"
        );
        assert!(record(&held)
            .unwrap_err()
            .contains("a key is never overwritten"));
        let unasked = Recorded::collect(&root, Some("ssh".into()), None, None, &Prompt::silent())
            .map(|_| ())
            .unwrap_err();
        assert!(unasked.contains("a key is never overwritten"), "{unasked}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn what_a_recorded_public_half_has_to_look_like_is_per_generator() {
        let declared = |generator: &str, format: &str| Declared {
            kind: "k".into(),
            generator: generator.into(),
            profile: None,
            bits: 4096,
            public: "/k".into(),
            format: format.into(),
            private: "k".into(),
            span: Default::default(),
        };
        let cosign = declared("cosign", "pem");
        assert!(holds(&cosign, b"-----BEGIN PUBLIC KEY-----\n").is_ok());
        assert!(holds(&cosign, b"-----BEGIN CERTIFICATE-----\n").is_err());

        let pem = declared("openssl", "pem");
        assert!(holds(&pem, b"-----BEGIN CERTIFICATE-----\n").is_ok());
        assert!(holds(&pem, b"\x30\x82").is_err());

        let der = declared("openssl", "der");
        assert!(holds(&der, b"\x30\x82\x03\x01").is_ok());
        assert!(holds(&der, b"-----BEGIN CERTIFICATE-----\n").is_err());

        let ssh = declared("ssh-keygen", "pem");
        assert!(holds(&ssh, b"ssh-ed25519 AAAA c\n").is_ok());
        assert!(holds(&ssh, b"sk-ssh-ed25519@openssh.com AAAA c\n").is_ok());
        assert!(holds(&ssh, b"not a key\n").is_err());
    }

    #[test]
    fn private_key_ignore_spelling_is_flexible() {
        for line in ["keys/private/", "/keys/private/", "keys/private"] {
            assert!(ignores_private(line));
        }
    }
}
