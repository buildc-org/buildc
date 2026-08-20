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

impl Key {
    pub fn collect(
        root: &Path,
        kind: Option<String>,
        module: Option<String>,
        cn: Option<String>,
        prompt: &Prompt,
    ) -> Result<Self, String> {
        let mut disk = Disk::scan(root);
        let kind = match kind {
            Some(kind) => kind,
            None => which(&disk, prompt)?,
        };

        let mut declaring = disk.keys.remove(&kind).unwrap_or_default();
        declaring.retain(|(dir, _)| !dir.starts_with(&format!("{REMOTE_DIR}/")));
        if declaring.is_empty() {
            return Err(absent(root, &kind, &disk));
        }
        let at = provider(&declaring, &kind, module, prompt)?;
        let (_, declared) = declaring.swap_remove(at);

        let public = layout::public_key(root, &declared.public);
        let private = layout::private_key(root, &declared.private);
        unwritten(&public)?;
        unwritten(&private)?;

        let cn = match (declared.generator.as_str(), declared.profile.as_deref()) {
            ("cosign", _) if cn.is_some() => {
                return Err(
                    "`--cn` is a certificate's common name, and `cosign` writes no certificate"
                        .into(),
                )
            }
            ("cosign", _) => None,
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

fn which(disk: &Disk, prompt: &Prompt) -> Result<String, String> {
    let kinds: Vec<&String> = disk.keys.keys().collect();
    if kinds.is_empty() {
        return Err(undeclared("", disk));
    }
    let options: Vec<Choice> = kinds.iter().map(|kind| Choice::new(*kind, "")).collect();
    prompt
        .choose("which key", &options)?
        .map(|at| kinds[at].clone())
        .ok_or_else(|| {
            format!(
                "`buildc create key <kind>` names which key; this repository declares {}",
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
    let dirs: Vec<&str> = declaring.iter().map(|(dir, _)| dir.as_str()).collect();
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
                .choose(
                    &format!("{listed} all declare the {kind} key; which one"),
                    &options,
                )?
                .ok_or_else(|| {
                    format!("the {kind} key is declared by {listed}; name one with `--module`")
                })
        }
    }
}

fn absent(root: &Path, kind: &str, disk: &Disk) -> String {
    let (list, ..) = crate::declarations(root);
    let found: Vec<String> = crate::import::catalog(root, &list.sources)
        .unwrap_or_default()
        .into_iter()
        .filter(|module| module.keys.iter().any(|declared| declared == kind))
        .map(|module| module.qualified)
        .collect();
    match found.first() {
        Some(first) => format!(
            "The {} module must be imported before creating {kind} keys.\n\
             You can add it with `buildc import module {first}`",
            found.join(" or ")
        ),
        None => undeclared(kind, disk),
    }
}

fn undeclared(kind: &str, disk: &Disk) -> String {
    let kind = match kind.is_empty() {
        true => "<kind>",
        false => kind,
    };
    let here = match disk.keys.is_empty() {
        true => String::new(),
        false => format!("\nThe keys declared here are {}.", listed(disk)),
    };
    format!(
        "nothing here or in the collections this repository declares carries a {kind} key.\n\
         The module shipping the public half is what declares one, as `key \"{kind}\"` \
         naming a generator, the path the image ships the public half at, and the file \
         the private half is written to.{here}"
    )
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
        "common name, which is what the enrolment prompt shows",
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
        "get it from https://github.com/sigstore/cosign/releases, or `dnf install cosign`",
    )?;
    Ok((work.join("cosign.pub"), work.join("cosign.key")))
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
    finish(generate, "openssl", "`dnf install openssl`")?;
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
    let ignored = std::fs::read_to_string(root.join(".gitignore"))
        .is_ok_and(|text| text.lines().any(|line| line.trim() == name));
    if !ignored {
        eprintln!("buildc: nothing in .gitignore covers {name}; add that line before committing");
    }
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

    fn have(tool: &str) -> bool {
        Command::new(tool)
            .arg("version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    fn repo(name: &str, manifest: &str) -> PathBuf {
        let root = std::env::temp_dir().join(name);
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
        assert!(message.contains("carries a cosign key"), "{message}");
        assert!(
            message.contains("declared here are secureboot"),
            "{message}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
