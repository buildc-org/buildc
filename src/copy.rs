pub const PRODUCT: &str = "Buildc";

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
pub const IMAGE_BOOTLOADER: &str = "Which bootloader does it install?";
pub const BOOTLOADER_GRUB2: &str = "GRUB";
pub const BOOTLOADER_SYSTEMD: &str = "systemd-boot";
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

pub const INSTALL_DISK: &str = "Installation disk";
pub const INSTALL_NAME: &str = "Computer name";
pub const INSTALL_USER: &str = "Username";
pub const INSTALL_PASSWORD: &str = "Password";
pub const INSTALL_ENCRYPTION: &str = "Encryption type";
pub const LUKS_PASSPHRASE: &str = "LUKS passphrase";
pub const ENC_NONE: &str = "none";
pub const ENC_TPM2: &str = "luks with TPM2 decryption and recovery key";
pub const ENC_PASSPHRASE: &str = "luks with passphrase decryption";
pub const ENC_BOTH: &str = "luks with TPM2 decryption and recovery passphrase";
pub const ENC_ANY_HOLDER: &str = "opens for anyone who has the machine";
pub const ENC_ONLY_YOU: &str = "nothing opens it without you";
pub const NO_TPM: &str = "No TPM available";
pub const REMOVABLE: &str = "removable";

pub const DATA_HERE: &str = "this disk, sized";
pub const DATA_ERASED: &str = "erased";
pub const DATA_KEPT: &str = "keep what is on it";
pub const DATA_UNENCRYPTED: &str = "an unencrypted home under an encrypted root";
pub const DATA_SAME_DISK: &str = "that is the disk this installs to";
pub const CUSTOM_ENCRYPTION: &str = "custom layouts cannot create encryption";
pub const CUSTOM_OTHER_DISK: &str = "the custom layout belongs to another disk";
pub const CUSTOM_ROOT: &str = "still needs a / partition";
pub const CUSTOM_DUPLICATE: &str = "each mount point can be used only once";
pub const CUSTOM_NEEDS_DISK: &str = "choose a disk before editing its partitions";
pub const CUSTOM_FORMAT_FILESYSTEM: &str = "fisherman cannot format this filesystem";
pub const CUSTOM_KEEP_FILESYSTEM: &str = "not a supported Linux filesystem here";
pub const CUSTOM_KEEP_ESP: &str = "an EFI system partition must use FAT";
pub const LEAVE_PARTITION: &str = "leave unchanged";

pub fn open_at(target: &str) -> String {
    format!("open it as {target}")
}

pub const CUSTOM_OPEN_BOOT: &str = "the bootloader cannot read an encrypted /boot";

pub fn open_cost(target: &str) -> &'static str {
    match target {
        "/boot" => "decrypt it, and /boot has to be ext4",
        _ => "decrypt it and keep its filesystem",
    }
}

pub fn open_question(partition: &str, target: &str) -> String {
    format!("How does {partition} open as {target}?")
}

pub fn opened(partition: &str) -> String {
    format!("{partition}  decrypted and kept")
}

pub const OPEN_WITH: &str = "opens with";
pub const USE_KEY: &str = "Open it";
pub const KEY_PASSPHRASE: &str = "passphrase";
pub const KEY_PASSPHRASE_COST: &str = "typed here, and asked for again at boot";
pub const KEY_FILE: &str = "key file";
pub const KEY_FILE_COST: &str = "a file this live system can read";
pub const KEY_FILE_PATH: &str = "key file path";
pub const KEY_FILE_MISSING: &str = "there is no file at that path";

pub const OLD_SYSTEM: &str = "old system";
pub const OLD_ROOT_NONE: &str = "no partition on this disk reads as an old system";
pub fn old_root_unnamed(partition: &str) -> String {
    format!("no old system on this disk names a key for {partition}")
}
pub fn old_root_partly(unread: &str) -> String {
    format!("{unread}, and nothing readable named a key")
}
pub fn old_root_key_missing(volume: &str, at: &str, why: &str) -> String {
    format!("the key {at} that opens {volume} could not be read: {why}")
}
pub fn old_root_key_outside(at: &str) -> String {
    format!("the key {at} is not a file inside the system that names it")
}
pub fn old_root_key_wrong(partition: &str) -> String {
    format!("the key the old system named for {partition} does not open it")
}
pub fn old_root_key_unreadable(volume: &str) -> String {
    format!("{volume} opens with a script this installer cannot run")
}

pub fn custom_keep_boot(bootloader: &str) -> String {
    match bootloader {
        "systemd" => "systemd-boot does not use a separate /boot".to_string(),
        _ => format!("{bootloader} installs with a supported ext4 /boot"),
    }
}

pub fn on_disk(disk: &str, how: &str) -> String {
    format!("{disk}, {how}")
}

pub fn erasing(disk: &str) -> String {
    format!("Everything on {disk} will be erased. Are you sure?")
}

