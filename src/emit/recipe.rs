use crate::emit::json::Json;
use crate::model::image::{Layout, List};

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

fn settle(family: Option<&str>, layout: Option<&Layout>) -> Option<(Family, String)> {
    let known = family.and_then(self::family);
    let told = |pick: fn(&Layout) -> &str| {
        layout
            .map(pick)
            .filter(|declared| !declared.is_empty())
            .map(str::to_string)
    };
    let bootloader = told(|l| &l.bootloader)?;
    let family = Family {
        composefs: layout
            .and_then(|l| l.composefs)
            .or(known.as_ref().map(|f| f.composefs))?,
        generic: layout
            .and_then(|l| l.generic)
            .or(known.as_ref().map(|f| f.generic))?,
        filesystem: told(|l| &l.filesystem)
            .or_else(|| known.as_ref().map(|f| f.filesystem.clone()))?,
        admin: told(|l| &l.admin_group).or_else(|| known.as_ref().map(|f| f.admin.clone()))?,
    };
    Some((family, bootloader))
}

pub fn unanswered(family: &str, layout: Option<&Layout>) -> Vec<&'static str> {
    if settle(Some(family), layout).is_some() {
        return Vec::new();
    }
    let known = self::family(family);
    let told = |pick: fn(&Layout) -> &str| layout.is_some_and(|l| !pick(l).is_empty());
    [
        ("filesystem", told(|l| &l.filesystem)),
        ("composefs", layout.is_some_and(|l| l.composefs.is_some())),
        ("generic-image", layout.is_some_and(|l| l.generic.is_some())),
        ("admin-group", told(|l| &l.admin_group)),
        ("bootloader", told(|l| &l.bootloader)),
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
         its base's row in bases.kdl lists",
        unanswered(&base.family, image.layout.as_ref()).join(", ")
    )
}

