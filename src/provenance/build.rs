use crate::emit::json::Json;
use std::path::Path;
use std::process::Command;

pub const RECORD: &str = "/usr/share/buildc/build.json";

pub const MANIFEST: &str = "/usr/share/buildc/manifest.json";

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

fn output(program: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(program).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!text.is_empty()).then_some(text)
}

pub fn base(reference: &str) -> Option<String> {
    if reference.contains('@') {
        return Some(reference.to_string());
    }
    let digest = output(
        "skopeo",
        &[
            "inspect",
            "--format",
            "{{.Digest}}",
            &format!("docker://{reference}"),
        ],
    )?;
    digest
        .starts_with("sha256:")
        .then(|| format!("{reference}@{digest}"))
}

pub fn clone_commit(url: &str, version: &str) -> Option<String> {
    if version.len() == 40 && version.chars().all(|c| c.is_ascii_hexdigit()) {
        return Some(version.to_lowercase());
    }
    let listed = output("git", &["ls-remote", url, version])?;
    let sha = listed.split_whitespace().next()?;
    (sha.len() == 40).then(|| sha.to_lowercase())
}

pub fn source_commit(root: &Path) -> Option<String> {
    output(
        "git",
        &["-C", &root.display().to_string(), "rev-parse", "HEAD"],
    )
}

fn pairs(raw: &str) -> Vec<Vec<&str>> {
    raw.split_whitespace()
        .map(|field| field.split('|').collect())
        .collect()
}

pub fn build() -> Json {
    record(&env)
}

pub const SCHEMA_VERSION: u32 = 1;

fn record(env: &dyn Fn(&str) -> Option<String>) -> Json {
    let declared = env("BASE_DECLARED");
    let resolved = env("BASE");
    Json::object([
        ("schema_version", Json::Number(SCHEMA_VERSION)),
        ("target", Json::optional(env("TARGET"))),
        ("image", Json::optional(env("IMAGE_ID"))),
        ("version", Json::optional(env("IMAGE_VERSION"))),
        ("buildc", Json::string(env!("CARGO_PKG_VERSION"))),
        ("backend", Json::optional(env("BUILD_BACKEND"))),
        ("source_commit", Json::optional(env("SOURCE_COMMIT"))),
        (
            "base",
            Json::object([
                ("declared", Json::optional(declared)),
                ("resolved", Json::optional(resolved)),
            ]),
        ),
        (
            "modules",
            Json::array(
                pairs(&env("MODULE_HASHES").unwrap_or_default())
                    .iter()
                    .filter(|f| f.len() == 2)
                    .map(|f| {
                        Json::object([
                            ("path", Json::string(f[0])),
                            ("content", Json::string(f[1])),
                        ])
                    }),
            ),
        ),
        (
            "assets",
            Json::array(
                pairs(&env("ASSET_RESOLUTIONS").unwrap_or_default())
                    .iter()
                    .filter(|f| f.len() == 4)
                    .map(|f| {
                        Json::object([
                            ("module", Json::string(f[0])),
                            ("name", Json::string(f[1])),
                            ("selector", Json::string(f[2])),
                            ("resolved", Json::string(f[3])),
                        ])
                    }),
            ),
        ),
        (
            "audit",
            Json::object([(
                "enforce",
                Json::Bool(env("AUDIT_ENFORCE").as_deref() == Some("true")),
            )]),
        ),
        ("verified", Json::Null),
    ])
}

pub fn write() -> Result<(), String> {
    let path = Path::new(RECORD);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|err| format!("{}: {err}", dir.display()))?;
    }
    std::fs::write(path, format!("{}\n", build().render()))
        .map_err(|err| format!("{RECORD}: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_record_names_the_digest_and_says_whether_anything_was_enforced() {
        let given = |name: &str| {
            Some(str::to_string(match name {
                "BASE" => "quay.io/fedora/fedora-bootc:44@sha256:abc",
                "BASE_DECLARED" => "quay.io/fedora/fedora-bootc:44",
                "TARGET" => "example-desktop",
                "IMAGE_ID" => "example",
                "SOURCE_COMMIT" => "deadbeef",
                "BUILD_BACKEND" => "buildah",
                "MODULE_HASHES" => "core/hello|aaa apps/browser|bbb",
                "ASSET_RESOLUTIONS" => "mods/xone|xone|B7|3484f60",
                _ => return None,
            }))
        };
        let out = record(&given).render();

        assert!(out.contains("\"resolved\": \"quay.io/fedora/fedora-bootc:44@sha256:abc\""));
        assert!(out.contains("\"declared\": \"quay.io/fedora/fedora-bootc:44\""));
        assert!(out.contains("\"target\": \"example-desktop\""));
        assert!(out.contains("\"backend\": \"buildah\""));
        assert!(out.contains("\"path\": \"apps/browser\""));
        assert!(out.contains("\"selector\": \"B7\""));
        assert!(out.contains("\"enforce\": false"));
        assert!(out.contains("\"verified\": null"));
    }

    #[test]
    fn a_reference_that_already_carries_a_digest_resolves_to_itself() {
        let pinned = "quay.io/fedora/fedora-bootc:44@sha256:abc";
        assert_eq!(base(pinned).as_deref(), Some(pinned));
        assert_eq!(
            clone_commit("", "3484f603484782dd7551c64e5a33fc602b127051").as_deref(),
            Some("3484f603484782dd7551c64e5a33fc602b127051")
        );
    }
}
