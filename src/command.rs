use crate::emit::schema_md::Area;
use clap::{ArgMatches, ColorChoice, CommandFactory, Parser, Subcommand};
use common::ui::Choice;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

mod help;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verb {
    Upgrade,
    CreateRepo,
    CreateGit,
    CreateImage,
    CreateFlavour,
    CreateModule,
    CreateKey,
    CreateScripts,
    ImportModule,
    SetWorkflows,
    SetLibrary,
    SetConforms,
    SetClaims,
    SetKey,
    SetModule,
    Check,
    Generate,
    Build,
    VmBuild,
    VmRun,
    VmSpawn,
    MediaBuild,
    MediaRun,
    Section,
    Graph,
    Why,
    Coverage,
    Plan,
    Verify,
    Summary,
    Sbom,
    FetchModules,
    Scap,
    ScapContent,
    ScapRules,
    ScapTailoring,
    RegistryNamespace,
    RegistryRef,
    Recipe,
    BuildRecord,
    Fetch,
    ValidateImage,
    Validate,
    Attribute,
}

impl Verb {
    pub fn schema(self) -> Option<(&'static str, Area, &'static str)> {
        match self {
            Self::CreateRepo => Some(("repository&nbsp;layout", Area::Repository, "")),
            Self::CreateImage => Some(("`image.kdl`", Area::Image, "")),
            Self::CreateFlavour => Some((
                "`image.kdl`&nbsp;›&nbsp;`flavours`",
                Area::Image,
                "image-flavours",
            )),
            Self::CreateModule => Some(("`manifest.kdl`", Area::Component, "")),
            Self::CreateKey | Self::SetKey => {
                Some(("`manifest.kdl`&nbsp;›&nbsp;`key`", Area::Component, "key"))
            }
            Self::CreateScripts => Some(("`scripts/`", Area::Repository, "")),
            Self::ImportModule | Self::FetchModules | Self::SetModule => {
                Some(("`components.lock.kdl`", Area::Lock, ""))
            }
            Self::SetWorkflows => Some((
                "`repo.kdl`&nbsp;›&nbsp;`workflows`",
                Area::Repo,
                "workflows",
            )),
            Self::SetLibrary => Some(("`repo.kdl`&nbsp;›&nbsp;`sources`", Area::Repo, "sources")),
            Self::SetConforms | Self::Coverage => Some((
                "`image.kdl`&nbsp;›&nbsp;`conforms`",
                Area::Image,
                "image-conforms",
            )),
            Self::SetClaims => Some((
                "`manifest.kdl`&nbsp;›&nbsp;`satisfies`",
                Area::Component,
                "satisfies",
            )),
            Self::Generate => Some(("`generated/`", Area::Repository, "")),
            Self::Fetch => Some((
                "`manifest.kdl`&nbsp;›&nbsp;`asset`",
                Area::Component,
                "asset",
            )),
            Self::ValidateImage => Some((
                "`manifest.kdl`&nbsp;›&nbsp;`allow-verify`",
                Area::Component,
                "allow-verify",
            )),
            Self::Upgrade
            | Self::Check
            | Self::Build
            | Self::VmBuild
            | Self::VmRun
            | Self::VmSpawn
            | Self::MediaBuild
            | Self::MediaRun
            | Self::Section
            | Self::Graph
            | Self::Why
            | Self::Plan
            | Self::Verify
            | Self::Summary
            | Self::Sbom
            | Self::Scap
            | Self::ScapContent
            | Self::ScapRules
            | Self::ScapTailoring
            | Self::RegistryNamespace
            | Self::RegistryRef
            | Self::Recipe
            | Self::BuildRecord
            | Self::Validate
            | Self::Attribute
            | Self::CreateGit => None,
        }
    }
}

pub const ALL: &[Verb] = &[
    Verb::Upgrade,
    Verb::CreateRepo,
    Verb::CreateGit,
    Verb::CreateImage,
    Verb::CreateFlavour,
    Verb::CreateModule,
    Verb::CreateKey,
    Verb::CreateScripts,
    Verb::ImportModule,
    Verb::SetWorkflows,
    Verb::SetLibrary,
    Verb::SetConforms,
    Verb::SetClaims,
    Verb::SetKey,
    Verb::SetModule,
    Verb::Check,
    Verb::Generate,
    Verb::Build,
    Verb::VmBuild,
    Verb::VmRun,
    Verb::VmSpawn,
    Verb::MediaBuild,
    Verb::MediaRun,
    Verb::Section,
    Verb::Graph,
    Verb::Why,
    Verb::Coverage,
    Verb::Plan,
    Verb::Verify,
    Verb::Summary,
    Verb::Sbom,
    Verb::FetchModules,
    Verb::Scap,
    Verb::ScapContent,
    Verb::ScapRules,
    Verb::ScapTailoring,
    Verb::RegistryNamespace,
    Verb::RegistryRef,
    Verb::Recipe,
    Verb::BuildRecord,
    Verb::Fetch,
    Verb::ValidateImage,
    Verb::Validate,
    Verb::Attribute,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    Anywhere,
    Repo,
    Script,
    Layer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Topic {
    Tool,
    Repository,
    Image,
    Components,
    Keys,
    Check,
    Build,
    Ci,
    Inspect,
    Audit,
    Layer,
}

pub const TOPICS: [Topic; 11] = [
    Topic::Repository,
    Topic::Image,
    Topic::Components,
    Topic::Keys,
    Topic::Check,
    Topic::Build,
    Topic::Ci,
    Topic::Inspect,
    Topic::Audit,
    Topic::Layer,
    Topic::Tool,
];

impl Topic {
    pub fn heading(self) -> &'static str {
        match self {
            Self::Tool => "CLI tool",
            Self::Repository => "Repository",
            Self::Image => "Image",
            Self::Components => "Components",
            Self::Keys => "Keys",
            Self::Check => "Check and generate",
            Self::Build => "Build and boot",
            Self::Ci => "CI",
            Self::Inspect => "Inspect",
            Self::Audit => "Audit",
            Self::Layer => "Inside a build",
        }
    }
}

