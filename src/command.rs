use clap::{ArgMatches, ColorChoice, CommandFactory, Parser, Subcommand};
use common::ui::Choice;
use std::path::{Path, PathBuf};

mod help;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verb {
    Upgrade,
    CreateRepo,
    CreateImage,
    CreateFlavour,
    CreateModule,
    CreateKey,
    ImportModule,
    CopyModule,
    SetWorkflows,
    SetConforms,
    SetClaims,
    SetKey,
    Check,
    Generate,
    Build,
    VmBuild,
    VmRun,
    VmSpawn,
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
    OsRelease,
    BuildRecord,
    Fetch,
    ValidateImage,
}

pub const ALL: &[Verb] = &[
    Verb::Upgrade,
    Verb::CreateRepo,
    Verb::CreateImage,
    Verb::CreateFlavour,
    Verb::CreateModule,
    Verb::CreateKey,
    Verb::ImportModule,
    Verb::CopyModule,
    Verb::SetWorkflows,
    Verb::SetConforms,
    Verb::SetClaims,
    Verb::SetKey,
    Verb::Check,
    Verb::Generate,
    Verb::Build,
    Verb::VmBuild,
    Verb::VmRun,
    Verb::VmSpawn,
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
    Verb::OsRelease,
    Verb::BuildRecord,
    Verb::Fetch,
    Verb::ValidateImage,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    Anywhere,
    Repo,
    Script,
    Layer,
}

const SURFACE: &[(&str, Verb, Family, bool)] = &[
    ("upgrade", Verb::Upgrade, Family::Anywhere, false),
    ("create repo", Verb::CreateRepo, Family::Anywhere, false),
    ("create image", Verb::CreateImage, Family::Repo, false),
    ("create flavour", Verb::CreateFlavour, Family::Repo, false),
    ("create module", Verb::CreateModule, Family::Repo, false),
    ("create key", Verb::CreateKey, Family::Repo, false),
    ("import module", Verb::ImportModule, Family::Repo, false),
    ("copy module", Verb::CopyModule, Family::Repo, false),
    ("set workflows", Verb::SetWorkflows, Family::Repo, false),
    ("set conforms", Verb::SetConforms, Family::Repo, false),
    ("set claims", Verb::SetClaims, Family::Repo, false),
    ("set key", Verb::SetKey, Family::Repo, false),
    ("check", Verb::Check, Family::Repo, false),
    ("generate", Verb::Generate, Family::Repo, false),
    ("build", Verb::Build, Family::Repo, false),
    ("vm build", Verb::VmBuild, Family::Repo, false),
    ("vm run", Verb::VmRun, Family::Repo, false),
    ("vm spawn", Verb::VmSpawn, Family::Repo, false),
    ("section", Verb::Section, Family::Repo, false),
    ("graph", Verb::Graph, Family::Repo, false),
    ("why", Verb::Why, Family::Repo, true),
    ("coverage", Verb::Coverage, Family::Repo, false),
    ("plan", Verb::Plan, Family::Script, true),
    ("verify", Verb::Verify, Family::Script, false),
    ("summary", Verb::Summary, Family::Script, true),
    ("sbom", Verb::Sbom, Family::Script, false),
    ("fetch modules", Verb::FetchModules, Family::Script, false),
    ("scap content", Verb::ScapContent, Family::Script, true),
    ("scap rules", Verb::ScapRules, Family::Script, true),
    ("scap tailoring", Verb::ScapTailoring, Family::Script, false),
    ("scap", Verb::Scap, Family::Script, false),
    (
        "registry namespace",
        Verb::RegistryNamespace,
        Family::Script,
        false,
    ),
    ("registry ref", Verb::RegistryRef, Family::Script, false),
    ("recipe", Verb::Recipe, Family::Script, false),
    ("os-release", Verb::OsRelease, Family::Layer, false),
    ("build-record", Verb::BuildRecord, Family::Layer, false),
    ("fetch", Verb::Fetch, Family::Layer, false),
    ("validate-image", Verb::ValidateImage, Family::Layer, false),
];

