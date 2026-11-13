use crate::emit::plan::{of_target, provides};
use crate::model::image::{Layout, List};
use common::json::Json;

pub const LUKS_INITRAMFS: &str = "luks-initramfs";

#[cfg_attr(test, derive(Debug, PartialEq))]
struct Family {
    composefs: bool,
    generic: bool,
    filesystem: String,
    admin: String,
}

fn family(name: &str) -> Option<Family> {
    Some(match name {
        "fedora" | "rhel" => Family {
            composefs: false,
            generic: false,
            filesystem: "xfs".into(),
            admin: "wheel".into(),
        },
        "debian" | "ubuntu" => Family {
            composefs: true,
            generic: true,
            filesystem: "ext4".into(),
            admin: "sudo".into(),
        },
        _ => return None,
    })
}

fn settle(family: Option<&str>, layout: Option<&Layout>, boot: &str) -> Option<(Family, String)> {
    let known = family.and_then(self::family);
    let uki = !boot.is_empty();
    let told = |pick: fn(&Layout) -> &str| {
        layout
            .map(pick)
            .filter(|declared| !declared.is_empty())
            .map(str::to_string)
    };
    let bootloader = if boot.is_empty() {
        told(|l| &l.bootloader)?
    } else {
        "systemd".into()
    };
    let family = Family {
        composefs: uki
            || layout
                .and_then(|l| l.composefs)
                .or(known.as_ref().map(|f| f.composefs))?,
        generic: uki
            || layout
                .and_then(|l| l.generic)
                .or(known.as_ref().map(|f| f.generic))?,
        filesystem: if uki {
            "btrfs".into()
        } else {
            told(|l| &l.filesystem).or_else(|| known.as_ref().map(|f| f.filesystem.clone()))?
        },
        admin: told(|l| &l.admin_group).or_else(|| known.as_ref().map(|f| f.admin.clone()))?,
    };
    Some((family, bootloader))
}

pub fn unanswered(family: &str, layout: Option<&Layout>, boot: &str) -> Vec<&'static str> {
    if settle(Some(family), layout, boot).is_some() {
        return Vec::new();
    }
    let known = self::family(family);
    let told = |pick: fn(&Layout) -> &str| layout.is_some_and(|l| !pick(l).is_empty());
    [
        ("filesystem", told(|l| &l.filesystem) || !boot.is_empty()),
        (
            "composefs",
            layout.is_some_and(|l| l.composefs.is_some()) || !boot.is_empty(),
        ),
        (
            "generic-image",
            layout.is_some_and(|l| l.generic.is_some()) || !boot.is_empty(),
        ),
        ("admin-group", told(|l| &l.admin_group)),
        ("bootloader", told(|l| &l.bootloader) || !boot.is_empty()),
    ]
    .into_iter()
    .filter(|(name, _)| known.is_none() || *name == "bootloader")
    .filter(|(_, declared)| !declared)
    .map(|(name, _)| name)
    .collect()
}

pub fn refusal(list: &List, name: &str) -> String {
    let declared = list
        .targets()
        .into_iter()
        .find(|t| t.to_string() == name)
        .and_then(|t| list.images.iter().find(|i| i.id == t.image));
    let Some(image) = declared else {
        return format!("`{name}` is not a target here");
    };
    let Some(base) = image.base.as_ref() else {
        return format!("`{name}` declares no base, so there is nothing to install");
    };
    format!(
        "`{name}` answers no {}, and guessing one erases a disk before it fails to \
         boot\n\nhelp: declare it in the image's `layout {{ }}`; the bootloader is one \
         what the base component lists",
        unanswered(&base.family, image.layout.as_ref(), &image.boot).join(", ")
    )
}

pub fn seals(family: &str, layout: Option<&Layout>, boot: &str) -> bool {
    !boot.is_empty()
        || layout
            .and_then(|l| l.composefs)
            .unwrap_or_else(|| self::family(family).is_some_and(|f| f.composefs))
}

