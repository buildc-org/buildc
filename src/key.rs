use crate::model::remote::REMOTE_DIR;
use crate::parse::disk::Disk;
use crate::prompt::Prompt;
use crate::ui::Choice;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const COSIGN_PUB: &str = "/etc/pki/containers/cosign.pub";
const SB_CERT: &str = "/usr/share/secureboot/sb_cert.der";

const COSIGN_KEY: &str = "cosign.key";
const MOK_PRIV: &str = "MOK.priv";

pub struct Cosign {
    public: PathBuf,
    private: PathBuf,
}

impl Cosign {
    pub fn collect(root: &Path, module: Option<String>, prompt: &Prompt) -> Result<Self, String> {
        let dir = provider(root, COSIGN_PUB, module, prompt)?;
        let public = overlay(root, &dir, COSIGN_PUB);
        let private = root.join(COSIGN_KEY);
        unwritten(&public)?;
        unwritten(&private)?;
        Ok(Self { public, private })
    }

    pub fn apply(&self, root: &Path) -> Result<(), String> {
        let work = workspace("cosign")?;
        let mut generate = Command::new("cosign");
        generate
            .arg("generate-key-pair")
            .current_dir(&work)
            .env("COSIGN_PASSWORD", "");
        finish(
            generate,
            "cosign",
            "get it from https://github.com/sigstore/cosign/releases, or `dnf install cosign`",
        )?;

        install(&work.join("cosign.pub"), &self.public, 0o644)?;
        println!("wrote {}", shown(root, &self.public));
        install(&work.join("cosign.key"), &self.private, 0o600)?;
        println!("wrote {COSIGN_KEY}");
        let _ = std::fs::remove_dir_all(&work);

        println!(
            "\nthe key carries no password, which is what the build workflow decrypts it with.\n\n\
             next:\n\
             \x20 commit {}\n\
             \x20 gh secret set SIGNING_SECRET < {COSIGN_KEY}\n",
            shown(root, &self.public)
        );
        warn_unignored(root, COSIGN_KEY);
        Ok(())
    }
}

pub struct Mok {
    cert: PathBuf,
    private: PathBuf,
    cn: String,
}

impl Mok {
    pub fn collect(
        root: &Path,
        module: Option<String>,
        cn: Option<String>,
        prompt: &Prompt,
    ) -> Result<Self, String> {
        let dir = provider(root, SB_CERT, module, prompt)?;
        let cert = overlay(root, &dir, SB_CERT);
        let private = root.join(MOK_PRIV);
        unwritten(&cert)?;
        unwritten(&private)?;

        let fallback = format!("{} Secure Boot", named_after_root(root));
        let cn = prompt.text(
            cn,
            "common name, which is what the enrolment prompt shows",
            "`--cn`",
            Some(&fallback),
        )?;
        common_name(&cn)?;
        Ok(Self { cert, private, cn })
    }

    pub fn apply(&self, root: &Path) -> Result<(), String> {
        let work = workspace("mok")?;
        let config = work.join("openssl.cnf");
        crate::init::put(&config, &openssl_config(&self.cn))?;
        let mut generate = Command::new("openssl");
        generate
            .args([
                "req", "-x509", "-new", "-nodes", "-utf8", "-sha256", "-days", "36500", "-batch",
                "-newkey", "rsa:4096", "-outform", "DER",
            ])
            .arg("-config")
            .arg(&config)
            .arg("-out")
            .arg(work.join("cert.der"))
            .arg("-keyout")
            .arg(work.join("key.pem"));
        finish(generate, "openssl", "`dnf install openssl`")?;

        install(&work.join("cert.der"), &self.cert, 0o644)?;
        println!("wrote {}", shown(root, &self.cert));
        install(&work.join("key.pem"), &self.private, 0o600)?;
        println!("wrote {MOK_PRIV}");
        let _ = std::fs::remove_dir_all(&work);

        println!(
            "\nnext:\n\
             \x20 commit {cert}\n\
             \x20 gh secret set MOK_PRIVKEY < {MOK_PRIV}\n\
             \x20 MOK_KEY_PATH={MOK_PRIV} is what a local build reads it from\n\
             \n\
             every machine that boots this image enrols the certificate once, and\n\
             until it does the modules signed with it will not load:\n\
             \x20 sudo mokutil --import {cert}\n\
             \x20 it asks for a one-time password; reboot, choose Enroll MOK, and\n\
             \x20 give the same password\n",
            cert = shown(root, &self.cert)
        );
        warn_unignored(root, MOK_PRIV);
        Ok(())
    }
}