#[derive(Parser)]
#[command(
    name = "buildc",
    about = "the build tool for a bootc image repository",
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
    Copy {
        #[command(subcommand)]
        what: Option<CopyWhat>,
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
        /// SCAP content to read the profile from
        /// SCAP content; the file `scap content` names by default
        /// the file that holds the previous scan's pass set
        /// SCAP content for the profile offer; the family's installed copy by default, and no content means no offer
        /// SCAP content to choose the profile from; the image family's installed copy by default
        /// SCAP content to read the rules from; the installed copy for the component's first declared family by default, and required when the component declares no family restriction
        #[arg(long, value_name = "file")]
        datastream: Option<PathBuf>,
    },
    /// write the build files and list what was written
    #[command(long_about = help::GENERATE, after_long_help = help::GENERATE_NOTES)]
    Generate,
    /// build one target with the container backend
    #[command(long_about = help::BUILD, after_long_help = help::BUILD_NOTES)]
    /// make the disk for the target
    Build {
        /// the target
        /// the destination that the payload kind writes to
        #[arg(value_name = "target")]
        target_arg: Option<String>,
        /// the target, as an alternative to the positional argument
        /// the target; the default target if absent
        /// the target whose plan maps each layer to its paths
        /// the target; the image's own target if absent
        #[arg(long, value_name = "target")]
        target: Option<String>,
        /// an extra tag for the built image
        /// the tag the reference publishes under
        #[arg(long, value_name = "tag")]
        tag: Vec<String>,
        /// the kernel version to install
        /// the container backend: `buildx` or `buildah`
        /// the account or organisation that owns the repository
        /// also write a first image with this name
        /// the account or organisation; used when no image already names one
        /// the image that publishes the flavour
        /// an image that lists the component
        /// a package the component installs
        /// the component that declares the key
        /// the common name of a generated certificate
        /// the image or flavour that lists the component
        #[arg(long, value_name = "name")]
        kernel: Option<String>,
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
    /// print one image's generated Containerfile section
    #[command(long_about = help::SECTION)]
    Section {
        /// the image; the default image if absent
        /// the image; the default image if absent, or a picker in a terminal
        /// the image; the only image if absent, otherwise a picker
        #[arg(value_name = "image")]
        image: Option<String>,
    },
    /// print the capability graph and what the base already carries
    #[command(long_about = help::GRAPH)]
    Graph {
        /// `markdown` with a mermaid diagram, or `json`; `markdown` is the default
        /// `markdown` or `json`; `markdown` is the default
        #[arg(long, value_name = "md|json")]
        format: Option<String>,
    },
    /// print one component's provenance and contents
    #[command(long_about = help::WHY, after_long_help = help::WHY_NOTES)]
    Why {
        #[arg(value_name = "module")]
        module: Option<String>,
        #[arg(long, value_name = "md|json")]
        format: Option<String>,
    },
    /// print which component claims each rule of the image's profile
    #[command(long_about = help::COVERAGE, after_long_help = help::COVERAGE_NOTES)]
    Coverage {
        #[arg(value_name = "image")]
        image: Option<String>,
        #[arg(long, value_name = "md|json")]
        format: Option<String>,
        #[arg(long, value_name = "file")]
        datastream: Option<PathBuf>,
    },
    /// print every fact the repository derives as JSON
    #[command(long_about = help::PLAN)]
    Plan {
        /// no effect; `plan` always prints JSON
        /// add provider components for the requirements of the selected components
        /// stop tracking without asking
        #[arg(long)]
        json: bool,
    },
    /// compare every build file byte for byte with the one buildc writes
    #[command(long_about = help::VERIFY)]
    Verify,
    /// print a table of what one target is made of
    #[command(long_about = help::SUMMARY)]
    Summary {
        #[arg(value_name = "target")]
        target: Option<String>,
    },
    /// print one target's pinned payloads as SPDX packages
    #[command(long_about = help::SBOM)]
    Sbom {
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
        #[arg(long, value_name = "target")]
        target: Option<String>,
        #[arg(long, value_name = "file")]
        datastream: Option<PathBuf>,
        #[arg(long, value_name = "file")]
        baseline: Option<PathBuf>,
        /// a scan of the bare base to compare against
        #[arg(long = "base-scan", value_name = "file")]
        base_scan: Option<PathBuf>,
    },
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
        #[arg(long, value_name = "target")]
        target: Option<String>,
        #[arg(long, value_name = "tag")]
        tag: Vec<String>,
        /// the local image to install in place of the published reference
        /// the base image reference for the first image
        /// a bootc image reference, catalogued or not; skips the base picker
        #[arg(long, value_name = "ref")]
        image: Vec<String>,
    },
    #[command(long_about = help::OS_RELEASE)]
    OsRelease,
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
        /// the image name
        /// the flavour name
        /// the component name or path
        /// the scripts to keep
        /// the component to copy or `<owner>/<name>` to choose a library
        #[arg(value_name = "name")]
        name: Option<String>,
        /// the repository host; `github.com` by default, and with `--owner` it forms the base of every image URL
        #[arg(long, value_name = "domain")]
        host: Option<String>,
        #[arg(long, value_name = "name")]
        owner: Option<String>,
        #[arg(long, value_name = "name")]
        image: Vec<String>,
        #[arg(long, value_name = "ref")]
        base: Option<String>,
    },
    /// add an image and choose its base
    #[command(long_about = help::CREATE_IMAGE, after_long_help = help::CREATE_IMAGE_NOTES)]
    Image {
        #[arg(value_name = "name")]
        name: Option<String>,
        #[arg(long, value_name = "name")]
        owner: Option<String>,
        #[arg(long, value_name = "ref")]
        base: Option<String>,
    },
    /// add a second build of an image with more components
    #[command(long_about = help::CREATE_FLAVOUR, after_long_help = help::CREATE_FLAVOUR_NOTES)]
    Flavour {
        #[arg(value_name = "name")]
        name: Option<String>,
        #[arg(long, value_name = "name")]
        image: Vec<String>,
    },
    #[command(long_about = help::CREATE_MODULE, after_long_help = help::CREATE_MODULE_NOTES)]
    Module {
        #[arg(value_name = "name")]
        name: Option<String>,
        #[arg(long, value_name = "name")]
        image: Vec<String>,
        #[arg(long, value_name = "name")]
        pkg: Vec<String>,
        /// one more manifest line, such as `provides=browser`; repeatable
        #[arg(long, value_name = "verb=value")]
        with: Vec<String>,
    },
    /// generate a key that a component declares
    #[command(long_about = help::CREATE_KEY, after_long_help = help::CREATE_KEY_NOTES)]
    Key {
        /// the kind of key that a component declares
        #[arg(value_name = "kind")]
        kind: Option<String>,
        #[arg(long, value_name = "name")]
        module: Option<String>,
        #[arg(long, value_name = "name")]
        cn: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum ImportWhat {
    #[command(long_about = help::IMPORT_MODULE, after_long_help = help::IMPORT_MODULE_NOTES)]
    Module {
        #[arg(value_name = "name")]
        name: Option<String>,
        #[arg(long, value_name = "name")]
        image: Vec<String>,
        #[arg(long, value_name = "file")]
        datastream: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
pub enum CopyWhat {
    #[command(long_about = help::COPY_MODULE)]
    Module {
        #[arg(value_name = "name")]
        name: Option<String>,
        #[arg(long, value_name = "name")]
        image: Vec<String>,
        #[arg(long, value_name = "file")]
        datastream: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
pub enum SetWhat {
    /// choose the CI workflows the repository generates
    #[command(long_about = help::SET_WORKFLOWS, after_long_help = help::SET_WORKFLOWS_NOTES)]
    Workflows,
    /// choose the benchmark profile an image is measured against
    #[command(long_about = help::SET_CONFORMS, after_long_help = help::SET_CONFORMS_NOTES)]
    Conforms {
        #[arg(value_name = "image")]
        image: Option<String>,
        #[arg(long, value_name = "file")]
        datastream: Option<PathBuf>,
    },
    /// choose the benchmark rules that a component claims
    #[command(long_about = help::SET_CLAIMS, after_long_help = help::SET_CLAIMS_NOTES)]
    Claims {
        #[arg(value_name = "module")]
        module: Option<String>,
        #[arg(long, value_name = "file")]
        datastream: Option<PathBuf>,
    },
    /// record an existing key instead of generating one
    #[command(long_about = help::SET_KEY, after_long_help = help::SET_KEY_NOTES)]
    Key {
        #[arg(value_name = "kind")]
        kind: Option<String>,
        #[arg(long, value_name = "name")]
        module: Option<String>,
        /// the file that holds the public half
        #[arg(long, value_name = "path")]
        from: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum VmWhat {
    Build {
        /// the disk type: `qcow2`, `raw` or `iso`
        /// the disk type: `qcow2` or `raw`
        #[arg(value_name = "type")]
        kind: Option<String>,
        #[command(flatten)]
        disk: Disk,
    },
    /// boot that disk under qemu and build it if missing
    Run {
        #[arg(value_name = "type")]
        kind: Option<String>,
        #[command(flatten)]
        disk: Disk,
    },
    /// boot the disk with systemd-vmspawn
    Spawn {
        #[arg(value_name = "type")]
        kind: Option<String>,
        #[command(flatten)]
        disk: Disk,
    },
}

// The flags every `vm` noun takes, declared once so their help is too.
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
        #[arg(long, value_name = "target")]
        target: Option<String>,
    },
    /// print the XCCDF tailoring that a scan runs
    #[command(long_about = help::SCAP_TAILORING, after_long_help = help::SCAP_TAILORING_NOTES)]
    Tailoring {
        #[arg(long, value_name = "target")]
        target: Option<String>,
        #[arg(long, value_name = "file")]
        datastream: Option<PathBuf>,
    },
    /// print the rule name for each benchmark number
    Rules {
        #[arg(long, value_name = "file")]
        datastream: Option<PathBuf>,
        /// a benchmark rule number
        #[arg(num_args(0..), value_name = "number")]
        number: Vec<String>,
    },
}

#[derive(Subcommand)]
pub enum FetchWhat {
    #[command(long_about = help::FETCH_MODULES, after_long_help = help::FETCH_MODULES_NOTES)]
    Modules,
}

#[derive(Subcommand)]
pub enum RegistryWhat {
    /// print the registry namespace that images publish to
    #[command(long_about = help::REGISTRY_NAMESPACE)]
    Namespace,
    /// print the full reference that one target publishes under
    #[command(long_about = help::REGISTRY_REF)]
    Ref {
        #[arg(long, value_name = "target")]
        target: Option<String>,
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

fn surface(path: &str) -> Option<(&'static str, Verb, Family, bool)> {
    SURFACE.iter().copied().find(|(words, ..)| *words == path)
}

pub fn verb(path: &str) -> Option<Verb> {
    surface(path).map(|(_, verb, _, _)| verb)
}

pub fn on_host(verb: Verb) -> bool {
    SURFACE
        .iter()
        .find(|(_, row, _, _)| *row == verb)
        .is_some_and(|(.., host)| *host)
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
        .map(|name| name.to_string().to_lowercase())
        .unwrap_or_else(|| arg.get_id().to_string());
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
            let (_, verb, _, _) = surface(&path).expect("every leaf has a place row");
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
    let (_, _, family, host) = surface(path).expect("every command path has a place row");
    match family {
        Family::Anywhere | Family::Layer => true,
        Family::Repo | Family::Script => match here {
            Context::Repo(_) => true,
            Context::Host => host,
            Context::Loose => false,
        },
    }
}

pub fn choices(rows: &[Row], here: &Context) -> (Vec<Row>, Vec<Choice>) {
    let kept: Vec<Row> = rows
        .iter()
        .filter(|row| runs_in(&row.path, here))
        .cloned()
        .collect();
    let drawn = kept
        .iter()
        .map(|row| Choice::new(row.label.clone(), row.about.clone()))
        .collect();
    (kept, drawn)
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
Every command takes a flag for everything it needs. What no flag gave is asked
for, and `--no-tui` asks nothing, failing and naming the flag instead.

docs/commands.md is the reference. Data goes to stdout and diagnostics to
stderr; exit 1 is the invocation, exit 2 the repository.
";

fn family(path: &str) -> Family {
    surface(path).expect("every command path has a place row").2
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
        kept.iter()
            .filter(|row| keep(&row.path))
            .map(|row| format!("  {:width$}  {}\n", row.label, row.about))
            .collect()
    };
    let anywhere = block(&|path| family(path) == Family::Anywhere);
    let repo =
        |host: bool| block(&move |path| family(path) == Family::Repo && is_host(path) == host);
    match here {
        Context::Repo(_) => format!(
            "{HEAD}\n{anywhere}{}\n{RULE}",
            block(&|path| family(path) == Family::Repo)
        ),
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
    Module,
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
            Self::Why | Self::WhyJson => Some(Arg::Module),
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
            | Self::CreateImage
            | Self::CreateFlavour
            | Self::CreateModule
            | Self::CreateKey
            | Self::ImportModule
            | Self::CopyModule
            | Self::SetWorkflows
            | Self::SetConforms
            | Self::SetClaims
            | Self::SetKey
            | Self::Build
            | Self::VmBuild
            | Self::VmRun
            | Self::VmSpawn
            | Self::FetchModules
            | Self::Scap
            | Self::ScapContent
            | Self::ScapRules
            | Self::ScapTailoring
            | Self::RegistryNamespace
            | Self::RegistryRef
            | Self::Recipe
            | Self::OsRelease
            | Self::BuildRecord
            | Self::Fetch
            | Self::ValidateImage => return None,
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
                SURFACE.iter().filter(|(_, row, _, _)| row == verb).count(),
                1,
                "{verb:?} has no one row"
            );
        }
    }

    #[test]
    fn a_word_whose_forms_all_take_a_noun_has_no_verb_of_its_own() {
        for word in ["create", "import", "copy", "registry", "set", "vm"] {
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
    fn fetch_takes_its_three_positionals_unless_a_noun_does() {
        Cli::command()
            .try_get_matches_from(["buildc", "fetch", "modules"])
            .expect("the noun negates the required positionals");
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
            .all(|row| surface(&row.path)
                .is_some_and(|(_, _, family, _)| family == Family::Anywhere)));
        assert_eq!(kept.len(), drawn.len());
        let (kept, _) = choices(&rows(None), &Context::Host);
        let verbs: Vec<Verb> = kept.iter().map(|row| row.verb).collect();
        assert!(verbs.contains(&Verb::Why) && !verbs.contains(&Verb::Check));

        let host = usage(&Context::Host);
        let (answers, rest) = host
            .split_once("these read the source tree")
            .expect("the host help groups what it can answer");
        assert!(answers.contains("  why [module]"), "{host}");
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
        assert_eq!(takes("import"), "`module [name]`");
        assert!(takes("create").starts_with("`repo [name]`"));
    }

    #[test]
    fn the_picker_and_prompt_rows_come_from_the_tree() {
        let repo = Context::Repo(".".into());
        let (kept, drawn) = choices(&rows(Some("create")), &repo);
        assert_eq!(kept.len(), drawn.len());
        assert!(kept.iter().all(|row| row.path.starts_with("create ")));
        assert!(drawn.iter().all(|choice| !choice.detail.is_empty()));
        assert_eq!(
            host_labels(),
            [
                "why [module]",
                "plan",
                "summary [target]",
                "scap content",
                "scap rules [number]..."
            ]
        );
    }
}
