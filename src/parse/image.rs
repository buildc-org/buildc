use crate::diag::{Issue, Issues, Source, Span};
use crate::model::image::Layout;
use crate::model::image::{is_name, Image};
use crate::parse::schema::{Arg, Kind, Node, Prop, Say, NEEDS_VALUE};
use crate::parse::{bool_arg, child, kids};
use crate::parse::{string_arg, text};
use kdl::{KdlDocument, KdlNode};

const FILESYSTEMS: &[&str] = &["xfs", "ext4", "btrfs"];

pub(crate) const BOOTLOADERS: &[&str] = &["grub2", "systemd"];

#[rustfmt::skip]
pub(crate) const OCI_LABELS: Node = Node::new("oci-labels", "The labels that the build stamps on the published image.")
    .about("The build also stamps `description` as the label `org.opencontainers.image.description`.")
    .arg(Arg::None, Say::new("`oci-labels` takes no argument", "the labels belong in the block",
        "`oci-labels { keywords \"desktop\" }`"))
    .once("")
    .empty(Say::new("`oci-labels` declares nothing", "empty block", "omit the block if the image takes no keywords and no logo"))
    .children(&[
        Node::new("keywords", "OCI label `io.artifacthub.package.keywords`. The keywords of the image, which the build joins with commas.").example("\"desktop\" \"kde\"")
            .arg(Arg::Strs, Say::NONE),
        Node::new("logo-url", "OCI label `io.artifacthub.package.logo-url`. The URL that points at the logo of the image.").example("\"https://github.com/owner/workstation/raw/main/logo.svg\"")
            .arg(Arg::Str, NEEDS_VALUE).once(""),
    ], Say::new("unknown `oci-labels` node `{}`", "not part of the schema",
        "`oci-labels` holds `keywords` and `logo-url`"));

#[rustfmt::skip]
const ALLOW_REMEDIATION: Node = Node::new("allow-remediation",
    "One rule an installed component refuses that this image lets remediation set anyway.")
    .about("The rule must be one that a component the image installs refuses, or the \
        allowance lifts nothing and `buildc scap` reports it.")
    .example("\"grub2_nousb_argument\" because=\"this machine has no USB keyboard\"")
    .arg(Arg::Str, Say::new("`allow-remediation` needs a rule ID", "nothing named",
        "`allow-remediation \"grub2_nousb_argument\" because=\"this machine has no USB \
         keyboard\"`"))
    .unique(Say::new("`{}` is allowed twice", "already allowed above", ""))
    .props(&[
        Prop { name: "because", kind: Kind::Str,
            desc: "Why this image overrides the component's judgement.",
            say: Say::new("`because` must be a string", "not a string", ""),
            missing: Say::NONE },
    ], Say::new("unknown `allow-remediation` property `{}`", "not part of the schema",
        "`allow-remediation` accepts `because`"));

#[rustfmt::skip]
pub(crate) const CONFORMS: Node = Node::new("conforms", "The benchmark profile that a scan measures the image against.")
    .about("The name selects a profile in the SSG datastream that the base pins, and `buildc \
        scap` measures the target against its rules. `buildc scap` refuses a name the \
        datastream does not carry.")
    .example("\"standard\"")
    .arg(Arg::Str, NEEDS_VALUE).once("")
    .children(&[ALLOW_REMEDIATION], Say::new("`conforms` holds no `{}`", "not allowed in `conforms`",
        "`conforms` holds `allow-remediation`, one rule this image lets remediation set"))
    .notes(&["A scan reports the result. A failed rule fails `buildc scap` only if \
        `audit { enforce #true }` is set."]);