pub fn seals(family: &str, layout: Option<&Layout>) -> bool {
    layout
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
    let target = list.targets().into_iter().find(|t| t.to_string() == name)?;
    let declared = list.images.iter().find(|i| i.id == target.image)?;
    let layout = declared.layout.as_ref();
    let (settled, bootloader) = settle(Some(&declared.base.as_ref()?.family), layout)?;

    let mut fields = vec![
        ("image", Json::string(image)),
        ("targetImgref", Json::string(imgref)),
        ("composeFsBackend", Json::Bool(settled.composefs)),
        ("genericImage", Json::Bool(settled.generic)),
        ("bootloader", Json::string(&bootloader)),
        ("filesystem", Json::string(&settled.filesystem)),
        ("hostname", Json::string(target.published())),
        (
            "user",
            Json::object([("groups", Json::strings([&settled.admin]))]),
        ),
    ];
    if layout.is_some_and(|l| l.subvolumes) {
        fields.push(("btrfsSubvolumes", Json::Bool(true)));
    }
    if let Some(pool) = layout.map(|l| l.pool.as_str()).filter(|p| !p.is_empty()) {
        fields.push(("zfsPoolName", Json::string(pool)));
    }
    if let Some(var) = layout.and_then(|l| l.var_disk.as_ref()) {
        fields.push((
            "varDisk",
            Json::object([
                ("disk", Json::string(&var.disk)),
                ("keepExisting", Json::Bool(var.keep_existing)),
            ]),
        ));
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

pub const EFI_PATCH: &str = include_str!("installer-efi-from-image.patch");

pub fn live(published: &str) -> String {
    format!("localhost/{published}-installer:latest")
}

pub fn media(list: &List, name: &str, image: &str, imgref: &str) -> Option<Json> {
    let target = list.targets().into_iter().find(|t| t.to_string() == name)?;
    let declared = list.images.iter().find(|i| i.id == target.image)?;
    settle(
        Some(&declared.base.as_ref()?.family),
        declared.layout.as_ref(),
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
                (
                    "title",
                    Json::string(format!("{} installer", declared.name)),
                ),
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
    fn a_declared_layout_replaces_the_family_filesystem_and_adds_the_var_disk() {
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
        assert_eq!(field("btrfsSubvolumes").as_deref(), Some("true"));
        assert_eq!(field("bootloader").as_deref(), Some("\"systemd\""));
        assert_eq!(
            field("varDisk").as_deref(),
            Some("{\n  \"disk\": \"/dev/sdb\",\n  \"keepExisting\": false\n}")
        );
        assert_eq!(field("composeFsBackend").as_deref(), Some("true"));

        let plain = build(&deb, "forky", "image", "imgref", &[]).expect("a debian target");
        let Json::Object(fields) = plain else {
            panic!("a recipe is an object")
        };
        for absent in ["varDisk", "btrfsSubvolumes", "zfsPoolName"] {
            assert!(fields.iter().all(|(had, _)| had != absent), "{absent}");
        }
        assert!(fields
            .iter()
            .any(|(had, value)| had == "bootloader" && value.render().trim() == "\"grub2\""));
    }

    #[test]
    fn a_sealed_family_is_the_only_one_that_refuses_a_filesystem() {
        assert!(seals("debian", None));
        assert!(seals("ubuntu", None));
        assert!(!seals("fedora", None));
        assert!(!seals("plan9", None));

        let sealing = layout(Some(true));
        let plain = layout(Some(false));
        assert!(seals("plan9", Some(&sealing)));
        assert!(seals("fedora", Some(&sealing)), "the image overrides");
        assert!(!seals("debian", Some(&plain)), "in both directions");
    }

    fn layout(composefs: Option<bool>) -> crate::model::image::Layout {
        crate::model::image::Layout {
            filesystem: String::new(),
            subvolumes: false,
            pool: String::new(),
            bootloader: String::new(),
            composefs,
            generic: None,
            admin_group: String::new(),
            var_disk: None,
            span: crate::diag::Span::default(),
        }
    }

    #[test]
    fn rhel_installs_the_way_fedora_does() {
        let mut grub = layout(None);
        grub.bootloader = "grub2".into();
        assert!(settle(Some("rhel"), Some(&grub)).is_some());
        assert_eq!(
            settle(Some("rhel"), Some(&grub)),
            settle(Some("fedora"), Some(&grub))
        );
        assert_eq!(unanswered("rhel", Some(&grub)), Vec::<&str>::new());
        assert!(!seals("rhel", None));
    }

    #[test]
    fn the_bootloader_is_the_image_s_own_answer() {
        assert_eq!(unanswered("fedora", None), ["bootloader"]);
        assert_eq!(settle(Some("debian"), None), None);
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
            unanswered("plan9", None),
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
            unanswered("plan9", Some(&half)),
            ["generic-image", "admin-group", "bootloader"]
        );
        assert_eq!(unanswered("fedora", Some(&half)), ["bootloader"]);

        let mut whole = half;
        whole.generic = Some(true);
        whole.admin_group = "wheel".into();
        whole.bootloader = "systemd".into();
        assert_eq!(unanswered("plan9", Some(&whole)), Vec::<&str>::new());
        let (settled, bootloader) =
            settle(Some("plan9"), Some(&whole)).expect("all five are declared");
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
             \"user\": {\n    \"groups\": [\n      \"wheel\"\n    ]\n  },\n  \
             \"btrfsSubvolumes\": true\n}\n"
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
             \"Forky installer\",\n      \"modes\": [\n        \"live\"\n      ]\n    }\n  ],\n  \"offline_payloads\": [\n    {\n      \"source\": \
             \"localhost/forky:latest\",\n      \"ref\": \
             \"ghcr.io/someone/forky:latest\"\n    }\n  ]\n}\n"
        );

        assert!(media(&deb, "not-a-target", "image", "imgref").is_none());
    }

    #[test]
    fn the_verb_the_live_environment_autostarts_is_one_that_resolves() {
        let typed: Vec<&str> = LIVE_ENV
            .lines()
            .map(str::trim)
            .find(|line| line.starts_with("buildc "))
            .expect("the live environment types one command")
            .split_whitespace()
            .skip(1)
            .collect();
        let resolved = crate::command::resolve(&typed);
        assert!(resolved.is_ok(), "{typed:?}: {:?}", resolved.err());
    }

    #[test]
    fn a_family_with_no_measured_answer_is_refused_rather_than_defaulted() {
        assert!(settle(Some("plan9"), None).is_none());
        assert!(settle(Some(""), None).is_none());
        let mut half = layout(Some(true));
        half.filesystem = "btrfs".into();
        assert!(settle(Some("plan9"), Some(&half)).is_none());
        let deb = fixture("deb-families");
        assert!(build(&deb, "not-a-target", "image", "imgref", &[]).is_none());
    }
}
