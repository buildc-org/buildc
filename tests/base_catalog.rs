use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use buildc::base::BASES_FILE;
use buildc::diag::{Issues, Span};
use buildc::model::remote::{At, Collection};

static ENV: Mutex<()> = Mutex::new(());

fn flat(text: &str) -> String {
    text.split_whitespace()
        .filter(|word| !matches!(*word, "x" | "|"))
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn flattening_survives_a_wrap_mid_phrase() {
    let wrapped = "  x /home/runner/work/buildc/buildc/target/tmp/io/bases.kdl could not be\n  | read: Is a directory (os error 21)\n";

    let flat = flat(wrapped);
    assert!(flat.contains("could not be read"), "{flat}");
    assert!(
        flat.contains("/home/runner/work/buildc/buildc/target/tmp/io/bases.kdl"),
        "{flat}"
    );
}

struct Assets {
    _lock: MutexGuard<'static, ()>,
    old: Option<std::ffi::OsString>,
    path: PathBuf,
}

impl Assets {
    fn new(name: &str) -> Self {
        let lock = ENV
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        let old = std::env::var_os("BUILDC_ASSETS");
        std::env::set_var("BUILDC_ASSETS", &path);
        Self {
            _lock: lock,
            old,
            path,
        }
    }

    fn write(&self, text: &str) {
        std::fs::write(self.path.join(BASES_FILE), text).unwrap();
    }
}

impl Drop for Assets {
    fn drop(&mut self) {
        match self.old.take() {
            Some(value) => std::env::set_var("BUILDC_ASSETS", value),
            None => std::env::remove_var("BUILDC_ASSETS"),
        }
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[test]
fn public_catalog_lists_every_shipped_base_in_order() {
    let _assets = Assets::new("task-2-characterized-bases");
    let mut issues = Issues::default();

    let (bases, shadows) = buildc::base::catalog(Path::new("."), &[], &mut issues);

    assert!(issues.is_empty());
    assert!(shadows.is_empty());
    assert_eq!(
        bases
            .iter()
            .map(|base| base.image.as_str())
            .collect::<Vec<_>>(),
        [
            "quay.io/fedora/fedora-bootc:44",
            "ghcr.io/ublue-os/bazzite:stable",
            "ghcr.io/ublue-os/aurora:stable",
            "ghcr.io/ublue-os/bluefin:stable",
            "ghcr.io/ublue-os/kinoite-main:44",
            "docker.io/library/debian:forky",
            "docker.io/library/ubuntu:26.04",
        ]
    );
}

#[test]
fn runtime_file_replaces_embedded_catalog() {
    let assets = Assets::new("task-2-runtime-replaces");
    assets.write(
        r#"base "example.invalid/runtime:1" {
    about "runtime only"
    family "runtime"
}
"#,
    );

    let mut issues = Issues::default();
    let (bases, shadows) = buildc::base::catalog(Path::new("."), &[], &mut issues);

    assert!(issues.is_empty(), "{}", issues.plain());
    assert!(shadows.is_empty());
    assert_eq!(bases.len(), 1);
    assert_eq!(bases[0].image, "example.invalid/runtime:1");
}

#[test]
fn missing_runtime_file_falls_back_to_embedded_catalog() {
    let _assets = Assets::new("task-2-runtime-missing");

    let mut issues = Issues::default();
    let (bases, shadows) = buildc::base::catalog(Path::new("."), &[], &mut issues);

    assert!(issues.is_empty(), "{}", issues.plain());
    assert!(shadows.is_empty());
    assert_eq!(bases.len(), 7);
    for (at, image, family) in [
        (5, "docker.io/library/debian:forky", "debian"),
        (6, "docker.io/library/ubuntu:26.04", "ubuntu"),
    ] {
        assert_eq!(bases[at].image, image);
        assert_eq!(bases[at].family, family);
        assert!(bases[at].provides.is_empty(), "{image}");
        assert_eq!(bases[at].requires, ["bootc-base"], "{image}");
        assert!(!bases[at].signed, "{image}");
    }
}

#[test]
fn a_digest_pinned_reference_still_finds_its_catalog_row() {
    let _assets = Assets::new("task-2-digest-pinned");
    let mut issues = Issues::default();
    let (bases, _) = buildc::base::catalog(Path::new("."), &[], &mut issues);
    assert!(issues.is_empty(), "{}", issues.plain());

    let tagged = "docker.io/library/ubuntu:26.04";
    let pinned = format!("{tagged}@sha256:{}", "0".repeat(64));
    let found = buildc::base::find(&bases, &pinned).expect("the pinned base is the catalogued base");

    assert_eq!(found.image, tagged);
    assert_eq!(found.family, "ubuntu");
    assert_eq!(found.requires, ["bootc-base"]);

    assert!(buildc::base::find(
        &bases,
        &format!("example.invalid/nosuch:1@sha256:{}", "0".repeat(64))
    )
    .is_none());
}

#[test]
fn malformed_runtime_file_does_not_substitute_embedded_catalog() {
    let assets = Assets::new("task-2-runtime-malformed");
    assets.write("base {");

    let mut issues = Issues::default();
    let (bases, shadows) = buildc::base::catalog(Path::new("."), &[], &mut issues);

    assert!(bases.is_empty());
    assert!(shadows.is_empty());
    assert!(flat(&issues.plain()).contains(&assets.path.join(BASES_FILE).display().to_string()));
}

#[test]
fn present_unreadable_runtime_file_is_diagnosed_without_fallback() {
    let assets = Assets::new("io");
    let path = assets.path.join(BASES_FILE);
    std::fs::create_dir(&path).unwrap();

    let mut issues = Issues::default();
    let (bases, shadows) = buildc::base::catalog(Path::new("."), &[], &mut issues);

    assert!(bases.is_empty());
    assert!(shadows.is_empty());
    let diagnostic = flat(&issues.plain());
    assert!(
        diagnostic.contains(&path.display().to_string()),
        "{diagnostic}"
    );
    assert!(diagnostic.contains("could not be read"), "{diagnostic}");
}

#[test]
fn create_image_surfaces_an_unreadable_runtime_catalog() {
    let assets = Assets::new("create-io");
    let path = assets.path.join(BASES_FILE);
    std::fs::create_dir(&path).unwrap();
    let root = assets.path.join("repo");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("repo.kdl"), "schema-version 1\n").unwrap();

    let error = buildc::create::Image::collect(
        &root,
        Some("Example".to_string()),
        Some("example.invalid/base:1".to_string()),
        "Example",
        None,
        "a name argument",
        buildc::create::Field::Image,
        None,
        &buildc::prompt::Prompt::silent(),
    )
    .err()
    .expect("the unreadable runtime catalog must stop image creation");

    let error = flat(&error);
    assert!(error.contains(&path.display().to_string()), "{error}");
    assert!(error.contains("could not be read"), "{error}");
}

#[test]
fn collection_still_overrides_and_shadows_selected_catalog() {
    let assets = Assets::new("task-2-collection-shadow-assets");
    let root = assets.path.join("root");
    let collection_dir = root.join("collection");
    std::fs::create_dir_all(&collection_dir).unwrap();
    std::fs::write(
        collection_dir.join(BASES_FILE),
        r#"base "quay.io/fedora/fedora-bootc:44" {
    about "collection replacement"
    family "fedora"
    signed #true
}
"#,
    )
    .unwrap();
    let sources = [Collection {
        name: "one".to_string(),
        at: At::Dir("collection".to_string()),
        span: Span::default(),
    }];

    let mut issues = Issues::default();
    let (bases, shadows) = buildc::base::catalog(&root, &sources, &mut issues);

    assert!(issues.is_empty(), "{}", issues.plain());
    assert_eq!(bases.len(), 7);
    assert_eq!(bases[0].about, "collection replacement");
    assert!(bases[0].signed);
    assert_eq!(shadows.len(), 1);
    assert_eq!(shadows[0].image, "quay.io/fedora/fedora-bootc:44");
    assert_eq!(shadows[0].collection, "one");
}
