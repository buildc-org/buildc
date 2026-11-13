pub const PRODUCT: &str = "Buildc";

pub const REPO_NAME: &str = "repo name";
pub const SCHEDULED: &str = "Host this repo on a provider?";
pub const REPO_HOST: &str = "repo host";
pub const FORGEJO_ADDRESS: &str = "Forgejo instance address";
pub const CREATE_REMOTE: &str = "Create the repo on Github?";
pub const NO_GH: &str = "Install the Github CLI?";

pub fn username(host: &str) -> String {
    match host {
        crate::forge::HOST => "Github username".to_string(),
        host => format!("username on {host}"),
    }
}

pub const LIBRARIES: &str = "libraries";
pub const WHICH_LIBRARY: &str = "library";
pub const OTHER_SOURCE: &str = "Another source";
pub const OTHER_SOURCE_ABOUT: &str = "a library outside the defaults";
pub const SOURCE_ALIAS: &str = "source alias";
pub const SOURCE_URL: &str = "repository URL";
pub const SOURCE_PATH: &str = "directory inside the repository";

pub const IMAGES: &str = "Define an image?";
pub const IMAGE_NAME: &str = "image name";
pub const IMAGE_BASE: &str = "base image";
pub const BASE_IMAGE: &str = "base image";
pub const OTHER_BASE: &str = "Another base image";
pub const IS_BASE: &str = "a base component that an image lists as its base";
pub const OTHER_BASE_ABOUT: &str = "an image reference no base component names";
pub const IMAGE_BOOTLOADER: &str = "bootloader";
pub const BOOTLOADER_GRUB2: &str = "GRUB";
pub const BOOTLOADER_SYSTEMD: &str = "systemd-boot";
pub const FLAVOUR_NAME: &str = "flavour name";
pub const FLAVOUR_IMAGE: &str = "image that publishes the flavour";

pub const COMPONENT_NAME: &str = "component name";
pub const COMPONENT_DESCRIPTION: &str = "description (optional)";
pub const COMPONENT_SUPPORTS: &str = "supported base families, separated by spaces (optional)";
pub const PACKAGE_NAMES: &str = "package names, separated by spaces";
pub const WHICH_COMPONENT: &str = "component";
pub const WHICH_COMPONENTS: &str = "components";
pub const LIST_IN_IMAGES: &str = "images and flavours that list it";

pub fn list_in(target: &str) -> String {
    format!("List it in {target}?")
}

pub const TARGET_IMAGE: &str = "image or flavour";
pub const TARGET_NONE: &str = "None, only add it to the repository";

pub fn unsupported(targets: &[String], family: &str) -> String {
    format!("targets {}; this image is {family}", targets.join(", "))
}

pub fn present(name: &str) -> String {
    format!("`{name}` is already in this repository.")
}

pub fn add_local(name: &str, image: &str) -> String {
    format!("Add the local `{name}` to `{image}`")
}

pub const ANOTHER_NAME: &str = "Import it under another name";

pub fn exclusive_to(capability: &str, holder: &str) -> String {
    format!("`{capability}` is exclusive to `{holder}`")
}

pub fn exclusive_clash(capability: &str, holder: &str, image: &str) -> String {
    format!("`{capability}` is exclusive. `{holder}` already provides it in `{image}`.")
}

pub fn replace(holder: &str) -> String {
    format!("Replace `{holder}`")
}

pub const CANCEL: &str = "Cancel";
pub const LOCAL_NAME: &str = "local name";

pub fn track(named: &str) -> String {
    format!("Track {named} upstream?")
}

pub fn untrack(key: &str) -> String {
    format!("Stop tracking `components/{key}`? The copy becomes the repository's own.")
}

pub const REQUIRED: &str = "Dependencies the images do not provide:";
pub const BRING_REQUIRED: &str = "Add these provider components?";