const SURFACE: &[(&str, Verb, Family, bool, Topic)] = &[
    (
        "upgrade",
        Verb::Upgrade,
        Family::Anywhere,
        false,
        Topic::Tool,
    ),
    (
        "create repo",
        Verb::CreateRepo,
        Family::Anywhere,
        false,
        Topic::Repository,
    ),
    (
        "create git",
        Verb::CreateGit,
        Family::Anywhere,
        false,
        Topic::Repository,
    ),
    (
        "create image",
        Verb::CreateImage,
        Family::Repo,
        false,
        Topic::Image,
    ),
    (
        "create flavour",
        Verb::CreateFlavour,
        Family::Repo,
        false,
        Topic::Image,
    ),
    (
        "create component",
        Verb::CreateModule,
        Family::Repo,
        false,
        Topic::Components,
    ),
    (
        "create key",
        Verb::CreateKey,
        Family::Repo,
        false,
        Topic::Keys,
    ),
    (
        "create scripts",
        Verb::CreateScripts,
        Family::Repo,
        false,
        Topic::Repository,
    ),
    (
        "import component",
        Verb::ImportModule,
        Family::Repo,
        false,
        Topic::Components,
    ),
    (
        "set workflows",
        Verb::SetWorkflows,
        Family::Repo,
        false,
        Topic::Ci,
    ),
    (
        "set library",
        Verb::SetLibrary,
        Family::Repo,
        false,
        Topic::Repository,
    ),
    (
        "set conforms",
        Verb::SetConforms,
        Family::Repo,
        false,
        Topic::Audit,
    ),
    (
        "set claims",
        Verb::SetClaims,
        Family::Repo,
        false,
        Topic::Audit,
    ),
    ("set key", Verb::SetKey, Family::Repo, false, Topic::Keys),
    (
        "set component",
        Verb::SetModule,
        Family::Repo,
        false,
        Topic::Components,
    ),
    ("check", Verb::Check, Family::Repo, false, Topic::Check),
    (
        "generate",
        Verb::Generate,
        Family::Repo,
        false,
        Topic::Check,
    ),
    ("build", Verb::Build, Family::Repo, false, Topic::Build),
    ("vm build", Verb::VmBuild, Family::Repo, false, Topic::Build),
    ("vm run", Verb::VmRun, Family::Repo, false, Topic::Build),
    ("vm spawn", Verb::VmSpawn, Family::Repo, false, Topic::Build),
    (
        "media build",
        Verb::MediaBuild,
        Family::Repo,
        false,
        Topic::Build,
    ),
    (
        "media run",
        Verb::MediaRun,
        Family::Repo,
        false,
        Topic::Build,
    ),
    (
        "section",
        Verb::Section,
        Family::Repo,
        false,
        Topic::Inspect,
    ),
    ("graph", Verb::Graph, Family::Repo, false, Topic::Inspect),
    ("why", Verb::Why, Family::Repo, true, Topic::Inspect),
    (
        "coverage",
        Verb::Coverage,
        Family::Repo,
        false,
        Topic::Audit,
    ),
    ("plan", Verb::Plan, Family::Script, true, Topic::Inspect),
    ("verify", Verb::Verify, Family::Script, false, Topic::Check),
    (
        "summary",
        Verb::Summary,
        Family::Script,
        true,
        Topic::Inspect,
    ),
    ("sbom", Verb::Sbom, Family::Script, false, Topic::Inspect),
    (
        "fetch components",
        Verb::FetchModules,
        Family::Repo,
        false,
        Topic::Components,
    ),
    (
        "scap content",
        Verb::ScapContent,
        Family::Script,
        true,
        Topic::Audit,
    ),
    (
        "scap rules",
        Verb::ScapRules,
        Family::Script,
        true,
        Topic::Audit,
    ),
    (
        "scap tailoring",
        Verb::ScapTailoring,
        Family::Script,
        false,
        Topic::Audit,
    ),
    ("scap", Verb::Scap, Family::Script, false, Topic::Audit),
    (
        "registry namespace",
        Verb::RegistryNamespace,
        Family::Script,
        false,
        Topic::Inspect,
    ),
    (
        "registry ref",
        Verb::RegistryRef,
        Family::Script,
        false,
        Topic::Inspect,
    ),
    (
        "recipe",
        Verb::Recipe,
        Family::Script,
        false,
        Topic::Inspect,
    ),
    (
        "build-record",
        Verb::BuildRecord,
        Family::Layer,
        false,
        Topic::Layer,
    ),
    ("fetch", Verb::Fetch, Family::Layer, false, Topic::Layer),
    (
        "validate-image",
        Verb::ValidateImage,
        Family::Layer,
        false,
        Topic::Layer,
    ),
    (
        "validate",
        Verb::Validate,
        Family::Repo,
        false,
        Topic::Build,
    ),
    (
        "attribute",
        Verb::Attribute,
        Family::Repo,
        false,
        Topic::Build,
    ),
];

#[derive(Parser)]
#[command(
    name = "buildc",
    about = "manage a repository of bootc images",
    long_about = "buildc manages a bootc image repository. With no command, it opens a picker \
                  of the commands that run where it is typed. If no terminal is watching, then it \
                  lists those commands instead.",
    after_long_help = help::ROOT_NOTES,
    disable_help_subcommand = true,
    disable_version_flag = true,
    color = ColorChoice::Never,
    // A global is listed after every command's own flags.
    next_display_order = 100
)]
pub struct Cli {
    /// the repository root; the nearest `repo.kdl` at or above the working directory if absent
    #[arg(long, global = true, value_name = "dir")]
    pub root: Option<PathBuf>,
    /// ask no questions; use flags and defaults, and fail when an answer has no default
    #[arg(long = "no-tui", global = true)]
    pub no_tui: bool,
    #[command(subcommand)]
    pub verb: Option<Surface>,
}