pub fn changing_partitions(disk: &str) -> String {
    format!("Partitions marked format on {disk} will be erased. Are you sure?")
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
pub const INSTALL_KEYS: &str = "\u{2191}\u{2193} move, enter change, esc leave";
pub const FORM_KEYS: &str = "up and down to move, enter to change a field";
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

pub const INSTALL: &str = "Install";
pub const SHUT_DOWN: &str = "Shut down";
pub const CONTINUE: &str = "Continue";
pub const GO_BACK: &str = "Go back";
pub const USE_LAYOUT: &str = "Use layout";
pub const AUTOMATIC_LAYOUT: &str = "Use automatic layout";
pub const ROW_CONFIRM: &str = "password (confirm)";
pub const ROW_LAYOUT: &str = "layout";

pub fn layout(filesystem: &str, bootloader: &str, boot: &str) -> String {
    let partitions = match bootloader {
        "systemd" => format!("esp + {filesystem} root"),
        _ => format!("esp + ext4 /boot + {filesystem} root"),
    };
    match boot_chain(boot) {
        Some(chain) => format!("{partitions}; {chain}"),
        None => partitions,
    }
}

pub fn custom_layout(disk: &str, mounts: usize, kept: usize, opened: usize) -> String {
    let said = format!("custom on {disk}: {mounts} mounted partitions, {kept} kept");
    match opened {
        0 => said,
        opened => format!("{said}, {opened} opened"),
    }
}

pub fn keep_as(target: &str) -> String {
    format!("keep as {target}")
}

pub fn format_at(filesystem: &str, target: &str) -> String {
    format!("format as {filesystem} at {target}")
}

pub fn boot_chain(boot: &str) -> Option<&'static str> {
    match boot {
        "uki-shim" => Some("owner-signed UKI through Microsoft shim"),
        "uki-db" => Some("owner-signed UKI through firmware keys"),
        _ => None,
    }
}

pub fn written_over(
    bootloader: &str,
    filesystem: &str,
    var: &str,
    boot: &str,
) -> Vec<(String, String)> {
    let mut rows = vec![("esp".to_string(), "2 GB  fat32".to_string())];
    if let Some(chain) = boot_chain(boot) {
        rows.push(("boot chain".to_string(), chain.to_string()));
    }
    if bootloader != "systemd" {
        rows.push(("/boot".to_string(), "2 GB  ext4".to_string()));
    }
    if !var.is_empty() {
        rows.push(("/var".to_string(), format!("{var}  {filesystem}")));
    }
    rows.push(("root".to_string(), format!("the rest  {filesystem}")));
    rows
}
pub const NO_MATCH_ROW: &str = "the passwords do not match";
pub const ROW_DISK: &str = "disk";
pub const ROW_HOSTNAME: &str = "computer name";
pub const ROW_ACCOUNT: &str = "username";
pub const ROW_PASSWORD: &str = "password";
pub const ROW_ENCRYPTION: &str = "encryption";
pub const ROW_PASSPHRASE: &str = "passphrase";
pub const ROW_DATA: &str = "home and data";
pub const ROW_SIZE: &str = "size";
pub const PASSWORD_SET: &str = "set";
pub const NOT_SET: &str = "not set";

pub fn still_needs(fields: &[&str]) -> String {
    format!("still needs a {}", fields.join(", a "))
}

pub fn installing(image: &str) -> String {
    format!("{PRODUCT} installer \u{2014} {image}")
}

pub fn logging(log: Option<&std::path::Path>) -> String {
    match log {
        Some(at) => format!("the install log is at {}", at.display()),
        None => "nothing here is writable, so this screen is the only copy".to_string(),
    }
}

pub fn writing(log: Option<&std::path::Path>) -> String {
    match log {
        Some(at) => format!("no going back from here \u{2014} log: {}", at.display()),
        None => "no going back from here \u{2014} and this screen is the log".to_string(),
    }
}

pub fn recovery(key: &str) -> String {
    format!("{WRITE_DOWN} {key}")
}

pub const WRITE_DOWN: &str = "write this down, it is the recovery key:";

pub const LEAVING: &str = "Leave the installer?";
pub const LEAVE_BACK: &str = "Keep going";
pub const LEAVE_OVER: &str = "Start the form again";
pub const LEAVE_SHELL: &str = "Quit the installer";
pub const INSTALL_DONE: &str = "Installation Complete!";
pub const RESTART: &str = "Restart now";
pub const DONE_KEYS: &str = "enter to restart, esc to quit the installer";
pub fn enrollment(boot: &str) -> Option<&'static str> {
    match boot {
        "uki-shim" => Some("on first boot: use MokManager to enroll EFI/BOOT/MOK.cer"),
        "uki-db" => Some("before restart: clear the platform key in firmware setup"),
        _ => None,
    }
}
pub const KEY_NOT_LOGGED: &str = "It is not in the install log.";

#[cfg(test)]
mod tests {
    #[test]
    fn the_cost_names_the_disk_and_asks() {
        let said = super::erasing("/dev/vda");
        assert!(said.contains("/dev/vda"), "{said}");
        assert!(said.ends_with('?'), "{said}");
    }

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

    #[test]
    fn uki_chains_are_visible_before_and_after_install() {
        assert!(super::layout("ext4", "systemd", "uki-shim").contains("Microsoft shim"));
        assert!(super::written_over("systemd", "ext4", "", "uki-db")
            .iter()
            .any(|(name, value)| name == "boot chain" && value.contains("firmware keys")));
        assert!(super::enrollment("uki-db")
            .is_some_and(|instruction| instruction.contains("clear the platform key")));
        assert!(super::enrollment("uki-shim")
            .is_some_and(|instruction| instruction.contains("EFI/BOOT/MOK.cer")));
    }
}
