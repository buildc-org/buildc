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
pub const LIBRARY_ELSEWHERE: &str = "URL / local path";
pub const LIBRARY_LOCATION: &str = "library location";
pub const LIBRARY_LOCATION_HINT: &str = "URL or local path to the library";
pub const LIBRARY_NAME: &str = "library name";
pub const LIBRARY_NAME_HINT: &str = "lowercase, digits and dashes";
pub const ADD_LIBRARY: &str = "Add this library?";

pub fn create_local_library(folder: &str) -> String {
    format!("The default location for the local library is '{folder}'. Would you like to create it now?")
}

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

pub const COMPONENT_NAME: &str = "component name";
pub const COMPONENT_DESCRIPTION: &str = "description (optional)";
pub const COMPONENT_SUPPORTS: &str = "supported base families, separated by spaces (optional)";
pub const PACKAGE_NAMES: &str = "packages";
pub const PACKAGE_NAMES_HINT: &str = "separated by a space";
pub const GROUP_NAME: &str = "group name";
pub fn group_name_hint() -> String {
    format!("enter for default ({DEFAULT_GROUP})")
}
pub const DEFAULT_GROUP: &str = "apps";
pub const SELECT_COMPONENT: &str = "select component";
pub const COMPONENT_INSIDE: &str = "component that holds it";
pub const ADD_PACKAGES: &str = "Add packages";
pub const WHICH_COMPONENT: &str = "component";
pub const WHICH_COMPONENTS: &str = "components";
pub const COPY_INTO: &str = "library to copy into";
pub const ADD_TO: &str = "add to";
pub const SHOW_ALL_COMPONENTS: &str = "show all components";

pub fn which_library(path: &str) -> String {
    format!("library to read `{path}` from")
}
pub const COPY_COMPONENTS: &str = "Copy the components?";
pub const LIST_IN_BUILDS: &str = "builds that list it";

pub fn list_in(target: &str) -> String {
    format!("List it in {target}?")
}

pub fn package_group_builds(name: &str) -> String {
    format!("select build to add \"{name}\" package group")
}

pub const TARGET_IMAGE: &str = "image";
pub const TARGET_BUILD: &str = "build";
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

pub const REQUIRED: &str = "Dependencies the images do not provide:";
pub const BRING_REQUIRED: &str = "Add these provider components?";

pub fn which_provider(component: &str, capability: &str) -> String {
    format!("provider of `{capability}` for `{component}`")
}
pub const BRING_FOR_BASE: &str = "Add these components?";
pub const BRING_CLAIMING: &str = "components that claim rules in this profile";
pub const ADD_REFERENCE: &str = "Add a reference";
pub const COPY_INTO_REPOSITORY: &str = "Copy into the repository";
pub const PIN: &str = "pin";
pub const PIN_FOLLOW: &str = "Follow the library version";
pub const PIN_CURRENT: &str = "Pin to the current commit";
pub const PIN_TAG: &str = "Pin to a tag…";
pub const PIN_COMMIT: &str = "Pin to a commit…";
pub const PIN_TAG_NAME: &str = "tag";
pub const PIN_COMMIT_NAME: &str = "commit";

pub fn cleared(named: &str, unmet: &str) -> String {
    format!("{named} was cleared: it {unmet}")
}

pub const GENERATE_WORKFLOWS: &str = "Generate the workflows these components unlock?";

pub const WORKFLOWS: &str = "workflows";

pub fn workflow_builds(workflow: &str) -> String {
    format!("builds {workflow} runs for")
}

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
pub const ROW_SETUP: &str = "setup mode";
pub const REPO_SETUP: &str = "repo setup";
pub const SETUP_DEFAULTS: &str = "sensible defaults";
pub const SETUP_DEFAULTS_ABOUT: &str = "fill every row that has a default";
pub const SETUP_MINIMAL: &str = "minimal repo";
pub const SETUP_MINIMAL_ABOUT: &str = "write only the tree, git and repo.kdl";
pub const SET_PROVIDER: &str = "`buildc create image --owner`";
pub const SET_IMAGE: &str = "`buildc create image`";
pub const SET_WORKFLOWS_COMMAND: &str = "`buildc set workflows`";
pub const SET_SCRIPTS_COMMAND: &str = "`buildc create scripts`";
pub const ROW_PROVIDER: &str = "provider";
pub const ROW_REMOTE: &str = "github repo";
pub const ROW_LIBRARIES: &str = "libraries";
pub const ROW_IMAGE: &str = "image";
pub const ROW_BASE: &str = "base";
pub const ROW_WORKFLOWS: &str = "workflows";
pub const ROW_SCRIPTS: &str = "scripts kept";
pub const REMOTE_MADE: &str = "created on push";
pub const REMOTE_NOT: &str = "not created";

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
