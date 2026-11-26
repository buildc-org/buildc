use crate::model::component::TrustRoot;
use common::json::{field, items, text, Json};
use std::path::{Path, PathBuf};
use std::process::Command;

pub const SPDX: &str = "https://spdx.dev/Document";

const DSSE: &str = "application/vnd.dsse.envelope.v1+json";

pub struct Attestation {
    pub predicate: String,
    pub subject: String,
    pub sha256: String,
    pub payload: String,
    pub payload_sha256: String,
    pub packages: usize,
}

pub struct Recorded {
    pub attestation: Option<Attestation>,
    pub signature: bool,
    pub attestation_checked: bool,
    pub gaps: Vec<String>,
}

impl Recorded {
    pub fn absent(why: String) -> Self {
        Self {
            attestation: None,
            signature: false,
            attestation_checked: false,
            gaps: vec![why],
        }
    }
}

/// A subject mismatch refuses the build. An unreadable attestation becomes a gap.
#[derive(Debug)]
enum Problem {
    Mismatch(String),
    Unreadable(String),
}

pub fn fetch(root: &Path, base: &str, verifier: Option<&TrustRoot>) -> Result<Recorded, String> {
    let mut recorded = Recorded {
        attestation: None,
        signature: false,
        attestation_checked: false,
        gaps: Vec::new(),
    };
    // A base with no digest still verifies its signature because cosign reads the reference itself.
    let Some((att_ref, digest)) = attestation_ref(base) else {
        recorded.gaps.push(format!(
            "`{base}` carries no digest, so its attestation cannot be fetched"
        ));
        return verify(root, base, verifier, recorded);
    };
    let scratch = Scratch::new()?;
    match pull(&att_ref, scratch.path()) {
        Ok(()) => match read(scratch.path(), &digest) {
            Ok(Some(attestation)) => recorded.attestation = Some(attestation),
            Ok(None) => recorded
                .gaps
                .push(format!("`{base}` publishes no SPDX attestation")),
            Err(Problem::Mismatch(why)) => return Err(why),
            Err(Problem::Unreadable(why)) => recorded.gaps.push(why),
        },
        Err(why) if absent(&why) => recorded
            .gaps
            .push(format!("`{base}` publishes no attestation")),
        Err(why) => recorded.gaps.push(format!(
            "the attestation of `{base}` could not be fetched: {why}"
        )),
    }
    verify(root, base, verifier, recorded)
}

fn verify(
    root: &Path,
    base: &str,
    verifier: Option<&TrustRoot>,
    mut recorded: Recorded,
) -> Result<Recorded, String> {
    let Some(verifier) = verifier else {
        return Ok(recorded);
    };
    let cosign = Cosign::of(root, verifier)?;
    cosign.verify(base)?;
    recorded.signature = true;
    if let Some(attestation) = &recorded.attestation {
        let verified = cosign.verify_attestation(base)?;
        if !verified_payload(&verified, attestation) {
            return Err(format!(
                "`{base}` verified an attestation whose payload is not the one buildc fetched; \
                 the registry served two different attestations"
            ));
        }
        recorded.attestation_checked = true;
    }
    Ok(recorded)
}

fn verified_payload(stdout: &str, recorded: &Attestation) -> bool {
    let parsed = std::iter::once(stdout)
        .chain(stdout.lines())
        .filter(|text| !text.trim().is_empty())
        .filter_map(|text| Json::parse(text).ok());
    parsed
        .into_iter()
        .any(|value| holds_payload(&value, recorded))
}

fn holds_payload(value: &Json, recorded: &Attestation) -> bool {
    if let Some(payload) = text(value, "payload") {
        if let Some(bytes) = base64(&payload) {
            if crate::runtime::sha256_stdin(&bytes)
                .is_ok_and(|hash| hash == recorded.payload_sha256)
            {
                return true;
            }
        }
    }
    match value {
        Json::Object(fields) => fields
            .iter()
            .any(|(_, value)| holds_payload(value, recorded)),
        Json::Array(items) => items.iter().any(|value| holds_payload(value, recorded)),
        Json::Null | Json::Bool(_) | Json::Number(_) | Json::String(_) => false,
    }
}

enum Cosign {
    Key { path: PathBuf },
    Identity { identity: String, issuer: String },
}