// The tree every word is parsed by. A word whose forms all take a noun parses
// with `what` absent, which is the picker's half. A `///` here would become the
// root command's help.
#[derive(Subcommand)]
pub enum Surface {
    /// replace this buildc and its assets with the latest release
    #[command(long_about = help::UPGRADE, after_long_help = help::UPGRADE_NOTES)]
    Upgrade,
    /// write a repository and add images, components, keys or scripts to it
    Create {
        #[command(subcommand)]
        what: Option<CreateWhat>,
    },
    /// copy a component from a declared library into the repository
    Import {
        #[command(subcommand)]
        what: Option<ImportWhat>,
    },
    /// change a repository setting or record an existing key
    Set {
        #[command(subcommand)]
        what: Option<SetWhat>,
    },
    /// check every manifest and report each problem at its line
    #[command(long_about = help::CHECK, after_long_help = help::CHECK_NOTES)]
    Check {
        /// SCAP content to count each profile's unclaimed rules against
        #[arg(long, value_name = "file")]
        datastream: Option<PathBuf>,
    },
    /// write the build files and list what was written
    #[command(long_about = help::GENERATE, after_long_help = help::GENERATE_NOTES)]
    Generate,
    /// build one target with the container backend
    #[command(long_about = help::BUILD, after_long_help = help::BUILD_NOTES)]
    Build {
        /// the target
        #[arg(value_name = "target")]
        target_arg: Option<String>,
        /// the target, as an alternative to the positional argument
        #[arg(long, value_name = "target")]
        target: Option<String>,
        /// an extra tag for the built image
        #[arg(long, value_name = "tag")]
        tag: Vec<String>,
        /// the kernel version to install
        #[arg(long, value_name = "name")]
        kernel: Option<String>,
        /// the container backend: `buildx` or `buildah`
        #[arg(long, value_name = "name")]
        backend: Option<String>,
        /// write an OCI archive instead of loading the image; needs `--backend buildx`
        #[arg(long = "oci-output", value_name = "path")]
        oci_output: Option<String>,
        /// a build secret as `<id>=<path>`
        #[arg(long, value_name = "id=path")]
        secret: Vec<String>,
        /// export the layer cache to the registry
        #[arg(long = "cache-to")]
        cache_to: bool,
        /// build without importing the registry layer cache
        #[arg(long = "no-cache-from")]
        no_cache_from: bool,
    },
    /// turn the built image into a disk and boot it
    #[command(long_about = help::VM, after_long_help = help::VM_NOTES)]
    Vm {
        #[command(subcommand)]
        what: Option<VmWhat>,
    },
    /// assemble the installer media and boot it
    #[command(long_about = help::MEDIA, after_long_help = help::MEDIA_NOTES)]
    Media {
        #[command(subcommand)]
        what: Option<MediaWhat>,
    },
    /// print one image's generated Containerfile section
    #[command(long_about = help::SECTION)]
    Section {
        /// the image; the default image if absent
        #[arg(value_name = "image")]
        image: Option<String>,
    },
    /// print the capability graph and what the base already carries
    #[command(long_about = help::GRAPH)]
    Graph {
        /// `markdown` with a mermaid diagram, or `json`; `markdown` is the default
        #[arg(long, value_name = "md|json")]
        format: Option<String>,
    },
    /// print one component's provenance and contents
    #[command(long_about = help::WHY, after_long_help = help::WHY_NOTES)]
    Why {
        /// the component to explain
        #[arg(value_name = "component")]
        component: Option<String>,
        /// `markdown` or `json`; `markdown` is the default
        #[arg(long, value_name = "md|json")]
        format: Option<String>,
    },
    /// print which component claims each rule of the image's profile
    #[command(long_about = help::COVERAGE, after_long_help = help::COVERAGE_NOTES)]
    Coverage {
        /// the image; the default image if absent, or a picker in a terminal
        #[arg(value_name = "image")]
        image: Option<String>,
        /// `markdown` or `json`; `markdown` is the default
        #[arg(long, value_name = "md|json")]
        format: Option<String>,
        /// SCAP content to read the profile from
        #[arg(long, value_name = "file")]
        datastream: Option<PathBuf>,
    },
    /// print every fact the repository derives as JSON
    #[command(long_about = help::PLAN)]
    Plan {
        /// no effect; `plan` always prints JSON
        #[arg(long)]
        json: bool,
    },
    /// compare every build file byte for byte with the one buildc writes
    #[command(long_about = help::VERIFY)]
    Verify,
    /// check one built image with the tool that built it
    #[command(long_about = help::VALIDATE, after_long_help = help::VALIDATE_NOTES)]
    Validate {
        /// the built image reference or id
        #[arg(value_name = "image", required = true)]
        image: Option<String>,
        /// the target; the default target if absent
        #[arg(long, value_name = "target")]
        target: Option<String>,
    },
    /// map each layer of a built image to its component
    #[command(long_about = help::ATTRIBUTE, after_long_help = help::ATTRIBUTE_NOTES)]
    Attribute {
        /// the built image reference or id
        #[arg(value_name = "image", required = true)]
        image: Option<String>,
        /// the target whose plan maps each layer to its paths
        #[arg(long, value_name = "target")]
        target: Option<String>,
    },
    /// print a table of what one target is made of
    #[command(long_about = help::SUMMARY)]
    Summary {
        /// the target
        #[arg(value_name = "target")]
        target: Option<String>,
    },
    /// print one target's pinned payloads as SPDX packages
    #[command(long_about = help::SBOM)]
    Sbom {
        /// the target
        #[arg(value_name = "target")]
        target: Option<String>,
    },
    /// print the findings of one scan for the target
    #[command(long_about = help::SCAP, after_long_help = help::SCAP_NOTES)]
    Scap {
        #[command(subcommand)]
        sub: Option<ScapWhat>,
        /// the ARF report the scan wrote
        #[arg(value_name = "arf.xml")]
        arf: Option<String>,
        /// the target; the image's own target if absent
        #[arg(long, value_name = "target")]
        target: Option<String>,
        /// SCAP content; the file `scap content` names by default
        #[arg(long, value_name = "file")]
        datastream: Option<PathBuf>,
        /// the file that holds the previous scan's pass set
        #[arg(long, value_name = "file")]
        baseline: Option<PathBuf>,
        /// a scan of the bare base to compare against
        #[arg(long = "base-scan", value_name = "file")]
        base_scan: Option<PathBuf>,
    },
    /// download one payload, verify it and place it
    #[command(long_about = help::FETCH, subcommand_negates_reqs = true)]
    Fetch {
        #[command(subcommand)]
        sub: Option<FetchWhat>,
        /// the payload kind: `file`, `tree`, `bin`, `rpm` or `deb`
        #[arg(value_name = "what", required = true)]
        what: Option<String>,
        /// the URL to download
        #[arg(value_name = "url", required = true)]
        url: Option<String>,
        /// the expected SHA-256 of the download
        #[arg(value_name = "sha256", required = true)]
        sha256: Option<String>,
        /// the destination that the payload kind writes to
        #[arg(value_name = "target")]
        target: Option<String>,
        /// extra arguments for the payload kind
        #[arg(num_args(0..), value_name = "extra")]
        extra: Vec<String>,
    },
    /// print the registry namespace or a full image reference
    Registry {
        #[command(subcommand)]
        what: Option<RegistryWhat>,
    },
    /// print the install recipe that the repository answers
    #[command(long_about = help::RECIPE, after_long_help = help::RECIPE_NOTES)]
    Recipe {
        /// the target; the image's own target if absent
        #[arg(long, value_name = "target")]
        target: Option<String>,
        /// the tag the reference publishes under
        #[arg(long, value_name = "tag")]
        tag: Vec<String>,
        /// the local image to install in place of the published reference
        #[arg(long, value_name = "ref")]
        image: Vec<String>,
    },
    /// write the build record that an image carries
    #[command(long_about = help::BUILD_RECORD)]
    BuildRecord,
    /// run every check that a built image must pass
    #[command(long_about = help::VALIDATE_IMAGE)]
    ValidateImage,
}

