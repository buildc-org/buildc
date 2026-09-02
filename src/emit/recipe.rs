use crate::emit::json::Json;
use crate::model::image::List;
use crate::resolve::workflow::FEDORA;

struct Family {
    composefs: bool,
    generic: bool,
    bootloader: &'static str,
    filesystem: &'static str,
    admin: &'static str,
}

fn family(name: &str) -> Option<Family> {
    Some(match name {
        FEDORA => Family {
            composefs: false,
            generic: false,
            bootloader: "",
            filesystem: "xfs",
            admin: "wheel",
        },
        "debian" | "ubuntu" => Family {
            composefs: true,
            generic: true,
            bootloader: "systemd",
            filesystem: "ext4",
            admin: "sudo",
        },
        _ => return None,
    })
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
    let family = self::family(&declared.base.as_ref()?.family)?;

    let mut fields = vec![
        ("image", Json::string(image)),
        ("targetImgref", Json::string(imgref)),
        ("composeFsBackend", Json::Bool(family.composefs)),
        ("genericImage", Json::Bool(family.generic)),
        ("bootloader", Json::string(family.bootloader)),
        ("filesystem", Json::string(family.filesystem)),
        ("hostname", Json::string(target.published())),
        (
            "user",
            Json::object([("groups", Json::strings([family.admin]))]),
        ),
    ];
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
    self::family(&declared.base.as_ref()?.family)?;
    let published = target.published();

    Some(Json::object([
        ("media_name", Json::string(format!("{published}-install"))),
        ("size", Json::string(SIZE)),
        ("esp_partition", Json::Bool(true)),
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
                 \"genericImage\": true,\n  \"bootloader\": \"systemd\",\n  \
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
            Some("\"systemd\"")
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
             \"esp_partition\": true,\n  \
             \"bootable_environments\": [\n    {\n      \"id\": \"installer\",\n      \
             \"image\": \"localhost/forky-installer:latest\",\n      \"title\": \
             \"Forky installer\",\n      \"modes\": [\n        \"live\"\n      ]\n    }\n  ],\n  \"offline_payloads\": [\n    {\n      \"source\": \
             \"localhost/forky:latest\",\n      \"ref\": \
             \"ghcr.io/someone/forky:latest\"\n    }\n  ]\n}\n"
        );

        assert!(media(&deb, "not-a-target", "image", "imgref").is_none());
    }

    #[test]
    fn a_family_with_no_measured_answer_is_refused_rather_than_defaulted() {
        assert!(self::family("plan9").is_none());
        assert!(self::family("").is_none());
        let deb = fixture("deb-families");
        assert!(build(&deb, "not-a-target", "image", "imgref", &[]).is_none());
    }
}