pub fn which_provider(component: &str, capability: &str) -> String {
    format!("provider of `{capability}` for `{component}`")
}
pub const BRING_FOR_BASE: &str = "Add these components?";
pub const IMPORT_CLAIMING: &str = "Import the components that claim rules in this profile?";

pub fn cleared(named: &str, unmet: &str) -> String {
    format!("{named} was cleared: it {unmet}")
}

pub const GENERATE_WORKFLOWS: &str = "Generate the workflows these components unlock?";

pub const WORKFLOWS: &str = "workflows";
pub const PUBLISH_SCHEDULED: &str = "Publish images only on scheduled builds?";
pub const SCAN_SCHEDULED: &str = "Run image scans only on scheduled builds?";
pub const DAILY_AT: &str = "daily build time, UTC";

pub const SCRIPTS: &str = "files to keep in scripts/";
pub const SCRIPTS_NEXT: &str =
    "run `buildc generate` so the generated scripts and workflows call the copies in scripts/";

pub const WHICH_IMAGE: &str = "image";
pub const MEASURED_IMAGE: &str = "image to measure";
pub const WHICH_PROFILE: &str = "SCAP profile";

pub fn claimed_rules(named: &str) -> String {
    format!("rules `{named}` claims")
}

pub const WHICH_KEY: &str = "key";
pub const KEY_CN: &str = "common name";
pub const KEY_FROM: &str = "path to the public key";

pub fn key_provider(kind: &str) -> String {
    format!("component declaring the {kind} key")
}

pub const WHICH_DISK: &str = "disk image type";
pub const DISK_QCOW2: &str = "written to out/qcow2/";
pub const DISK_RAW: &str = "written to out/raw/";

pub const WHICH_COMMAND: &str = "command";

pub const YES: &str = "Yes";
pub const NO: &str = "No";
pub const SKIP: &str = "Skip";
pub const SKIP_REMOTE: &str = "Skip Github repo creation";

pub const HOST_FORGEJO: &str = "a Forgejo instance";

pub fn host_github() -> String {
    format!("Github, with the workflows {PRODUCT} ships")
}

pub const REVIEW_KEYS: &str =
    "\u{2191}\u{2193} navigate \u{2022} \u{23ce}  select \u{2022} Esc cancel";
pub const FORM_KEYS: &str = "up and down to move, enter to change a field";

pub const REVIEW: &str = "Review what will be created";
pub const CREATE: &str = "Create";
pub const NONE: &str = "none";
pub const ROW_NAME: &str = "name";
pub const ROW_PROVIDER: &str = "provider";
pub const ROW_REMOTE: &str = "github repo";
pub const ROW_LIBRARIES: &str = "libraries";
pub const ROW_IMAGE: &str = "image";
pub const ROW_BASE: &str = "base";
pub const ROW_WORKFLOWS: &str = "workflows";
pub const ROW_PUBLISH: &str = "publish";
pub const ROW_SCANS: &str = "image scans";
pub const ROW_DAILY: &str = "daily build";
pub const ROW_SCRIPTS: &str = "scripts kept";
pub const REMOTE_MADE: &str = "created on push";
pub const REMOTE_NOT: &str = "not created";
pub const ON_EVERY_PUSH: &str = "on every push";
pub const ON_EVERY_BUILD: &str = "on every build";
pub const ON_SCHEDULED: &str = "on scheduled builds only";

#[cfg(test)]
mod tests {
    #[test]
    fn every_string_here_is_one_short_line() {
        for line in include_str!("copy.rs").lines() {
            let Some(rest) = line.strip_prefix("pub const ") else {
                continue;
            };
            if !rest.ends_with("\";") {
                continue;
            }
            let name = rest.split_once(':').expect("a const name").0;
            if name.ends_with("_KEYS") {
                continue;
            }
            let text = rest.split_once("= \"").expect("a string literal").1;
            assert!(text.chars().count() - 2 < 60, "{line}");
        }
    }
}