#[derive(Subcommand)]
pub enum CreateWhat {
    /// write a repository and its first image
    #[command(long_about = help::CREATE_REPO, after_long_help = help::CREATE_REPO_NOTES)]
    Repo {
        /// the repository directory name
        #[arg(value_name = "name")]
        name: Option<String>,
        /// the repository host; `github.com` by default, and with `--owner` it forms `base-url`
        #[arg(long, value_name = "domain")]
        host: Option<String>,
        /// the account or organisation that owns the repository; with `--host` it forms `base-url`
        #[arg(long, value_name = "name")]
        owner: Option<String>,
        /// also write a first image with this name
        #[arg(long, value_name = "name")]
        image: Vec<String>,
        /// the base image reference for the first image
        #[arg(long, value_name = "ref")]
        base: Option<String>,
    },
    /// put a directory under git control
    #[command(long_about = help::CREATE_GIT, after_long_help = help::CREATE_GIT_NOTES)]
    Git,
    /// add an image and choose its base
    #[command(long_about = help::CREATE_IMAGE, after_long_help = help::CREATE_IMAGE_NOTES)]
    Image {
        /// the image name
        #[arg(value_name = "name")]
        name: Option<String>,
        /// the account or organisation for the seeded home URL; ignored when repo.kdl declares base-url
        #[arg(long, value_name = "name")]
        owner: Option<String>,
        /// a bootc image reference, catalogued or not; skips the base picker
        #[arg(long, value_name = "ref")]
        base: Option<String>,
    },
    /// add a second build of an image with more components
    #[command(long_about = help::CREATE_FLAVOUR, after_long_help = help::CREATE_FLAVOUR_NOTES)]
    Flavour {
        /// the flavour name
        #[arg(value_name = "name")]
        name: Option<String>,
        /// the image that publishes the flavour
        #[arg(long, value_name = "name")]
        image: Vec<String>,
    },
    /// write a new component in components/ or inline in an image
    #[command(long_about = help::CREATE_COMPONENT, after_long_help = help::CREATE_COMPONENT_NOTES)]
    Component {
        /// the component name or path
        #[arg(value_name = "name")]
        name: Option<String>,
        /// an image that lists the component
        #[arg(long, value_name = "name")]
        image: Vec<String>,
        /// a package the component installs
        #[arg(long, value_name = "name")]
        pkg: Vec<String>,
        /// one more manifest line, such as `provides=browser`; repeatable
        #[arg(long, value_name = "verb=value")]
        with: Vec<String>,
        /// write the component as a block in one image file
        #[arg(long)]
        inline: bool,
    },
    /// generate a key that a component declares
    #[command(long_about = help::CREATE_KEY, after_long_help = help::CREATE_KEY_NOTES)]
    Key {
        /// the kind of key that a component declares
        #[arg(value_name = "kind")]
        kind: Option<String>,
        /// the component that declares the key
        #[arg(long, value_name = "name")]
        component: Option<String>,
        /// the common name of a generated certificate
        #[arg(long, value_name = "name")]
        cn: Option<String>,
    },
    /// copy the scripts that buildc supplies into scripts/
    #[command(long_about = help::CREATE_SCRIPTS, after_long_help = help::CREATE_SCRIPTS_NOTES)]
    Scripts {
        /// the scripts to keep
        #[arg(value_name = "name")]
        names: Vec<String>,
    },
}

#[derive(Subcommand)]
pub enum ImportWhat {
    /// copy a component from a declared library into this repository
    #[command(long_about = help::IMPORT_COMPONENT, after_long_help = help::IMPORT_COMPONENT_NOTES)]
    Component {
        /// the component to copy or `<owner>/<name>` to choose a library
        #[arg(value_name = "name")]
        name: Option<String>,
        /// the image or flavour that lists the component
        #[arg(long, value_name = "name")]
        image: Option<String>,
        /// SCAP content for the profile offer; the family's installed copy by default, and no content means no offer
        #[arg(long, value_name = "file")]
        datastream: Option<PathBuf>,
        /// add provider components for the requirements of the selected components
        #[arg(long)]
        dependencies: bool,
    },
}

#[derive(Subcommand)]
pub enum SetWhat {
    /// choose the CI workflows the repository generates
    #[command(long_about = help::SET_WORKFLOWS, after_long_help = help::SET_WORKFLOWS_NOTES)]
    Workflows,
    /// add a component library to the repository
    #[command(long_about = help::SET_LIBRARY)]
    Library,
    /// choose the benchmark profile an image is measured against
    #[command(long_about = help::SET_CONFORMS, after_long_help = help::SET_CONFORMS_NOTES)]
    Conforms {
        /// the image; the only image if absent, otherwise a picker
        #[arg(value_name = "image")]
        image: Option<String>,
        /// SCAP content to choose the profile from; the image family's installed copy by default
        #[arg(long, value_name = "file")]
        datastream: Option<PathBuf>,
    },
    /// choose the benchmark rules that a component claims
    #[command(long_about = help::SET_CLAIMS, after_long_help = help::SET_CLAIMS_NOTES)]
    Claims {
        /// the component whose claims are written
        #[arg(value_name = "component")]
        component: Option<String>,
        /// SCAP content to read the rules from; the installed copy for the component's first declared family by default, and required when the component declares no family restriction
        #[arg(long, value_name = "file")]
        datastream: Option<PathBuf>,
    },
    /// record an existing key instead of generating one
    #[command(long_about = help::SET_KEY, after_long_help = help::SET_KEY_NOTES)]
    Key {
        /// the kind of key that a component declares
        #[arg(value_name = "kind")]
        kind: Option<String>,
        /// the component that declares the key
        #[arg(long, value_name = "name")]
        component: Option<String>,
        /// the file that holds the public half
        #[arg(long, value_name = "path")]
        from: Option<String>,
    },
    /// make an imported component the repository's own
    #[command(long_about = help::SET_COMPONENT, after_long_help = help::SET_COMPONENT_NOTES)]
    Component {
        /// the imported component to make the repository's own
        #[arg(value_name = "path")]
        path: Option<String>,
        /// stop tracking without asking
        #[arg(long)]
        untracked: bool,
    },
}

#[derive(Subcommand)]
pub enum VmWhat {
    /// make the disk for the target
    Build {
        /// the disk type: `qcow2`, `raw` or `iso`
        #[arg(value_name = "type")]
        kind: Option<String>,
        #[command(flatten)]
        disk: Disk,
    },
    /// boot that disk under qemu and build it if missing
    Run {
        /// the disk type: `qcow2`, `raw` or `iso`
        #[arg(value_name = "type")]
        kind: Option<String>,
        #[command(flatten)]
        disk: Disk,
    },
    /// boot the disk with systemd-vmspawn
    Spawn {
        /// the disk type: `qcow2` or `raw`
        #[arg(value_name = "type")]
        kind: Option<String>,
        #[command(flatten)]
        disk: Disk,
    },
}