pub fn build(
    list: &List,
    name: &str,
    image: &str,
    imgref: &str,
    stores: &[String],
) -> Option<Json> {
    let published = list
        .targets()
        .into_iter()
        .find(|target| target.to_string() == name)?
        .published();
    let (declared, _, entries) = of_target(list, name)?;
    let layout = declared.layout.as_ref();
    let (settled, bootloader) = settle(
        Some(&declared.base.as_ref()?.family),
        layout,
        &declared.boot,
    )?;

    let mut fields = vec![
        ("image", Json::string(image)),
        ("targetImgref", Json::string(imgref)),
        ("composeFsBackend", Json::Bool(settled.composefs)),
        ("genericImage", Json::Bool(settled.generic)),
        ("bootloader", Json::string(&bootloader)),
        ("filesystem", Json::string(&settled.filesystem)),
        ("hostname", Json::string(published)),
        (
            "user",
            Json::object([("groups", Json::strings([&settled.admin]))]),
        ),
    ];
    if !declared.boot.is_empty() {
        fields.push(("boot", Json::string(&declared.boot)));
    }
    if provides(declared, &entries, LUKS_INITRAMFS) {
        fields.push(("luksInitramfs", Json::Bool(true)));
    }
    if !stores.is_empty() {
        fields.push((
            "additionalImageStores",
            Json::strings(stores.iter().cloned()),
        ));
    }
    Some(Json::object(fields))
}

pub const STORE: &str = "/var/lib/buildc/store";

const SIZE: &str = "20G";

pub const LIVE_ENV: &str = include_str!("installer.Containerfile");

pub fn live(published: &str) -> String {
    format!("localhost/{published}-installer:latest")
}