impl Cosign {
    fn of(root: &Path, verifier: &TrustRoot) -> Result<Self, String> {
        match (&verifier.key, &verifier.identity) {
            (Some(key), _) => {
                let path = root.join(key);
                if !path.is_file() {
                    return Err(format!(
                        "`{key}` names no cosign public key in the repository; a base verifier \
                         reads the key from the repository"
                    ));
                }
                if let Some(expected) = &verifier.key_sha256 {
                    let actual = crate::runtime::sha256_file(&path)?;
                    if &actual != expected {
                        return Err(format!(
                            "`{key}` hashes to {actual}, not {expected}; the repository pins the \
                             key's content"
                        ));
                    }
                }
                Ok(Self::Key { path })
            }
            (None, Some(identity)) => Ok(Self::Identity {
                identity: identity.clone(),
                issuer: verifier.issuer.clone().unwrap_or_default(),
            }),
            (None, None) => Err("the base names no verifier".to_string()),
        }
    }

    fn flags(&self) -> Vec<String> {
        match self {
            // The repository key is the trust root. Cosign does not check the transparency log.
            Self::Key { path } => vec![
                "--key".to_string(),
                path.display().to_string(),
                "--insecure-ignore-tlog=true".to_string(),
            ],
            Self::Identity { identity, issuer } => vec![
                "--certificate-identity".to_string(),
                identity.clone(),
                "--certificate-oidc-issuer".to_string(),
                issuer.clone(),
            ],
        }
    }

    fn verify(&self, base: &str) -> Result<(), String> {
        self.run(&["verify"], base).map(|_| ())
    }

    fn verify_attestation(&self, base: &str) -> Result<String, String> {
        self.run(&["verify-attestation", "--type", "spdxjson"], base)
    }

    fn run(&self, command: &[&str], base: &str) -> Result<String, String> {
        let mut args: Vec<String> = command.iter().map(|arg| (*arg).to_string()).collect();
        args.extend(self.flags());
        args.push(base.to_string());
        let out = Command::new("cosign").args(&args).output().map_err(|err| {
            format!("cosign: {err}; a base verifier needs cosign installed on this machine")
        })?;
        if out.status.success() {
            return Ok(String::from_utf8_lossy(&out.stdout).to_string());
        }
        let said = String::from_utf8_lossy(&out.stderr);
        let line = said
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or_default()
            .trim();
        Err(format!("`{base}` did not verify: {line}"))
    }
}

fn pull(att_ref: &str, dir: &Path) -> Result<(), String> {
    let source = format!("docker://{att_ref}");
    let destination = format!("dir:{}", dir.display());
    let out = Command::new("skopeo")
        .args(["copy", "--retry-times", "3", &source, &destination])
        .output()
        .map_err(|err| format!("skopeo: {err}"))?;
    if out.status.success() {
        return Ok(());
    }
    Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
}

/// Skopeo reports a missing attestation tag as `manifest unknown` or `name unknown`.
fn absent(said: &str) -> bool {
    said.contains("manifest unknown") || said.contains("name unknown")
}