#[derive(Subcommand)]
pub enum MediaWhat {
    /// assemble the installer media for the target
    Build {
        #[command(flatten)]
        disk: Disk,
    },
    /// boot that media under qemu and build it if missing
    Run {
        #[command(flatten)]
        disk: Disk,
    },
}

// The flags every `vm` and `media` noun takes, declared once so their help is too.
#[derive(clap::Args)]
pub struct Disk {
    #[arg(long, value_name = "target")]
    target: Option<String>,
    #[arg(long, value_name = "ref")]
    image: Vec<String>,
    #[arg(long, value_name = "tag")]
    tag: Vec<String>,
    #[arg(long, value_name = "size")]
    ram: Option<String>,
    #[arg(long)]
    rebuild: bool,
}

#[derive(Subcommand)]
pub enum ScapWhat {
    /// print the datastream that the target is measured with
    #[command(long_about = help::SCAP_CONTENT)]
    Content {
        /// the target; the image's own target if absent
        #[arg(long, value_name = "target")]
        target: Option<String>,
    },
    /// print the XCCDF tailoring that a scan runs
    #[command(long_about = help::SCAP_TAILORING, after_long_help = help::SCAP_TAILORING_NOTES)]
    Tailoring {
        /// the target; the image's own target if absent
        #[arg(long, value_name = "target")]
        target: Option<String>,
        /// SCAP content; the file `scap content` names by default
        #[arg(long, value_name = "file")]
        datastream: Option<PathBuf>,
    },
    /// print the rule name for each benchmark number
    Rules {
        /// SCAP content; the file `scap content` names by default
        #[arg(long, value_name = "file")]
        datastream: Option<PathBuf>,
        /// a benchmark rule number
        #[arg(num_args(0..), value_name = "number")]
        number: Vec<String>,
    },
}

#[derive(Subcommand)]
pub enum FetchWhat {
    /// refresh each tracked component from its library
    #[command(long_about = help::FETCH_COMPONENTS, after_long_help = help::FETCH_COMPONENTS_NOTES)]
    Components {
        /// the components to refresh; every tracked component if absent
        #[arg(value_name = "path")]
        paths: Vec<String>,
    },
}

#[derive(Subcommand)]
pub enum RegistryWhat {
    /// print the registry namespace that images publish to
    #[command(long_about = help::REGISTRY_NAMESPACE)]
    Namespace,
    /// print the full reference that one target publishes under
    #[command(long_about = help::REGISTRY_REF)]
    Ref {
        /// the target; the image's own target if absent
        #[arg(long, value_name = "target")]
        target: Option<String>,
        /// the tag the reference publishes under
        #[arg(long, value_name = "tag")]
        tag: Vec<String>,
    },
}

#[derive(Debug, Clone)]
pub struct Row {
    pub verb: Verb,
    pub path: String,
    pub label: String,
    pub about: String,
}

fn surface(path: &str) -> Option<(&'static str, Verb, Family, bool, Topic)> {
    SURFACE.iter().copied().find(|(words, ..)| *words == path)
}

pub fn verb(path: &str) -> Option<Verb> {
    surface(path).map(|(_, verb, ..)| verb)
}

pub fn on_host(verb: Verb) -> bool {
    SURFACE
        .iter()
        .find(|(_, row, ..)| *row == verb)
        .is_some_and(|(_, _, _, host, _)| *host)
}

fn levels(matches: &ArgMatches) -> Vec<&ArgMatches> {
    let mut levels = vec![matches];
    let mut at = matches;
    while let Some((_, sub)) = at.subcommand() {
        levels.push(sub);
        at = sub;
    }
    levels
}

pub fn flag<T: Clone + Send + Sync + 'static>(matches: &ArgMatches, id: &str) -> Option<T> {
    levels(matches)
        .into_iter()
        .rev()
        .find_map(|at| at.try_get_one::<T>(id).ok().flatten().cloned())
}

pub fn flag_all<T: Clone + Send + Sync + 'static>(matches: &ArgMatches, id: &str) -> Vec<T> {
    levels(matches)
        .into_iter()
        .rev()
        .find_map(|at| {
            at.try_get_many::<T>(id)
                .ok()
                .flatten()
                .map(|values| values.cloned().collect())
        })
        .unwrap_or_default()
}

// clap accepts a flag the parent command defines, so a leaf that never reads it would silently ignore the value the user gave.
pub fn ignored(matches: &ArgMatches) -> Vec<String> {
    let tree = Cli::command();
    let mut commands: Vec<&clap::Command> = vec![&tree];
    let mut at = matches;
    while let Some((name, sub)) = at.subcommand() {
        let Some(next) = commands
            .last()
            .expect("a command was just pushed")
            .get_subcommands()
            .find(|command| command.get_name() == name)
        else {
            break;
        };
        commands.push(next);
        at = sub;
    }
    let levels = levels(matches);
    let Some((leaf, above)) = commands.split_last() else {
        return Vec::new();
    };
    let mut ignored = Vec::new();
    for (command, at) in above.iter().zip(levels.iter()) {
        for arg in command.get_arguments() {
            if arg.is_global_set() {
                continue;
            }
            let id = arg.get_id().as_str();
            if leaf.get_arguments().any(|own| own.get_id().as_str() == id) {
                continue;
            }
            if at.try_get_raw(id).ok().flatten().is_some() {
                ignored.push(id.replace('_', "-"));
            }
        }
    }
    ignored
}

fn placeholder(arg: &clap::Arg) -> String {
    let name = arg
        .get_value_names()
        .and_then(|names| names.first())
        .map_or_else(
            || arg.get_id().to_string(),
            |name| name.to_string().to_lowercase(),
        );
    let many = arg
        .get_num_args()
        .is_some_and(|range| range.max_values() > 1);
    let shaped = match arg.is_required_set() {
        true => format!("<{name}>"),
        false => format!("[{name}]"),
    };
    format!("{shaped}{}", if many { "..." } else { "" })
}

fn label(path: &str, command: &clap::Command) -> String {
    let mut label = path.to_string();
    for arg in command.get_positionals() {
        label.push(' ');
        label.push_str(&placeholder(arg));
    }
    label
}

fn collect(command: &clap::Command, prefix: &str, rows: &mut Vec<Row>) {
    for sub in command.get_subcommands() {
        let path = match prefix.is_empty() {
            true => sub.get_name().to_string(),
            false => format!("{prefix} {}", sub.get_name()),
        };
        let nested = sub.has_subcommands();
        if nested {
            collect(sub, &path, rows);
        }
        if !nested || sub.get_positionals().next().is_some() {
            let (_, verb, ..) = surface(&path).expect("every leaf has a place row");
            rows.push(Row {
                verb,
                label: label(&path, sub),
                about: sub
                    .get_about()
                    .map(|about| about.to_string())
                    .unwrap_or_default(),
                path,
            });
        }
    }
}

