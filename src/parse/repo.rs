use crate::diag::{Issue, Issues, Source, Span};
use crate::layout;
use crate::model::image::{
    is_name, Backend, Build, Image, List, NetRule, Network, On, Seed, Selection, Trigger, Workflow,
    BUILDC_VERSION, SCHEMA_VERSION,
};
use crate::parse::remote::{self, parse_library};
use crate::parse::schema::{check_doc, Arg, Kind, Node, Prop, Say, NEEDS_VALUE};
use crate::parse::{
    bool_arg, check_sha256, child, int_arg, kids, prop, prop_span, string_arg, string_args,
    syntax_issue,
};
use common::json::Json;
use kdl::{KdlDocument, KdlNode};
use std::path::Path;

#[rustfmt::skip]
const NETWORK: Node = Node::new("network", "The rule that decides whether each step of a component layer reaches the network.")
    .about("The value on the line applies to both kinds of step, and a `packages` or `scripts` \
        child overrides one kind. A denied step runs under `--network=none` in the generated \
        Containerfile.")
    .example("\"strict\"")
    .arg(Arg::MaybeOne(&["allow", "strict"]), Say::new("`{}` is not a network rule",
        "not a rule",
        "`network \"strict\"` holds every component to what it declares; a block sets \
         `packages` and `scripts` one at a time"))
    .once("a second rule would contradict the first")
    .children(&[
        Node::new("packages", "The rule that every package step runs under.").example("\"allow\"")
            .arg(Arg::One(&["allow", "strict", "deny"]), Say::new(
                "`{}` is not a network rule", "not a rule",
                "`packages` takes `allow`, `strict` or `deny`"))
            .once("")
            .values(&[
                ("`allow`", "Every package step reaches the network."),
                ("`strict`", "Every package step reaches the network, because a package step \
                  runs only where the component declares packages, a COPR or a repo file."),
                ("`deny`", "Every package step runs under `--network=none`."),
            ]),
        Node::new("scripts", "The rule that every script step runs under.").example("\"strict\"")
            .arg(Arg::One(&["allow", "strict", "deny"]), Say::new(
                "`{}` is not a network rule", "not a rule",
                "`scripts` takes `allow`, `strict` or `deny`; under `strict` a component \
                 opens its script step with `permissions { scripts { network #true } }`"))
            .once("")
            .values(&[
                ("`allow`", "Every script step reaches the network."),
                ("`strict`", "A script step reaches the network only if its component declares \
                  `permissions { scripts { network #true } }`."),
                ("`deny`", "Every script step runs under `--network=none`."),
            ])
            .notes(&[
                "If the rule is `strict` or `deny`, then `buildc check` names each component that \
                 ships a Containerfile fragment. The rule does not reach the `RUN` lines of a \
                 fragment.",
            ]),
    ], Say::new("unknown node `{}` in network", "not part of the schema",
        "a network block holds `packages` and `scripts`"))
    .lists(&[
        ("A component layer runs two kinds of step:", &[
            "A package step installs the repo files, the COPRs and the packages of the component.",
            "A script step runs the `component.sh` of the component and its finalize hook.",
        ]),
    ])
    .values(&[
        ("`allow`", "Both kinds of step take `allow`, unless `packages` or `scripts` sets \
          another rule."),
        ("`strict`", "Both kinds of step take `strict`, unless `packages` or `scripts` sets \
          another rule."),
        ("No value", "Both kinds of step take `allow`, unless `packages` or `scripts` sets \
          another rule."),
    ]);

#[rustfmt::skip]
const REMOVE: Node = Node::new("remove", "Builds that leave the selection.").example("\"workstation-dev\"")
    .arg(Arg::Strs, Say::new("`remove` needs a build name", "no build given",
        "`remove \"workstation-dev\"`, a name in `builds` in `repo.kdl`"));

#[rustfmt::skip]
const SELECTED: Node = Node::new("builds", "The builds a workflow or a trigger runs for.")
    .about("If `builds` is absent, then a workflow runs for every build. Named builds replace the \
        builds that the enclosing workflow runs for, and `remove` drops a build from them.")
    .example("\"workstation\"")
    .arg(Arg::Many(None), Say::new("a build name has to be a string", "not a string",
        "`builds \"workstation\"`"))
    .once("a second `builds` would split one selection in two")
    .children(&[REMOVE], Say::new("unknown node `{}` in builds", "not part of the schema",
        "`builds` takes build names, and a `remove` child"));

#[rustfmt::skip]
const PUSH: Node = Node::new("push", "Runs the workflow on a push to `main`.")
    .once("a second `push` would say nothing the first did not")
    .children(&[SELECTED], Say::new("unknown node `{}` in push", "not part of the schema",
        "`push` takes a `builds` child"));

#[rustfmt::skip]
const PR: Node = Node::new("pr", "Runs the workflow on a pull request to `main`. A pull request build never publishes.")
    .about("A bare `pr` builds the default build, and `builds` names others.")
    .once("a second `pr` would say nothing the first did not")
    .children(&[SELECTED], Say::new("unknown node `{}` in pr", "not part of the schema",
        "`pr` takes a `builds` child"));

#[rustfmt::skip]
const SCHEDULE: Node = Node::new("schedule", "Runs the workflow on a schedule.")
    .about("A schedule without `frequency` or `time` takes the workflow's shipped one.")
    .example("frequency=\"daily\" time=\"12:30\"")
    .once("one workflow runs on one schedule")
    .props(&[
        Prop { name: "frequency", kind: Kind::One(&["daily", "weekly"]),
            desc: "How often the schedule runs. `weekly` runs on Monday.",
            say: Say::new("`{}` is not a frequency", "not daily or weekly",
                "`frequency=\"daily\"`, or `frequency=\"weekly\"` to run on Monday"),
            missing: Say::NONE },
        Prop { name: "time", kind: Kind::Str,
            desc: "The hour and minute in UTC that the schedule runs at.",
            say: Say::new("`{}` needs an hour and minute", "not a string", "`time=\"04:00\"`, in UTC"),
            missing: Say::NONE },
    ], Say::new("unknown schedule property `{}`", "not part of the schema",
        "a schedule accepts `frequency` and `time`"))
    .children(&[SELECTED], Say::new("unknown node `{}` in schedule", "not part of the schema",
        "a schedule takes `builds`; `frequency` and `time` are properties"));

#[rustfmt::skip]
const TAG: Node = Node::new("tag", "Runs the workflow on a pushed Git tag that matches the pattern. A tag build publishes under the tag.")
    .example("\"v*\"")
    .arg(Arg::Str, Say::new("`tag` needs a tag pattern", "no pattern given", "`tag \"v*\"`"))
    .once("one workflow runs on one tag pattern")
    .children(&[SELECTED], Say::new("unknown node `{}` in tag", "not part of the schema",
        "`tag` takes a `builds` child"));

#[rustfmt::skip]
const WORKFLOW: Node = Node::new("", "One workflow, named by the stem of its file.").example("image-build")
    .scaffolds(&["pr", "push", "schedule"])
    .arg(Arg::Many(Some(&["push", "pr", "schedule"])), Say::new("`{}` is not a bare trigger", "not push, pr or schedule",
        "a bare trigger is `push`, `pr` or `schedule`; `tag` needs a pattern"))
    .children(&[PUSH, PR, SCHEDULE, TAG, SELECTED], Say::new("unknown node `{}` in a workflow", "not part of the schema",
        "a workflow takes the triggers `push`, `pr`, `schedule` and `tag`, and `builds`"));

#[rustfmt::skip]
const WORKFLOWS: Node = Node::new("workflows", "The CI workflows that `buildc generate` writes into `.github/workflows/`.")
    .about("A workflow that is not listed does not run. A workflow with no trigger runs on its \
        shipped triggers, and a listed trigger replaces them. Every workflow also runs by hand.")
    .once("a second block would split one set of workflows in two")
    .empty(Say::new("`workflows` has no workflows in it", "empty block",
        "omit the block entirely; a repository with nothing here generates no CI"))
    .scaffolds(&[""])
    .children(&[WORKFLOW], Say::NONE);

#[rustfmt::skip]
pub(crate) const GITHUB: Node = Node::new("github", "Everything that runs on GitHub.")
    .about("`buildc generate` writes the workflows this block lists into `.github/workflows/`.")
    .scaffolds(&["workflows"])
    .once("a second block would split one configuration in two")
    .children(&[WORKFLOWS], Say::new("unknown node `{}` in github", "not part of the schema",
        "`github` holds `workflows`"))
    .notes(&[
        "`image-build`, `smoke-test` and `build-disk` run once for each build they select. \
         `cleanup-registry`, `update-components`, `kernel-freshness` and `base-sig-probe` run once \
         for the repository, and `buildc check` refuses `builds` on them.",
        "The shipped schedules run at whole hours in UTC, in the order the work depends on: \
         `update-components` at 02:00, `base-sig-probe` and `kernel-freshness` at 03:00, \
         `image-build` at 04:00, `smoke-test` at 06:00 and `cleanup-registry` at 08:00.",
        "`buildc generate` writes only the workflows named here. After a tool upgrade, it rewrites \
         each named workflow.",
        "`buildc set workflows` edits this block.",
    ]);