#[rustfmt::skip]
pub(crate) const INSTALL: Node = Node::new("install", "What the installer lays down if the answer of the base family is not the one that the build wants.")
    .about("The block answers the install questions that the base family leaves open. If \
        neither the family nor a UKI seal settles an answer, then the build has no \
        install media unless the block gives it.")
    .scaffolds(&["bootloader"])
    .arg(Arg::None, Say::new("`install` takes no argument", "the declarations belong in the block",
        "`install { filesystem \"btrfs\" }`"))
    .once("a build is installed one way")
    .empty(Say::new("`install` declares nothing", "empty block",
        "omit the block to take the family's filesystem and no separate `/var`"))
    .children(&[
        Node::new("filesystem", "The root filesystem that the installer formats in place of the one that the base family settles.").example("\"btrfs\"")
            .arg(Arg::One(FILESYSTEMS), Say::new("`{}` is not a filesystem the installer formats",
                "not a filesystem",
                "the installer auto-partitions `xfs`, `ext4` and `btrfs`"))
            .once("")
            .props(&[], Say::new("unknown filesystem property `{}`", "not part of the schema",
                "`filesystem` takes only its filesystem name")),
        Node::new("composefs", "Whether the install seals the deployment, which replaces the answer of the base family.").example("#true")
            .arg(Arg::Bool, Say::new("`composefs` needs #true or #false", "not a boolean",
                "`composefs #true` passes `--composefs-backend`, which needs a \
                 filesystem with fs-verity"))
            .once(""),
        Node::new("generic-image", "Whether the install skips the bootupd check, which replaces the answer of the base family.").example("#false")
            .arg(Arg::Bool, Say::new("`generic-image` needs #true or #false", "not a boolean",
                "`generic-image #true` passes `--generic-image`, which a base packaging \
                 no bootupd needs or the install aborts"))
            .once(""),
        Node::new("admin-group", "The group that the installer creates an administrator in, which replaces the group of the base family.").example("\"wheel\"")
            .arg(Arg::Str, Say::new("`admin-group` needs a group name", "no group given",
                "`admin-group \"wheel\"`; `useradd` refuses the whole call when a listed \
                 group is missing on the target, so this names one"))
            .once(""),
        Node::new("bootloader", "The bootloader that the installer installs. The catalog entry of the base lists the bootloaders that the base carries.")
            .about("No family answers this row, so the block declares a bootloader unless a \
                UKI seal settles it as `systemd`.")
            .example("\"grub2\"")
            .arg(Arg::One(BOOTLOADERS), Say::new("`{}` is not a bootloader the installer installs",
                "not a bootloader",
                "the installer writes `grub2` or `systemd`; no family answers this. \
                 A build without it has no install media"))
            .once(""),
    ], Say::new("unknown node `{}` in install", "not part of the schema",
        "an install block accepts `filesystem`, `composefs`, `generic-image`, `admin-group` \
         and `bootloader`"));

pub(crate) const FIELDS: &[Node] = &[OCI_LABELS, CONFORMS];

pub fn is_field(name: &str) -> bool {
    FIELDS.iter().any(|node| node.name == name)
}

pub(crate) fn is_header(name: &str) -> bool {
    matches!(name, "schema-version" | "description") || is_field(name)
}

pub(crate) fn is_manifest_node(name: &str) -> bool {
    crate::parse::component::COMPONENT
        .children
        .iter()
        .any(|node| !node.name.is_empty() && node.name == name)
}

pub(crate) const ROOT: &str = "/";

pub fn root_chain(path: &str) -> Vec<&str> {
    std::iter::once(ROOT)
        .chain(path.split('/').filter(|part| !part.is_empty()))
        .collect()
}

/// A file inside a library writes a rooted path under `component "/"`.
/// A file outside every library writes it at the top level because its bare children already read from the library root.
pub fn chain(outside: bool, path: &str) -> Vec<&str> {
    match outside {
        true => path.split('/').filter(|part| !part.is_empty()).collect(),
        false => root_chain(path),
    }
}

fn trail<'a>(nodes: &'a [KdlNode], chain: &[&str]) -> Option<Vec<&'a KdlNode>> {
    let Some((name, rest)) = chain.split_first() else {
        return Some(Vec::new());
    };
    let matching: Vec<&KdlNode> = nodes
        .iter()
        .filter(|kid| match *name {
            ROOT => crate::parse::tree::is_context(kid) && string_arg(kid) == Some(ROOT),
            _ => kid.name().value() == *name,
        })
        .collect();
    let blocks = matching.iter().filter(|kid| kid.children().is_some());
    let bare = matching.iter().filter(|kid| kid.children().is_none());
    blocks.chain(bare).find_map(|kid| {
        let mut found = trail(kids(kid), rest)?;
        found.insert(0, kid);
        Some(found)
    })
}