fn read(dir: &Path, base_digest: &str) -> Result<Option<Attestation>, Problem> {
    let manifest_text = std::fs::read_to_string(dir.join("manifest.json")).map_err(|err| {
        Problem::Unreadable(format!("the attestation manifest could not be read: {err}"))
    })?;
    let manifest = Json::parse(&manifest_text).map_err(|err| {
        Problem::Unreadable(format!("the attestation manifest is not JSON: {err}"))
    })?;
    let Some(layer) = items(&manifest, "layers").iter().rev().find(|layer| {
        text(layer, "mediaType").as_deref() == Some(DSSE)
            && field(layer, "annotations")
                .and_then(|annotations| text(annotations, "predicateType"))
                .as_deref()
                == Some(SPDX)
    }) else {
        return Ok(None);
    };
    let layer_digest = text(layer, "digest").unwrap_or_default();
    let Some(hex) = layer_digest.strip_prefix("sha256:") else {
        return Err(Problem::Unreadable(format!(
            "the attestation names no sha256 layer: {layer_digest}"
        )));
    };
    if hex.len() != 64 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(Problem::Unreadable(format!(
            "the attestation layer digest is not 64 hex digits: {layer_digest}"
        )));
    }
    let path = dir.join(hex);
    let actual = crate::runtime::sha256_file(&path).map_err(|err| {
        Problem::Unreadable(format!("the attestation layer could not be hashed: {err}"))
    })?;
    if actual != hex {
        return Err(Problem::Unreadable(format!(
            "the attestation layer hashes to {actual}, not {hex}"
        )));
    }
    let envelope_text = std::fs::read_to_string(&path).map_err(|err| {
        Problem::Unreadable(format!("the attestation layer could not be read: {err}"))
    })?;
    let envelope = Json::parse(&envelope_text).map_err(|err| {
        Problem::Unreadable(format!("the attestation envelope is not JSON: {err}"))
    })?;
    let encoded = text(&envelope, "payload").ok_or_else(|| {
        Problem::Unreadable("the attestation envelope carries no payload".to_string())
    })?;
    let payload = base64(&encoded)
        .ok_or_else(|| Problem::Unreadable("the attestation payload is not base64".to_string()))?;
    let payload = String::from_utf8(payload).map_err(|err| {
        Problem::Unreadable(format!("the attestation payload is not text: {err}"))
    })?;
    let payload_sha256 =
        crate::runtime::sha256_stdin(payload.as_bytes()).map_err(Problem::Unreadable)?;
    let statement = Json::parse(&payload).map_err(|err| {
        Problem::Unreadable(format!("the attestation payload is not JSON: {err}"))
    })?;
    let predicate = text(&statement, "predicateType").unwrap_or_default();
    if predicate != SPDX {
        return Err(Problem::Unreadable(format!(
            "the attestation names {SPDX} and its payload names {predicate}"
        )));
    }
    let subject = items(&statement, "subject").iter().find_map(|subject| {
        let said = field(subject, "digest").and_then(|digest| text(digest, "sha256"))?;
        (said == digest_hex(base_digest)).then(|| format!("sha256:{said}"))
    });
    let Some(subject) = subject else {
        return Err(Problem::Mismatch(format!(
            "the base's attestation subject is not {base_digest}"
        )));
    };
    let packages = field(&statement, "predicate")
        .map(|predicate| items(predicate, "packages").len())
        .unwrap_or(0);
    Ok(Some(Attestation {
        predicate: SPDX.to_string(),
        subject,
        sha256: layer_digest,
        payload: encoded,
        payload_sha256,
        packages,
    }))
}

fn digest_hex(digest: &str) -> &str {
    digest.strip_prefix("sha256:").unwrap_or(digest)
}

/// Cosign publishes a base's attestation at the tag `sha256-<hex>.att` in the same repository.
pub fn attestation_ref(base: &str) -> Option<(String, String)> {
    let (name, digest) = base.rsplit_once('@')?;
    let hex = digest.strip_prefix("sha256:")?;
    if hex.len() != 64 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let repository = match name.rsplit_once(':') {
        Some((repository, tag)) if !tag.contains('/') => repository,
        _ => name,
    };
    Some((format!("{repository}:sha256-{hex}.att"), digest.to_string()))
}