#[rustfmt::skip]
pub const REPO: Node = Node::new("repo",
    "What is true of the repository as a whole, apart from its images.")
    .about("The file holds at most one of each repository setting, and `buildc` reports an unknown \
        node rather than ignoring it. It must declare `schema-version`, and each image lives in \
        an image file.")
    .scaffolds(&["schema-version", "name", "base-url", "registry", "builds", "libraries", "github", "renovate"])
    .children(&[
        Node::new("schema-version", "The schema release that this repository is written against.")
            .about("The number names the schema release the repository uses, and `buildc` stops at \
                any number it does not implement. It reports the difference and reads no \
                repository node.")
            .example("1")
            .arg(Arg::Int, Say::new("`{}` needs a number", "not a version", "`schema-version 1`"))
            .missing(Say::new("repo.kdl declares no `{}`", "no schema version",
                "`schema-version 1`, so a tool from a different release says so plainly. Without \
                 it, the tool reports every node it does not recognise"))
            .once("")
            .notes(&[
                "The version picks the reader, so a repository written against an earlier \
                 release keeps building.",
                "If `buildc` does not read the version, then it says so once and reports no node \
                 it cannot place.",
                "`buildc` does not move a repository to a newer version. The release that the \
                 repository pins reads it.",
            ]),
        Node::new("buildc-version", "The `buildc` release that builds this repository.")
            .about("The value names the release that builds and reads the repository. `buildc` \
                reports a pin that names another release, and `sha256=` must be 64 lowercase \
                hexadecimal digits.")
            .example("\"0.6.49\"")
            .arg(Arg::Str, Say::new("`{}` needs a release", "not a version",
                concat!("`buildc-version \"", env!("CARGO_PKG_VERSION"),
                        "\"`, the release the build fetches; another release reads the \
                         repository and says so")))
            .props(&[
                Prop { name: "sha256", kind: Kind::Str,
                    desc: "The sha256 that `buildc.sh` holds the release tarball to.",
                    say: Say::new("`{}` must be a hash", "not a string", ""),
                    missing: Say::NONE },
            ], Say::new("unknown buildc-version property `{}`", "not part of the schema",
                "a `buildc-version` accepts `sha256`"))
            .once("")
            .lists(&[
                ("If a different release of `buildc` reads the repository:", &[
                    "`buildc` says once that the pin names a different release.",
                    "`schema-version` decides whether that release can read the repository.",
                    "`buildc verify` reports the difference as drift.",
                    "`buildc generate` resolves the drift.",
                ]),
            ])
            .notes(&[
                "`buildc.sh` fetches this release, so the build does not use the `buildc` \
                 installed on the machine.",
                "If `sha256=` is absent, then `buildc.sh` checks the checksum published \
                 beside the tarball. That check proves only that the download is complete.",
                "If the repository pins no release, then `buildc` says nothing about the release.",
            ]),
        Node::new("name", "An optional human-readable label for the repository.")
            .about("The label is for people, and the directory may use another name. It names no \
                image and no target.")
            .example("\"Workstation\"")
            .arg(Arg::Str, Say::new("`{}` needs a name", "no name given",
                "`name \"Workstation\"`, which the directory is free to disagree with"))
            .once(""),
        Node::new("base-url", "The HTTPS URL of the repository, which an image option value reads as `{base-url}`.")
            .about("`buildc` writes the value in place of each `{base-url}` reference when it generates. The generated files hold the URL itself. The value must start with `https://` and hold none of `\"`, `\\`, `$`, a backtick or a brace. An image option value that reads `{base-url}` fails `buildc check` when the repository declares no `base-url`.")
            .example("\"https://github.com/owner/workstation\"")
            .arg(Arg::Str, Say::new("`{}` needs a URL", "no URL given",
                "`base-url \"https://github.com/owner/workstation\"`"))
            .once(""),
        Node::new("registry", "The namespace that every build publishes under.")
            .about("A build publishes to `<registry>/<name>`. A repository with no `registry` \
                builds and does not push, unless a build's `publish` holds a full reference. \
                `IMAGE_REGISTRY` in the environment overrides the value.")
            .example("\"ghcr.io/owner\"")
            .arg(Arg::Str, Say::new("`{}` needs a namespace", "no namespace given",
                "`registry \"ghcr.io/owner\"`"))
            .once(""),
        Node::new("builds", "The build targets of the repository.")
            .about("Each child is one build. `buildc` refuses two builds with one name.")
            .scaffolds(&[""])
            .example("default=\"workstation\"")
            .once("a second block would split one set of builds in two")
            .empty(Say::new("`builds` has no builds in it", "empty block",
                "name one build, such as `builds { workstation }`, or omit the block"))
            .props(&[
                Prop { name: "default", kind: Kind::Str,
                    desc: "The build that a command uses when it names none. The name must name \
                        a declared build. If the repository declares one build, then that build \
                        is the default.",
                    say: Say::new("`{}` must be a build name", "not a string",
                        "`builds default=\"workstation\"`, naming one of the builds"),
                    missing: Say::NONE },
            ], Say::new("unknown builds property `{}`", "not part of the schema",
                "a builds block accepts `default`"))
            .children(&[
                Node::new("", "One build target, which builds one component and names what it publishes.")
                    .about("The node name is the target, cache and published name, and an option \
                        value reads it as `{image-id}`. The argument names the component that \
                        the build builds. A path that starts with `./` reads `<path>.kdl` or \
                        `<path>/component.kdl` from the repository root, and any other path names \
                        a component in the components folder. With no argument, the build uses \
                        the component of its own name in the components folder.")
                    .example("workstation \"./workstation\"")
                    .scaffolds(&["install"])
                    .arg(Arg::MaybeStr, Say::new("`{}` needs a component path", "not a path",
                        "`workstation \"./workstation\"` for a file at the repository root"))
                    .children(&[
                        Node::new("publish", "The name the build publishes under, in place of its own name.")
                            .about("A bare name renames the artifact under `registry`. A value \
                                that holds `/` is a full reference. Neither holds a tag or a \
                                digest, because the build sets the tags.")
                            .example("\"workstation-nightly\"")
                            .arg(Arg::Str, NEEDS_VALUE).once(""),
                        crate::parse::image::INSTALL,
                    ], Say::new("unknown node `{}` in a build", "not part of the schema",
                        "a build accepts `publish` and `install`")),
            ], Say::NONE),
        Node::new("backend", "The container build tool that `buildc build` runs and that the generated `image-build.yml` sets up.")
            .about("The value selects the one tool that `buildc build` drives, and `buildah` applies \
                when the node is absent. `buildx` needs a default build to name the layer cache, \
                and it cannot build a build that seals a UKI.")
            .example("\"buildx\"")
            .arg(Arg::One(&["buildah", "buildx"]), Say::new("`{}` is not a build backend",
                "not a backend", "`backend` takes `buildah` or `buildx`"))
            .once("a repository builds with one backend")
            .values(&[
                ("`buildah`", "`buildc build` runs `podman build`. A build that seals a UKI \
                  needs it."),
                ("`buildx`", "`buildc build` runs `docker buildx build`. The generated \
                  `image-build.yml` loads its OCI archive into the podman store that the later \
                  steps read, and it can use a registry layer cache."),
            ])
            .notes(&[
                "`--backend` or `$BUILD_BACKEND` overrides it for one build. The flag wins \
                 over the variable, and the variable wins over this node.",
                "`buildc check` refuses `buildx` for a build that seals a UKI. A sealed UKI \
                 carries the composefs digest, and `buildc build` reads that digest from the \
                 local buildah store.",
            ]),
        Node::new("seed",
            "The image that this repository publishes as a starting point for a new repository.")
            .about("The image must be declared in the repository, and `collection=` must name a \
                library that `libraries` declares. Every component in the seed is \
                fetched through that collection, so an inline definition leaves the seed \
                unbuildable.")
            .example("\"workstation\" collection=\"owner\"")
            .arg(Arg::Str, Say::new("`{}` needs an image id", "no image given",
                "`seed \"workstation\" collection=\"owner\"`, naming one of the images declared \
                 at the root"))
            .once("a repository publishes one seed")
            .props(&[
                Prop { name: "collection", kind: Kind::Str,
                    desc: "The collection that this repository publishes its own components as.",
                    say: Say::new("`{}` must be a collection name", "not a string", ""),
                    missing: Say::new("`{}` says nothing about where its components are published",
                        "no `collection`",
                        "`{} collection=\"owner\"`, one of the libraries in `libraries`: every \
                         component in a seed is fetched through one, so a repository publishing no \
                         collection of its own has nothing a seeded repository can import") },
            ], Say::new("unknown seed property `{}`", "not part of the schema",
                "a seed accepts `collection`"))
            .lists(&[
                ("`buildc generate` writes the seed to `generated/seed.kdl`. The seed carries:", &[
                    "the base of the image;",
                    "the component list of the image;",
                    "the collections that those components come from.",
                ]),
            ])
            .notes(&[
                "The seed carries no name, URL or owner of this repository.",
                "The seed names each component through the collection it is fetched from, so \
                 `collection=` names a library in `libraries`. A repository is seedable only \
                 if it publishes its own `components/` as a collection.",
                "If the seed image lists a component that no collection can import, then `buildc` \
                 reports it, because a new repository could not build that seed.",
            ]),
        GITHUB,
        Node::new("renovate", "The Renovate policy that `buildc generate` writes into `.github/renovate.json5`.")
            .about("The block maps member by member to Renovate's JSON, so a property is a scalar, \
                arguments are an array and a child is an object. A node that holds no value, or \
                both values and members, is refused.")
            .example("rebaseWhen=\"never\"")
            .scaffolds(&[""])
            .once("a second block would split one policy in two")
            .children(&[
                Node::new("", "One Renovate setting, named as Renovate names it.").example("schedule \"* 0-5 * * *\""),
            ], Say::NONE)
            .notes(&[
                "A property is a scalar setting. The arguments of a child are an array, because \
                 Renovate wants array syntax for a list of one.",
                "The properties and named children of a child are an object. A block of `-` \
                 children is an array, and each `-` holds one value or one object.",
                "`$schema`, `ignorePaths`, `dockerfile` and `customManagers` belong to `buildc \
                 generate`, and the block refuses them.",
                "If the block is absent, then `buildc generate` writes no `.github/renovate.json5`, \
                 and leaves one the user wrote in place.",
            ]),
        Node::new("libraries", "The libraries this repository reads components from.")
            .about("`local` holds the libraries that the user edits in place, and `remote` holds \
                the libraries that buildc reads at a commit. Each child of a block is one library, \
                named by its node. Declaring a library makes its components available, and a build \
                uses one only when the repository names it. If no `local` block is declared, then \
                the repository root is the local library. If a `local` block is declared, then \
                buildc reads the repository's own components only through a local library whose \
                source is `.`.")
            .scaffolds(&["local", "remote"])
            .example("default=\"stable\"")
            .once("a second block would split one set of libraries in two")
            .empty(Say::new("`libraries` has no libraries in it", "empty block",
                "omit the block entirely; a repository that reads no other library declares nothing here"))
            .props(&[
                Prop { name: "default", kind: Kind::Str,
                    desc: "The library that a path from a library root reads when several \
                        libraries hold it. The name must name a declared library. If `default=` \
                        is absent, then the only local library is the default.",
                    say: Say::new("`{}` must be a library name", "not a string",
                        "`libraries default=\"stable\"`, naming one of the libraries"),
                    missing: Say::NONE },
            ], Say::new("unknown libraries property `{}`", "not part of the schema",
                "a libraries block accepts `default`"))
            .children(&[remote::LOCAL, remote::REMOTE], Say::new(
                "unknown node `{}` in libraries", "not part of the schema",
                "a library goes in `local` or `remote`: `remote { stable { source \"https://github.com/owner/library\" } }`"))
            .notes(&[
                "`buildc` never reads the `libraries` block of a library, so the repository declares \
                 every library its builds use.",
            ]),
        Node::new("library", "Where this repository keeps the components it offers as a library.")
            .about("A repository that another repository declares as a library offers the \
                components in this folder. The repository's own builds read them from there too.")
            .once("a second block would split one library in two")
            .empty(Say::new("`library` has nothing in it", "empty block",
                "omit the block entirely; the components then live in `components/`"))
            .children(&[
                Node::new("components", "The folder that holds the components, relative to the repository root.")
                    .about("If `components` is absent, then the components live in `components/`.")
                    .example("\"src/components\"")
                    .arg(Arg::Str, Say::new("`components` needs a folder", "not a string",
                        "`components \"src/components\"`"))
                    .once(""),
            ], Say::new("unknown node `{}` in `library`", "not part of the schema",
                "a library block holds `components`")),
        Node::new("manifest",
            "Whether a build stamps the generated manifest onto the image as an OCI label.")
            .about("The block holds one `label`, and a build without the block stamps no label. \
                `label #true` writes `org.buildc.manifest` with the path where the manifest \
                sits in the image.")
            .once("a second block would split one setting in two")
            .empty(Say::new("`manifest` has no `label` in it", "empty block",
                "omit the block entirely; a build with nothing here stamps no label"))
            .children(&[
                Node::new("label",
                    "Whether the build stamps `org.buildc.manifest` with the path of the \
                     baked manifest file.").example("#true")
                    .arg(Arg::Bool, Say::new("`label` needs #true or #false", "not a boolean",
                        "`label #true` stamps the built image with an `org.buildc.manifest` \
                         label"))
                    .once(""),
            ], Say::new("unknown node `{}` in manifest", "not part of the schema",
                "a manifest block holds `label`")),

        Node::new("audit", "How the repository treats a provenance check that fails.")
            .about("The posture applies to every provenance check that `buildc` runs. The facts are \
                recorded whether or not the block exists, and `enforce #true` is what makes a \
                failed check stop the command.")
            .once("a second block would split one posture in two")
            .empty(Say::new("`audit` has no `enforce` in it", "empty block",
                "omit the block entirely; a repository with nothing here records every \
                 provenance fact and fails on none of them"))
            .children(&[
                Node::new("enforce", "Whether a failed provenance check stops the command that runs it.").example("#true")
                    .arg(Arg::Bool, Say::new("`enforce` needs #true or #false", "not a boolean",
                        "`enforce #true` makes each failed provenance check an error, and \
                         the `repo.kdl` reference lists the checks"))
                    .once("")
                    .values(&[
                        ("`#true`", "A failed check is an error, and the command stops."),
                        ("`#false`, or no `enforce`", "`buildc` reports a failed check, and the \
                          command continues."),
                    ])
                    .lists(&[
                        ("Each check runs in the command named before it:", &[
                            "`buildc build`: the base tag resolves to a manifest digest.",
                            "`buildc build`: the repository is at a commit.",
                            "`buildc build`: each cloned asset resolves to a commit.",
                            "`buildc scap`: the scan report passes every rule of the profile.",
                        ]),
                    ])
                    .notes(&[
                        "`buildc` also reports a `component.sh` or `finalize.sh` that reaches the \
                         network with no `asset` that declares the download. This report does \
                         not depend on `enforce`.",
                        "`buildc` refuses a remote component whose cached content no longer \
                         matches its entry in `repo.lock.kdl`. This refusal does not depend on \
                         `enforce`.",
                    ]),
            ], Say::new("unknown node `{}` in audit", "not part of the schema",
                "an audit block holds `enforce`"))
            .lists(&[
                ("`buildc` records these provenance facts whether or not this block exists:", &[
                    "the hash of each component;",
                    "the source of each import;",
                    "the digest of the base tag;",
                    "the commit of each cloned asset.",
                ]),
            ]),
        Node::new("security-policy",
            "The security rules that the repository sets for every image it defines.")
            .about("The block holds one `network` rule for every image the repository declares. \
                An image cannot set a rule of its own.")
            .once("a second block would contradict the first")
            .children(&[NETWORK],
                Say::new("unknown node `{}` in security-policy", "not part of the schema",
                    "a security-policy block holds a `network` rule"))
            .notes(&[
                "If the `security-policy` block is absent, then every step keeps the network.",
            ]),
    ], Say::new("unknown node `{}` in repo.kdl", "not part of the schema",
        "repo.kdl holds `schema-version`, `buildc-version`, `name`, `base-url`, `registry`, \
         `backend`, `seed`, a `builds` block, a `github` block, \
         a `libraries` block, a `library` block, a `renovate` block, a `manifest` block, an `audit` block and a \
         `security-policy` block. These nodes state what is true of the repository. An image \
         is the component that a build in `builds` names"));

fn network(node: &KdlNode) -> Network {
    let both = string_arg(node).and_then(NetRule::of).unwrap_or_default();
    let kind = |name| child(node, name).and_then(string_arg).and_then(NetRule::of);
    Network {
        packages: kind("packages").unwrap_or(both),
        scripts: kind("scripts").unwrap_or(both),
    }
}

struct Pins {
    schema: Option<(i128, Span)>,
    buildc: Option<(String, Span)>,
    buildc_sha: Option<String>,
    src: Source,
}

fn pins(root: &Path) -> Option<Pins> {
    let path = root.join(layout::REPO_FILE);
    let text = std::fs::read_to_string(&path).ok()?;
    let doc: KdlDocument = text.parse().ok()?;
    let node = |name: &str| {
        doc.nodes()
            .iter()
            .find(|n| n.name().value() == name)
            .cloned()
    };
    Some(Pins {
        schema: node("schema-version").and_then(|n| Some((int_arg(&n)?, n.name().span().into()))),
        buildc: node("buildc-version")
            .and_then(|n| Some((string_arg(&n)?.to_string(), n.name().span().into()))),
        buildc_sha: node("buildc-version").and_then(|n| prop(&n, "sha256").map(str::to_string)),
        src: Source::new(path.display().to_string(), text),
    })
}

