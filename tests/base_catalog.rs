#![expect(
    clippy::unwrap_used,
    reason = "a test helper panics on a failed step, which fails the calling test"
)]

use std::path::PathBuf;
use buildc::diag::{Issues, Span};
use buildc::model::remote::{At, Collection, Kind as SourceKind};

fn flat(text: &str) -> String {
    text.split_whitespace()
        .filter(|word| !matches!(*word, "x" | "|"))
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn flattening_survives_a_wrap_mid_phrase() {
    let wrapped = "  x /home/runner/work/buildc/buildc/target/tmp/io/fedora.base.kdl could not be\n  | read: Is a directory (os error 21)\n";

    let flat = flat(wrapped);
    assert!(flat.contains("could not be read"), "{flat}");
    assert!(
        flat.contains("/home/runner/work/buildc/buildc/target/tmp/io/fedora.base.kdl"),
        "{flat}"
    );
}

struct Library {
    root: PathBuf,
}

impl Library {
    fn new(name: &str) -> Self {
        let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("library")).unwrap();
        Self { root }
    }

    fn at(&self, name: &str) -> PathBuf {
        let path = self.root.join("library").join(name);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn write(&self, name: &str, file: &str, text: &str) {
        std::fs::write(self.at(name).join(file), text).unwrap();
    }

    fn source(&self, name: &str) -> Collection {
        Collection {
            kind: SourceKind::BaseImages,
            name: name.to_string(),
            at: At::Dir(format!("library/{name}")),
            span: Span::default(),
        }
    }
}

impl Drop for Library {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

const FEDORA: &str = r#"base {
    image "quay.io/fedora/fedora-bootc:44"
    about "Fedora 44"
    family "fedora"
    provides "rechunking" "bootc"
    signed #true
    bootloader "grub2"
}
"#;

#[test]
fn one_file_describes_one_base_named_by_its_stem() {
    let library = Library::new("one-base");
    library.write("core", "fedora-bootc-44.base.kdl", FEDORA);

    let mut issues = Issues::default();
    let bases = buildc::base::catalog(&library.root, &[library.source("core")], &mut issues);

    assert!(issues.is_empty(), "{}", issues.plain());
    assert_eq!(bases.len(), 1);
    assert_eq!(bases[0].name, "fedora-bootc-44");
    assert_eq!(bases[0].image, "quay.io/fedora/fedora-bootc:44");
    assert_eq!(bases[0].family, "fedora");
    assert!(bases[0].signed);
}

#[test]
fn a_base_is_found_by_its_image_reference_or_catalog_name() {
    let library = Library::new("find-base");
    library.write("core", "fedora-bootc-44.base.kdl", FEDORA);
    let mut issues = Issues::default();
    let bases = buildc::base::catalog(&library.root, &[library.source("core")], &mut issues);
    assert!(issues.is_empty(), "{}", issues.plain());
    assert_eq!(
        buildc::base::find(&bases, "fedora-bootc-44")
            .expect("the catalog name finds it")
            .image,
        "quay.io/fedora/fedora-bootc:44"
    );
}

#[test]
fn a_base_file_holds_exactly_one_base_node() {
    let library = Library::new("two-nodes");
    let second = FEDORA.replace("fedora-bootc:44", "fedora-bootc:43");
    library.write("core", "two.base.kdl", &format!("{FEDORA}\n{second}"));
    library.write("core", "none.base.kdl", "");
    let mut issues = Issues::default();
    let bases = buildc::base::catalog(&library.root, &[library.source("core")], &mut issues);
    assert!(bases.is_empty());
    let text = issues.plain();
    assert!(text.contains("`base` is declared twice"), "{text}");
    assert!(text.contains("holds no `base` node"), "{text}");
}

#[test]
fn a_base_library_is_not_an_unread_module_collection() {
    let library = Library::new("unread-kinds");
    library.write("core", "fedora-bootc-44.base.kdl", FEDORA);
    let disk = buildc::parse::disk::Disk::scan(&library.root);
    let index = buildc::provider::Index::scan(&library.root, &[library.source("core")], &disk, false);
    assert!(!index.sourced());
    assert!(index.unread().is_empty(), "{:?}", index.unread());
}

#[test]
fn a_source_that_is_not_on_this_machine_describes_nothing() {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("no-library");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let source = Collection {
        kind: SourceKind::BaseImages,
        name: "core".to_string(),
        at: At::Dir("missing".to_string()),
        span: Span::default(),
    };

    let mut issues = Issues::default();
    let bases = buildc::base::catalog(&root, &[source], &mut issues);

    assert!(bases.is_empty());
    assert!(issues.is_empty(), "{}", issues.plain());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn two_files_describing_one_image_are_refused_naming_both() {
    let library = Library::new("two-bases");
    library.write("core", "fedora.base.kdl", FEDORA);
    library.write(
        "ublue",
        "another.base.kdl",
        &FEDORA.replace("about \"Fedora 44\"", "about \"another row\""),
    );
    let sources = [library.source("core"), {
        let mut source = library.source("ublue");
        source.name = "ublue".to_string();
        source
    }];

    let mut issues = Issues::default();
    let bases = buildc::base::catalog(&library.root, &sources, &mut issues);

    assert_eq!(bases.len(), 1);
    let said = flat(&issues.plain());
    assert!(
        said.contains("`quay.io/fedora/fedora-bootc:44` is described by two base files"),
        "{said}"
    );
    assert!(said.contains("fedora.base.kdl"), "{said}");
}

#[test]
fn a_capability_family_block_overrides_the_default_path() {
    let library = Library::new("family-capability");
    let capabilities = library.at("core");
    std::fs::write(
        capabilities.join("capabilities.kdl"),
        r#"capability "ssh" {
    path "/usr/sbin/sshd"
    family "debian" "ubuntu" {
        path "/usr/bin/ssh"
    }
}
"#,
    )
    .unwrap();
    let source = Collection {
        kind: SourceKind::Capabilities,
        name: "core".to_string(),
        at: At::Dir("library/core".to_string()),
        span: Span::default(),
    };

    let mut issues = Issues::default();
    let rows = buildc::base::capabilities(&library.root, &[source], &mut issues);
    assert!(issues.is_empty(), "{}", issues.plain());
    assert_eq!(rows.len(), 1);

    assert_eq!(
        buildc::base::witness("ssh", "debian", None, &rows),
        Some(vec!["/usr/bin/ssh".to_string()])
    );
    assert_eq!(
        buildc::base::witness("ssh", "fedora", None, &rows),
        Some(vec!["/usr/sbin/sshd".to_string()])
    );
    assert_eq!(
        buildc::base::witness("ssh", "fedora", Some("/opt/ssh"), &rows),
        Some(vec!["/opt/ssh".to_string()])
    );
}

#[test]
fn an_abstract_row_suppresses_the_conventional_probe() {
    let library = Library::new("abstract-capability");
    let core = library.at("core");
    std::fs::write(core.join("capabilities.kdl"), "capability \"rechunking\"\n").unwrap();
    let source = Collection {
        kind: SourceKind::Capabilities,
        name: "core".to_string(),
        at: At::Dir("library/core".to_string()),
        span: Span::default(),
    };
    let mut issues = Issues::default();
    let rows = buildc::base::capabilities(&library.root, &[source], &mut issues);

    assert_eq!(
        buildc::base::witness("rechunking", "fedora", None, &rows),
        None
    );
    assert_eq!(buildc::base::probe("rechunking", "fedora", &rows), None);
    assert_eq!(
        buildc::base::probe("crun", "fedora", &rows),
        Some(vec![
            "/usr/bin/crun".to_string(),
            "/usr/sbin/crun".to_string()
        ])
    );
}

#[test]
fn a_digest_pinned_reference_still_finds_its_catalog_row() {
    let library = Library::new("digest-pinned");
    library.write("core", "fedora.base.kdl", FEDORA);
    let mut issues = Issues::default();
    let bases = buildc::base::catalog(&library.root, &[library.source("core")], &mut issues);
    assert!(issues.is_empty(), "{}", issues.plain());

    let pinned = format!("quay.io/fedora/fedora-bootc:44@sha256:{}", "0".repeat(64));
    let found = buildc::base::find(&bases, &pinned).expect("the pinned base is the catalogued base");

    assert_eq!(found.image, "quay.io/fedora/fedora-bootc:44");
    assert!(buildc::base::find(
        &bases,
        &format!("example.invalid/nosuch:1@sha256:{}", "0".repeat(64))
    )
    .is_none());
}

#[test]
fn an_unreadable_base_file_is_diagnosed_and_reads_no_row() {
    let library = Library::new("io");
    let path = library.at("core").join("fedora.base.kdl");
    std::fs::write(&path, [0xff, 0xfe, 0x00]).unwrap();

    let mut issues = Issues::default();
    let bases = buildc::base::catalog(&library.root, &[library.source("core")], &mut issues);

    assert!(bases.is_empty());
    let diagnostic = flat(&issues.plain());
    assert!(diagnostic.contains("could not be read"), "{diagnostic}");
    let squashed = diagnostic.replace(' ', "");
    assert!(
        squashed.contains(&path.display().to_string().replace(' ', "")),
        "{diagnostic}"
    );
}

#[test]
fn create_image_surfaces_an_unreadable_base_file() {
    let library = Library::new("create-io");
    let root = library.root.join("repo");
    let path = root.join("library/core/fedora.base.kdl");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, [0xff, 0xfe, 0x00]).unwrap();
    std::fs::write(
        root.join("repo.kdl"),
        "schema-version 1\n\nsources {\n    base-images \"core\" {\n        dir \"library/core\"\n    }\n}\n",
    )
    .unwrap();

    let error = buildc::create::Image::collect(
        buildc::create::Home {
            root: &root,
            repo: "Example",
            url: None,
            sources_text: "",
        },
        Some("Example".to_string()),
        Some("example.invalid/base:1".to_string()),
        "a name argument",
        buildc::create::Field::Image,
        None,
        &common::prompt::Prompt::silent(),
    )
    .err()
    .expect("the unreadable library must stop image creation");

    let error = flat(&error);
    assert!(error.contains("could not be read"), "{error}");
    let squashed = error.replace(' ', "");
    assert!(
        squashed.contains(&path.display().to_string().replace(' ', "")),
        "{error}"
    );
}

