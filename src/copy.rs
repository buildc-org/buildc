pub const PRODUCT: &str = "Buildc";

pub const REPO_NAME: &str = "What will the repo be called?";
pub const SCHEDULED: &str = "Sync this repo to a provider?";
pub const REPO_HOST: &str = "Where will the repo be hosted?";
pub const FORGEJO_ADDRESS: &str = "What is the address of the Forgejo instance?";
pub const CREATE_REMOTE: &str = "Create this repo on Github now?";
pub const NO_GH: &str = "Install the Github CLI now?";

pub fn username(host: &str) -> String {
    match host {
        crate::create::HOST => "Github username?".to_string(),
        host => format!("Username on {host}?"),
    }
}

pub const LIBRARIES: &str = "Included libraries:";
pub const NO_BASE_LIBRARY: &str = "No base image library in repo, Add one now?";
pub const WHICH_LIBRARY: &str = "Which library?";
pub const WHICH_KIND: &str = "Which kind of library?";
pub const OTHER_SOURCE: &str = "Another source";
pub const OTHER_SOURCE_ABOUT: &str = "Type the alias, the Git URL and the directory";
pub const SOURCE_ALIAS: &str = "source alias";
pub const SOURCE_URL: &str = "repository URL";
pub const SOURCE_PATH: &str = "directory inside the repository";

pub const IMAGES: &str = "Define an image now?";
pub const IMAGE_NAME: &str = "What will the image be called?";
pub const IMAGE_BASE: &str = "What is the base image for this image?";
pub const BASE_IMAGE: &str = "base image";
pub const OTHER_BASE: &str = "Another base image";
pub const OTHER_BASE_ABOUT: &str = "Type an image reference that is not in this catalog";
pub const BASE_FAMILY: &str = "base family";
pub const IMAGE_BOOTLOADER: &str = "Which bootloader does it install?";
pub const BOOTLOADER_GRUB2: &str = "GRUB";
pub const BOOTLOADER_SYSTEMD: &str = "systemd-boot";
pub const FLAVOUR_NAME: &str = "flavour name";
pub const FLAVOUR_IMAGE: &str = "Which image publishes it?";

pub const MODULE_NAME: &str = "module name";
pub const MODULE_DESCRIPTION: &str = "description (optional)";
pub const MODULE_SUPPORTS: &str = "supported base families, separated by spaces (optional)";
pub const MODULE_PACKAGES: &str = "Does this module install packages?";
pub const PACKAGE_NAMES: &str = "package names, separated by spaces";
pub const WHICH_MODULE: &str = "Which module?";
pub const WHICH_MODULES: &str = "Which modules?";
pub const LIST_IN_IMAGES: &str = "Which images list it?";

pub fn list_in(target: &str) -> String {
    format!("List it in {target}?")
}

pub const BRING_REQUIRED: &str = "Bring what these modules require?";
pub const BRING_FOR_BASE: &str = "Add them now?";
pub const IMPORT_CLAIMING: &str = "Import the modules claiming these rules?";

pub fn cleared(named: &str, unmet: &str) -> String {
    format!("{named} {unmet} and was cleared")
}

pub const GENERATE_WORKFLOWS: &str = "Generate the workflows this makes runnable?";

pub const WORKFLOWS: &str = "Which workflows?";
pub const PUBLISH_SCHEDULED: &str = "Publish images only on scheduled builds?";
pub const SCAN_SCHEDULED: &str = "Run image scans only on scheduled builds?";
pub const DAILY_AT: &str = "what time the daily build runs, UTC";

pub const SCRIPTS: &str = "Which files to keep in scripts/?";
pub const SCRIPTS_NEXT: &str =
    "run `buildc generate` so the generated scripts and workflows call the copies in scripts/";

pub const WHICH_IMAGE: &str = "Which image?";
pub const MEASURED_IMAGE: &str = "Which image is measured?";
pub const WHICH_PROFILE: &str = "Which profile?";

pub fn claimed_rules(named: &str) -> String {
    format!("Which rules does `{named}` claim?")
}

pub const WHICH_KEY: &str = "Which key?";
pub const KEY_CN: &str = "common name, which is what the enrolment prompt shows";
pub const KEY_FROM: &str = "path to the public half already held";

pub fn key_provider(kind: &str) -> String {
    format!("Which module's {kind} key?")
}

pub const WHICH_DISK: &str = "Which kind of disk image?";
pub const DISK_QCOW2: &str = "a qemu disk, under out/qcow2/";
pub const DISK_RAW: &str = "a raw disk, under out/raw/";
pub const DISK_ISO: &str = "an installer iso, under out/bootiso/";
pub const NO_ISO_SPAWN: &str = "systemd-vmspawn cannot boot an iso";

pub const WHICH_COMMAND: &str = "Which command?";

pub const YES: &str = "Yes";
pub const NO: &str = "No";
pub const SKIP: &str = "Skip";
pub const SKIP_REMOTE: &str = "Skip Github repo creation";

pub const HOST_FORGEJO: &str = "a Forgejo instance, at an address asked next";

pub fn host_github() -> String {
    format!("Github, and the workflows {PRODUCT} ships")
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