pub fn unlisted(kdl: &str, chain: &[&str], held: impl Fn(&str) -> bool) -> Result<String, String> {
    let doc: KdlDocument = kdl.parse().map_err(|_| "is not valid KDL".to_string())?;
    let named = chain
        .iter()
        .filter(|link| **link != ROOT)
        .copied()
        .collect::<Vec<_>>()
        .join("/");
    let nodes = trail(doc.nodes(), chain).ok_or_else(|| format!("does not list `{named}`"))?;
    let target = nodes
        .last()
        .ok_or_else(|| format!("does not list `{named}`"))?;
    let below: Vec<&str> = kids(target)
        .iter()
        .filter(|kid| crate::parse::tree::is_segment(kid))
        .map(|kid| kid.name().value())
        .collect();
    if !below.is_empty() {
        return Err(format!(
            "lists `{named}` with {} below it, so unlisting it would unlist them too",
            below.join(", ")
        ));
    }
    let ancestors: Vec<(usize, String)> = nodes[..nodes.len() - 1]
        .iter()
        .zip(chain)
        .scan(Vec::new(), |parts: &mut Vec<&str>, (node, name)| {
            if *name != ROOT {
                parts.push(name);
            }
            Some((Span::from(node.span()).offset, parts.join("/")))
        })
        .collect();
    let mut out = kdl.to_string();
    cut(&mut out, target.span().into());
    for (offset, path) in ancestors.iter().rev() {
        let Some(doc) = out.parse::<KdlDocument>().ok() else {
            break;
        };
        let Some(node) = starting_at(doc.nodes(), *offset) else {
            break;
        };
        if node
            .children()
            .is_none_or(|block| !block.nodes().is_empty())
        {
            break;
        }
        let span: Span = node.span().into();
        if held(path) {
            let end = span.offset + span.len;
            let open = out[span.offset..end]
                .rfind('{')
                .map_or(end, |at| span.offset + at);
            let from = out[..open].trim_end().len();
            out.replace_range(from..end, "");
            break;
        }
        cut(&mut out, span);
    }
    Ok(out)
}

fn starting_at(nodes: &[KdlNode], offset: usize) -> Option<&KdlNode> {
    nodes
        .iter()
        .find_map(|node| match Span::from(node.span()).offset == offset {
            true => Some(node),
            false => starting_at(kids(node), offset),
        })
}

fn cut(text: &mut String, span: Span) {
    let from = span.offset;
    let after = &text[span.offset + span.len..];
    let to = match after.trim_start_matches([' ', '\t']).strip_prefix(';') {
        Some(rest) => text.len() - rest.trim_start_matches([' ', '\t']).len(),
        None => span.offset + span.len,
    };
    let start = text[..from].rfind('\n').map_or(0, |at| at + 1);
    let end = text[to..].find('\n').map_or(text.len(), |at| to + at + 1);
    let alone = text[start..from].trim().is_empty() && text[to..end].trim().is_empty();
    let last = text[to..end].trim().is_empty();
    match (alone, last) {
        (true, _) => text.replace_range(start..end, ""),
        (false, true) => {
            let from = text[..from].trim_end_matches([' ', '\t']).len();
            text.replace_range(from..to, "");
        }
        (false, false) => text.replace_range(from..to, ""),
    }
}

pub fn block_close(kdl: &str, chain: &[&str]) -> Option<usize> {
    let doc: KdlDocument = kdl.parse().ok()?;
    match trail(doc.nodes(), chain)?.last() {
        Some(node) => super::block_close(node, kdl),
        None => Some(kdl.len()),
    }
}

pub fn listed(kdl: &str, path: &str) -> usize {
    crate::model::component::Component::parse(
        "",
        "",
        std::path::Path::new("."),
        kdl.to_string(),
        None,
        &mut Issues::default(),
    )
    .map_or(0, |component| {
        component
            .imports
            .iter()
            .filter(|import| import.path == path && !import.organises)
            .count()
    })
}

pub(crate) fn nodes_at(src: &Source, spans: &[Span]) -> Vec<KdlNode> {
    fn walk(nodes: &[KdlNode], spans: &[Span], out: &mut Vec<KdlNode>) {
        for node in nodes {
            if spans.contains(&Span::from(node.span())) {
                out.push(node.clone());
            }
            walk(kids(node), spans, out);
        }
    }
    let mut out = Vec::new();
    if let Ok(doc) = src.text().parse::<KdlDocument>() {
        walk(doc.nodes(), spans, &mut out);
    }
    out
}

pub fn segments(path: &str) -> Vec<&str> {
    path.split('/').collect()
}

pub(crate) fn install(node: &KdlNode, src: &Source) -> Layout {
    Layout {
        filesystem: text(node, "filesystem"),
        bootloader: text(node, "bootloader"),
        composefs: child(node, "composefs").and_then(bool_arg),
        generic: child(node, "generic-image").and_then(bool_arg),
        admin_group: text(node, "admin-group"),
        span: node.name().span().into(),
        src: src.clone(),
    }
}

impl Image {
    pub(crate) fn new(
        id: &str,
        component: &str,
        outside: bool,
        src: &Source,
        issues: &mut Issues,
    ) -> Self {
        let mut image = Image {
            src: src.clone(),
            file: src.name().to_string(),
            id: id.to_string(),
            component: component.to_string(),
            repo_base_url: String::new(),
            description: String::new(),
            keywords: Vec::new(),
            logo_url: String::new(),
            conforms: String::new(),
            base: None,
            base_entry: None,
            layout: None,
            allows: Vec::new(),
            entries: Vec::new(),
            suppressed: Vec::new(),
            outside,
            span: Span::default(),
            fields: Vec::new(),
        };
        image.check_id(issues);
        image
    }

