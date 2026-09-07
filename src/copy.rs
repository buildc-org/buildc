pub const REPO_NAME: &str = "What will the repo be called?";
pub const SCHEDULED: &str = "Sync this repo to a provider?";
pub const REPO_HOST: &str = "Where will the repo be hosted?";
pub const FORGEJO_ADDRESS: &str = "What is the address of the Forgejo instance?";
pub const CREATE_REMOTE: &str = "Would you like to create this repo on Github now?";
pub const NO_GH: &str = "Install the Github CLI now?";

pub fn username(host: &str) -> String {
    match host {
        crate::create::HOST => "What is your github username?".to_string(),
        host => format!("What is your username on {host}?"),
    }
}

pub const IMAGES: &str = "Define an image now?";
pub const IMAGE_NAME: &str = "What will the image be called?";
pub const IMAGE_BASE: &str = "What is the base image for this image?";
pub const BASE_IMAGE: &str = "base image";
pub const BASE_FAMILY: &str = "base family";
pub const FLAVOUR_NAME: &str = "flavour name";
pub const FLAVOUR_IMAGE: &str = "Which image publishes it?";

pub const MODULE_NAME: &str = "module name";
pub const MODULE_PACKAGES: &str = "Does this module install packages?";
pub const PACKAGE_NAMES: &str = "package names, separated by spaces";
pub const WHICH_MODULE: &str = "Which module?";
pub const WHICH_MODULES: &str = "Which modules?";
pub const LIST_IN_IMAGES: &str = "Which images list it?";

pub fn list_in(target: &str) -> String {
    format!("List it in {target}?")
}

pub const BRING_REQUIRED: &str = "Bring what these modules require?";
pub const BRING_FOR_BASE: &str = "Do you want to add them now?";
pub const IMPORT_CLAIMING: &str = "Import the modules claiming these rules?";

pub fn cleared(named: &str, unmet: &str) -> String {
    format!("{named} {unmet} and was cleared")
}

pub const GENERATE_WORKFLOWS: &str = "Generate the workflows this makes runnable?";

pub const WORKFLOWS: &str = "Which workflows?";
pub const PUBLISH_SCHEDULED: &str = "Publish images only on scheduled builds?";
pub const SCAN_SCHEDULED: &str = "Run image scans only on scheduled builds?";
pub const DAILY_AT: &str = "what time the daily build runs, UTC";

pub const WHICH_IMAGE: &str = "Which image?";
pub const MEASURED_IMAGE: &str = "Which image is measured?";
pub const WHICH_PROFILE: &str = "Which profile?";

pub fn claimed_rules(named: &str) -> String {
    format!("Which rules does `{named}` claim?")
}

pub const WHICH_KEY: &str = "Which key?";
pub const KEY_CN: &str = "common name, which is what the enrolment prompt shows";
pub const KEY_FROM: &str = "path to the public half you already hold";

pub fn key_provider(kind: &str) -> String {
    format!("Which module's {kind} key?")
}

pub const WHICH_DISK: &str = "Which kind of disk image?";
pub const DISK_QCOW2: &str = "a qemu disk, under out/qcow2/";
pub const DISK_RAW: &str = "a raw disk, under out/raw/";
pub const DISK_ISO: &str = "an installer iso, under out/bootiso/";
pub const NO_ISO_SPAWN: &str = "systemd-vmspawn cannot boot an iso";

pub const INSTALL_DISK: &str = "Installation Disk:";
pub const INSTALL_NAME: &str = "Computer Name:";
pub const INSTALL_USER: &str = "Username:";
pub const INSTALL_PASSWORD: &str = "Password";
pub const PASSWORD_AGAIN: &str = "Retype password";
pub const NO_MATCH: &str = "Passwords did not match.";
pub const INSTALL_ENCRYPTION: &str = "Encryption type";
pub const LUKS_PASSPHRASE: &str = "LUKS passphrase";
pub const ENC_NONE: &str = "not encrypted";
pub const ENC_TPM2: &str = "unlocked by TPM";
pub const ENC_PASSPHRASE: &str = "unlocked by passphrase";
pub const ENC_BOTH: &str = "TPM with passphrase fallback";
pub const NO_TPM: &str = "No TPM available";
pub const REMOVABLE: &str = "removable";

pub fn erasing(disk: &str) -> String {
    format!("Everything on {disk} is erased. Nothing survives.")
}

pub const WHICH_COMMAND: &str = "Which command?";

pub const YES: &str = "Yes";
pub const NO: &str = "No";
pub const SKIP: &str = "Skip";
pub const SKIP_REMOTE: &str = "Skip Github repo creation";

pub const HOST_GITHUB: &str = "Github, and the workflows Buildc ships";
pub const HOST_FORGEJO: &str = "a Forgejo instance, whose address you give";

pub const PICK: &str = "up and down to move, enter to choose, esc cancels";
pub const TOGGLE: &str = "space toggles, enter confirms, esc cancels";
pub const EITHER: &str = "up and down to move, enter to answer";
pub const NEST: &str = "filter, space toggles, ←/→ opens, enter confirms";
pub const REVIEW_KEYS: &str = "enter to change a field, Create to write, esc cancels";
pub const INSTALL_KEYS: &str = "enter to change a field, esc cancels";
pub const SECRET_KEYS: &str = "typed and not shown, enter confirms";
pub const LINE_KEYS: &str = "enter confirms";

pub const REVIEW: &str = "Review what will be created";
pub const CREATE: &str = "Create";
pub const NONE: &str = "none";
pub const ROW_NAME: &str = "name";
pub const ROW_PROVIDER: &str = "provider";
pub const ROW_REMOTE: &str = "github repo";
pub const ROW_IMAGE: &str = "image";
pub const ROW_BASE: &str = "base";
pub const ROW_WORKFLOWS: &str = "workflows";
pub const ROW_PUBLISH: &str = "publish";
pub const ROW_SCANS: &str = "image scans";
pub const ROW_DAILY: &str = "daily build";
pub const REMOTE_MADE: &str = "created on push";
pub const REMOTE_NOT: &str = "not created";
pub const ON_EVERY_PUSH: &str = "on every push";
pub const ON_EVERY_BUILD: &str = "on every build";
pub const ON_SCHEDULED: &str = "on scheduled builds only";

pub const INSTALL: &str = "Erase and install";
pub const ROW_DISK: &str = "disk";
pub const ROW_HOSTNAME: &str = "machine name";
pub const ROW_ACCOUNT: &str = "user name";
pub const ROW_PASSWORD: &str = "password";
pub const ROW_ENCRYPTION: &str = "encryption";
pub const PASSWORD_SET: &str = "set";

#[cfg(test)]
mod tests {
    #[test]
    fn every_string_here_is_one_short_line() {
        for line in include_str!("copy.rs").lines() {
            let Some(rest) = line.strip_prefix("pub const ") else {
                continue;
            };
            assert!(rest.ends_with("\";"), "{line}");
            let text = rest.split_once("= \"").expect("a string literal").1;
            assert!(text.chars().count() - 2 < 60, "{line}");
        }
    }
}