fn base64(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let mut buffer = 0u32;
    let mut bits = 0u32;
    for byte in text.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            b'\n' | b'\r' | b' ' | b'\t' => continue,
            _ => return None,
        };
        buffer = (buffer << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(u8::try_from((buffer >> bits) & 0xFF).ok()?);
            buffer &= (1 << bits) - 1;
        }
    }
    Some(out)
}

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Result<Self, String> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or_default();
        let dir = std::env::temp_dir().join(format!("buildc-base-{}-{now}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).map_err(|err| format!("{}: {err}", dir.display()))?;
        Ok(Self(dir))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if let Err(err) = std::fs::remove_dir_all(&self.0) {
            eprintln!("buildc: {}: {err}", self.0.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_attestation_tag_strips_the_reference_tag_and_keeps_the_digest() {
        let at = format!("quay.io/fedora/fedora-bootc:44@sha256:{}", "a".repeat(64));
        assert_eq!(
            attestation_ref(&at),
            Some((
                format!("quay.io/fedora/fedora-bootc:sha256-{}.att", "a".repeat(64)),
                format!("sha256:{}", "a".repeat(64)),
            ))
        );
        assert_eq!(
            attestation_ref(&format!("localhost:5000/repo@sha256:{}", "b".repeat(64)))
                .map(|(at, _)| at),
            Some(format!("localhost:5000/repo:sha256-{}.att", "b".repeat(64)))
        );
        assert_eq!(
            attestation_ref(&format!("fedora:44@sha256:{}", "c".repeat(64))).map(|(at, _)| at),
            Some(format!("fedora:sha256-{}.att", "c".repeat(64)))
        );
        assert_eq!(attestation_ref("quay.io/fedora/fedora-bootc:44"), None);
        assert_eq!(
            attestation_ref("quay.io/fedora/fedora-bootc@sha256:aa"),
            None
        );
    }

    #[test]
    fn a_base64_payload_decodes_with_and_without_padding() {
        assert_eq!(base64("aGVsbG8=").as_deref(), Some(&b"hello"[..]));
        assert_eq!(base64("aGVsbG8").as_deref(), Some(&b"hello"[..]));
        assert_eq!(base64("aGVs\nbG8=").as_deref(), Some(&b"hello"[..]));
        assert_eq!(base64("+/8=").as_deref(), Some(&[0xfb, 0xff][..]));
        assert_eq!(base64("a$b"), None);
    }

    #[test]
    fn an_absent_attestation_is_told_from_another_failure() {
        assert!(absent("reading manifest: manifest unknown"));
        assert!(absent("name unknown"));
        assert!(!absent("connection refused"));
    }

    const MATCHING: &str = "eyJwcmVkaWNhdGVUeXBlIjoiaHR0cHM6Ly9zcGR4LmRldi9Eb2N1bWVudCIsInN1YmplY3QiOlt7Im5hbWUiOiJleGFtcGxlIiwiZGlnZXN0Ijp7InNoYTI1NiI6ImFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWEifX1dLCJwcmVkaWNhdGUiOnsicGFja2FnZXMiOlt7Im5hbWUiOiJvbmUifSx7Im5hbWUiOiJ0d28ifV19fQ==";

    const MISMATCHED: &str = "eyJwcmVkaWNhdGVUeXBlIjoiaHR0cHM6Ly9zcGR4LmRldi9Eb2N1bWVudCIsInN1YmplY3QiOlt7Im5hbWUiOiJleGFtcGxlIiwiZGlnZXN0Ijp7InNoYTI1NiI6ImJiYmJiYmJiYmJiYmJiYmJiYmJiYmJiYmJiYmJiYmJiYmJiYmJiYmJiYmJiYmJiYmJiYmJiYmJiYmJiYmJiYmIifX1dLCJwcmVkaWNhdGUiOnsicGFja2FnZXMiOltdfX0=";

    fn layout(name: &str, payload: &str) -> (PathBuf, String) {
        let dir =
            std::env::temp_dir().join(format!("buildc-attestation-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("the test directory is writable");
        let envelope = format!(
            r#"{{"payloadType":"application/vnd.in-toto+json","payload":"{payload}","signatures":[]}}"#
        );
        let hex = crate::runtime::sha256_stdin(envelope.as_bytes()).expect("the envelope hashes");
        std::fs::write(dir.join(&hex), &envelope).expect("the layer is written");
        std::fs::write(
            dir.join("manifest.json"),
            format!(
                r#"{{"layers":[{{"mediaType":"{DSSE}","digest":"sha256:{hex}","annotations":{{"predicateType":"{SPDX}"}}}}]}}"#
            ),
        )
        .expect("the manifest is written");
        (dir, hex)
    }

    #[test]
    fn an_attestation_names_its_subject_and_counts_the_sbom_packages() {
        let (dir, hex) = layout("matching", MATCHING);
        let read = read(&dir, &format!("sha256:{}", "a".repeat(64)));
        let _ = std::fs::remove_dir_all(&dir);
        let attestation = read
            .expect("the attestation is read")
            .expect("one SPDX layer");
        assert_eq!(attestation.subject, format!("sha256:{}", "a".repeat(64)));
        assert_eq!(attestation.sha256, format!("sha256:{hex}"));
        assert_eq!(attestation.packages, 2);
        assert_eq!(attestation.predicate, SPDX);
    }

    #[test]
    fn an_attestation_that_names_another_digest_is_a_mismatch() {
        let (dir, _) = layout("mismatch", MISMATCHED);
        let read = read(&dir, &format!("sha256:{}", "a".repeat(64)));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(matches!(read, Err(Problem::Mismatch(_))));
    }

    #[test]
    fn a_spdx_layer_is_chosen_before_another_dsse_layer() {
        let dir = std::env::temp_dir().join(format!(
            "buildc-attestation-two-layers-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("the test directory is writable");
        // Both layers carry the same payload. The predicate annotation is what selects the SPDX layer.
        let first = format!(
            r#"{{"payloadType":"application/vnd.in-toto+json","payload":"{MATCHING}","signatures":[]}}"#
        );
        let second = format!(
            r#"{{"payloadType":"application/vnd.in-toto+json","payload":"{MATCHING}","signatures":[{{"keyid":"","sig":"x"}}]}}"#
        );
        let first_hex = crate::runtime::sha256_stdin(first.as_bytes()).expect("the layer hashes");
        let second_hex = crate::runtime::sha256_stdin(second.as_bytes()).expect("the layer hashes");
        std::fs::write(dir.join(&first_hex), first).expect("the first layer is written");
        std::fs::write(dir.join(&second_hex), second).expect("the second layer is written");
        std::fs::write(
            dir.join("manifest.json"),
            format!(
                r#"{{"layers":[
                    {{"mediaType":"{DSSE}","digest":"sha256:{first_hex}","annotations":{{"predicateType":"https://slsa.dev/provenance/v1"}}}},
                    {{"mediaType":"{DSSE}","digest":"sha256:{second_hex}","annotations":{{"predicateType":"{SPDX}"}}}}
                ]}}"#
            ),
        )
        .expect("the manifest is written");
        let read = read(&dir, &format!("sha256:{}", "a".repeat(64)));
        let _ = std::fs::remove_dir_all(&dir);
        let attestation = read
            .expect("the attestation is read")
            .expect("the SPDX layer is found");
        assert_eq!(attestation.sha256, format!("sha256:{second_hex}"));
    }

    #[test]
    fn a_layer_digest_that_is_not_hex_is_unreadable() {
        let dir =
            std::env::temp_dir().join(format!("buildc-attestation-invalid-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("the test directory is writable");
        std::fs::write(
            dir.join("manifest.json"),
            format!(
                r#"{{"layers":[{{"mediaType":"{DSSE}","digest":"sha256:/etc/hostname","annotations":{{"predicateType":"{SPDX}"}}}}]}}"#
            ),
        )
        .expect("the manifest is written");
        let read = read(&dir, &format!("sha256:{}", "a".repeat(64)));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(matches!(read, Err(Problem::Unreadable(_))));
    }

    #[test]
    fn a_local_key_hash_pins_the_key_file() {
        let dir = std::env::temp_dir().join(format!("buildc-key-hash-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("keys")).expect("the test directory is writable");
        std::fs::write(dir.join("keys/base.pub"), "public").expect("the key is written");
        let hash = crate::runtime::sha256_stdin(b"public").expect("the key hashes");
        let verifier = |sha256: &str| TrustRoot {
            key: Some("keys/base.pub".to_string()),
            key_sha256: Some(sha256.to_string()),
            identity: None,
            issuer: None,
        };
        assert!(Cosign::of(&dir, &verifier(&hash)).is_ok());
        assert!(Cosign::of(&dir, &verifier(&"0".repeat(64))).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_verified_payload_must_match_the_recorded_envelope() {
        let bytes = base64(MATCHING).expect("the fixture decodes");
        let hash = crate::runtime::sha256_stdin(&bytes).expect("the payload hashes");
        let recorded = Attestation {
            predicate: SPDX.to_string(),
            subject: format!("sha256:{}", "a".repeat(64)),
            sha256: "sha256:def".to_string(),
            payload: MATCHING.to_string(),
            payload_sha256: hash,
            packages: 2,
        };
        assert!(verified_payload(
            &format!(r#"{{"payload":"{MATCHING}"}}"#),
            &recorded
        ));
        assert!(!verified_payload(
            &format!(r#"{{"payload":"{MISMATCHED}"}}"#),
            &recorded
        ));
    }

    #[test]
    fn a_scratch_directory_is_removed_on_drop() {
        let path = std::env::temp_dir().join(format!("buildc-scratch-dir-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("the scratch path is writable");
        {
            let _scratch = Scratch(path.clone());
        }
        assert!(!path.exists(), "the scratch directory is removed");
    }

    #[test]
    fn a_scratch_that_cannot_be_removed_keeps_the_path() {
        let path = std::env::temp_dir().join(format!("buildc-scratch-file-{}", std::process::id()));
        std::fs::write(&path, "keep").expect("the scratch path is writable");
        {
            let _scratch = Scratch(path.clone());
        }
        assert!(
            path.is_file(),
            "a file is not a cleanable scratch directory"
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_key_verifier_ignores_the_transparency_log() {
        let cosign = Cosign::Key {
            path: PathBuf::from("keys/cosign.pub"),
        };
        assert_eq!(
            cosign.flags(),
            ["--key", "keys/cosign.pub", "--insecure-ignore-tlog=true"]
        );
    }

    #[test]
    fn an_identity_verifier_names_the_certificate_and_its_issuer() {
        let cosign = Cosign::Identity {
            identity: "https://example.test/workflow@refs/heads/main".to_string(),
            issuer: "https://token.actions.githubusercontent.com".to_string(),
        };
        assert_eq!(
            cosign.flags(),
            [
                "--certificate-identity",
                "https://example.test/workflow@refs/heads/main",
                "--certificate-oidc-issuer",
                "https://token.actions.githubusercontent.com"
            ]
        );
    }
}