pub fn media(list: &List, name: &str, image: &str, imgref: &str) -> Option<Json> {
    let target = list.targets().into_iter().find(|t| t.to_string() == name)?;
    let declared = list.images.iter().find(|i| i.id == target.image)?;
    settle(
        Some(&declared.base.as_ref()?.family),
        declared.layout.as_ref(),
        &declared.boot,
    )?;
    let published = target.published();

    Some(Json::object([
        ("media_name", Json::string(format!("{published}-install"))),
        ("size", Json::string(SIZE)),
        (
            "bootable_environments",
            Json::array([Json::object([
                ("id", Json::string("installer")),
                ("image", Json::string(live(&published))),
                ("title", Json::string(format!("{} installer", declared.id))),
                ("modes", Json::strings(["live"])),
            ])]),
        ),
        (
            "offline_payloads",
            Json::array([Json::object([
                ("source", Json::string(image)),
                ("ref", Json::string(imgref)),
            ])]),
        ),
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn fixture(name: &str) -> List {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/repos")
            .join(name);
        let (list, issues) = List::load(&root);
        assert!(issues.is_empty(), "{name} is a clean fixture");
        list
    }

    #[test]
    fn the_family_settles_the_boot_chain_and_nothing_else_does() {
        let deb = fixture("deb-families");
        let recipe = build(
            &deb,
            "forky",
            "localhost/forky:latest",
            "ghcr.io/someone/forky:latest",
            &[STORE.to_string()],
        )
        .expect("a debian target has an answer");
        assert_eq!(
            recipe.render(),
            format!(
                "{{\n  \"image\": \"localhost/forky:latest\",\n  \"targetImgref\": \
                 \"ghcr.io/someone/forky:latest\",\n  \"composeFsBackend\": true,\n  \
                 \"genericImage\": true,\n  \"bootloader\": \"grub2\",\n  \
                 \"filesystem\": \"ext4\",\n  \"hostname\": \"forky\",\n  \"user\": \
                 {{\n    \"groups\": [\n      \"sudo\"\n    ]\n  }},\n  \
                 \"additionalImageStores\": [\n    \"{STORE}\"\n  ]\n}}\n"
            )
        );

        let field =
            |list: &List, name: &str, key: &str| match build(list, name, "image", "imgref", &[]) {
                Some(Json::Object(fields)) => fields
                    .iter()
                    .find(|(had, _)| had == key)
                    .map(|(_, value)| value.render().trim().to_string()),
                _ => None,
            };
        assert_eq!(
            field(&deb, "ubuntu", "bootloader").as_deref(),
            Some("\"grub2\"")
        );
        assert_eq!(field(&deb, "ubuntu", "user"), field(&deb, "forky", "user"));

        let fedora = fixture("minimal");
        let target = fedora.ungated_target().expect("the fixture publishes one");
        let target = target.to_string();
        assert_eq!(
            field(&fedora, &target, "composeFsBackend").as_deref(),
            Some("false")
        );
        assert_eq!(
            field(&fedora, &target, "genericImage").as_deref(),
            Some("false")
        );
        assert_eq!(
            field(&fedora, &target, "filesystem").as_deref(),
            Some("\"xfs\"")
        );
        assert_eq!(
            field(&fedora, &target, "user").as_deref(),
            Some("{\n  \"groups\": [\n    \"wheel\"\n  ]\n}")
        );

        assert!(field(&deb, "forky", "additionalImageStores").is_none());
    }

    #[test]
    fn a_declared_layout_replaces_the_family_filesystem() {
        let deb = fixture("deb-families");
        let recipe = build(&deb, "debian", "image", "imgref", &[]).expect("a debian target");
        let field = |key: &str| match &recipe {
            Json::Object(fields) => fields
                .iter()
                .find(|(had, _)| had == key)
                .map(|(_, value)| value.render().trim().to_string()),
            _ => None,
        };
        assert_eq!(field("filesystem").as_deref(), Some("\"btrfs\""));
        assert_eq!(field("bootloader").as_deref(), Some("\"systemd\""));
        assert_eq!(field("composeFsBackend").as_deref(), Some("true"));

        let plain = build(&deb, "forky", "image", "imgref", &[]).expect("a debian target");
        let Json::Object(fields) = plain else {
            panic!("a recipe is an object")
        };
        assert!(fields
            .iter()
            .any(|(had, value)| had == "bootloader" && value.render().trim() == "\"grub2\""));
    }

    #[test]
    fn a_sealed_family_is_the_only_one_that_refuses_a_filesystem() {
        assert!(seals("debian", None, ""));
        assert!(seals("ubuntu", None, ""));
        assert!(!seals("fedora", None, ""));
        assert!(!seals("plan9", None, ""));

        let sealing = layout(Some(true));
        let plain = layout(Some(false));
        assert!(seals("plan9", Some(&sealing), ""));
        assert!(seals("fedora", Some(&sealing), ""), "the image overrides");
        assert!(!seals("debian", Some(&plain), ""), "in both directions");
    }

    fn layout(composefs: Option<bool>) -> crate::model::image::Layout {
        crate::model::image::Layout {
            filesystem: String::new(),
            bootloader: String::new(),
            composefs,
            generic: None,
            admin_group: String::new(),
            span: crate::diag::Span::default(),
        }
    }

    #[test]
    fn rhel_installs_the_way_fedora_does() {
        let mut grub = layout(None);
        grub.bootloader = "grub2".into();
        assert!(settle(Some("rhel"), Some(&grub), "").is_some());
        assert_eq!(
            settle(Some("rhel"), Some(&grub), ""),
            settle(Some("fedora"), Some(&grub), "")
        );
        assert_eq!(unanswered("rhel", Some(&grub), ""), Vec::<&str>::new());
        assert!(!seals("rhel", None, ""));
    }

    #[test]
    fn the_bootloader_is_the_image_s_own_answer() {
        assert_eq!(unanswered("fedora", None, ""), ["bootloader"]);
        assert_eq!(settle(Some("debian"), None, ""), None);
        let deb = fixture("deb-families");
        assert!(build(&deb, "ubuntu", "image", "imgref", &[]).is_some());

        let silent = fixture("multi-image");
        let name = silent.ungated_target().expect("a target").to_string();
        assert!(build(&silent, &name, "image", "imgref", &[]).is_none());
        assert!(refusal(&silent, &name).contains("answers no bootloader"));
    }

    #[test]
    fn an_unmeasured_family_answers_for_itself_in_full_or_not_at_all() {
        assert_eq!(
            unanswered("plan9", None, ""),
            [
                "filesystem",
                "composefs",
                "generic-image",
                "admin-group",
                "bootloader"
            ]
        );

        let mut half = layout(Some(true));
        half.filesystem = "btrfs".into();
        assert_eq!(
            unanswered("plan9", Some(&half), ""),
            ["generic-image", "admin-group", "bootloader"]
        );
        assert_eq!(unanswered("fedora", Some(&half), ""), ["bootloader"]);
        assert_eq!(unanswered("plan9", None, "uki-db"), ["admin-group"]);

        let mut whole = half;
        whole.generic = Some(true);
        whole.admin_group = "wheel".into();
        whole.bootloader = "systemd".into();
        assert_eq!(unanswered("plan9", Some(&whole), ""), Vec::<&str>::new());
        let (settled, bootloader) =
            settle(Some("plan9"), Some(&whole), "").expect("all five are declared");
        assert_eq!(bootloader, "systemd");
        assert_eq!(settled.filesystem, "btrfs");
        assert_eq!(settled.admin, "wheel");
        assert!(settled.composefs && settled.generic);
    }

    #[test]
    fn an_image_on_an_unmeasured_family_installs_off_its_own_declaration() {
        let arch = fixture("unmeasured-family");
        let recipe = build(&arch, "arch", "localhost/arch:latest", "imgref", &[])
            .expect("all five are declared");
        assert_eq!(
            recipe.render(),
            "{\n  \"image\": \"localhost/arch:latest\",\n  \"targetImgref\": \"imgref\",\n  \
             \"composeFsBackend\": true,\n  \"genericImage\": true,\n  \"bootloader\": \
             \"systemd\",\n  \"filesystem\": \"btrfs\",\n  \"hostname\": \"arch\",\n  \
             \"user\": {\n    \"groups\": [\n      \"wheel\"\n    ]\n  }\n}\n"
        );
        assert!(media(&arch, "arch", "localhost/arch:latest", "imgref").is_some());
    }

    #[test]
    fn the_media_embeds_the_local_build_under_the_published_name() {
        let deb = fixture("deb-families");
        let assembled = media(
            &deb,
            "forky",
            "localhost/forky:latest",
            "ghcr.io/someone/forky:latest",
        )
        .expect("a debian target has an answer");
        assert_eq!(
            assembled.render(),
            "{\n  \"media_name\": \"forky-install\",\n  \"size\": \"20G\",\n  \
             \"bootable_environments\": [\n    {\n      \"id\": \"installer\",\n      \
             \"image\": \"localhost/forky-installer:latest\",\n      \"title\": \
             \"forky installer\",\n      \"modes\": [\n        \"live\"\n      ]\n    }\n  ],\n  \"offline_payloads\": [\n    {\n      \"source\": \
             \"localhost/forky:latest\",\n      \"ref\": \
             \"ghcr.io/someone/forky:latest\"\n    }\n  ]\n}\n"
        );

        assert!(media(&deb, "not-a-target", "image", "imgref").is_none());
    }

    #[test]
    fn the_live_graphroot_is_a_required_native_tmpfs() {
        assert!(LIVE_ENV
            .contains("COPY <<'MOUNT' /usr/lib/systemd/system/var-lib-containers-storage.mount"));
        assert!(LIVE_ENV.contains("What=tmpfs\nWhere=/var/lib/containers/storage"));
        assert_eq!(
            LIVE_ENV
                .lines()
                .filter(|line| *line == "Requires=var-lib-containers-storage.mount")
                .count(),
            2
        );
        assert!(!LIVE_ENV.contains("mount_program"));
    }

    #[test]
    fn both_installer_units_hand_over_to_the_binary_the_live_environment_stages() {
        const STAGES: &str = "tar -xf /tmp/installer.tar.gz -C /usr/bin bootc-anywhere";
        const RUNS: &str = "/usr/bin/bootc-anywhere";
        assert!(
            LIVE_ENV.contains(STAGES),
            "the live environment does not stage the installer"
        );
        for unit in ["bootc-anywhere.service", "bootc-anywhere-vt.service"] {
            let heredoc = format!("/usr/lib/systemd/system/{unit}\n");
            let body = LIVE_ENV.split_once(&heredoc).map_or_else(
                || panic!("{unit} is never written"),
                |(_, rest)| rest.split_once("\nUNIT\n").map_or(rest, |(body, _)| body),
            );
            let exec: Vec<&str> = body
                .lines()
                .filter_map(|line| line.strip_prefix("ExecStart="))
                .collect();
            assert_eq!(exec.len(), 1, "{unit} runs {} things", exec.len());
            assert_eq!(
                exec[0].split_whitespace().last(),
                Some(RUNS),
                "{unit} hands over to something else: {}",
                exec[0]
            );
        }
    }

    #[test]
    fn the_units_that_autostart_the_installer_own_the_console_they_draw_on() {
        for unit in ["bootc-anywhere.service", "bootc-anywhere-vt.service"] {
            assert!(
                LIVE_ENV.contains(&format!("systemctl enable {unit}")),
                "{unit} is never enabled"
            );
            assert!(
                LIVE_ENV.contains(&format!("/usr/lib/systemd/system/{unit}")),
                "{unit} is enabled but never written"
            );
        }
        let directives = |want: &str| LIVE_ENV.lines().filter(|line| line.trim() == want).count();
        assert_eq!(directives("Conflicts=getty@tty1.service"), 2);
        let masks = LIVE_ENV
            .lines()
            .find(|line| line.contains("systemctl mask getty@"))
            .expect("the getty on the installer's console is masked");
        for masked in ["getty@tty1.service", "autovt@tty1.service"] {
            assert!(masks.contains(masked), "{masked} is not in {masks}");
        }
        assert_eq!(directives("TTYPath=/dev/tty1"), 1);
        let kmscon: Vec<&str> = LIVE_ENV
            .lines()
            .filter(|line| line.starts_with("ExecStart=") && line.contains("kmscon"))
            .collect();
        assert_eq!(kmscon.len(), 1, "{kmscon:?}");
        for want in ["--vt=1", "--oneshot", "--no-switchvt"] {
            assert!(kmscon[0].contains(want), "{want} is not in {}", kmscon[0]);
        }
    }

    #[test]
    fn a_family_with_no_measured_answer_is_refused_rather_than_defaulted() {
        assert!(settle(Some("plan9"), None, "").is_none());
        assert!(settle(Some(""), None, "").is_none());
        let mut half = layout(Some(true));
        half.filesystem = "btrfs".into();
        assert!(settle(Some("plan9"), Some(&half), "").is_none());
        let deb = fixture("deb-families");
        assert!(build(&deb, "not-a-target", "image", "imgref", &[]).is_none());
    }

    #[test]
    fn a_uki_chain_sets_systemd_boot_without_a_layout_answer() {
        let (family, bootloader) = settle(Some("fedora"), None, "uki-db").expect("a known family");
        assert_eq!(bootloader, "systemd");
        assert!(family.composefs);
        assert!(family.generic);
        assert_eq!(family.filesystem, "btrfs");
        assert!(seals("fedora", None, "uki-db"));
        assert_eq!(unanswered("fedora", None, "uki-db"), Vec::<&str>::new());

        let list = fixture("uki");
        let recipe = build(&list, "shim", "image", "imgref", &[])
            .expect("an installable UKI image")
            .render();
        assert!(recipe.contains("\"bootloader\": \"systemd\""), "{recipe}");
        assert!(recipe.contains("\"boot\": \"uki-shim\""), "{recipe}");
        assert!(recipe.contains("\"luksInitramfs\": true"), "{recipe}");
    }
}