    pub(crate) fn check_layout(&self, issues: &mut Issues) {
        let (Some(layout), Some(base)) = (&self.layout, &self.base) else {
            return;
        };
        let src = &layout.src;
        let uki = self.seals_uki();
        let missing = crate::emit::recipe::unanswered(&base.family, Some(layout), uki);
        if missing == ["bootloader"] {
            issues.push(
                Issue::new(format!("`{}` names no bootloader", self.id), src)
                    .at(layout.span, "no bootloader")
                    .help(
                        "no family answers the bootloader: `bootloader \"grub2\"` here, from those \
                         the base component lists",
                    ),
            );
        } else if !missing.is_empty() {
            issues.push(
                Issue::new(format!("`{}` declares half an install block", self.id), src)
                    .at(layout.span, format!("no {}", missing.join(", ")))
                    .help(format!(
                        "nothing here has measured how `{}` installs, so this build answers for \
                     itself: an `install` block naming all of `filesystem`, `composefs`, \
                     `generic-image`, `admin-group` and `bootloader` builds install media, and no \
                     `install` block builds none",
                        base.family
                    )),
            );
        }
        let mut conflicts = Vec::new();
        if uki {
            // The recipe installs `UKI_FILESYSTEM` under a UKI chain.
            // A declared ext4 would pass the check and then install as btrfs.
            if !layout.filesystem.is_empty()
                && layout.filesystem != crate::emit::recipe::UKI_FILESYSTEM
            {
                conflicts.push("filesystem");
            }
            if layout.composefs == Some(false) {
                conflicts.push("composefs");
            }
            if layout.generic == Some(false) {
                conflicts.push("generic-image");
            }
            if !layout.bootloader.is_empty() && layout.bootloader != "systemd" {
                conflicts.push("bootloader");
            }
        }
        if !conflicts.is_empty() {
            issues.push(
                Issue::new(
                    format!("`{}` declares an install block that conflicts with its UKI chain", self.id),
                    src,
                )
                .at(layout.span, format!("conflicting {}", conflicts.join(", ")))
                .help(format!("a UKI chain settles {}, composefs, generic-image and systemd-boot; drop those install rows or make them agree", crate::emit::recipe::UKI_FILESYSTEM)),
            );
        }
        if layout.filesystem == "xfs"
            && !uki
            && crate::emit::recipe::seals(&base.family, Some(layout), uki)
        {
            issues.push(
                Issue::new(
                    format!("`{}` installs a sealed deployment onto xfs", self.id),
                    src,
                )
                .at(layout.span, "xfs has no fs-verity")
                .help(format!(
                    "the `{}` family installs with the composefs backend, which needs fs-verity: \
                     declare `ext4` or `btrfs`, or drop `filesystem` and take the family's own",
                    base.family
                )),
            );
        }
    }

    pub(crate) fn check_bootc(&self, issues: &mut Issues) {
        if self.is_bootc() {
            return;
        }
        if let Some(layout) = &self.layout {
            issues.push(
                Issue::new(
                    format!(
                        "`{}` declares an install block, and nothing in the image provides `bootc`",
                        self.id
                    ),
                    &layout.src,
                )
                .at(layout.span, "a bootc-only node")
                .help(
                    "an install block describes how bootc installs the image: use a base that \
                     provides `bootc`, or drop `install`",
                ),
            );
        }
    }