fn provider(
    root: &Path,
    path: &str,
    given: Option<String>,
    prompt: &Prompt,
) -> Result<String, String> {
    let mut found = Disk::scan(root).providers.remove(path).unwrap_or_default();
    found.retain(|dir| !dir.starts_with(&format!("{REMOTE_DIR}/")));

    if let Some(given) = given {
        return match found.contains(&given) {
            true => Ok(given),
            false => Err(format!(
                "`modules/{given}` does not declare `provides-file \"{path}\"`"
            )),
        };
    }
    match found.as_slice() {
        [] => Err(format!(
            "no module declares `provides-file \"{path}\"`, and that module is where \
             the public half goes; write or import one first"
        )),
        [one] => Ok(one.clone()),
        many => {
            let options: Vec<Choice> = many.iter().map(|dir| Choice::new(dir, "")).collect();
            let listed = many.join(", ");
            prompt
                .choose(&format!("{listed} all ship `{path}`; which one"), &options)?
                .map(|chosen| many[chosen].clone())
                .ok_or_else(|| {
                    format!("`{path}` is declared by {listed}; name one with `--module`")
                })
        }
    }
}

fn overlay(root: &Path, dir: &str, path: &str) -> PathBuf {
    root.join("modules")
        .join(dir)
        .join("files")
        .join(path.trim_start_matches('/'))
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

fn named_after_root(root: &Path) -> String {
    std::fs::canonicalize(root)
        .ok()
        .as_deref()
        .and_then(Path::file_name)
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "buildc".to_string())
}

fn common_name(cn: &str) -> Result<&str, String> {
    let usable = !cn.is_empty()
        && cn.len() <= 64
        && cn.trim() == cn
        && cn
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || " -._".contains(c));
    match usable {
        true => Ok(cn),
        false => Err(format!(
            "`{cn}` is not a usable common name: up to 64 letters, digits, spaces, \
             dashes, dots and underscores"
        )),
    }
}

fn openssl_config(cn: &str) -> String {
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

fn warn_unignored(root: &Path, name: &str) {
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

    fn have(tool: &str) -> bool {
        Command::new(tool)
            .arg("version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    fn repo(name: &str, provides: &str) -> PathBuf {
        let root = std::env::temp_dir().join(name);
        let _ = std::fs::remove_dir_all(&root);
        crate::init::put(
            &root.join("modules/keyholder/module.kdl"),
            &format!("description \"x\"\n\nsupports \"fedora\"\n\nprovides-file \"{provides}\"\n"),
        )
        .unwrap();
        root
    }

    fn openssl(args: &[&str]) -> Vec<u8> {
        let out = Command::new("openssl").args(args).output().unwrap();
        assert!(out.status.success(), "openssl {args:?}");
        out.stdout
    }

    #[test]
    fn mok_key_is_well_formed() {
        if !have("openssl") {
            return;
        }
        let root = repo("buildc-mok-key-test", SB_CERT);
        let ask = || Mok::collect(&root, None, Some("Test Key".into()), &Prompt::silent());
        ask().unwrap().apply(&root).unwrap();

        let cert = overlay(&root, "keyholder", SB_CERT);
        let private = root.join(MOK_PRIV);
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
    fn a_module_has_to_declare_the_file() {
        let root = repo("buildc-no-provider-test", "/usr/bin/elsewhere");
        let message = provider(&root, SB_CERT, None, &Prompt::silent()).unwrap_err();
        assert!(message.contains("no module declares"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