fn leaves() -> Vec<Row> {
    let mut rows = Vec::new();
    collect(&Cli::command(), "", &mut rows);
    rows
}

pub fn rows(word: Option<&str>) -> Vec<Row> {
    let leaves = leaves();
    match word {
        Some(word) => {
            let prefix = format!("{word} ");
            leaves
                .into_iter()
                .filter(|row| row.path.starts_with(&prefix))
                .collect()
        }
        None => leaves.into_iter().filter(|row| shown(&row.path)).collect(),
    }
}

pub fn runs_in(path: &str, here: &Context) -> bool {
    let (_, _, family, host, _) = surface(path).expect("every command path has a place row");
    match family {
        Family::Anywhere | Family::Layer => true,
        Family::Repo | Family::Script => match here {
            Context::Repo(_) => true,
            Context::Host => host,
            Context::Loose => false,
        },
    }
}

pub fn choices(rows: &[Row], here: &Context) -> (Vec<Option<Row>>, Vec<Choice>) {
    let runs: Vec<Row> = rows
        .iter()
        .filter(|row| runs_in(&row.path, here))
        .cloned()
        .collect();
    let groups = grouped(&runs);
    let headed = groups.len() > 1;
    let mut kept = Vec::new();
    let mut drawn = Vec::new();
    for (topic, rows) in groups {
        if headed {
            kept.push(None);
            drawn.push(Choice::new(topic.heading(), "").heading());
        }
        for row in rows {
            kept.push(Some(row.clone()));
            drawn.push(Choice::new(row.label.clone(), row.about.clone()));
        }
    }
    (kept, drawn)
}

const INTRO: &str = "\
# The `buildc` CLI

`buildc` is the one tool that manages a bootc image repository. The user describes each image \
in KDL files, and `buildc` turns those files into the Containerfiles and the CI workflows that \
build the image. `buildc` works out the component order, the options, the names and the tags in \
one place, so the build scripts hold no logic of their own and the user can see what goes \
into each image.

## Where it runs

`buildc` runs in three places:

- On the user's machine, it scaffolds the repository and its images, components and keys. It \
checks the KDL files, writes the build files, and builds and boots an image.
- In CI, the generated workflows call it to write the build files, build and publish each \
image, and scan it.
- Inside a build, the generated Containerfile calls it to write the image identity, record \
what the build resolved and check the finished image. A component script can call `buildc fetch` \
to download a payload and check it against its hash.

";

fn link(verb: Verb) -> Option<String> {
    verb.schema().map(|(label, area, at)| match at {
        "" => format!("[{label}](schema/{})", area.file()),
        at => format!("[{label}](schema/{}#{at})", area.file()),
    })
}

pub fn schema_links() -> Vec<(String, String)> {
    leaves()
        .into_iter()
        .filter_map(|row| link(row.verb).map(|link| (row.path, link)))
        .collect()
}

pub fn overview() -> String {
    let mut out = String::from(INTRO);
    globals(&mut out);
    out.push_str("## Commands\n\n");
    for (topic, rows) in grouped(&leaves()) {
        let _ = write!(
            out,
            "### {}\n\n| Command | Description | Schema |\n| --- | --- | --- |\n",
            topic.heading()
        );
        if topic == Topic::Tool {
            out.push_str(
                "| [`buildc`](#buildc) | open a picker of the commands that run here, or list them \
                 where no terminal is watching |  |\n",
            );
        }
        for row in rows {
            let anchor = format!("buildc-{}", row.path.replace(' ', "-"));
            let schema = link(row.verb).unwrap_or_default();
            let _ = writeln!(
                out,
                "| [`buildc {}`](#{anchor}) | {} | {schema} |",
                row.label, row.about
            );
        }
        out.push('\n');
    }
    out
}

fn globals(out: &mut String) {
    out.push_str(
        "## Global options\n\nEvery command takes these options, before or after its own \
         words. [`buildc`](#buildc) says how a command asks for an answer, and what `--no-tui` \
         changes.\n\n| Option | Description |\n| --- | --- |\n",
    );
    for arg in Cli::command()
        .get_arguments()
        .filter(|arg| arg.is_global_set())
    {
        let long = arg.get_long().expect("a global flag has a long name");
        let value = match (arg.get_action().takes_values(), arg.get_value_names()) {
            (true, Some(names)) => format!(" <{}>", names.join("> <")),
            _ => String::new(),
        };
        let help = arg.get_help().map(ToString::to_string).unwrap_or_default();
        let _ = writeln!(out, "| `--{long}{value}` | {help} |");
    }
    out.push('\n');
}

pub fn takes(word: &str) -> String {
    let tree = Cli::command();
    let Some(command) = tree
        .get_subcommands()
        .find(|command| command.get_name() == word)
    else {
        return String::new();
    };
    let quoted: Vec<String> = command
        .get_subcommands()
        .map(|sub| format!("`{}`", label(sub.get_name(), sub)))
        .collect();
    join_or(&quoted)
}

fn join_or(quoted: &[String]) -> String {
    match quoted.split_last() {
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} or {last}", rest.join(", ")),
        None => String::new(),
    }
}

pub fn host_labels() -> Vec<String> {
    leaves()
        .into_iter()
        .filter(|row| on_host(row.verb))
        .map(|row| row.label)
        .collect()
}

#[derive(Debug, PartialEq)]
pub enum Context {
    Repo(PathBuf),
    Host,
    Loose,
}

impl Context {
    pub fn of(root: Option<&Path>) -> Self {
        if let Some(root) = root {
            return Self::Repo(root.to_path_buf());
        }
        let here = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        if let Some(found) = crate::find_root(&here) {
            let up = here.strip_prefix(&found).map(|d| d.components().count());
            return Self::Repo(match up {
                Ok(0) => PathBuf::from("."),
                Ok(up) => (0..up).map(|_| "..").collect(),
                Err(_) => found,
            });
        }
        // A booted image carries the generated manifest, so its presence means this tool runs inside its own image.
        match Path::new(crate::provenance::build::MANIFEST).is_file() {
            true => Self::Host,
            false => Self::Loose,
        }
    }
}

const HEAD: &str = "usage: buildc [--root <dir>] <command>\n";

const RULE: &str = "\
Every answer a command may ask for has a matching flag. What no flag gave is
asked for, and `--no-tui` asks nothing. An unattended question uses its default
or fails and names the flag when it has no default.

docs/cli.md is the reference. Data goes to stdout and diagnostics to
stderr; exit 1 is the invocation, exit 2 the repository.
";