pub fn compatible(root: &Path) -> Issues {
    let mut issues = Issues::default();
    let Some(pins) = pins(root) else {
        return issues;
    };

    if let Some((version, span)) = pins
        .schema
        .filter(|(v, _)| *v != i128::from(SCHEMA_VERSION))
    {
        let ahead = version > i128::from(SCHEMA_VERSION);
        issues.push(
            Issue::new(
                format!("this repository is written against schema version {version}"),
                &pins.src,
            )
            .at(span, format!("this tool knows {SCHEMA_VERSION}"))
            .help(match ahead {
                true => "the repository is ahead of the tool; `buildc-version` names the release \
                         that reads it, and `buildc.sh` fetches that one"
                    .to_string(),
                false => format!(
                    "nothing here moves a repository to schema {SCHEMA_VERSION}, and nothing \
                     else in it is read either, because every diagnostic under a grammar this \
                     release does not have would be noise; run the release `buildc-version` names, \
                     which `buildc.sh` fetches"
                ),
            })
            .blocks_edit(),
        );
        return issues;
    }

    issues
}

pub fn pinned_elsewhere(root: &Path) -> Option<String> {
    let version = pins(root)?.buildc?.0;
    (version != BUILDC_VERSION).then_some(version)
}

pub fn pinned_unverified(root: &Path) -> Option<String> {
    let pins = pins(root)?;
    pins.buildc
        .filter(|_| pins.buildc_sha.is_none())
        .map(|(version, _)| version)
}

pub fn libraries_in(text: &str) -> Vec<crate::model::remote::Collection> {
    let src = Source::new(layout::REPO_FILE, text);
    let Ok(doc) = text.parse::<KdlDocument>() else {
        return Vec::new();
    };
    let mut list = List::empty(Path::new(""));
    let mut issues = Issues::default();
    for node in doc
        .nodes()
        .iter()
        .filter(|n| n.name().value() == "libraries")
    {
        for (kind, library) in remote::declared(node) {
            list.parse_library(library, kind, &src, &mut issues);
        }
    }
    list.libraries
}

/// A local library source can name a folder that does not exist yet.
/// The longest existing prefix resolves its symlinks and the missing segments apply by name.
/// `top` must be canonical because the function compares it with a canonical path.
fn inside(top: &Path, dir: &Path) -> bool {
    let parts: Vec<std::path::Component> = dir.components().collect();
    let Some((mut found, kept)) = (0..=parts.len()).rev().find_map(|kept| {
        let prefix: std::path::PathBuf = parts[..kept].iter().collect();
        std::fs::canonicalize(&prefix)
            .ok()
            .map(|found| (found, kept))
    }) else {
        return false;
    };
    for part in &parts[kept..] {
        match part {
            std::path::Component::ParentDir => {
                found.pop();
            }
            std::path::Component::CurDir => {}
            other => found.push(other),
        }
    }
    found.starts_with(top)
}

pub fn default_library_in(text: &str) -> Option<String> {
    let doc: KdlDocument = text.parse().ok()?;
    prop(doc.get("libraries")?, "default").map(str::to_string)
}