#[test]
fn one_name_leaves_the_family_it_coerces() {
    let library = Library::new("family");
    library.write(
        "core",
        "ubuntu.base.kdl",
        r#"base {
    image "docker.io/library/ubuntu:26.04"
    about "Ubuntu 26.04"
    family "ubuntu"
    requires "bootc-base"
    bootloader "grub2" "systemd"
}
"#,
    );

    let mut issues = Issues::default();
    let bases = buildc::base::catalog(&library.root, &[library.source("core")], &mut issues);
    assert!(issues.is_empty(), "{}", issues.plain());

    assert_eq!(bases.len(), 1);
    let base = &bases[0];
    assert_eq!(base.name, "ubuntu");
    assert_eq!(base.family, "ubuntu");
    assert_eq!(base.requires, ["bootc-base"]);
    assert_eq!(base.bootloaders, ["grub2", "systemd"]);
    assert!(!base.signed);
}

#[test]
fn a_capability_two_libraries_describe_differently_is_reported() {
    let library = Library::new("two-capabilities");
    let source = |name: &str, path: &str| {
        std::fs::write(
            library.at(name).join("capabilities.kdl"),
            format!("capability \"ssh\" {{\n    path \"{path}\"\n}}\n"),
        )
        .unwrap();
        Collection {
            kind: SourceKind::Capabilities,
            name: name.to_string(),
            at: At::Dir(format!("library/{name}")),
            span: Span::default(),
        }
    };
    let same = source("same", "/usr/sbin/sshd");
    let other = source("other", "/usr/bin/ssh");

    let mut issues = Issues::default();
    buildc::base::capabilities(
        &library.root,
        &[source("core", "/usr/sbin/sshd"), same],
        &mut issues,
    );
    assert!(issues.is_empty(), "{}", issues.plain());

    buildc::base::capabilities(
        &library.root,
        &[source("core", "/usr/sbin/sshd"), other],
        &mut issues,
    );
    let said = flat(&issues.plain());
    assert!(
        said.contains("capability `ssh` is described by two libraries"),
        "{said}"
    );
}