fn family(path: &str) -> Family {
    surface(path).expect("every command path has a place row").2
}

pub fn topic(path: &str) -> Topic {
    surface(path).expect("every command path has a place row").4
}

pub fn grouped(rows: &[Row]) -> Vec<(Topic, Vec<&Row>)> {
    TOPICS
        .iter()
        .map(|&at| {
            (
                at,
                rows.iter()
                    .filter(|row| topic(&row.path) == at)
                    .collect::<Vec<_>>(),
            )
        })
        .filter(|(_, rows)| !rows.is_empty())
        .collect()
}

fn is_host(path: &str) -> bool {
    surface(path).expect("every command path has a place row").3
}

fn shown(path: &str) -> bool {
    matches!(family(path), Family::Anywhere | Family::Repo)
}

pub fn usage(here: &Context) -> String {
    let rows = leaves();
    let kept: Vec<&Row> = rows.iter().filter(|row| shown(&row.path)).collect();
    let width = kept.iter().map(|row| row.label.len()).max().unwrap_or(0);
    let block = |keep: &dyn Fn(&str) -> bool| -> String {
        let rows: Vec<Row> = kept
            .iter()
            .filter(|row| keep(&row.path))
            .map(|row| (*row).clone())
            .collect();
        grouped(&rows)
            .iter()
            .map(|(topic, rows)| {
                let lines: String = rows
                    .iter()
                    .map(|row| format!("  {:width$}  {}\n", row.label, row.about))
                    .collect();
                format!("{}:\n{lines}", topic.heading().to_lowercase())
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let anywhere = block(&|path| family(path) == Family::Anywhere);
    let repo =
        |host: bool| block(&move |path| family(path) == Family::Repo && is_host(path) == host);
    match here {
        Context::Repo(_) => format!("{HEAD}\n{}\n{RULE}", block(&|_| true)),
        Context::Host => format!(
            "{HEAD}\n{anywhere}\nthis is a buildc image, and it answers these about itself:\n\n\
             {}\nthese read the source tree, and there is none here:\n\n{}\n{RULE}",
            repo(true),
            repo(false),
        ),
        Context::Loose => format!(
            "{HEAD}\n{anywhere}\nthese need a repository, and there is none here or above:\n\n\
             {}\n{RULE}",
            block(&|path| family(path) == Family::Repo)
        ),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Plan,
    Check,
    Generate,
    Verify,
    Section,
    Graph,
    GraphJson,
    Summary,
    Sbom,
    Why,
    WhyJson,
    Coverage,
    CoverageJson,
}

pub enum Arg {
    Image,
    Target,
    Component,
}

impl Command {
    pub fn arg(self) -> Option<Arg> {
        match self {
            Self::Plan
            | Self::Check
            | Self::Generate
            | Self::Verify
            | Self::Graph
            | Self::GraphJson => None,
            Self::Section | Self::Coverage | Self::CoverageJson => Some(Arg::Image),
            Self::Summary | Self::Sbom => Some(Arg::Target),
            Self::Why | Self::WhyJson => Some(Arg::Component),
        }
    }
}

impl Verb {
    pub fn reads(self) -> Option<Command> {
        Some(match self {
            Self::Plan => Command::Plan,
            Self::Check => Command::Check,
            Self::Generate => Command::Generate,
            Self::Verify => Command::Verify,
            Self::Section => Command::Section,
            Self::Graph => Command::Graph,
            Self::Summary => Command::Summary,
            Self::Sbom => Command::Sbom,
            Self::Why => Command::Why,
            Self::Coverage => Command::Coverage,
            Self::Upgrade
            | Self::CreateRepo
            | Self::CreateGit
            | Self::CreateImage
            | Self::CreateFlavour
            | Self::CreateModule
            | Self::CreateKey
            | Self::CreateScripts
            | Self::ImportModule
            | Self::SetWorkflows
            | Self::SetLibrary
            | Self::SetConforms
            | Self::SetClaims
            | Self::SetKey
            | Self::SetModule
            | Self::Build
            | Self::VmBuild
            | Self::VmRun
            | Self::VmSpawn
            | Self::MediaBuild
            | Self::MediaRun
            | Self::FetchModules
            | Self::Scap
            | Self::ScapContent
            | Self::ScapRules
            | Self::ScapTailoring
            | Self::RegistryNamespace
            | Self::RegistryRef
            | Self::Recipe
            | Self::BuildRecord
            | Self::Fetch
            | Self::ValidateImage
            | Self::Validate
            | Self::Attribute => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::error::ErrorKind;

    fn paths() -> Vec<String> {
        leaves().into_iter().map(|row| row.path).collect()
    }

    #[test]
    fn every_command_path_has_one_row_and_every_row_names_a_leaf() {
        let tree = paths();
        let mut rows: Vec<&str> = SURFACE.iter().map(|(path, ..)| *path).collect();
        rows.sort_unstable();
        rows.dedup();
        assert_eq!(rows.len(), SURFACE.len(), "a path has two rows");
        let mut sorted = tree.clone();
        sorted.sort();
        assert_eq!(sorted, rows, "the tree and the place table disagree");
        assert_eq!(tree.len(), ALL.len(), "a verb has no path or two");
        for verb in ALL {
            assert_eq!(
                SURFACE.iter().filter(|(_, row, ..)| row == verb).count(),
                1,
                "{verb:?} has no one row"
            );
        }
    }

    #[test]
    fn a_word_whose_forms_all_take_a_noun_has_no_verb_of_its_own() {
        for word in ["create", "import", "registry", "set", "vm", "media"] {
            assert!(verb(word).is_none(), "{word}");
        }
        for word in ["scap", "fetch", "check", "build"] {
            assert!(verb(word).is_some(), "{word}");
        }
    }

    #[test]
    fn a_parent_flag_reaches_its_noun_at_either_spelling() {
        let matches = Cli::command()
            .try_get_matches_from(["buildc", "scap", "--target", "foo", "content"])
            .expect("a flag before the noun parses");
        assert_eq!(flag::<String>(&matches, "target").as_deref(), Some("foo"));

        let matches = Cli::command()
            .try_get_matches_from(["buildc", "scap", "rules", "--datastream", "x.xml", "1"])
            .expect("a noun takes its own flag");
        assert_eq!(
            flag::<PathBuf>(&matches, "datastream"),
            Some(PathBuf::from("x.xml"))
        );
    }

    #[test]
    fn a_flag_the_leaf_does_not_take_is_refused() {
        let matches = Cli::command()
            .try_get_matches_from(["buildc", "scap", "--datastream", "x.xml", "content"])
            .expect("the parent parses the flag");
        assert_eq!(ignored(&matches), ["datastream"]);

        let matches = Cli::command()
            .try_get_matches_from(["buildc", "scap", "--target", "foo", "content"])
            .expect("the leaf takes the flag");
        assert!(ignored(&matches).is_empty());
    }

    #[test]
    fn a_switch_reaches_only_the_command_that_reads_it() {
        Cli::command()
            .try_get_matches_from(["buildc", "build", "--cache-to"])
            .expect("build reads the cache switches");
        Cli::command()
            .try_get_matches_from(["buildc", "vm", "run", "--rebuild"])
            .expect("the vm nouns read the rebuild switch");
        let error = Cli::command()
            .try_get_matches_from(["buildc", "check", "--cache-to"])
            .expect_err("check does not read the cache switches");
        assert_eq!(error.kind(), ErrorKind::UnknownArgument);
    }

    #[test]
    fn import_takes_the_dependencies_switch() {
        let matches = Cli::command()
            .try_get_matches_from(["buildc", "import", "component", "browser", "--dependencies"])
            .expect("import component takes --dependencies");
        assert_eq!(flag::<bool>(&matches, "dependencies"), Some(true));
    }

    #[test]
    fn copy_is_not_a_command() {
        let error = Cli::command()
            .try_get_matches_from(["buildc", "copy", "component", "browser"])
            .expect_err("copy is gone");
        assert_eq!(error.kind(), ErrorKind::InvalidSubcommand);
    }

    #[test]
    fn fetch_takes_its_three_positionals_unless_a_noun_does() {
        Cli::command()
            .try_get_matches_from(["buildc", "fetch", "components"])
            .expect("the noun negates the required positionals");
        let matches = Cli::command()
            .try_get_matches_from(["buildc", "fetch", "file", "https://host/f", "abc", "/usr/f"])
            .expect("the in-build form parses beside the noun");
        let fetch = matches
            .subcommand_matches("fetch")
            .expect("fetch is the word");
        assert_eq!(
            fetch.get_one::<String>("what").map(String::as_str),
            Some("file")
        );
        assert_eq!(
            fetch.get_one::<String>("target").map(String::as_str),
            Some("/usr/f")
        );
        let error = Cli::command()
            .try_get_matches_from(["buildc", "fetch", "file"])
            .expect_err("the bare form needs its url and sha256");
        assert_eq!(error.kind(), ErrorKind::MissingRequiredArgument);
    }

    #[test]
    fn build_and_recipe_take_their_target_as_a_flag() {
        let matches = Cli::command()
            .try_get_matches_from(["buildc", "build", "--target", "foo"])
            .expect("the flag parses");
        assert_eq!(flag::<String>(&matches, "target").as_deref(), Some("foo"));

        let matches = Cli::command()
            .try_get_matches_from(["buildc", "build", "foo"])
            .expect("the argument parses");
        let (_, build) = matches.subcommand().expect("build was given");
        assert_eq!(
            build.get_one::<String>("target_arg").map(String::as_str),
            Some("foo")
        );

        Cli::command()
            .try_get_matches_from(["buildc", "recipe", "--target", "foo"])
            .expect("recipe takes the flag");
    }

    #[test]
    fn a_place_decides_what_runs_and_the_help_groups_by_it() {
        let repo = Context::Repo(".".into());
        for path in ["why", "check", "upgrade"] {
            assert!(runs_in(path, &repo), "{path}");
        }
        assert!(runs_in("why", &Context::Host) && !runs_in("check", &Context::Host));
        assert!(runs_in("upgrade", &Context::Loose) && !runs_in("why", &Context::Loose));

        let (kept, drawn) = choices(&rows(None), &Context::Loose);
        assert!(kept
            .iter()
            .flatten()
            .all(|row| surface(&row.path)
                .is_some_and(|(_, _, family, ..)| family == Family::Anywhere)));
        assert_eq!(kept.len(), drawn.len());
        let (kept, _) = choices(&rows(None), &Context::Host);
        let verbs: Vec<Verb> = kept.iter().flatten().map(|row| row.verb).collect();
        assert!(verbs.contains(&Verb::Why) && !verbs.contains(&Verb::Check));

        let host = usage(&Context::Host);
        let (answers, rest) = host
            .split_once("these read the source tree")
            .expect("the host help groups what it can answer");
        assert!(answers.contains("  why [component]"), "{host}");
        assert!(!answers.contains("  check "), "{host}");
        assert!(rest.contains("  check "), "{host}");
        for row in rows(None) {
            for here in [Context::Repo(".".into()), Context::Host, Context::Loose] {
                assert!(usage(&here).contains(&row.label), "{}", row.label);
            }
        }
    }

    #[test]
    fn a_refusal_lists_the_tails_a_word_wanted() {
        assert_eq!(takes("registry"), "`namespace` or `ref`");
        assert_eq!(takes("import"), "`component [name]`");
        assert!(takes("create").starts_with("`repo [name]`"));
    }

    #[test]
    fn every_schema_link_lands_on_an_anchor_its_page_holds() {
        for verb in ALL {
            let Some((label, area, at)) = verb.schema() else {
                continue;
            };
            let page = crate::emit::schema_md::page(area);
            assert!(
                at.is_empty() || page.contains(&format!("<a id=\"{at}\"></a>")),
                "{verb:?} links {label} to `{at}`, which {} does not hold",
                area.file()
            );
        }
    }

    #[test]
    fn the_picker_heads_each_topic_only_where_rows_span_several() {
        let repo = Context::Repo(".".into());
        let (_, drawn) = choices(&rows(None), &repo);
        let headings: Vec<&str> = drawn
            .iter()
            .filter(|choice| choice.heading)
            .map(|choice| choice.label.as_str())
            .collect();
        assert_eq!(headings.last(), Some(&"CLI tool"), "{headings:?}");
        assert!(headings.contains(&"Components"), "{headings:?}");
        let (_, drawn) = choices(&rows(Some("vm")), &repo);
        assert!(drawn.iter().all(|choice| !choice.heading));
    }

    #[test]
    fn the_picker_and_prompt_rows_come_from_the_tree() {
        let repo = Context::Repo(".".into());
        let (kept, drawn) = choices(&rows(Some("create")), &repo);
        assert_eq!(kept.len(), drawn.len());
        assert!(kept
            .iter()
            .flatten()
            .all(|row| row.path.starts_with("create ")));
        for (row, choice) in kept.iter().zip(&drawn) {
            assert_eq!(row.is_none(), choice.heading, "{}", choice.label);
            assert!(
                row.is_none() || !choice.detail.is_empty(),
                "{}",
                choice.label
            );
        }
        assert_eq!(
            host_labels(),
            [
                "why [component]",
                "plan",
                "summary [target]",
                "scap content",
                "scap rules [number]..."
            ]
        );
    }
}