fn valid_folder(path: &str) -> bool {
    !path.starts_with('/')
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

pub fn components_folder(text: &str) -> Option<String> {
    let doc: KdlDocument = text.parse().ok()?;
    let library = doc.nodes().iter().find(|n| n.name().value() == "library")?;
    let folder = string_arg(child(library, "components")?)?;
    valid_folder(folder).then(|| folder.to_string())
}

pub fn base_url(text: &str) -> Option<String> {
    let doc: KdlDocument = text.parse().ok()?;
    let node = doc
        .nodes()
        .iter()
        .find(|n| n.name().value() == "base-url")?;
    string_arg(node).map(str::to_string)
}

impl List {
    pub fn load(root: &Path) -> (Self, Issues) {
        Self::load_following(root, false)
    }

    pub(crate) fn load_following(root: &Path, follow: bool) -> (Self, Issues) {
        let issues = compatible(root);
        match issues.is_empty() {
            true => Self::read(root, follow),
            false => (Self::empty(root), issues),
        }
    }

    pub(crate) fn editable(root: &Path) -> Result<Self, Issues> {
        let (list, issues) = Self::load(root);
        let blocking = issues.blocking_edits();
        match blocking.is_empty() {
            true => Ok(list),
            false => Err(blocking),
        }
    }

    pub(super) fn empty(root: &Path) -> Self {
        Self {
            name: String::new(),
            base_url: String::new(),
            id: String::new(),
            images: Vec::new(),
            builds: Vec::new(),
            registry: None,
            workflows: Vec::new(),
            libraries: Vec::new(),
            root_library: None,
            default_library: None,
            resolver: crate::resolve::library::Libraries::read(root, &[], None, None, Vec::new()),
            default_image_id: None,
            seed: None,
            manifest_label: false,
            backend: Backend::default(),
            renovate: None,
            audit_enforce: false,
            network: Network::default(),
            schema_version: None,
            repo_src: Source::new(root.join(layout::REPO_FILE).display().to_string(), ""),
            files: Vec::new(),
        }
    }

    fn read(root: &Path, follow: bool) -> (Self, Issues) {
        let mut issues = Issues::default();
        let mut list = Self::empty(root);

        if let Some(found) = pins(root) {
            list.schema_version = found.schema.and_then(|(version, _)| {
                u32::try_from(version)
                    .ok()
                    .filter(|version| *version == SCHEMA_VERSION)
            });
            list.repo_src = found.src;
        }

        let mut names: Vec<String> = Vec::new();
        match std::fs::read_dir(root) {
            Ok(entries) => {
                for entry in entries.flatten() {
                    if !entry.path().is_file() {
                        continue;
                    }
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if name.ends_with(".kdl") {
                        names.push(name);
                    }
                }
            }
            Err(err) => {
                issues.push(
                    Issue::new(
                        format!("cannot read {}: {err}", root.display()),
                        &list.repo_src,
                    )
                    .blocks_edit(),
                );
                return (list, issues);
            }
        }
        names.sort();

        let mut strays = Vec::new();
        for name in names
            .iter()
            .filter(|name| *name != crate::provenance::repo_lock::FILE)
        {
            if name != layout::REPO_FILE {
                strays.push(name);
                continue;
            }
            let path = root.join(name).display().to_string();
            let text = match std::fs::read_to_string(root.join(name)) {
                Ok(text) => text,
                Err(err) => {
                    issues.push(
                        Issue::new(
                            format!("cannot read {path}: {err}"),
                            &Source::new(&path, ""),
                        )
                        .blocks_edit(),
                    );
                    continue;
                }
            };
            list.files.push(path.clone());
            let src = Source::new(&path, text.clone());
            list.repo_src = src.clone();
            list.parse_file(&src, &text, &mut issues);
        }
        (list.libraries, list.root_library) =
            crate::resolve::library::split_root(root, std::mem::take(&mut list.libraries));
        list.check_remote_paths(root, &mut issues);
        list.check_local_paths(root, &mut issues);
        list.load_images(root, &mut issues);
        for image in &mut list.images {
            image.repo_base_url.clone_from(&list.base_url);
        }
        let mut root_files: Vec<(String, std::path::PathBuf)> = list
            .images
            .iter()
            .filter_map(|image| {
                let home = image.entries.first()?.home.clone()?;
                Some((
                    image
                        .component
                        .strip_prefix("./")
                        .unwrap_or(&image.component)
                        .to_string(),
                    home,
                ))
            })
            .collect();
        let mut at = 0;
        let mut reached = true;
        while at < root_files.len() {
            let name = root_files[at].0.clone();
            at += 1;
            let Some(children) = crate::parse::component::root_imports(
                root,
                &name,
                list.schema_version,
                &list.repo_src,
            ) else {
                reached = false;
                break;
            };
            for child in children {
                if root_files.iter().any(|(held, _)| held == &child) {
                    continue;
                }
                let home = root.join(&child);
                // The reach skips a missing root file so the resolver reports the reference.
                if layout::manifest_at(&home).is_file() {
                    let file = layout::manifest_at(&home);
                    if git_ignores(root, &[&file, &home]) {
                        issues.push(
                            Issue::new(
                                format!("`component \"./{child}\"` reads a file that git ignores"),
                                &Source::new(file.display().to_string(), ""),
                            )
                            .help(
                                "a build reads a file that the repository carries, so CI reads \
                                 it too",
                            ),
                        );
                    }
                    root_files.push((child, home));
                }
            }
        }
        if reached {
            for name in strays {
                let stem = name.trim_end_matches(".kdl");
                if root_files.iter().any(|(held, _)| held == stem) {
                    continue;
                }
                if list
                    .builds
                    .iter()
                    .any(|build| build.component.strip_prefix("./") == Some(stem))
                {
                    continue;
                }
                let path = root.join(name).display().to_string();
                issues.push(
                    Issue::new(format!("nothing reads `{name}`"), &Source::new(&path, "")).help(
                        format!("a build reads a file at the repository root when `builds` names it, as in `{stem} \"./{stem}\"`. A root file that a build reaches names another root file with `component \"./{stem}\"`"),
                    ),
                );
            }
        }
        list.resolver = crate::resolve::library::Libraries::read(
            root,
            &list.libraries,
            list.root_library.as_deref(),
            list.default_library.as_ref().map(|(name, _)| name.as_str()),
            root_files,
        );
        list.resolver.follow = follow;
        list.load_bases(root, &mut issues);
        list.check_backend(&mut issues);
        list.check_names(root, &mut issues);
        (list, issues)
    }

    fn check_backend(&self, issues: &mut Issues) {
        if self.backend == Backend::Buildx && self.cache_image().is_none() {
            issues.push(
                Issue::new(
                    "`backend \"buildx\"` needs a default build to name the layer cache",
                    &self.repo_src,
                )
                .help(
                    "the registry layer cache is one repository named after the default build: \
                     declare `builds default=\"<name>\"`",
                ),
            );
        }
    }

    fn load_bases(&mut self, root: &Path, issues: &mut Issues) {
        let disk = crate::parse::disk::Disk::scan_at(
            &crate::layout::components(root),
            &self.resolver.collector_names(),
        );
        let version = self.schema_version;
        let repo_src = &self.repo_src;
        let mut load = |entry: &crate::model::image::Entry,
                        image: &crate::model::image::Image,
                        issues: &mut Issues| {
            crate::model::component::Component::load(
                entry, image, root, &disk, version, repo_src, issues,
            )
        };
        check_keyword_dirs(root, issues);
        for image in &mut self.images {
            // This early read takes the image fields and finds the base.
            // It drops its issues so that `crate::load` reports them once when it reads the component again.
            let own = crate::model::component::Component::load(
                &image.entries[0],
                image,
                root,
                &disk,
                version,
                repo_src,
                &mut Issues::default(),
            );
            if let Some(own) = &own {
                image.description.clone_from(&own.description);
                image.keywords.clone_from(&own.keywords);
                image.logo_url.clone_from(&own.logo_url);
                image.conforms.clone_from(&own.conforms);
                image.allows.clone_from(&own.allows);
                let own_path = image.component.clone();
                for (name, given) in [
                    ("description", !own.description.is_empty()),
                    ("keywords", !own.keywords.is_empty()),
                    ("logo-url", !own.logo_url.is_empty()),
                    ("conforms", !own.conforms.is_empty()),
                ] {
                    if given {
                        image.field_source(name, &own_path);
                    }
                }
                crate::resolve::imports::place(image, own, &self.resolver);
            }
            image.base_entry = take_base(image, own.as_ref(), &self.resolver, &mut load, issues);
            if let Some(held) = &mut image.base_entry {
                if let Err(why) = self.resolver.revise(&mut held.entry, &image.id) {
                    issues.push(
                        Issue::new(why, &image.src)
                            .at(held.entry.span, "the base")
                            .help("the build reads a remote component at one commit"),
                    );
                }
            }
            if let Some(held) = &image.base_entry {
                let component = crate::model::component::Component::load(
                    &held.entry,
                    image,
                    root,
                    &disk,
                    version,
                    repo_src,
                    issues,
                );
                let base = component.and_then(|component| {
                    if component.path != image.component {
                        component.refuse_image_facts(image, issues);
                    }
                    based(held, component, &image.src, issues)
                });
                image.base = base;
            }
        }
    }

    fn parse_file(&mut self, src: &Source, text: &str, issues: &mut Issues) {
        let doc: KdlDocument = match text.parse() {
            Ok(doc) => doc,
            Err(err) => {
                issues.push(syntax_issue(&err, src.name(), src));
                return;
            }
        };

        check_doc(&doc, &REPO, src, issues);

        for node in doc.nodes() {
            match node.name().value() {
                "builds" => {
                    self.default_image_id = prop(node, "default").map(str::to_string);
                    for build in kids(node) {
                        self.parse_build(build, src, issues);
                    }
                }
                "registry" => {
                    self.registry = string_arg(node).map(str::to_string);
                    if let Some(bad) = self.registry.as_deref().filter(|at| !lowercase_path(at)) {
                        issues.push(
                            Issue::new(format!("`{bad}` holds a capital letter in its path"), src)
                                .at(Span::from(node.span()), "not lowercase")
                                .help(
                                    "an image repository path is lowercase, as in \
                                     `registry \"ghcr.io/owner\"`",
                                ),
                        );
                    }
                }
                "github" => {
                    if let Some(block) = child(node, "workflows") {
                        self.parse_workflows(block, src, issues);
                    }
                }
                "renovate" => self.renovate = Some(renovate(node, src, issues)),
                "libraries" => {
                    self.default_library = prop(node, "default").map(|name| {
                        (
                            name.to_string(),
                            prop_span(node, "default").unwrap_or_default(),
                        )
                    });
                    for (kind, library) in remote::declared(node) {
                        self.parse_library(library, kind, src, issues);
                    }
                }
                "library" => {
                    if let Some(kid) = child(node, "components") {
                        let folder = string_arg(kid).unwrap_or_default();
                        if !valid_folder(folder) {
                            issues.push(
                                Issue::new(format!("`{folder}` is not a components folder"), src)
                                    .at(kid.name().span(), "not a clean relative path")
                                    .help("name a folder inside the repository without `/` at the start and without `.` or `..` segments"),
                            );
                        }
                    }
                }
                "backend" => {
                    if let Some(backend) = string_arg(node).and_then(Backend::parse) {
                        self.backend = backend;
                    }
                }
                "seed" => {
                    self.seed = string_arg(node).map(|image| Seed {
                        image: image.to_string(),
                        collection: prop(node, "collection").unwrap_or_default().to_string(),
                    });
                }
                "schema-version" => {
                    self.schema_version = int_arg(node).map(|_| SCHEMA_VERSION);
                }
                "buildc-version" => {
                    if let Some(sha256) = prop(node, "sha256") {
                        check_sha256(
                            sha256,
                            "`buildc-version`",
                            prop_span(node, "sha256").unwrap_or_default(),
                            src,
                            issues,
                        );
                    }
                }
                "name" => {
                    self.name = string_arg(node).unwrap_or_default().to_string();
                    self.id = self.name.to_lowercase().replace(' ', "-");
                }
                "base-url" => {
                    let url = string_arg(node).unwrap_or_default();
                    match url.starts_with("https://") && !url.contains(['"', '\\', '$', '`', '{', '}']) {
                        true => self.base_url = url.to_string(),
                        false => issues.push(
                            Issue::new(format!("`{url}` is not a usable `base-url`"), src)
                                .at(node.name().span(), "not an HTTPS URL that a value can hold")
                                .help("`base-url \"https://host/owner/repository\"`: an HTTPS URL with none of `\"`, `\\`, `$`, a backtick or a brace"),
                        ),
                    }
                }
                "audit" => {
                    self.audit_enforce = child(node, "enforce").and_then(bool_arg).unwrap_or(false)
                }
                "security-policy" => {
                    self.network = child(node, "network").map(network).unwrap_or_default()
                }
                "manifest" => {
                    self.manifest_label = child(node, "label").and_then(bool_arg).unwrap_or(false);
                }
                _ => {}
            }
        }
    }

    fn parse_build(&mut self, node: &KdlNode, src: &Source, issues: &mut Issues) {
        let name = node.name().value().to_string();
        let span: Span = node.name().span().into();
        if !is_name(&name) {
            issues.push(
                Issue::new(format!("`{name}` is not a build name"), src)
                    .at(span, "not a build name")
                    .help("a build name takes lowercase letters, digits and dashes, starting with a letter"),
            );
            return;
        }
        if let Some(dup) = self.builds.iter().find(|build| build.name == name) {
            issues.push(
                Issue::new(format!("build `{name}` is declared twice"), src)
                    .at(dup.span, "first here")
                    .at(span, "and again here")
                    .help("one name is one target and one published image; rename one build"),
            );
            return;
        }
        let component = string_arg(node).map_or_else(|| name.clone(), str::to_string);
        let path = component.strip_prefix("./").unwrap_or(&component);
        if !path.split('/').all(crate::model::image::is_segment) {
            issues.push(
                Issue::new(format!("`{component}` is not a component path"), src)
                    .at(span, "not a path inside the repository")
                    .help(
                        "a component path is lowercase segments joined by `/`, such as \
                         `./workstation` at the repository root or `images/workstation` in the \
                         components folder",
                    ),
            );
            return;
        }
        let publish_node = child(node, "publish");
        let mut publish = publish_node.and_then(string_arg).map(str::to_string);
        if let (Some(bad), Some(at)) = (
            publish.as_deref().filter(|publish| !is_published(publish)),
            publish_node,
        ) {
            issues.push(
                Issue::new(
                    format!("`{bad}` is not an image name to publish under"),
                    src,
                )
                .at(at.span(), "not an image name")
                .help(
                    "`publish` takes a bare name or a reference without a tag or digest, \
                         such as `\"workstation-nightly\"` or `\"quay.io/team/box\"`; the build \
                         sets the tags itself",
                ),
            );
            publish = None;
        }
        self.builds.push(Build {
            component,
            name,
            publish,
            install: child(node, "install")
                .map(|install| crate::parse::image::install(install, src)),
            span,
        });
    }

    fn declares(&self, name: &str) -> bool {
        self.root_library.as_deref() == Some(name)
            || self.libraries.iter().any(|library| library.name == name)
    }

    fn check_local_paths(&self, root: &Path, issues: &mut Issues) {
        let Ok(top) = std::fs::canonicalize(root) else {
            return;
        };
        for library in &self.libraries {
            let crate::model::remote::At::Dir(dir) = &library.at else {
                continue;
            };
            if inside(&top, &top.join(dir)) {
                continue;
            }
            issues.push(
                Issue::new(
                    format!(
                        "local library `{}` is outside this repository",
                        library.name
                    ),
                    &self.repo_src,
                )
                .at(library.span, "outside")
                .help(format!(
                    "a local library is a folder of this repository; move `{}` under `remote` to \
                     read a Git checkout elsewhere at a commit",
                    library.name
                ))
                .blocks_edit(),
            );
        }
    }

    fn check_remote_paths(&self, root: &Path, issues: &mut Issues) {
        for library in &self.libraries {
            let Some(source) = library.pin().and_then(|pin| pin.url.as_deref()) else {
                continue;
            };
            if let Err(why) = crate::collection::git_source(root, source) {
                issues.push(
                    Issue::new(
                        format!("remote library `{}`: {why}", library.name),
                        &self.repo_src,
                    )
                    .at(library.span, "not a Git checkout")
                    .help(format!(
                        "a remote path names an existing Git checkout. buildc reads it at a commit; \
                             correct `source`, or move `{}` under `local` if the folder is inside this repository",
                        library.name
                    )),
                );
            }
        }
    }

    fn load_images(&mut self, root: &Path, issues: &mut Issues) {
        for build in &self.builds {
            let at_root = build.component.strip_prefix("./");
            let rel = at_root.unwrap_or(&build.component);
            let dir = match at_root {
                Some(path) => root.join(path),
                None => layout::component(root, rel),
            };
            let files = layout::manifests_at(&dir);
            let Some(file) = files.first().cloned() else {
                issues.push(
                    Issue::new(
                        format!(
                            "build `{}` names `{}`, which is not a component",
                            build.name, build.component
                        ),
                        &self.repo_src,
                    )
                    .at(build.span, "no manifest")
                    .help(format!(
                        "write `{}`, or name another component as the build's argument",
                        layout::under(root, &layout::sibling(&dir)).display()
                    )),
                );
                continue;
            };
            if let Some(issue) = crate::parse::component::two_manifests(&build.component, &files) {
                issues.push(issue);
                continue;
            }
            if at_root.is_some() && git_ignores(root, &[&file, &dir]) {
                issues.push(
                    Issue::new(
                        format!(
                            "build `{}` names `{}`, which git ignores",
                            build.name, build.component
                        ),
                        &self.repo_src,
                    )
                    .at(build.span, "an ignored path")
                    .help("a build reads a file that the repository carries, so CI reads it too"),
                );
                continue;
            }
            let path = file.display().to_string();
            let text = match std::fs::read_to_string(&file) {
                Ok(text) => text,
                Err(err) => {
                    issues.push(
                        Issue::new(
                            format!("cannot read {path}: {err}"),
                            &Source::new(&path, ""),
                        )
                        .blocks_edit(),
                    );
                    continue;
                }
            };
            let src = Source::new(&path, text.clone());
            if let Err(err) = text.parse::<KdlDocument>() {
                issues.push(syntax_issue(&err, src.name(), &src));
                continue;
            }
            if !self.files.contains(&path) {
                self.files.push(path);
            }
            let mut image = Image::new(&build.name, rel, at_root.is_some(), &src, issues);
            image.entries.push(crate::model::image::Entry {
                path: rel.to_string(),
                home: at_root.map(|_| dir.clone()),
                ..Default::default()
            });
            image.file = format!("./{}", layout::under(root, &file).display());
            image.layout.clone_from(&build.install);
            self.images.push(image);
        }
    }

    fn check_names(&self, root: &Path, issues: &mut Issues) {
        if let Some((name, span)) = &self.default_library {
            if !self.declares(name) {
                issues.push(
                    Issue::new(
                        format!(
                            "`libraries default=` names `{name}`, which is not a declared library"
                        ),
                        &self.repo_src,
                    )
                    .at(*span, "not declared")
                    .help(format!(
                        "libraries: {}",
                        self.root_library
                            .iter()
                            .chain(self.libraries.iter().map(|library| &library.name))
                            .map(String::as_str)
                            .collect::<Vec<_>>()
                            .join(", ")
                    )),
                );
            }
        }

        if let Some(id) = &self.default_image_id {
            if !self.images.iter().any(|i| &i.id == id) {
                issues.push(
                    Issue::new(
                        format!("`builds default=` names `{id}`, which is not a declared build"),
                        &self.repo_src,
                    )
                    .help(format!(
                        "builds: {}",
                        self.images
                            .iter()
                            .map(|i| i.id.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )),
                );
            }
        }

        if let Some(seed) = &self.seed {
            self.check_seed(seed, root, issues);
        }
    }

    fn check_seed(&self, seed: &Seed, root: &Path, issues: &mut Issues) {
        let Some(image) = self.images.iter().find(|i| i.id == seed.image) else {
            issues.push(
                Issue::new(
                    format!(
                        "`seed` names `{}`, which is not a declared image",
                        seed.image
                    ),
                    &self.repo_src,
                )
                .help("a repository publishes a seed of one of the images declared at its root"),
            );
            return;
        };

        if !self.declares(&seed.collection) {
            issues.push(
                Issue::new(
                    format!(
                        "`{}` is not a library this repository declares",
                        seed.collection
                    ),
                    &self.repo_src,
                )
                .help(
                    "a repository is seedable only if it publishes its own components/ as a \
                     library, and declares it in `libraries` under the name a seeded repository \
                     reads it by",
                ),
            );
        }

        // A listed component has content of its own.
        // The refusal reads the build's component and no other.
        let own = image
            .entries
            .iter()
            .chain(image.base_entry.iter().map(|held| &held.entry))
            .find(|entry| entry.path == image.component);
        if own.is_some_and(|entry| declares_own(image, entry, root)) {
            issues.push(
                Issue::new(
                    format!(
                        "`{}` declares more than the components it lists, so the seed cannot name it",
                        image.component
                    ),
                    &image.src,
                )
                .help(
                    "a seed lists the components that the build's component lists; move its own \
                     declarations into a component and list that",
                ),
            );
        }
        for entry in image
            .entries
            .iter()
            .chain(image.base_entry.iter().map(|held| &held.entry))
            .filter(|entry| entry.inline.is_some())
        {
            issues.push(
                Issue::new(
                    format!(
                        "`{}` is defined inline, so the seed cannot name it",
                        entry.path
                    ),
                    &image.src,
                )
                .at(entry.span, "defined in the image file")
                .help(
                    "a seed lists a component by its collection's name; move the definition into \
                     components/ and publish it with the collection",
                ),
            );
        }
        for entry in image
            .entries
            .iter()
            .filter(|entry| entry.inline.is_none() && entry.path != image.component)
        {
            let qualified = entry.qualified(&seed.collection);
            let owner = qualified.split('/').next().unwrap_or_default();
            if self.declares(owner) {
                continue;
            }
            issues.push(
                Issue::new(
                    format!("`{}` is in no collection the seed can name", entry.path),
                    &image.src,
                )
                .at(
                    entry.span,
                    format!("`{owner}` is not declared in `libraries`"),
                )
                .help(
                    "a seed lists a component by name and nothing else, so one nothing can import \
                     leaves a seeded repository unbuildable",
                ),
            );
        }
    }

    fn parse_workflows(&mut self, block: &KdlNode, src: &Source, issues: &mut Issues) {
        for node in kids(block) {
            let name = node.name().value().to_string();
            let span: Span = node.name().span().into();
            if let Some(dup) = self.workflows.iter().find(|w| w.name == name) {
                issues.push(
                    Issue::new(format!("workflow `{name}` is declared twice"), src)
                        .at(dup.span, "first here")
                        .at(span, "and again here")
                        .help("a workflow is either generated or absent, so naming it twice says nothing the once did not"),
                );
                continue;
            }
            let mut triggers: Vec<Trigger> = Vec::new();
            for word in string_args(node) {
                let on = match word {
                    "push" => On::Push,
                    "pr" => On::Pr,
                    "schedule" => On::Schedule {
                        weekly: None,
                        time: None,
                    },
                    _ => continue,
                };
                if triggers
                    .iter()
                    .any(|held| std::mem::discriminant(&held.on) == std::mem::discriminant(&on))
                {
                    issues.push(
                        Issue::new(format!("`{name}` lists `{word}` twice"), src)
                            .at(span, "here")
                            .help("write the trigger once, as a bare argument or as a block"),
                    );
                    continue;
                }
                triggers.push(Trigger {
                    on,
                    builds: Selection::default(),
                    span,
                });
            }
            for given in kids(node) {
                let at: Span = given.name().span().into();
                let on = match given.name().value() {
                    "push" => On::Push,
                    "pr" => On::Pr,
                    "schedule" => On::Schedule {
                        weekly: prop(given, "frequency").map(|frequency| frequency == "weekly"),
                        time: schedule_time(given, src, issues),
                    },
                    "tag" => On::Tag(string_arg(given).unwrap_or_default().to_string()),
                    _ => continue,
                };
                if let Some(held) = triggers
                    .iter()
                    .find(|held| std::mem::discriminant(&held.on) == std::mem::discriminant(&on))
                {
                    issues.push(
                        Issue::new(
                            format!("`{name}` lists `{}` twice", given.name().value()),
                            src,
                        )
                        .at(held.span, "first here")
                        .at(at, "and again here")
                        .help("write the trigger once, as a bare argument or as a block"),
                    );
                    continue;
                }
                triggers.push(Trigger {
                    on,
                    builds: selection(given),
                    span: at,
                });
            }
            self.workflows.push(Workflow {
                name,
                span,
                builds: selection(node),
                triggers,
            });
        }
    }

    fn parse_library(
        &mut self,
        node: &KdlNode,
        kind: remote::Kind,
        src: &Source,
        issues: &mut Issues,
    ) {
        let Some(library) = parse_library(node, kind, src, issues) else {
            return;
        };
        if let Some(dup) = self.libraries.iter().find(|held| held.name == library.name) {
            issues.push(
                Issue::new(format!("library `{}` is declared twice", library.name), src)
                    .at(dup.span, "first here")
                    .at(library.span, "and again here")
                    .help("one name is one library; merge the fields or rename one declaration"),
            );
            return;
        }
        self.libraries.push(library);
    }
}

const RENOVATE_OWNED: [&str; 4] = ["$schema", "ignorePaths", "dockerfile", "customManagers"];

fn renovate(block: &KdlNode, src: &Source, issues: &mut Issues) -> Vec<(String, Json)> {
    let names = block
        .entries()
        .iter()
        .filter_map(|entry| entry.name().map(|name| (name.value(), entry.span())))
        .chain(
            kids(block)
                .iter()
                .map(|child| (child.name().value(), child.name().span())),
        );
    for (name, span) in names {
        if RENOVATE_OWNED.contains(&name) {
            issues.push(
                Issue::new(format!("`{name}` belongs to `buildc generate`"), src)
                    .at(span, "the generated settings carry it")
                    .help(
                        "`buildc generate` writes `$schema`, `ignorePaths`, `dockerfile` and \
                         `customManagers` to match the files it writes",
                    ),
            );
        }
    }
    renovate_object(block, src, issues)
}

fn renovate_object(node: &KdlNode, src: &Source, issues: &mut Issues) -> Vec<(String, Json)> {
    let mut members: Vec<(String, Json)> = Vec::new();
    let mut add = |name: &str, span: miette::SourceSpan, value: Json, issues: &mut Issues| {
        if members.iter().any(|(held, _)| held == name) {
            issues.push(
                Issue::new(format!("`{name}` is declared twice"), src)
                    .at(span, "a JSON object holds one member of each name"),
            );
            return;
        }
        members.push((name.to_string(), value));
    };
    for entry in node.entries() {
        if let Some(name) = entry.name() {
            let value = json_value(entry.value(), entry.span(), src, issues);
            add(name.value(), entry.span(), value, issues);
        }
    }
    for child in kids(node) {
        let name = child.name().value();
        if name == "-" {
            issues.push(
                Issue::new("a `-` item sits beside named members", src)
                    .at(child.name().span(), "an array item")
                    .help(
                        "a block of `-` items is an array, and a block of named nodes is an object",
                    ),
            );
            continue;
        }
        let value = renovate_member(child, src, issues);
        add(name, child.name().span(), value, issues);
    }
    members
}

fn renovate_member(node: &KdlNode, src: &Source, issues: &mut Issues) -> Json {
    let args: Vec<Json> = node
        .entries()
        .iter()
        .filter(|entry| entry.name().is_none())
        .map(|entry| json_value(entry.value(), entry.span(), src, issues))
        .collect();
    let props = node.entries().iter().any(|entry| entry.name().is_some());
    let children = kids(node);
    let name = node.name().value();
    if !args.is_empty() {
        if props || !children.is_empty() {
            issues.push(
                Issue::new(format!("`{name}` holds both values and members"), src)
                    .at(
                        node.name().span(),
                        "arguments are an array, members are an object",
                    )
                    .help(
                        "give it arguments for an array, or properties and children for an object",
                    ),
            );
        }
        return Json::Array(args);
    }
    if !props && !children.is_empty() && children.iter().all(|c| c.name().value() == "-") {
        return Json::Array(
            children
                .iter()
                .map(|item| renovate_item(item, src, issues))
                .collect(),
        );
    }
    if !props && children.is_empty() {
        issues.push(
            Issue::new(format!("`{name}` holds no value"), src)
                .at(node.name().span(), "nothing to map")
                .help("give it arguments for an array, or properties and children for an object"),
        );
    }
    Json::Object(renovate_object(node, src, issues))
}

fn renovate_item(node: &KdlNode, src: &Source, issues: &mut Issues) -> Json {
    let args: Vec<&kdl::KdlEntry> = node
        .entries()
        .iter()
        .filter(|entry| entry.name().is_none())
        .collect();
    match args.as_slice() {
        [] => Json::Object(renovate_object(node, src, issues)),
        [one] if node.entries().len() == 1 && kids(node).is_empty() => {
            json_value(one.value(), one.span(), src, issues)
        }
        _ => {
            issues.push(
                Issue::new("a `-` item holds one value or one object", src)
                    .at(node.name().span(), "more than one")
                    .help("write `- \"value\"`, or `- key=\"value\" { ... }` for an object"),
            );
            Json::Null
        }
    }
}

fn json_value(
    value: &kdl::KdlValue,
    span: miette::SourceSpan,
    src: &Source,
    issues: &mut Issues,
) -> Json {
    match value {
        kdl::KdlValue::String(text) => Json::string(text.as_str()),
        kdl::KdlValue::Bool(flag) => Json::Bool(*flag),
        kdl::KdlValue::Null => Json::Null,
        kdl::KdlValue::Integer(_) | kdl::KdlValue::Float(_) => {
            match Json::parse(&value.to_string()) {
                Ok(number) => number,
                Err(_) => {
                    issues.push(
                        Issue::new(format!("`{value}` is not a JSON number"), src)
                            .at(span, "JSON has no infinity or NaN"),
                    );
                    Json::Null
                }
            }
        }
    }
}

fn schedule_time(node: &KdlNode, src: &Source, issues: &mut Issues) -> Option<(u32, u32)> {
    let text = prop(node, "time")?;
    let parsed = time(text);
    if let (None, Some(at)) = (parsed, prop_span(node, "time")) {
        issues.push(
            Issue::new(format!("`{text}` is not a time of day"), src)
                .at(at, "not `HH:MM`")
                .help("`time=\"04:00\"`, the hour and minute in UTC"),
        );
    }
    parsed
}

fn git_ignores(root: &Path, paths: &[&Path]) -> bool {
    // The checkout's own config can set `core.fsmonitor` to a program that git would run.
    // The ceiling keeps git from reading a repository above the root.
    std::process::Command::new("git")
        .args([
            "--no-optional-locks",
            "-c",
            "core.fsmonitor=false",
            "check-ignore",
            "--",
        ])
        .args(paths)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .current_dir(root)
        .env("GIT_CEILING_DIRECTORIES", root.parent().unwrap_or(root))
        .status()
        .is_ok_and(|status| status.success())
}

fn selection(node: &KdlNode) -> Selection {
    let Some(builds) = child(node, "builds") else {
        return Selection::default();
    };
    Selection {
        named: string_args(builds)
            .iter()
            .map(ToString::to_string)
            .collect(),
        removed: kids(builds)
            .iter()
            .filter(|n| n.name().value() == "remove")
            .flat_map(|n| string_args(n).into_iter().map(str::to_string))
            .collect(),
        span: Some(builds.name().span().into()),
    }
}

pub fn time(value: &str) -> Option<(u32, u32)> {
    let (hour, minute) = value.split_once(':')?;
    let (hour, minute) = (hour.parse().ok()?, minute.parse().ok()?);
    (hour < 24 && minute < 60).then_some((hour, minute))
}

/// An empty `triggers` or `builds` keeps what the existing entry writes for it.
/// If both are empty, then the function returns `None` and the file keeps the text it holds.
/// The returned text sits at column zero and the calling command indents it to its depth.
pub fn workflow_entry(
    text: &str,
    stem: &str,
    triggers: &[String],
    builds: &[String],
) -> Option<String> {
    if triggers.is_empty() && builds.is_empty() {
        return None;
    }
    let doc: Option<KdlDocument> = text.parse().ok();
    let held = doc.as_ref().and_then(|doc| {
        doc.nodes()
            .iter()
            .filter(|node| node.name().value() == "github")
            .filter_map(|github| child(github, "workflows"))
            .flat_map(kids)
            .find(|node| node.name().value() == stem)
    });
    let source = |node: &KdlNode| {
        let span = node.span();
        let column = text[..span.offset()]
            .rsplit('\n')
            .next()
            .map_or(0, str::len);
        let indent = " ".repeat(column);
        let written = text
            .get(span.offset()..span.offset() + span.len())
            .unwrap_or_default()
            .trim();
        written
            .lines()
            .map(|line| {
                line.strip_prefix(indent.as_str())
                    .unwrap_or(line.trim_start())
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let mut head = stem.to_string();
    let (mut on, mut selection) = (triggers.to_vec(), Vec::new());
    if let Some(held) = held {
        if triggers.is_empty() {
            for value in held.entries() {
                head.push(' ');
                head.push_str(value.to_string().trim());
            }
        }
        for child in kids(held) {
            match child.name().value() == "builds" {
                true if builds.is_empty() => selection.push(source(child)),
                false if triggers.is_empty() => on.push(source(child)),
                _ => {}
            }
        }
    }
    if !builds.is_empty() {
        let named: Vec<String> = builds.iter().map(|build| format!("{build:?}")).collect();
        selection.push(format!("builds {}", named.join(" ")));
    }
    let children = [on, selection].concat();
    Some(match children.is_empty() {
        true => head,
        false => {
            let body: String = children
                .iter()
                .flat_map(|child| child.lines())
                .map(|line| format!("    {line}\n"))
                .collect();
            format!("{head} {{\n{body}}}")
        }
    })
}

fn is_published(publish: &str) -> bool {
    let mut parts = publish.split('/');
    // The host part may carry a port after a `:`.
    // Only the last part is read for a tag or a digest.
    let image = parts.next_back().unwrap_or_default();
    let component = |part: &str| {
        part.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
            && part.chars().all(|c| {
                c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-')
            })
    };
    component(image) && parts.all(|part| !part.is_empty()) && lowercase_path(publish)
}

/// The container reference grammar reads the first part as a registry host if it holds a `.` or a `:` or is `localhost`.
/// A host is case-insensitive and only the repository path after it must be lowercase.
fn lowercase_path(reference: &str) -> bool {
    let mut parts = reference.split('/').peekable();
    if let Some(first) = parts.peek() {
        let host = first.contains(['.', ':']) || *first == "localhost";
        if host && reference.contains('/') {
            parts.next();
        }
    }
    parts.all(|part| !part.chars().any(|c| c.is_ascii_uppercase()))
}

pub fn github_span(text: &str) -> Option<Span> {
    let doc: KdlDocument = text.parse().ok()?;
    let node = doc.nodes().iter().find(|n| n.name().value() == "github")?;
    Some(node.span().into())
}

pub fn derived_id(text: &str) -> Option<String> {
    let doc: KdlDocument = text.parse().ok()?;
    let name = doc
        .nodes()
        .iter()
        .find(|node| node.name().value() == "name")
        .and_then(string_arg)?;
    crate::init::id(name).ok()
}

pub fn add_build(text: &str, build: &str, default: Option<&str>) -> Option<String> {
    let doc: KdlDocument = text.parse().ok()?;
    let Some(builds) = doc
        .nodes()
        .iter()
        .find(|node| node.name().value() == "builds")
    else {
        let default = default
            .map(|name| format!(" default=\"{name}\""))
            .unwrap_or_default();
        let gap = match text.is_empty() || text.ends_with('\n') {
            true => "",
            false => "\n",
        };
        return Some(format!(
            "{text}{gap}\nbuilds{default} {{\n    {build}\n}}\n"
        ));
    };
    let mut out = match super::block_close(builds, text) {
        Some(close) => crate::parse::declaration::inside(text, close, build),
        None => {
            let span: Span = builds.span().into();
            let end = text[..span.offset + span.len].trim_end().len();
            let mut out = text.to_string();
            out.insert_str(end, &format!(" {{\n    {build}\n}}"));
            out
        }
    };
    if let Some(name) = default.filter(|_| prop(builds, "default").is_none()) {
        let name_span: Span = builds.name().span().into();
        out.insert_str(
            name_span.offset + name_span.len,
            &format!(" default=\"{name}\""),
        );
    }
    Some(out)
}

fn declares_own(
    image: &crate::model::image::Image,
    entry: &crate::model::image::Entry,
    root: &Path,
) -> bool {
    let nodes = image
        .src
        .text()
        .parse::<KdlDocument>()
        .map(|doc| doc.nodes().to_vec())
        .unwrap_or_default();
    nodes.iter().any(|node| {
        let name = node.name().value();
        crate::parse::image::is_manifest_node(name)
            && !crate::parse::image::is_header(name)
            && !crate::parse::tree::is_context(node)
    }) || layout::has_content(&entry.dir(root))
}

fn take_base(
    image: &mut crate::model::image::Image,
    own: Option<&crate::model::component::Component>,
    libraries: &crate::resolve::library::Libraries,
    load: &mut impl FnMut(
        &crate::model::image::Entry,
        &crate::model::image::Image,
        &mut Issues,
    ) -> Option<crate::model::component::Component>,
    issues: &mut Issues,
) -> Option<crate::model::image::BaseEntry> {
    let own_base = own.is_some_and(|component| component.base.is_some());
    // The image's first entry is the build's own component.
    let nested = match own {
        Some(component) => {
            let own_entry = image.entries.first()?;
            crate::resolve::imports::find_bases(
                image, own_entry, component, libraries, load, issues,
            )
        }
        None => Vec::new(),
    };
    let mut bases: Vec<crate::model::image::Entry> = own_base
        .then(|| image.entries.remove(0))
        .into_iter()
        .collect();
    bases.extend(nested);
    let src = &image.src;
    if bases.is_empty() {
        issues.push(
            Issue::new(format!("`{}` lists no base", image.id), src)
                .at(image.span, "nothing to build on")
                .help(
                    "list a base component, such as `base-image { fedora-bootc { version \
                     \"44\" } }`, or define one inline with `base \"<image reference>\"`",
                ),
        );
        return None;
    }
    if bases.len() > 1 {
        let mut issue = Issue::new(format!("`{}` lists two bases", image.id), src)
            .help("an image builds on one base; a second base is a second image");
        for entry in &bases {
            issue = issue.at(entry.span, "a base");
        }
        issues.push(issue);
        return None;
    }
    let entry = bases.pop().expect("one base");
    image.entries.retain(|other| other.path != entry.path);
    for other in &image.entries {
        crate::parse::tree::refuse_base_facts(
            &other.path,
            &[
                ("digest", other.digest.as_ref().map(|(_, span)| *span)),
                ("signed", other.signed.map(|(_, span)| span)),
                ("key", other.key.as_ref().map(|(_, span)| *span)),
                (
                    "key-sha256",
                    other.key_sha256.as_ref().map(|(_, span)| *span),
                ),
                ("identity", other.identity.as_ref().map(|(_, span)| *span)),
                ("issuer", other.issuer.as_ref().map(|(_, span)| *span)),
            ],
            src,
            issues,
        );
    }
    let facts = own_base
        .then(|| own.and_then(|component| component.base.as_ref()))
        .flatten();
    let digest = match facts {
        Some(facts) => facts.digest.clone(),
        None => entry.digest.as_ref().map(|(digest, _)| digest.clone()),
    };
    let signed = match facts {
        Some(facts) => facts.signed,
        None => entry.signed.is_some_and(|(signed, _)| signed),
    };
    let verify = match facts {
        Some(facts) => facts.verify.clone(),
        None => crate::model::component::TrustRoot::of(
            entry.key.as_ref(),
            entry.key_sha256.as_ref(),
            entry.identity.as_ref(),
            entry.issuer.as_ref(),
        ),
    };
    Some(crate::model::image::BaseEntry {
        entry,
        digest,
        signed,
        verify,
    })
}

fn check_keyword_dirs(root: &Path, issues: &mut Issues) {
    let Ok(dirs) = std::fs::read_dir(layout::components(root)) else {
        return;
    };
    for dir in dirs.flatten() {
        let file = dir.file_name().to_string_lossy().into_owned();
        let name = match dir.path().is_dir() {
            true => file.as_str(),
            false => file.strip_suffix(".kdl").unwrap_or_default(),
        };
        if crate::parse::image::is_manifest_node(name) {
            let src = Source::new(dir.path().display().to_string(), String::new());
            issues.push(
                Issue::new(
                    format!("components/{file} is named after a manifest node"),
                    &src,
                )
                .help(format!(
                    "a component file reads a bare `{name}` as the manifest node, so no tree \
                     can list this component; rename it"
                )),
            );
        }
    }
}

fn based(
    held: &crate::model::image::BaseEntry,
    component: crate::model::component::Component,
    src: &Source,
    issues: &mut Issues,
) -> Option<crate::model::image::Base> {
    let Some(facts) = component.base else {
        issues.push(
            Issue::new(format!("`{}` is not a base", held.entry.path), src)
                .at(held.entry.span, "names no base image")
                .help("a base component declares `base \"<image reference>\"` and its `family`"),
        );
        return None;
    };
    let dir = &component.home;
    let ships = [
        layout::SCRIPT,
        layout::REPO_SCRIPT,
        layout::PACKAGES_SCRIPT,
        layout::FINALIZE_SCRIPT,
        layout::FRAGMENT,
    ]
    .iter()
    .any(|file| dir.join(file).is_file())
        || dir.join(layout::OVERLAY).is_dir()
        || !component.packages.is_empty()
        || !component.groups.is_empty()
        || !component.coprs.is_empty();
    if ships {
        issues.push(
            Issue::new(
                format!(
                    "`{}` is a base, and a base builds no layer",
                    held.entry.path
                ),
                src,
            )
            .at(
                held.entry.span,
                "its packages, scripts or files/ would not build",
            )
            .help("move that work into a component that the base imports"),
        );
    }
    let tracker = component
        .options
        .into_iter()
        .find(|opt| opt.name == "version")
        .and_then(|opt| opt.renovate)
        .or_else(|| {
            facts.template.strip_suffix(":{version}").map(|repository| {
                crate::model::options::VersionTracker {
                    datasource: "docker".to_string(),
                    dep_name: repository.to_string(),
                    extract_version: None,
                }
            })
        });
    let digest = held.digest.clone().or_else(|| facts.digest.clone());
    let image = match digest {
        Some(digest) => format!("{}@{digest}", facts.image),
        None => facts.image.clone(),
    };
    let signed = held.entry.signed.map_or(facts.signed, |(signed, _)| signed);
    // A segment that declares the base unsigned also drops the verifier declared on the base node.
    let verify = match held.entry.signed {
        Some((false, _)) => None,
        _ => held.verify.clone().or_else(|| facts.verify.clone()),
    };
    Some(crate::model::image::Base {
        path: held.entry.path.clone(),
        library: component.library.clone(),
        image,
        family: facts.family,
        provides: component.provides,
        requires: component.requires,
        imports: component.imports,
        src: component.src,
        satisfies: component.satisfies,
        signed,
        verify,
        scap_content: facts.scap_content,
        scap_pin: facts.scap_pin,
        bootloaders: facts.bootloaders,
        files: component.files,
        tracker,
        span: held.entry.span,
    })
}

#[cfg(test)]
mod tests {

    use super::*;

    fn root(name: &str, text: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("buildc-pin-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a temp root");
        std::fs::write(dir.join(layout::REPO_FILE), text).expect("repo.kdl");
        dir
    }

    #[test]
    fn a_pin_naming_another_release_is_reported_and_this_one_is_not() {
        let other = root("other", "schema-version 1\nbuildc-version \"0.0.1\"\n");
        assert_eq!(pinned_elsewhere(&other).as_deref(), Some("0.0.1"));
        let ours = root(
            "ours",
            &format!("schema-version 1\nbuildc-version \"{BUILDC_VERSION}\"\n"),
        );
        assert_eq!(pinned_elsewhere(&ours), None);
        let none = root("none", "schema-version 1\n");
        assert_eq!(pinned_elsewhere(&none), None);
    }

    #[test]
    fn a_buildx_backend_needs_one_build_or_a_default_to_name_its_cache() {
        let mut list = List::empty(Path::new("."));
        list.backend = Backend::Buildx;
        let mut issues = Issues::default();
        let src = Source::new(layout::REPO_FILE, "");
        list.images
            .push(Image::new("one", "one", false, &src, &mut issues));
        list.check_backend(&mut issues);
        assert!(issues.plain().is_empty(), "{}", issues.plain());

        list.images
            .push(Image::new("two", "two", false, &src, &mut issues));
        list.check_backend(&mut issues);
        assert!(
            issues.plain().contains("needs a default build"),
            "{}",
            issues.plain()
        );

        let mut issues = Issues::default();
        list.default_image_id = Some("one".to_string());
        list.check_backend(&mut issues);
        assert!(issues.plain().is_empty(), "{}", issues.plain());
    }

    #[test]
    fn a_pin_naming_another_release_refuses_nothing() {
        let dir = root("open", "schema-version 1\nbuildc-version \"0.0.1\"\n");
        let issues = compatible(&dir);
        assert!(issues.is_empty(), "{}", issues.plain());
    }

    #[test]
    fn every_declared_file_names_a_compatible_schema() {
        let dir = root("file-schemas", "schema-version 1\nbuilds { example }\n");
        let file = layout::components(&dir).join("example.kdl");
        crate::init::put(&file, "schema-version 3\n\nexample\nhello\n").unwrap();
        let image = crate::load(&dir).issues.plain();
        assert!(image.contains("example.kdl declares schema"), "{image}");
        assert!(image.contains("version 3, but"), "{image}");
        assert!(image.contains("repo.kdl declares 1"), "{image}");

        std::fs::write(&file, "schema-version 1\n\nexample\nhello\n").unwrap();
        std::fs::create_dir_all(dir.join("components/hello")).unwrap();
        let manifest = dir.join("components/hello/component.kdl");
        std::fs::write(&manifest, "packages \"hello\"\n").unwrap();
        let missing = crate::load(&dir).issues.plain();
        assert!(missing.contains("component.kdl"), "{missing}");
        assert!(
            missing.contains("this file declares no `schema-version`"),
            "{missing}"
        );

        std::fs::write(&manifest, "schema-version 3\n\npackages \"hello\"\n").unwrap();
        let component = crate::load(&dir).issues.plain();
        assert!(component.contains("component.kdl"), "{component}");
        assert!(component.contains("version 3, but"), "{component}");
        assert!(component.contains("repo.kdl declares 1"), "{component}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_declared_sha256_is_the_only_verifier() {
        let hash = "a".repeat(64);
        let declared = root(
            "sha",
            &format!(
                "schema-version 1\n  buildc-version \"{BUILDC_VERSION}\" sha256=\"{hash}\"   // pinned\n"
            ),
        );
        assert_eq!(pinned_unverified(&declared), None);
        let bare = root("bare", "schema-version 1\nbuildc-version \"0.0.1\"\n");
        assert_eq!(pinned_unverified(&bare).as_deref(), Some("0.0.1"));
        let none = root("unpinned", "schema-version 1\n");
        assert_eq!(pinned_unverified(&none), None);
    }

    #[test]
    fn malformed_buildc_hashes_are_issues() {
        for sha256 in ["", "bad"] {
            let text = format!(
                "schema-version 1\nbuildc-version \"{BUILDC_VERSION}\" sha256=\"{sha256}\"\nname \"Example\"\n"
            );
            let mut list = List::empty(Path::new("."));
            let mut issues = Issues::default();
            list.parse_file(&Source::new(layout::REPO_FILE, &text), &text, &mut issues);
            let found = issues.plain();
            assert!(
                found.contains("`buildc-version` has a malformed sha256"),
                "{found}"
            );
        }
    }

    fn messages(text: &str) -> Vec<String> {
        let doc: KdlDocument = text.parse().expect("valid KDL");
        let src = Source::new(layout::REPO_FILE, text);
        let mut issues = Issues::default();
        check_doc(&doc, &REPO, &src, &mut issues);
        issues.findings()
    }

    #[test]
    fn the_table_catches_what_the_corpus_does_not() {
        let found = messages(
            r#"
schema-version
schema-version 1
name "Buildc"
buildc-version sha256=1 wat="x"
buildc-version "0.0.0"
builds wat="x" {
    one 1
    two { colour "red" }
}
github {
    workflows {
        smoke-test "on" push
        image-build { pull }
    }
}
colour "blue"
"#,
        );
        assert_eq!(
            found,
            [
                "`schema-version` needs a number",
                "`schema-version` is declared twice",
                "`buildc-version` needs a release",
                "`sha256` must be a hash",
                "unknown buildc-version property `wat`",
                "`buildc-version` is declared twice",
                "unknown builds property `wat`",
                "`one` needs a component path",
                "unknown node `colour` in a build",
                "`on` is not a bare trigger",
                "unknown node `pull` in a workflow",
                "unknown node `colour` in repo.kdl",
            ]
        );
    }

    #[test]
    fn a_repository_name_is_optional() {
        assert!(messages("schema-version 1\n").is_empty());
    }

    #[test]
    fn an_empty_workflows_block_is_a_block_with_nothing_in_it() {
        let found = messages("schema-version 1\nname \"Buildc\"\ngithub {\n    workflows { }\n}\n");
        assert_eq!(found, ["`workflows` has no workflows in it"]);
    }

    fn workflows(text: &str) -> (Vec<crate::resolve::workflow::Declared>, String) {
        let mut list = List::empty(Path::new("."));
        let mut issues = Issues::default();
        list.parse_file(&Source::new(layout::REPO_FILE, text), text, &mut issues);
        let basis = crate::resolve::workflow::Basis {
            selinux: true,
            kernel: true,
        };
        let declared = crate::resolve::workflow::resolve(&list, &basis, &mut issues);
        (declared, issues.plain())
    }

    fn listed(body: &str) -> String {
        format!("schema-version 1\nbuilds default=\"one\" {{\n    one\n    two\n}}\ngithub {{\n    workflows {{\n{body}    }}\n}}\n")
    }

    #[test]
    fn a_bare_workflow_runs_at_its_shipped_hour() {
        let (declared, said) = workflows(&listed(
            "        image-build\n        smoke-test\n        build-disk\n",
        ));
        assert!(said.is_empty(), "{said}");
        let cron = |file: &str| {
            declared
                .iter()
                .find(|declared| declared.file == file)
                .and_then(|declared| declared.cron.clone())
        };
        assert_eq!(cron("image-build.yml").as_deref(), Some("0 4 * * *"));
        assert_eq!(cron("smoke-test.yml").as_deref(), Some("0 6 * * 1"));
        assert_eq!(cron("build-disk.yml"), None);
        assert!(declared.iter().all(|declared| declared.triggers.is_none()));
    }

    #[test]
    fn a_schedule_takes_its_written_frequency_and_time() {
        let (declared, said) = workflows(&listed(
            "        smoke-test {\n            push\n            schedule frequency=\"daily\" time=\"12:30\"\n        }\n        update-components schedule\n",
        ));
        assert!(said.is_empty(), "{said}");
        let smoke = declared
            .iter()
            .find(|d| d.file == "smoke-test.yml")
            .unwrap();
        assert_eq!(smoke.cron.as_deref(), Some("30 12 * * *"));
        assert_eq!(smoke.triggers.as_ref().map(Vec::len), Some(2));
        let update = declared
            .iter()
            .find(|d| d.file == "update-components.yml")
            .unwrap();
        assert_eq!(update.cron.as_deref(), Some("0 2 * * 1"));
    }

    #[test]
    fn a_schedule_without_a_time_on_a_workflow_that_ships_none_is_refused() {
        let (_, said) = workflows(&listed("        build-disk { schedule }\n"));
        assert!(said.contains("`build-disk` ships no schedule"), "{said}");
        let (declared, said) =
            workflows(&listed("        build-disk { schedule time=\"05:00\" }\n"));
        assert!(said.is_empty(), "{said}");
        assert_eq!(declared[0].cron.as_deref(), Some("0 5 * * *"));
    }

    #[test]
    fn a_time_that_is_not_a_time_of_day_is_refused() {
        let (_, said) = workflows(&listed("        image-build { schedule time=\"25:00\" }\n"));
        assert!(said.contains("`25:00` is not a time of day"), "{said}");
    }

    #[test]
    fn builds_on_a_repository_workflow_and_an_unknown_build_are_refused() {
        let (_, said) = workflows(&listed(
            "        cleanup-registry { builds \"one\" }\n        image-build { push { builds { remove \"three\" } } }\n",
        ));
        assert!(
            said.contains("`cleanup-registry` runs once for the repository"),
            "{said}"
        );
        assert!(
            said.contains("`image-build` selects `three`, which is not a build"),
            "{said}"
        );
    }

    #[test]
    fn a_trigger_written_twice_is_refused() {
        let (_, said) = workflows(&listed(
            "        image-build push {\n            push\n        }\n",
        ));
        assert!(said.contains("`image-build` lists `push` twice"), "{said}");
        let (_, said) = workflows(&listed("        image-build push push\n"));
        assert!(said.contains("`image-build` lists `push` twice"), "{said}");
    }

    #[test]
    fn bare_triggers_are_arguments_and_a_bare_pr_builds_the_default() {
        let text = "schema-version 1\nregistry \"ghcr.io/owner\"\nbuilds default=\"one\" {\n    one\n    two\n}\ngithub {\n    workflows {\n        image-build pr schedule\n        smoke-test\n    }\n}\n";
        let mut list = List::empty(Path::new("."));
        let mut issues = Issues::default();
        list.parse_file(&Source::new(layout::REPO_FILE, text), text, &mut issues);
        assert!(issues.is_empty(), "{}", issues.plain());
        assert_eq!(list.registry.as_deref(), Some("ghcr.io/owner"));
        let kinds: Vec<&On> = list.workflows[0].triggers.iter().map(|t| &t.on).collect();
        assert_eq!(
            kinds,
            [
                &On::Pr,
                &On::Schedule {
                    weekly: None,
                    time: None
                }
            ]
        );
        use crate::resolve::workflow::{builds_on, dispatched_builds};
        assert_eq!(builds_on(&list, "image-build", &On::Pr), ["one"]);
        assert_eq!(
            builds_on(
                &list,
                "image-build",
                &On::Schedule {
                    weekly: None,
                    time: None
                }
            ),
            ["one", "two"]
        );
        assert!(builds_on(&list, "image-build", &On::Push).is_empty());
        let mut bare = List::empty(Path::new("."));
        let text = text.replace("image-build pr schedule", "image-build");
        bare.parse_file(&Source::new(layout::REPO_FILE, &text), &text, &mut issues);
        assert_eq!(builds_on(&bare, "image-build", &On::Pr), ["one"]);
        assert_eq!(builds_on(&bare, "image-build", &On::Push), ["one", "two"]);
        assert_eq!(dispatched_builds(&bare, "image-build"), ["one", "two"]);
        assert!(dispatched_builds(&bare, "cleanup-registry").is_empty());
    }

    #[test]
    fn each_trigger_selects_its_builds() {
        let text = "schema-version 1\nbuilds default=\"one\" {\n    one\n    dev\n    release\n}\ngithub {\n    workflows {\n        image-build {\n            pr { builds \"dev\" }\n            push { builds { remove \"release\" } }\n            tag \"v*\" { builds \"release\" }\n        }\n        smoke-test { builds { remove \"dev\" } }\n    }\n}\n";
        let mut list = List::empty(Path::new("."));
        let mut issues = Issues::default();
        list.parse_file(&Source::new(layout::REPO_FILE, text), text, &mut issues);
        use crate::resolve::workflow::builds_on;
        let on = |stem: &str, on: On| builds_on(&list, stem, &on);
        assert_eq!(on("image-build", On::Pr), ["dev"]);
        assert_eq!(on("image-build", On::Push), ["one", "dev"]);
        assert_eq!(on("image-build", On::Tag(String::new())), ["release"]);
        assert!(on(
            "image-build",
            On::Schedule {
                weekly: None,
                time: None
            }
        )
        .is_empty());
        let scheduled = On::Schedule {
            weekly: None,
            time: None,
        };
        assert_eq!(on("smoke-test", scheduled), ["one", "release"]);
        assert!(on("smoke-test", On::Push).is_empty());
        assert!(on("cleanup-registry", On::Push).is_empty());
    }

    fn renovate_of(text: &str) -> (Option<String>, String) {
        let text = format!("schema-version 1\n{text}");
        let mut list = List::empty(Path::new("."));
        let mut issues = Issues::default();
        list.parse_file(
            &Source::new(layout::REPO_FILE, text.as_str()),
            &text,
            &mut issues,
        );
        let json = list.renovate.map(|members| Json::Object(members).render());
        (json, issues.plain())
    }

    #[test]
    fn a_renovate_block_maps_to_renovate_json() {
        let (json, said) = renovate_of(
            "renovate rebaseWhen=\"never\" limit=2 {\n    schedule \"* 0-5 * * *\"\n    \
             packageRules {\n        - automerge=#true {\n            matchPackageNames \"*\"\n        \
             }\n    }\n    lockFileMaintenance enabled=#true\n    extends {\n        - \"a\"\n    }\n}\n",
        );
        assert!(said.is_empty(), "{said}");
        let expected = Json::parse(
            r#"{"rebaseWhen": "never", "limit": 2, "schedule": ["* 0-5 * * *"],
                "packageRules": [{"automerge": true, "matchPackageNames": ["*"]}],
                "lockFileMaintenance": {"enabled": true}, "extends": ["a"]}"#,
        )
        .unwrap()
        .render();
        assert_eq!(json.as_deref(), Some(expected.as_str()));
    }

    #[test]
    fn a_repository_with_no_renovate_block_has_no_policy() {
        assert_eq!(renovate_of("").0, None);
    }

    #[test]
    fn the_renovate_block_refuses_what_does_not_map() {
        for (text, refusal) in [
            (
                "renovate {\n    customManagers \"x\"\n}\n",
                "belongs to `buildc generate`",
            ),
            (
                "renovate {\n    a \"x\"\n    - \"y\"\n}\n",
                "sits beside named members",
            ),
            (
                "renovate {\n    a \"x\" {\n        b \"y\"\n    }\n}\n",
                "holds both values and members",
            ),
            ("renovate {\n    a\n}\n", "holds no value"),
            (
                "renovate {\n    a \"x\"\n    a \"y\"\n}\n",
                "is declared twice",
            ),
            (
                "renovate {\n    a {\n        - \"x\" \"y\"\n    }\n}\n",
                "holds one value or one object",
            ),
            ("renovate {\n    a #inf\n}\n", "is not a JSON number"),
            ("renovate {\n    a {\n        - \"x\" y=1\n    }\n}\n", "holds one value or one object"),
            ("renovate {\n    a {\n        - \"x\" {\n            b \"y\"\n        }\n    }\n}\n", "holds one value or one object"),
        ] {
            let (_, said) = renovate_of(text);
            assert!(said.contains(refusal), "{text}: {said}");
        }
    }

    #[test]
    fn a_build_component_outside_the_repository_is_refused() {
        for component in [
            "../outside",
            "/etc/x",
            "images/../../x",
            "Up",
            "./../outside",
            "./a/../../x",
            "./",
        ] {
            let text = format!("schema-version 1\nbuilds {{ x \"{component}\" }}\n");
            let mut list = List::empty(Path::new("."));
            let mut issues = Issues::default();
            list.parse_file(&Source::new(layout::REPO_FILE, &text), &text, &mut issues);
            assert!(list.builds.is_empty(), "{component}");
            assert!(
                issues.plain().contains("is not a component path"),
                "{component}: {}",
                issues.plain()
            );
        }
    }

    #[test]
    fn builds_name_their_entries_and_the_default() {
        let text = "schema-version 1\nbuilds default=\"two\" {\n    one\n    two \"images/two\"\n    one\n    \"Not An Id\"\n}\n";
        let mut list = List::empty(Path::new("."));
        let mut issues = Issues::default();
        list.parse_file(&Source::new(layout::REPO_FILE, text), text, &mut issues);
        let names: Vec<_> = list
            .builds
            .iter()
            .map(|b| (b.name.as_str(), b.component.as_str()))
            .collect();
        assert_eq!(names, [("one", "one"), ("two", "images/two")]);

        assert_eq!(list.default_image_id.as_deref(), Some("two"));
        let said = issues.plain();
        assert!(said.contains("build `one` is declared twice"), "{said}");
        assert!(said.contains("`Not An Id` is not a build name"), "{said}");
    }

    #[test]
    fn a_publish_with_a_tag_or_a_digest_is_refused() {
        for (publish, kept) in [
            ("nightly", true),
            ("quay.io/team/box", true),
            ("localhost:5000/box", true),
            ("quay.io/team/box:1", false),
            ("box@sha256:0", false),
            ("Box", false),
            ("quay.io//box", false),
            ("quay.io/team/", false),
            ("quay.io/Team/box", false),
            ("Quay.io/team/box", true),
            ("Team/box", false),
            ("localhost/box", true),
        ] {
            let text =
                format!("schema-version 1\nbuilds {{\n    one {{ publish \"{publish}\" }}\n}}\n");
            let mut list = List::empty(Path::new("."));
            let mut issues = Issues::default();
            list.parse_file(&Source::new(layout::REPO_FILE, &text), &text, &mut issues);
            assert_eq!(list.builds[0].publish.is_some(), kept, "{publish}");
            let said = issues.plain();
            assert_eq!(
                said.contains("is not an image name to publish under"),
                !kept,
                "{publish}: {said}"
            );
        }
    }

    #[test]
    fn a_registry_with_a_capital_in_its_path_is_refused() {
        for (registry, refused) in [
            ("ghcr.io/owner", false),
            ("GHCR.io/owner", false),
            ("ghcr.io/JohnGrant", true),
            ("Owner", true),
            ("Team/owner", true),
            ("localhost/owner", false),
        ] {
            let text = format!("schema-version 1\nregistry \"{registry}\"\n");
            let mut list = List::empty(Path::new("."));
            let mut issues = Issues::default();
            list.parse_file(&Source::new(layout::REPO_FILE, &text), &text, &mut issues);
            let said = issues.plain();
            assert_eq!(
                said.contains("holds a capital letter in its path"),
                refused,
                "{registry}: {said}"
            );
        }
    }

    #[test]
    fn a_build_is_added_inside_builds() {
        assert_eq!(
            add_build("schema-version 1\n", "one", None).unwrap(),
            "schema-version 1\n\nbuilds {\n    one\n}\n"
        );
        assert_eq!(
            add_build(
                "schema-version 1\nbuilds {\n    one\n}\n",
                "two",
                Some("one")
            )
            .unwrap(),
            "schema-version 1\nbuilds default=\"one\" {\n    one\n    two\n}\n"
        );
        assert_eq!(
            add_build("builds default=\"a\" { a }\n", "b", Some("a")).unwrap(),
            "builds default=\"a\" { a; b }\n"
        );
    }

    #[test]
    fn manifest_label_is_off_unless_declared() {
        let read = |text: &str| {
            let mut list = List::empty(Path::new("."));
            let mut issues = Issues::default();
            list.parse_file(&Source::new(layout::REPO_FILE, text), text, &mut issues);
            list.manifest_label
        };
        assert!(!read("schema-version 1\n"));
        assert!(read("schema-version 1\nmanifest {\n    label #true\n}\n"));
    }

    #[test]
    fn a_network_block_overrides_the_rule_one_kind_at_a_time() {
        let read = |text: &str| {
            let doc: KdlDocument = text.parse().expect("valid KDL");
            network(&doc.nodes()[0])
        };
        assert_eq!(read("network\n"), Network::default());
        assert_eq!(
            read("network \"strict\" {\n    packages \"allow\"\n}\n"),
            Network {
                packages: NetRule::Allow,
                scripts: NetRule::Strict,
            }
        );
    }

    #[test]
    fn a_network_rule_outside_the_set_is_refused() {
        let found = messages(
            "schema-version 1\nname \"Buildc\"\nsecurity-policy {\n    network \"block\"\n}\n",
        );
        assert_eq!(found, ["`block` is not a network rule"]);
    }

    #[test]
    fn a_network_rule_that_is_not_a_string_is_refused() {
        let found = messages(
            "schema-version 1\nname \"Buildc\"\nsecurity-policy {\n    network #false\n}\n",
        );
        assert_eq!(found, ["`#false` is not a network rule"]);
    }

    #[test]
    fn a_library_is_a_source_and_its_version() {
        let found = messages(
            r#"
schema-version 1
name "Buildc"
libraries branch="main" {
    remote {
        owner {
            source "https://example.invalid/library"
            version "v1"
            version "v2"
            subtree "components"
        }
        bare
    }
    local {
        mine {
            source "../mine"
            version "v1"
        }
        hosted { source "https://example.invalid/hosted" }
    }
    stray { source "../stray" }
}
library { path "src" }
"#,
        );
        assert_eq!(
            found,
            [
                "unknown libraries property `branch`",
                "`version` is declared twice",
                "unknown node `subtree` in a remote library",
                "this library declares no `source`",
                "unknown node `version` in a local library",
                "unknown node `stray` in libraries",
                "unknown node `path` in `library`",
            ]
        );
    }

    #[test]
    fn a_url_under_local_is_refused() {
        let text = "libraries {\n    local {\n        hosted { source \"https://example.invalid/hosted\" }\n    }\n}\n";
        let doc: KdlDocument = text.parse().expect("valid KDL");
        let src = Source::new(layout::REPO_FILE, text);
        let mut issues = Issues::default();
        let held: Vec<_> = remote::declared(&doc.nodes()[0])
            .filter_map(|(kind, node)| parse_library(node, kind, &src, &mut issues))
            .collect();
        assert!(held.is_empty());
        assert_eq!(issues.findings(), ["`hosted` is a local library"]);
    }

    #[test]
    fn a_library_without_a_source_is_the_folder_of_its_name() {
        let libraries =
            libraries_in("libraries {\n    local {\n        tools\n        mine { source \"../mine\" }\n    }\n    remote {\n        checkout { source \"../checkout/\" }\n    }\n}\n");
        let sources: Vec<&str> = libraries
            .iter()
            .filter_map(|library| match &library.at {
                crate::model::remote::At::Dir(dir) => Some(dir.as_str()),
                crate::model::remote::At::Git(pin) => pin.url.as_deref(),
            })
            .collect();
        assert_eq!(sources, ["./tools", "../mine", "../checkout"]);
        assert!(libraries[2].pin().is_some());
    }

    #[test]
    fn a_local_library_outside_the_repository_is_refused() {
        let at = std::env::temp_dir().join(format!("buildc-local-paths-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&at);
        let root = at.join("repo");
        std::fs::create_dir_all(root.join("inside")).unwrap();
        std::fs::create_dir_all(at.join("elsewhere")).unwrap();
        std::os::unix::fs::symlink(at.join("elsewhere"), root.join("linked")).unwrap();
        std::fs::create_dir_all(at.join("elsewhere/lib")).unwrap();
        std::os::unix::fs::symlink(at.join("elsewhere/lib"), root.join("vendor")).unwrap();
        // `later` names a folder that does not exist yet.
        // `sideways` climbs with `..` out of a symlinked folder. A climb by name would stay inside the repository.
        crate::init::put(
            &root.join(layout::REPO_FILE),
            "schema-version 1\n\nlibraries {\n    local {\n        own { source \".\" }\n        inside { source \"inside\" }\n        later { source \"not/yet\" }\n        beside { source \"../elsewhere\" }\n        linked { source \"linked\" }\n        climbing { source \"inside/../../elsewhere\" }\n        sideways { source \"vendor/../other\" }\n    }\n}\n",
        )
        .unwrap();
        // The test loads the repository through a symlink to its root, as `/home` links to `/var/home` on an image-based host.
        std::os::unix::fs::symlink(&root, at.join("alias")).unwrap();
        let (_, issues) = List::load(&at.join("alias"));
        let plain = issues.plain();
        std::fs::remove_dir_all(&at).unwrap();
        let outside = |name: &str| {
            plain.contains(&format!(
                "local library `{name}` is outside this repository"
            ))
        };
        assert!(
            !outside("own") && !outside("inside") && !outside("later"),
            "{plain}"
        );
        assert!(
            outside("beside") && outside("linked") && outside("climbing") && outside("sideways"),
            "{plain}"
        );
    }

    #[test]
    fn a_local_library_is_judged_the_same_through_a_relative_root() {
        let here = std::env::current_dir().unwrap();
        let crate_name = here.file_name().unwrap();
        let name = format!("buildc-local-relative-{}", std::process::id());
        // The root must be a relative path that exists from the test's working directory.
        // The crate's own target folder gives one.
        let at = Path::new("..").join(crate_name).join("target").join(&name);
        let root = at.join("repo");
        let _ = std::fs::remove_dir_all(&at);
        std::fs::create_dir_all(root.join("inside")).unwrap();
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::create_dir_all(root.join("../components")).unwrap();
        crate::init::put(
            &root.join(layout::REPO_FILE),
            "schema-version 1\n\nlibraries {\n    local {\n        own { source \".\" }\n        inside { source \"inside\" }\n        later { source \"not/yet\" }\n        beside { source \"../components\" }\n    }\n}\n",
        )
        .unwrap();
        let said = |at: &Path| List::load(at).1.plain();
        let from_root = said(&root);
        let from_sub = said(&root.join("sub/.."));
        std::fs::remove_dir_all(&at).unwrap();
        for plain in [from_root, from_sub] {
            let outside = |name: &str| {
                plain.contains(&format!(
                    "local library `{name}` is outside this repository"
                ))
            };
            assert!(!outside("inside") && !outside("later"), "{plain}");
            assert!(outside("beside"), "{plain}");
        }
    }

    #[test]
    fn the_default_library_is_read_from_the_libraries_node() {
        assert_eq!(
            default_library_in("libraries default=\"shelf\" {\n    local {\n        shelf { source \".\" }\n    }\n}\n").as_deref(),
            Some("shelf")
        );
        assert_eq!(default_library_in("libraries {\n}\n"), None);
        assert_eq!(default_library_in("builds default=\"one\"\n"), None);
    }

    #[test]
    fn the_components_folder_is_read_from_the_library_node() {
        assert_eq!(
            components_folder("library { components \"src/components\" }\n").as_deref(),
            Some("src/components")
        );
        assert_eq!(components_folder("library { components \"/abs\" }\n"), None);
        assert_eq!(components_folder("name \"x\"\n"), None);
    }

    fn root_repository(name: &str) -> std::path::PathBuf {
        let dir = root(
            name,
            "schema-version 1\nname \"Root\"\nbuilds { one \"./one\" }\n",
        );
        std::fs::create_dir_all(dir.join("components/image-base")).unwrap();
        std::fs::write(
            dir.join("components/image-base/component.kdl"),
            "schema-version 1\n\nbase \"quay.io/fedora/fedora-bootc:44\" {\n    family \"fedora\"\n}\n",
        )
        .unwrap();
        dir
    }

    #[test]
    fn a_root_file_that_a_build_reaches_is_a_component() {
        let dir = root_repository("root-reached");
        std::fs::write(
            dir.join("one.kdl"),
            "schema-version 1\n\nimage-base\n\ncomponent \"./shared\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("shared.kdl"),
            "schema-version 1\n\npackages \"htop\"\n",
        )
        .unwrap();

        let loaded = crate::load(&dir);
        let issues = loaded.issues.plain();
        assert!(!issues.contains("nothing reads `shared.kdl`"), "{issues}");
        let one = loaded
            .list
            .images
            .iter()
            .find(|image| image.id == "one")
            .expect("the build is declared");
        let shared = one
            .components()
            .find(|component| component.path == "shared")
            .expect("the root file is read as a component");
        assert_eq!(shared.packages[0].packages, ["htop"]);
        assert_eq!(
            one.base
                .as_ref()
                .expect("the bare child finds the base")
                .path,
            "image-base"
        );
    }

    #[test]
    fn a_root_component_reaches_another_root_component() {
        let dir = root_repository("root-chain");
        std::fs::write(
            dir.join("one.kdl"),
            "schema-version 1\n\nimage-base\n\ncomponent \"./between\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("between.kdl"),
            "schema-version 1\n\ncomponent \"./shared\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("shared.kdl"),
            "schema-version 1\n\npackages \"htop\"\n",
        )
        .unwrap();

        let loaded = crate::load(&dir);
        let issues = loaded.issues.plain();
        assert!(!issues.contains("nothing reads `between.kdl`"), "{issues}");
        assert!(!issues.contains("nothing reads `shared.kdl`"), "{issues}");
        let one = loaded
            .list
            .images
            .iter()
            .find(|image| image.id == "one")
            .expect("the build is declared");
        assert!(one.components().any(|component| component.path == "shared"));
    }

    #[test]
    fn a_root_file_that_no_build_reaches_is_refused() {
        let dir = root_repository("root-stray");
        std::fs::write(dir.join("one.kdl"), "schema-version 1\n\nimage-base\n").unwrap();
        std::fs::write(dir.join("notes.kdl"), "schema-version 1\n").unwrap();

        let issues = List::load(&dir).1.plain();
        assert!(issues.contains("nothing reads `notes.kdl`"), "{issues}");
    }

    #[test]
    fn a_root_reference_to_a_missing_file_is_reported_by_the_resolver() {
        let dir = root_repository("root-missing");
        std::fs::write(
            dir.join("one.kdl"),
            "schema-version 1\n\nimage-base\n\ncomponent \"./missing\"\n",
        )
        .unwrap();

        let issues = crate::load(&dir).issues.plain();
        assert!(
            issues.contains("`one` names `missing`, which is no component at the repository root"),
            "{issues}"
        );
    }

    #[test]
    fn a_rooted_component_path_in_a_root_file_reads_the_root() {
        let dir = root_repository("root-rooted");
        std::fs::write(
            dir.join("one.kdl"),
            "schema-version 1\n\nimage-base\n\ncomponent \"/shared\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("shared.kdl"),
            "schema-version 1\n\npackages \"htop\"\n",
        )
        .unwrap();

        let loaded = crate::load(&dir);
        let issues = loaded.issues.plain();
        assert!(!issues.contains("nothing reads `shared.kdl`"), "{issues}");
        let one = loaded
            .list
            .images
            .iter()
            .find(|image| image.id == "one")
            .expect("the build is declared");
        assert!(one.components().any(|component| component.path == "shared"));
    }

    #[test]
    fn an_inline_root_component_does_not_read_a_file_of_its_name() {
        let dir = root_repository("root-inline");
        std::fs::write(
            dir.join("one.kdl"),
            "schema-version 1\n\nimage-base\n\ncomponent \"shared\" { packages \"htop\" }\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("shared.kdl"),
            "schema-version 1\n\npackages \"vim\"\n",
        )
        .unwrap();

        let issues = List::load(&dir).1.plain();
        assert!(issues.contains("nothing reads `shared.kdl`"), "{issues}");
    }

    #[test]
    fn a_syntax_error_in_a_middle_root_file_hides_no_unread_file() {
        let dir = root_repository("root-syntax");
        std::fs::write(
            dir.join("one.kdl"),
            "schema-version 1\n\nimage-base\n\ncomponent \"./between\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("between.kdl"),
            "schema-version 1\n\ncomponent \"./shared\"\npackages \"htop\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("shared.kdl"),
            "schema-version 1\n\npackages \"vim\"\n",
        )
        .unwrap();

        let issues = crate::load(&dir).issues.plain();
        assert!(issues.contains("between.kdl"), "{issues}");
        assert!(!issues.contains("nothing reads `shared.kdl`"), "{issues}");
    }

    #[test]
    fn a_reached_root_file_that_git_ignores_is_reported() {
        let dir = root_repository("root-ignored");
        std::fs::write(
            dir.join("one.kdl"),
            "schema-version 1\n\nimage-base\n\ncomponent \"./shared\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("shared.kdl"),
            "schema-version 1\n\npackages \"htop\"\n",
        )
        .unwrap();
        std::fs::write(dir.join(".gitignore"), "shared.kdl\n").unwrap();
        let initialised = std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&dir)
            .status()
            .is_ok_and(|status| status.success());
        assert!(initialised);

        let issues = List::load(&dir).1.plain();
        assert!(issues.contains("reads a file that git ignores"), "{issues}");
    }
}