    fn check_id(&mut self, issues: &mut Issues) {
        let src = &self.src;
        if !self.id.is_empty() && !is_name(&self.id) {
            issues.push(
                Issue::new(format!("invalid image id `{}`", self.id), src)
                    .at(
                        self.span,
                        "must be lowercase letters, digits and dashes, starting with a letter",
                    )
                    .help("it becomes an image tag and a cache tag, both of which restrict it"),
            );
        }
        if self.id == crate::layout::LIB {
            issues.push(
                Issue::new(format!("`{}` is not available as an image id", self.id), src)
                    .at(self.span, "generated/lib/ is the shared helper library")
                    .help("use another id. Everything else this image generates would land in the shared helper directory"),
            );
            self.id = String::new();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::image::List;
    use crate::parse::schema::check_doc;

    fn messages(text: &str) -> Vec<String> {
        let doc: KdlDocument = text.parse().expect("valid KDL");
        let src = Source::new("x.kdl", text);
        let mut issues = Issues::default();
        check_doc(&doc, &crate::parse::component::COMPONENT, &src, &mut issues);
        issues
            .findings()
            .into_iter()
            .filter(|found| !found.contains("schema-version"))
            .collect()
    }

    fn repository(name: &str, text: &str, dirs: &[(&str, &str)]) -> std::path::PathBuf {
        // Several tests share one fixture name and `cargo test` runs them as threads of one process.
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "buildc-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("the temporary directory is writable");
        std::fs::write(
            root.join("repo.kdl"),
            "schema-version 1\nbuilds { x \"image\" }\n",
        )
        .expect("the temporary directory is writable");
        std::fs::create_dir_all(root.join("components"))
            .expect("the temporary directory is writable");
        std::fs::write(
            root.join("components/image.kdl"),
            format!("schema-version 1\n{text}"),
        )
        .expect("the temporary directory is writable");
        for (dir, manifest) in dirs {
            let dir = root.join("components").join(dir);
            std::fs::create_dir_all(&dir).expect("the temporary directory is writable");
            std::fs::write(
                dir.join("component.kdl"),
                format!("schema-version 1\n{manifest}"),
            )
            .expect("the temporary directory is writable");
        }
        root
    }

    fn installs(root: &std::path::Path, install: &str) {
        std::fs::write(
            root.join("repo.kdl"),
            format!(
                "schema-version 1\nbuilds {{ x \"image\" {{ install {{ {install}; }}; }}; }}\n"
            ),
        )
        .expect("the temporary directory is writable");
    }

    fn semantic_messages(text: &str) -> Vec<String> {
        let root = repository(
            "semantic",
            text,
            &[("x", "base \"x\" { family \"fedora\"; }\n")],
        );
        let issues = crate::load(&root).issues;
        let _ = std::fs::remove_dir_all(&root);
        issues.findings()
    }

    #[test]
    fn the_table_catches_what_the_corpus_does_not() {
        let found = messages("conforms \"a\"\nconforms \"b\"\nx\n");
        assert_eq!(found, ["`conforms` is declared twice"]);
    }

    #[test]
    fn a_segment_takes_no_argument_and_a_name_the_path_rule_allows() {
        let found = semantic_messages(
            "component \"/\" {\n x\n palette \"red\"\n Bad_Name\n tools flavor=\"x\"\n}\n",
        );
        for wanted in [
            "`palette` takes no argument",
            "`Bad_Name` is not a component path",
            "`flavor` is not a property of `tools`",
        ] {
            assert!(
                found.iter().any(|message| message.contains(wanted)),
                "{wanted}: {found:?}"
            );
        }
    }

    #[test]
    fn the_image_block_ends_at_a_closing_brace() {
        let text = "tools {\n    one\n}\n";
        let at = block_close(text, &["tools"]).expect("a tools block");
        assert_eq!(&text[at..=at], "}");
        assert!(block_close(text, &["tools", "one"]).is_none());
        assert_eq!(block_close(text, &[]), Some(text.len()));
    }

    #[test]
    fn the_root_chain_reaches_only_the_root_block() {
        let text = "component \"/fedora\" {\n    a\n}\nnote \"/\" {\n    c\n}\ncomponent \"/\" {\n    b\n}\n";
        let at = block_close(text, &root_chain("")).expect("a root block");
        assert_eq!(at, text.len() - 2);
        assert!(block_close("component \"/fedora\" {\n    a\n}\n", &root_chain("")).is_none());
    }

    fn uki_conflicts(case: &str, install: &str) -> bool {
        let root = repository(
            case,
            "component \"/\" { x; uki; }\n",
            &[
                (
                    "x",
                    "base \"x\" { family \"fedora\"; }\nprovides \"bootc\"\n",
                ),
                ("uki", "fragment position=\"tail\"\n"),
            ],
        );
        std::fs::write(
            root.join("components/uki/Containerfile.inc"),
            "FROM ${SPLIT_BASE} AS split\n",
        )
        .expect("the temporary directory is writable");
        installs(&root, install);
        let found = crate::load(&root).issues.findings();
        let _ = std::fs::remove_dir_all(&root);
        found
            .iter()
            .any(|message| message.contains("conflicts with its UKI chain"))
    }

    #[test]
    fn a_uki_chain_refuses_a_conflicting_install_block() {
        assert!(uki_conflicts(
            "uki-install",
            "filesystem \"xfs\"; composefs #false; generic-image #false; bootloader \"grub2\"",
        ));
    }

    #[test]
    fn a_uki_chain_takes_the_install_rows_the_recipe_settles() {
        assert!(!uki_conflicts(
            "uki-agrees",
            "filesystem \"btrfs\"; composefs #true; generic-image #true; bootloader \"systemd\"",
        ));
        assert!(uki_conflicts("uki-ext4", "filesystem \"ext4\""));
    }

    #[test]
    fn an_inline_base_at_an_organising_directory_builds_on_its_reference() {
        let root = repository(
            "inline-base",
            "component \"/\" {\n base-image digest=\"sha256:abc\" {\n  base \"quay.io/fedora/fedora-bootc:44\" { family \"fedora\"; }\n }\n}\n",
            &[("base-image/fedora-bootc", "base \"quay.io/fedora/fedora-bootc:44\"\n")],
        );
        let (list, issues) = List::load(&root);
        let _ = std::fs::remove_dir_all(&root);
        assert!(issues.findings().is_empty(), "{:?}", issues.findings());
        let base = list.images[0]
            .base
            .as_ref()
            .expect("the definition is a base");
        assert_eq!(base.path, "base-image");
        assert_eq!(base.image, "quay.io/fedora/fedora-bootc:44@sha256:abc");
        assert_eq!(base.family, "fedora");
        assert!(base.answers(crate::parse::component::BASE_IMAGE));
    }

    #[test]
    fn an_inline_definition_that_meets_a_component_is_refused_naming_both() {
        let root = repository(
            "inline-clash",
            "component \"/\" {\n x\n tools {\n  packages \"vim\"\n }\n}\n",
            &[
                ("x", "base \"x\" { family \"fedora\"; }\n"),
                ("tools", "packages \"vim\"\n"),
            ],
        );
        let found = crate::load(&root).issues.findings();
        let _ = std::fs::remove_dir_all(&root);
        assert!(
            found.iter().any(|message| message
                .contains("`tools` is defined inline and in a manifest file")),
            "{found:?}"
        );
    }

    #[test]
    fn a_second_base_is_refused() {
        let root = repository(
            "two-bases",
            "component \"/\" { x; y; }\n",
            &[("x", "base \"x\"\n"), ("y", "base \"y\"\n")],
        );
        let (_, issues) = List::load(&root);
        let _ = std::fs::remove_dir_all(&root);
        assert!(
            issues
                .findings()
                .iter()
                .any(|message| message.contains("`x` lists two bases")),
            "{:?}",
            issues.findings()
        );
    }

    #[test]
    fn the_labels_read_from_the_oci_labels_block() {
        let root = repository(
            "labels",
            "oci-labels {\n    keywords \"desktop\" \"kde\"\n    logo-url \"https://example.org/l.svg\"\n}\ncomponent \"/\" { x; }\n",
            &[("x", "base \"x\" { family \"fedora\"; }\n")],
        );
        let (list, issues) = List::load(&root);
        let _ = std::fs::remove_dir_all(&root);
        assert!(issues.findings().is_empty(), "{:?}", issues.findings());
        assert_eq!(list.images[0].keywords, ["desktop", "kde"]);
        assert_eq!(list.images[0].logo_url, "https://example.org/l.svg");
    }

    #[test]
    fn a_top_level_component_named_after_a_manifest_node_is_refused() {
        let root = repository(
            "keyword-dir",
            "component \"/\" { x; }\n",
            &[("x", "base \"x\"\n"), ("conforms/uki", "packages \"a\"\n")],
        );
        let (_, issues) = List::load(&root);
        let _ = std::fs::remove_dir_all(&root);
        assert!(
            issues
                .findings()
                .iter()
                .any(|message| message
                    .contains("components/conforms is named after a manifest node")),
            "{:?}",
            issues.findings()
        );
    }

    #[test]
    fn a_top_level_file_component_named_after_a_manifest_node_is_refused() {
        let root = repository(
            "keyword-file",
            "component \"/\" { x; }\n",
            &[("x", "base \"x\"\n")],
        );
        std::fs::write(
            root.join("components/permissions.kdl"),
            "schema-version 1\n",
        )
        .unwrap();
        let (_, issues) = List::load(&root);
        let _ = std::fs::remove_dir_all(&root);
        assert!(
            issues.findings().iter().any(|message| message
                .contains("components/permissions.kdl is named after a manifest node")),
            "{:?}",
            issues.findings()
        );
    }

    #[test]
    fn an_organising_segment_lists_nothing_and_may_repeat() {
        let root = repository(
            "organise",
            "component \"/\" {\n x\n hardware { nvidia; }\n hardware { audio; }\n}\n",
            &[
                ("x", "base \"x\"\n"),
                ("hardware/nvidia", "packages \"a\"\n"),
                ("hardware/audio", "packages \"b\"\n"),
            ],
        );
        let (list, issues) = List::load(&root);
        let _ = std::fs::remove_dir_all(&root);
        assert!(issues.findings().is_empty(), "{:?}", issues.findings());
        let paths: Vec<&str> = list.images[0]
            .entries
            .iter()
            .map(|entry| entry.path.as_str())
            .collect();
        assert_eq!(paths, ["image", "hardware/nvidia", "hardware/audio"]);
    }

    #[test]
    fn a_child_with_an_argument_sets_an_option_and_one_without_is_a_segment() {
        let root = repository(
            "options",
            "component \"/\" {\n x\n hardware {\n  nvidia {\n   version \"550\"\n   cuda\n  }\n }\n}\n",
            &[("x", "base \"x\"\n")],
        );
        let (list, _) = List::load(&root);
        let _ = std::fs::remove_dir_all(&root);
        let entries = &list.images[0].entries;
        let nvidia = entries
            .iter()
            .find(|entry| entry.path == "hardware/nvidia")
            .expect("nvidia sets an option, so it lists its component");
        assert!(nvidia.settings().any(|setting| setting.name == "version"));
        assert!(entries
            .iter()
            .any(|entry| entry.path == "hardware/nvidia/cuda"));
    }

    fn resolved(case: &str, image: &str, dirs: &[(&str, &str)]) -> Vec<String> {
        let mut all = vec![("x", "base \"x\" { family \"fedora\"; }\n")];
        all.extend_from_slice(dirs);
        let root = repository(case, image, &all);
        let found = crate::load(&root).issues.findings();
        let _ = std::fs::remove_dir_all(&root);
        found
    }

    fn holds(found: &[String], wanted: &str) {
        assert!(
            found.iter().any(|message| message.contains(wanted)),
            "{wanted}: {found:?}"
        );
    }

    #[test]
    fn a_bootc_only_node_on_a_non_bootc_image_is_refused() {
        let root = repository(
            "bootc-only",
            "component \"/\" { x; }\n",
            &[("x", "base \"x\" { family \"fedora\"; }\n")],
        );
        installs(&root, "bootloader \"grub2\"");
        let found = crate::load(&root).issues.findings();
        let _ = std::fs::remove_dir_all(&root);
        holds(
            &found,
            "`x` declares an install block, and nothing in the image provides `bootc`",
        );
    }

    #[test]
    fn an_inline_definition_is_held_to_the_manifest_grammar() {
        let found = resolved(
            "inline-grammar",
            "component \"/\" {\n x\n tools {\n  packages \"vim\"\n  palette \"red\"\n }\n}\n",
            &[],
        );
        holds(&found, "`palette` takes no argument");
    }

    #[test]
    fn a_block_under_base_is_refused() {
        let found = resolved(
            "base-block",
            "component \"/\" {\n base-image {\n  base \"quay.io/x:1\" { digest \"sha256:ab\" }\n }\n}\n",
            &[],
        );
        holds(&found, "`base` holds no `digest`");
    }

    #[test]
    fn a_hand_written_base_image_capability_is_refused() {
        let found = resolved(
            "base-image-by-hand",
            "component \"/\" {\n x\n tools {\n  provides \"base-image\"\n }\n}\n",
            &[],
        );
        holds(&found, "`base-image` is what a `base` node provides");
    }

    #[test]
    fn a_malformed_signed_and_a_second_digest_are_refused() {
        let found = semantic_messages(
            "component \"/\" { x signed=\"true\" digest=\"sha256:ab\" digest=\"sha256:cd\"; }\n",
        );
        holds(&found, "`signed` on `x` has the wrong value");
        holds(&found, "`digest` is declared twice");
    }

    #[test]
    fn a_base_segment_records_one_verifier_and_defaults_its_issuer() {
        let root = repository(
            "verify",
            "component \"/\" { x signed=#true identity=\"https://github.com/owner/repo/.github/workflows/image-build.yml@refs/heads/main\" }\n",
            &[("x", "base \"x\" { family \"fedora\"; }\n")],
        );
        let loaded = crate::load(&root);
        let found = loaded.issues.findings();
        let verify = loaded
            .list
            .images
            .first()
            .and_then(|image| image.base.as_ref())
            .and_then(|base| base.verify.as_ref());
        let _ = std::fs::remove_dir_all(&root);
        assert!(found.is_empty(), "{found:?}");
        let verify = verify.expect("the base records its verifier");
        assert_eq!(
            verify.identity.as_deref(),
            Some("https://github.com/owner/repo/.github/workflows/image-build.yml@refs/heads/main")
        );
        assert_eq!(
            verify.issuer.as_deref(),
            Some(crate::model::component::GITHUB_OIDC_ISSUER)
        );
        assert_eq!(verify.key, None);
    }

    #[test]
    fn a_base_that_names_two_trust_roots_is_refused() {
        let found = semantic_messages(
            "component \"/\" { x signed=#true key=\"keys/cosign.pub\" identity=\"me@example.org\" }\n",
        );
        holds(&found, "the base names a `key=` and an `identity=`");
    }

    #[test]
    fn a_key_hash_without_a_key_is_refused() {
        let found = semantic_messages(
            "component \"/\" { x key-sha256=\"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"; }\n",
        );
        holds(&found, "`key-sha256=` pins a key with no `key=`");
    }

    #[test]
    fn a_verifier_without_a_signed_base_is_refused() {
        let found = semantic_messages("component \"/\" { x key=\"keys/cosign.pub\" }\n");
        holds(
            &found,
            "a verifier names a base that is not declared signed",
        );
    }

    #[test]
    fn an_issuer_without_an_identity_is_refused() {
        let found = semantic_messages(
            "component \"/\" { x signed=#true issuer=\"https://issuer.example\" }\n",
        );
        holds(&found, "`issuer=` names an issuer with no `identity=`");
    }

    #[test]
    fn a_verifier_on_a_component_below_the_base_is_refused() {
        let found = semantic_messages(
            "component \"/\" { x\n tools key=\"keys/cosign.pub\" signed=#true }\n",
        );
        holds(&found, "`tools` is not the base, so it takes no `key=`");
    }

    #[test]
    fn a_key_path_that_leaves_the_repository_is_refused() {
        let found =
            semantic_messages("component \"/\" { x signed=#true key=\"/etc/cosign.pub\" }\n");
        holds(&found, "`key` on `x` has the wrong value");
    }

    #[test]
    fn a_seed_refuses_an_inline_definition() {
        let root = repository(
            "seed-inline",
            "component \"/\" {\n x\n tools {\n  packages \"vim\"\n }\n}\n",
            &[("x", "base \"x\" { family \"fedora\"; }\n")],
        );
        std::fs::write(
            root.join("repo.kdl"),
            "schema-version 1\nbuilds { x \"image\" }\nlibraries {\n local {\n  owner { source \".\" }\n }\n}\nseed \"x\" collection=\"owner\"\n",
        )
        .expect("the temporary directory is writable");
        let (_, issues) = List::load(&root);
        let _ = std::fs::remove_dir_all(&root);
        holds(
            &issues.findings(),
            "`tools` is defined inline, so the seed cannot name it",
        );
    }

    #[test]
    fn a_segment_argument_names_where_its_value_belongs() {
        let root = repository(
            "segment-help",
            "component \"/\" {\n x\n id \"y\"\n name \"z\"\n family \"f\"\n}\n",
            &[("x", "base \"x\" { family \"fedora\"; }\n")],
        );
        let helps = crate::load(&root).issues.helps();
        let _ = std::fs::remove_dir_all(&root);
        for wanted in [
            "the build name in `repo.kdl` is the image's published name",
            "the os-release fields are options of the `os-release` component",
            "`family` belongs in the `base` block",
        ] {
            assert!(
                helps.iter().any(|help| help.contains(wanted)),
                "{wanted}: {helps:?}"
            );
        }
    }

    #[test]
    fn a_seed_names_what_the_build_component_lists_and_refuses_what_it_declares() {
        let seed = "schema-version 1\nbuilds { x \"image\" }\nlibraries {\n local {\n  owner { source \".\" }\n }\n}\nseed \"x\" collection=\"owner\"\n";
        let refusal = "`image` declares more than the components it lists";
        let lists = repository(
            "seed-lists",
            "component \"/\" {\n x\n tools\n}\n",
            &[
                ("x", "base \"x\" { family \"fedora\"; }\n"),
                ("tools", "description \"Tools\"\n"),
            ],
        );
        std::fs::write(lists.join("repo.kdl"), seed).expect("the temporary directory is writable");
        std::fs::create_dir_all(lists.join("components/tools/files/etc"))
            .expect("the temporary directory is writable");
        std::fs::write(lists.join("components/tools/files/etc/tools"), "x\n")
            .expect("the temporary directory is writable");
        let listing = List::load(&lists).1.findings();
        let _ = std::fs::remove_dir_all(&lists);
        let declares = repository(
            "seed-declares",
            "packages \"vim\"\ncomponent \"/\" {\n x\n}\n",
            &[("x", "base \"x\" { family \"fedora\"; }\n")],
        );
        std::fs::write(declares.join("repo.kdl"), seed)
            .expect("the temporary directory is writable");
        let declaring = List::load(&declares).1.findings();
        let _ = std::fs::remove_dir_all(&declares);
        assert!(listing.is_empty(), "{listing:?}");
        holds(&declaring, refusal);
    }

    #[test]
    fn a_manifest_node_with_a_property_alone_defines_inline() {
        let found = resolved(
            "inline-fragment",
            "component \"/\" {\n x\n tools {\n  fragment position=\"after\"\n }\n}\n",
            &[],
        );
        holds(
            &found,
            "`tools` declares `fragment` but ships no Containerfile.inc",
        );
    }
}
