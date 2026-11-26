pub(super) const ROOT_NOTES: &str = r#"> [!NOTE]
> In a terminal, a command asks for each answer that its arguments and flags
> did not give. `buildc create repo` shows every answer on a review screen before
> it writes. Enter reopens an answer. Create writes. Esc writes nothing.
>
> With `--no-tui`, or with no terminal, a command asks nothing. A typed answer
> comes from its flag first, then its default. If neither exists, the command
> fails and names the flag. A yes or no question answers no, and a choice keeps
> the options that it opens with."#;

pub(super) const UPGRADE: &str = r#"Replaces this `buildc` and the assets it scaffolds from with the latest
published release. It prints the running and latest versions. It stops when
they match, or when the running build is newer than the latest tag."#;

pub(super) const UPGRADE_NOTES: &str = r#"> [!NOTE]
> The binary and its assets move together. A binary without its assets
> uses a stale `assets/` copy and gives no warning.
>
> Run as root, it installs to `/usr/local/bin` and
> `/usr/local/share/buildc/assets`. Run as another user, it installs to
> `~/.local/bin` and `$XDG_DATA_HOME/buildc/assets`, which is
> `~/.local/share/buildc/assets` by default. If it cannot write there, it
> stops and names the other location. It never switches to that location.
>
> It checks the download against the `.sha256` published beside it before it
> changes a file. If the check fails, then the old install stays.
>
> It replaces the assets as a whole, so a file that a release removed does not
> survive.
>
> An `assets` directory beside the binary takes priority over the installed
> assets, so it stops before it downloads. A set `BUILDC_ASSETS` also wins, and
> the warning names the path that wins.
>
> Releases exist for x86_64 and aarch64 Linux. On another platform it stops and
> names the platform.
>
> On a machine with no `buildc`, the install script does the same job:
> `curl -fsSL https://raw.githubusercontent.com/buildc-org/buildc/main/install.sh | sh`."#;

pub(super) const CREATE_REPO: &str = r#"Creates a repository for images. Every other command reads the `repo.kdl` that
it writes, so it runs first. It creates a directory named after the repository,
or uses the directory that `--root` names.

It asks for the repository name and the setup path. The defaults path fills the
provider, the libraries, the first image and its base, the workflows, the
schedule and the scripts. The minimal path writes the tree, git and `repo.kdl`.
`repo.kdl` declares the default libraries. When the user gives an owner,
`repo.kdl` also declares `base-url`, the repository URL that image option
values read as `{base-url}`. The minimal path's review screen names the
command that sets each other unset row. The command shows every answer on a
review screen before it writes."#;

pub(super) const CREATE_REPO_NOTES: &str = r#"> [!WARNING]
> The `buildc` library that `repo.kdl` declares names no `version`, so it
> follows its default branch. Each `buildc generate` moves its components to
> the head of that branch and records the commit in `repo.lock.kdl`.

> [!NOTE]
> It runs `git init` and does no other git step. It prints the commands for the
> first commit, the remote and the push.
>
> On GitHub, it offers to create the remote repository with `gh`. If `gh` is
> missing or logged out, it reports what `gh` still needs before it can create
> the remote.
>
> A repository cannot sit inside another repository, so it refuses such a
> directory."#;

pub(super) const CREATE_IMAGE: &str = r#"Adds an image to the repository as a new component file at the repository
root, and names it in `builds` in `repo.kdl`. The file name derives from the
image name, so "My Desktop" writes `my-desktop.kdl`.

It asks for the image name, which defaults to the repository name, and for the
base. The libraries offer the base, and `--base` names one. If the base offers
more than one bootloader, it asks which one the build installs and writes it
into the build's `install` block.

If the base lists no build environment for its family, the command offers the
library's family adapter. If the base lists no `os-release` component, it
offers the library's `os-release` and sets its `name` option to the image name.
If the base requires a capability, it offers a provider of it. The image file
lists each accepted component by its path. Nothing is copied, and the build
reads each component from its library. If two libraries hold the path, then
the line names the library with `library=`.

When `repo.kdl` declares `base-url`, the command also sets the `home-url` of
`os-release` to `{base-url}` and its `bug-report-url` to `{base-url}/issues`."#;

pub(super) const CREATE_IMAGE_NOTES: &str = r#"> [!NOTE]
> A base outside the libraries gets its own base component under `base-image/`
> with no `family` block. The tool then asks no family question and offers no
> family adapter. `buildc check` reports each capability that no component
> provides.
>
> If the repository holds one build and `builds` declares no `default=`, then a
> second image sets `default=` to the first build. A bare `buildc build` then
> builds what it built before.
>
> To offer the bases and their components, it fetches each declared library
> that is not on this machine.
>
> The family adapter is the component that `targets` the family and
> `provides "build-environment"`."#;

pub(super) const CREATE_COMPONENT: &str = r#"Writes a new component. A directory component is a manifest at
`<name>/component.kdl` in the folder of the library that `libraries default=`
names. In a new repository that library is `local-library` and its folder is
`components/`. The command offers to list it in builds.
`--build` names a build, and it is repeatable. The command lists the component
as a bare child of each build's component and writes its directory where that
child resolves. A build whose file sits at the repository root reads a bare
child from the default library. A build whose component is `components/<build>`
reads it from `components/<build>/`. `--inline` writes the component as a
block inside another component instead, and that body lists the block.

`--inline` takes one holder. `--build <name>` puts the block in that build's
component file. `--in <component>` puts it in a local component's body, and the
name is relative to that component. Without either flag, the command shows one
picker of the local components, with the builds' components first. A component
in a remote library is refused.

The name can be a path. As a directory component, `apps/firefox` writes
`components/apps/firefox/component.kdl`. As an inline component, it writes a
`firefox` block inside an `apps` block.

The command asks for the component name. For an inline component it then asks
for the component or build that holds the block. For both kinds it asks for an
optional description and optional base family restrictions. For a directory
component it then asks which builds list the component, and no row starts
checked. The command writes no packages. `buildc add package` adds them, and a
path the repository does not hold writes a new package group."#;

pub(super) const CREATE_COMPONENT_NOTES: &str = r#"> [!NOTE]
> Choosing no build is valid for a directory component. The component then
> exists in the default library with no listing. Leaving the picker writes
> nothing.
>
> If `libraries default=` names a remote library, then the command refuses.
> buildc writes no component into a remote library.
>
> The command refuses two `--build` values whose builds read the bare child
> from different folders. One component has only one directory.
>
> An inline component sits in one holder. `--inline` takes one `--build` or
> one `--in`, and the command refuses the two together. If nothing can be
> asked, then an inline component needs one of them. The command refuses an
> inline component when the user chooses no holder.
>
> `--in` names a component this repository holds. A component in a remote
> library is refused, and `buildc copy component` makes a local copy first.
>
> The command refuses a name whose first segment is a manifest node, such as
> `packages`. A component file reads a bare node of that name as the manifest
> node, so no tree could list the component.
>
> An inline component needs at least one manifest line. A description, a
> `targets` restriction or a `--with` line each counts.
>
> One name cannot be a directory component and an inline component at once.
> The command refuses an inline component when `components/<name>/` already
> holds a component. It refuses a directory component when a component's body
> already defines the name inline.
>
> `--with packages=...` is refused. `buildc add package <names...> --component
> <name>` adds the packages, and it creates the component when the path is
> new.
>
> The command writes a directory component unless `--inline` is given."#;

pub(super) const ADD_PACKAGE: &str = r#"Adds packages to a package group. The group is a directory component or a
block inside another component's body. A path the repository does not hold
writes a new group in the folder of the library that `libraries default=`
names. If the default library is remote, then the command refuses a new directory group.

The names are arguments and the command asks for them when none is given. The
prompt is `packages`, with `separated by a space` as its hint.

`--component` names the group. Without it, a terminal asks for a `group name`
first. The field uses `apps` when it is left empty. The second row is `select
component`, which opens a picker of the components this repository holds. If no
terminal is watching, the command refuses and names `--component`.

A new group with no build becomes a directory component. `--build` names a
build, and it is repeatable. Every build named receives an inline copy of the
group as a bare child of its component. With `--build`, `--component` also
names a group that the build's component lists bare. An existing inline group
with several copies is targeted with `select component` or one `--build`.

A component with no `packages` node gains one. A component directory that
holds only scripts gains a `component.kdl`. An inline definition gains the node
inside its own block. Otherwise the names join the first top-level `packages`
node in the shape it already uses. If the node lists its names on one line,
then the new names go after its last name. If the node holds its names in a
block, then each new name goes on its own line inside the block. A repeat call
writes only the names the component does not list yet."#;

pub(super) const ADD_PACKAGE_NOTES: &str = r#"> [!NOTE]
> A name inside a family-gated `on` block counts as listed. The command does
> not add it again at the top level.
>
> The command writes each name outside any `on` gate. Every base family that
> the component targets then installs it. A name that differs by family
> belongs in an `on` block. The user writes that block in the manifest or
> image file.
>
> If the `packages` node carries `enablerepo=`, then the new names install with
> that repository enabled too.
>
> A name takes letters, digits and `. _ + : -`, with at most one `=` before a
> version. An option reference such as `firefox={version}` is accepted in place
> of literal text. The rule holds because the build writes the name straight
> into its RUN line. A name that breaks the rule is refused before anything is
> written. An install that needs other characters belongs in `component.sh`.
>
> A path that is both a directory component and an inline component is
> refused the same way `buildc check` refuses it.
>
> A component marked `hidden #true` stays out of `select component` until `h`
> reveals it. An explicit `--component` path still selects it.
>
> `buildc create component` writes no packages. Run this command after it, or
> write the `packages` line in the manifest or image file by hand."#;

pub(super) const ADD_COMPONENT: &str = r#"Adds a reference to a component to one or more local components. The command
copies nothing. A build reads the component from its library when it builds.

It asks for:

1. The components that take the reference. The picker lists the builds'
   components, and `h` shows every other local component. If the repository
   has one build, then its component takes the reference without a question.
2. The components to add. The picker lists each component that at least one
   chosen build can use. A component that some chosen builds cannot use names
   them after `does not support`, and only the others take it.
3. The library, if more than one library holds a chosen path.
4. Whether to add the providers that the components require. A summary lists
   each requirement that a build does not meet and its provider. If several
   providers meet one requirement, then a picker for that requirement follows.
5. If a component provides a capability that an entry of the same build holds
   as exclusive, or the other way round, whether to replace that entry or
   cancel.

The reference reads the component at the library version. `--pin` fixes it at
a tag or a commit instead."#;

pub(super) const ADD_COMPONENT_NOTES: &str = r#"> [!NOTE]
> A reference below the receiving component's own path is written as a bare
> child. Every other reference is written inside `component "/"`. A nested path
> such as `hardware/nvidia/cuda` brings each parent folder that is itself a
> component.
>
> The command writes `library=` only where the path would resolve to another
> library without it.
>
> It reads each remote library from the cache, and it fetches one only when the
> cache holds none. `buildc generate` refreshes the cache.
>
> It refuses a component that a receiving component already lists. It refuses
> `--pin` with more than one path, and on a component from a `local` library.
>
> Without a terminal, `--dependencies` adds every provider without the summary.
> If several providers meet one requirement, then `--dependencies <provider>`
> names the one to add, and without it the command refuses and names each. A
> named provider that meets no requirement a build lacks is refused. A provider
> that this repository holds wins over a library's provider. An exclusive clash
> is refused without a terminal.
>
> After the write, the command loads the builds again. If a file can no longer
> be edited, or a reference reads another library or another pin, then every
> file goes back as it was. A problem that `buildc check` reports stops the
> command only if it keeps a file from being edited. If the user declines a
> required component, then the reference stays, and `buildc check` names the
> component that would satisfy it."#;

pub(super) const CREATE_GIT: &str = r#"Puts a directory written by hand or copied from elsewhere under git control
before its first commit. It writes the `.gitignore` that ignores
`keys/private/`, `out/` and `.cache/`, unless one is already there.

It refuses a directory that git already tracks, because a repository does not
nest."#;

pub(super) const CREATE_GIT_NOTES: &str = r#"> [!NOTE]
> `buildc create repo` already does this for a repository it scaffolds. This
> command is for a directory written by hand or copied from somewhere, and it
> leaves every file that is already there alone."#;

pub(super) const COPY_COMPONENT: &str = r#"Copies components from a library into a `local` library, and records where each
copy came from in that library's `origins.kdl`. The copy is the repository's
own, and the user may edit it. No fetch changes it.

A path is `<library>/<path>`, or the path alone with `--from`. With `--build`
and no path, the command copies every remote component that the build reads.
With neither, it asks for the components from a list of every library.

A copy takes the components nested in its folder. It reads the commit that the
builds read, so a pinned reference is copied at its pin. A path that no build
reads is copied at the library `version`.

The command lists each copy and each file whose references change, then asks
to confirm. It rewrites each reference that would not read the copy, and drops
its `library=` and `pin=`. If nothing references a copy, then it is copied
alone, and `buildc add component` adds it to a build."#;

pub(super) const COPY_COMPONENT_NOTES: &str = r#"> [!NOTE]
> If `libraries` declares more than one `local` library, then a terminal asks
> which one takes the copies. The library that `--from` or a path's first
> segment names is not offered.
>
> The command refuses a path that the destination already holds. It refuses a
> component that another component defines inline, and names the component
> that holds it.
>
> If two builds read a path at different commits, then the command refuses and
> `--build` names the build whose commit to copy.
>
> A reference that reads `library=` or `pin=` from an enclosing node is
> refused, with the node and its line named.
>
> After the copy, the command loads the builds again. If a build no longer
> loads, or a reference still reads the library, then every file goes back as
> it was."#;

pub(super) const CREATE_SCRIPTS: &str = r#"Copies scripts that `buildc` supplies into `scripts/`, where the repository owns
them. Git tracks each copy, the user can review and change it, and a `buildc`
upgrade does not replace it. The scripts are:

- `Containerfile.skeleton`, the Containerfile around the generated component layers
- `lint.sh`, `buildc.sh`, `smoke.sh`, `vm.sh` and `media.sh`, which `buildc generate`
  otherwise writes into `generated/scripts/`

It asks which scripts to keep. The list holds only the scripts that `scripts/`
does not hold yet."#;

pub(super) const CREATE_SCRIPTS_NOTES: &str = r#"> [!NOTE]
> Run `buildc generate` after it. Every generated script and workflow then calls
> the copy in `scripts/`.
>
> A kept script that still matches what `buildc` supplied is updated to call the
> new copies. A kept script that the user changed stays as it is.
>
> `buildc check` says when a kept script differs from what the running release
> supplies.
>
> `buildc create repo` asks the same question, with the skeleton chosen."#;

pub(super) const CREATE_KEY: &str = r#"Generates a key that a component in the repository declares. The public half goes
under `keys/public/`, at the path it has in the image. The private half goes
under `keys/private/`, which git ignores.

Each key kind uses its own tool:

- `cosign` makes the key pair that signs a published image. Set the private
  half as the `SIGNING_SECRET` repository secret. `cosign` must be installed.
- `openssl profile="module-signing"` makes the Secure Boot certificate that
  signs the kernel and its modules. Set the private half as the `MOK_PRIVKEY`
  repository secret, and `$MOK_KEY_PATH` points a local build at it. It
  writes the `format` that the component declares, and `sign-file` and `mokutil`
  read DER.
- `openssl profile="pcr-signing"` makes the RSA pair that signs the PCR 11
  policy of a sealed UKI. Set the private half as the `PCR_PRIVKEY` repository
  secret. A local build reads it through
  `buildc build --secret pcr_privkey=<path>`.
- `openssl profile="tls-ca"` makes a certificate authority that a TLS client
  trusts. It needs no repository secret, because a build issues its
  certificates.
- `ssh-keygen` makes the ed25519 key pair that the user logs in with. The key
  has no passphrase. The image ships the public half, and no build reads the
  private half.

It asks for the kind."#;

pub(super) const CREATE_KEY_NOTES: &str = r#"> [!WARNING]
> It never replaces an existing key, because a private half cannot be
> recovered.

> [!NOTE]
> Each machine enrols a `module-signing` certificate once with
> `mokutil --import`. Until then, the modules it signs do not load.
>
> If `.gitignore` does not cover `keys/private/`, then it says so and does not
> edit the file.
>
> If no component declares the kind, then it names a component in the declared
> libraries that does, and the `buildc add component` line that adds it."#;

pub(super) const SET_KEY: &str = r#"Records the public half of an existing key instead of generating one. Use it
for a cosign key, MOK certificate or SSH key. It writes to the path that the
component's `public` declaration gives.

It asks for the kind and the file to read."#;

pub(super) const SET_KEY_NOTES: &str = r#"> [!WARNING]
> It never replaces an existing key.

> [!NOTE]
> It takes the public half only. The private half stays with the user, or in a
> CI secret.
>
> It checks the form of the file first, so a wrong form stops here before a
> build sees it. Each kind takes one form:
>
> - `cosign` takes a PEM public key.
> - `module-signing` takes a PEM or DER certificate.
> - `pcr-signing` takes a bare PEM public key.
> - `ssh-keygen` takes an OpenSSH key line."#;

pub(super) const SET_WORKFLOWS: &str = r#"Chooses the CI workflows that the repository generates, and writes the choice
into the `github` block in `repo.kdl`. Run `buildc generate` next to write the
workflow files. A workflow that the repository cannot run shows its reason and
cannot be chosen.

With no workflow named, it asks for the workflows, and with no terminal it
changes nothing and says to edit `repo.kdl`. A newly chosen workflow runs on
its shipped triggers, and a workflow already listed keeps the triggers written
for it. If the repository declares more than one build, then it asks which
builds each newly chosen per-build workflow runs for. If the user chooses every
build, then the command writes no `builds`. A workflow with no `builds` runs
for every build.

With a workflow named, it lists that workflow and asks nothing. `--on`
replaces the workflow's triggers, and `--builds` replaces its builds."#;

pub(super) const SET_WORKFLOWS_NOTES: &str = r#"> [!NOTE]
> No workflow removes the `workflows` block, and the repository then generates
> no CI. Leaving the picker changes nothing.
>
> A trigger's own `builds` and a schedule's `frequency` and `time` are written
> in `repo.kdl` by hand."#;

pub(super) const SET_LIBRARY: &str = r#"Adds a library to the `libraries` block in `repo.kdl`. A library holds
components and base components alike.

A name on its own adds that library from the libraries buildc offers.
`--source` declares the library at an HTTPS Git URL or a path. Its name comes
from the `name` in the source's `repo.kdl`, else from a URL's owner segment or
a path's folder name. A name given with `--source` wins. With either argument,
the command asks nothing.

Without an argument, it asks for the library. The picker lists the libraries
buildc offers that the repository does not declare yet, and its last row is
`URL / local path`. That row asks for the library location and then for the
library name. If buildc has no library left to offer, then the command skips
the picker and asks for the location at once. A summary shows the location,
the name and the kind, and the user confirms or cancels.

buildc derives the kind from the location. A URL is remote. A path that git
tracks in this repository is local. Any other path is remote and is read as a
Git checkout at a commit."#;

pub(super) const SET_LIBRARY_NOTES: &str = r#"> [!NOTE]
> A name that `libraries` already declares is refused. The command never
> replaces a library.
>
> If no terminal is watching and no argument is given, then the command
> refuses.
>
> If the repository holds `components/`, `workflows/` or `plugins/` and
> declares no `local` library, then the command also declares this repository
> as `local-library` under `local`. It becomes the default library if
> `libraries` names none.
>
> If the command writes the first `libraries` block and `components/` does not
> exist, then it asks whether to create that folder. A yes creates the folder
> and declares this repository as `local-library`. With an argument, the
> command asks nothing and creates no folder.
>
> If the repository stops loading after the edit, then the command undoes
> it."#;

pub(super) const SET_CONFORMS: &str = r#"Chooses the benchmark profile that a scan measures an image against, and writes
it into the image's `conforms`. A `conforms` turns on the image scan for every
build. With no terminal, it changes nothing and says to write the line into the
image file.

It asks for the image if the repository holds several, and for the profile. If
a library holds components that claim rules of the profile that no component in
the image claims, then it asks whether to add a reference to them or copy them
into the repository. A reference asks for a pin, and a copy asks for the local
library that takes it."#;

pub(super) const SET_CONFORMS_NOTES: &str = r#"> [!NOTE]
> A `conforms` is not a claim that the image passes. It names what the image is
> measured against. If the repository sets `audit { enforce #true }`, then a
> failed rule fails the build.
>
> A second run replaces the profile.
>
> If no `--datastream` is given and no SCAP content is installed for the
> image's family, then it stops and names `scap-security-guide`.
>
> The offer covers library components only. A component the repository holds
> needs its path in the image, and `buildc check` names it."#;

pub(super) const SET_CLAIMS: &str = r#"Chooses the benchmark rules that a component in the repository claims to cover,
and writes their numbers into its `satisfies` block. A claim says what the
component supplies, and `buildc scap` measures whether the built image keeps it.

It asks for the profile and the rules."#;

pub(super) const SET_CLAIMS_NOTES: &str = r#"> [!NOTE]
> A group chooses every rule under it. The file holds the number of each rule,
> never the number of a group, so a claim does not grow when the content does.
>
> A rule with no number of its own cannot be claimed. The list leaves it out
> and counts it above the question.
>
> A claim on a rule that the chosen profile does not select stays, so a second
> profile does not remove what the first one wrote.
>
> No rule removes the `satisfies` block. Leaving the picker changes nothing.
>
> A component that declares `targets` uses the installed content for its first
> named family by default. A component with no `targets` declaration has no
> single default, so name `--datastream`. If that content is not installed, the
> command stops and names `scap-security-guide`.
>
> The benchmark node that holds the numbers does not change what they mean. A
> number resolves against the datastream, never against the benchmark name."#;

pub(super) const CHECK: &str = r#"Reads every file in the repository and reports each problem at the line that
caused it. Run it after an edit. The last line counts the images, the components
and the listed components that the base provides."#;

pub(super) const CHECK_NOTES: &str = r#"> [!NOTE]
> It also reports these findings, which do not change the exit code:
>
> - A build reads a remote component that `repo.lock.kdl` does not record.
> - A locked library has moved at its remote since `buildc generate` locked it.
> - A components folder is in no library, because no `local` library has the
>   source `.`.
> - `buildc-version` names a release with no `sha256`.
> - A component ships a Containerfile fragment, which the script rule does not
>   reach.
> - A scaffolded file differs from what this release of `buildc` scaffolds.
> - An image `conforms` to a profile, and no listed component claims a rule of it.
>
> With `--datastream`, it also counts the rules of each declared profile that
> no listed component claims, and names the components that would claim them.
> `buildc scap content` prints the path to give it."#;

pub(super) const GENERATE: &str = r#"Writes the build files, and the CI workflows that `repo.kdl` names. Run it
after a change to a KDL file. `buildc build` refuses build files that are not
current. It first fetches each remote library at its `version`. It moves each
unpinned remote component to the commit that `version` names now and records
that commit in `repo.lock.kdl`. A component that `pin=` names stays at its pin.
It also reaches the network for the SCAP datastream a conforming image's base
pins, and for the commit each cloned asset's tag names. It writes:

- `generated/<image>/Containerfile`, one for each image
- `generated/<image>/components/<component>.sh`, the build script of each component
- `generated/<image>/finalize.sh`
- `generated/<image>/scap/<file>`, the pinned datastream of a conforming image
- `generated/<image>/graph.md` and `graph.json`, the capability graph
- `generated/plan.json`
- `generated/seed.kdl`, if `repo.kdl` names a seedable image
- each workflow that the `github` block names, under `.github/workflows/`
- `.github/renovate.json5`, if `repo.kdl` holds a `renovate` block"#;

pub(super) const GENERATE_NOTES: &str = r#"> [!NOTE]
> It clears `generated/` first, so the files of a removed image or component go
> too. It removes only the workflows that `buildc` ships, and keeps the
> repository's own.
>
> A terminal draws a file tree. A pipe prints one path per line.
>
> If a library's remote does not answer, then it reads the commit that the
> cache holds and says so. Every other command reads each remote component at
> the commit that `repo.lock.kdl` records.
>
> The Containerfile copies `plan.json` into the image at
> `/usr/share/buildc/manifest.json`, so a built image can say what it is made
> of.
>
> A conforming image's datastream comes from the pin in its base's
> `scap-content`. It writes the datastream under `generated/<image>/scap/`, and
> maps each claimed benchmark number to its rule there. If the committed copy
> already matches the pin, then it fetches nothing."#;

pub(super) const BUILD: &str = r#"Builds one target with the container backend. A target is a build name. With
no target, it builds the default build. It checks the committed generated tree
against the snapshot `buildc generate` wrote, then builds from that tree alone.
Every build argument that the repository declares comes from
`generated/plan.json`, so a source edit reaches the build only through the next
`buildc generate`. The build resolves the version, the registry, the source
commit, the backend and the base digest when it runs. It then fetches the
base's SPDX attestation, checks that its subject names the digest, and
verifies the signature when the base names a verifier."#;

pub(super) const BUILD_NOTES: &str = r#"> [!NOTE]
> It writes no build file. Run `buildc generate` first, or `buildc vm run
> --rebuild` to run both.
>
> It reaches a remote for three answers. It resolves the base digest. It
> fetches the base's SPDX attestation. It asks each cloned asset's remote
> which commit the tag names. If a tag names another commit than the one
> `buildc generate` recorded, then it refuses the build. If a remote does not
> answer, then it warns and an enforced audit refuses.
>
> A base with no attestation leaves a gap in the build record. The build
> continues and `audit { enforce #true }` refuses. A base that names a verifier
> is different. A failed check, or a machine without `cosign`, refuses the
> build.
>
> It resolves the base tag to a digest once, and passes it as the `BASE` build
> argument. If `$BASE` is set, then it uses that value as already resolved.
>
> `$TAGS` adds tags and `$LABELS` adds OCI labels. `$IMAGE_VERSION` is stamped
> into the image, and is today's date in UTC by default.
>
> A UKI target builds in two passes. The first builds the image, and the second
> seals it with the storage digest of the first. The digest comes from the local
> buildah store, so it refuses `--backend buildx` for a UKI target. It checks the
> sealed image against the embedded digest before it applies a tag, so a failed
> check publishes nothing.
>
> An image is bootc when its base or an enabled component provides `bootc`. A
> build of a non-bootc image declares no `install`. Its build writes no
> `bootc container lint` step and `buildc validate-image` skips the bootc
> checks."#;

pub(super) const VM: &str = r#"Turns the built image into a disk and boots it, to try the image before it is
published. `<type>` is `qcow2` or `raw`. A terminal prompts for it.
`build` makes the disk. `run` boots it under qemu, and makes it first if it is
missing. `spawn` boots it with systemd-vmspawn. An installer ISO is media, and
`buildc media` owns it."#;

pub(super) const VM_NOTES: &str = r#"> [!NOTE]
> `--rebuild` runs `buildc generate` and `buildc build` in that order first.
> Without it, nothing is fetched, written or built, and the existing disk
> boots.
>
> It runs `vm.sh`, from `scripts/` if the repository keeps a copy, else from
> `generated/scripts/`. The script asks for sudo, because it reads the image out
> of rootful podman.
>
> Fedora and RHEL disks use bootc-image-builder, at the size that
> `disk_config/disk.toml` sets. Debian and Ubuntu disks use
> `bootc install to-disk`, at `DISK_SIZE`, which is 20G by default, and need
> `skopeo`.
>
> If a component imports `passwd.hashed-password.*`, then `run` and `spawn` add a
> `buildc` account for the console and ask for its password. `VM_USER` changes the
> account, and `VM_PASSWORD_HASH` gives a crypt(5) hash for a run with no
> terminal. The account is set on the first boot only."#;

pub(super) const MEDIA: &str = r#"Assembles the live installer media for the target and boots it, to try the
install before the image is published. `build` writes `out/bootiso/install.iso`.
`run` boots it under qemu, and makes it first if it is missing. The media
installs from the recipe that `buildc recipe` prints."#;

pub(super) const MEDIA_NOTES: &str = r#"> [!NOTE]
> `--rebuild` runs `buildc generate` and `buildc build` in that order first.
> Without it, nothing is fetched, written or built, and the existing media
> boots.
>
> It runs `media.sh`, from `scripts/` if the repository keeps a copy, else from
> `generated/scripts/`. The script asks for sudo, because it reads the image out
> of rootful podman.
>
> The media needs a published reference, because the installed machine updates
> from `$IMAGE_REGISTRY`. It carries the image, so an install needs no network.
> `run` keeps the installed disk under `out/bootiso/storage/`, so a reboot
> tests what was installed.
>
> If a component imports `passwd.hashed-password.*`, then `run` adds a `buildc`
> account for the console and asks for its password. `VM_USER` changes the
> account, and `VM_PASSWORD_HASH` gives a crypt(5) hash for a run with no
> terminal. The account is set on the first boot only."#;

pub(super) const SECTION: &str = r#"Prints the generated Containerfile component section of one image, or of the
default image if none is named."#;

pub(super) const GRAPH: &str = r#"Prints the capability graph of the default image. The graph shows what provides
each capability, what requires it, what only orders against it, and what the
base already carries."#;

pub(super) const COVERAGE: &str = r#"Prints each rule of the profile that an image `conforms` to, with the component
that claims it. If no component claims a rule, then it names a component in the
repository or its libraries that would. With no image, the default image
applies. A terminal offers a picker."#;

pub(super) const COVERAGE_NOTES: &str = r#"> [!NOTE]
> It reads no scan. It says what is claimed, and `buildc scap` measures what
> passes.
>
> It reads the content that `--datastream` names and never probes the host, so
> the result does not depend on the machine.
>
> A terminal gets a table, with each rule that nothing claims in red. A pipe
> gets markdown, so `buildc coverage > report.md` exports it.
>
> A rule with an empty `Number` cell has no number that a `satisfies` can name,
> so no component can claim it."#;

pub(super) const PLAN: &str = r#"Prints every fact that the repository derives as one JSON document. The
document holds the images, the targets of each image, and what each target is
made of. A script reads a field from it, and derives nothing from a name. It
reaches the network for a conforming image's datastream when the committed
copy does not match the pin, and for the commit each cloned asset's tag names."#;

pub(super) const ATTRIBUTE: &str = r#"Maps each layer of one built image to the component that made it and to that
component's paths. The image is a reference or an id in the local container
store. The layer order and the commands come from the container store, and the
paths come from `generated/plan.json`."#;

pub(super) const ATTRIBUTE_NOTES: &str = r#"> [!NOTE]
> Run `buildc build` first. `generate` runs each step at its own path, so the
> builder's history names the component. A layer the tool did not make, such as
> the base's, carries no component.
>
> The CI build runs this after the build and before the push, and uploads the
> document as an artifact."#;

pub(super) const VALIDATE: &str = r#"Checks one built image with the tool that built it. The image is a reference or
an id in the local container store. The check runs inside the image with the
pinned tool mounted, and reads `generated/plan.json` for the target's
contracts, verify exceptions, presets, LUKS witness and family."#;

pub(super) const VALIDATE_NOTES: &str = r#"> [!NOTE]
> Run `buildc build` first, because the check mounts the tool the build staged at
> `out/buildc`. The CI build runs this against the image id it produced, before
> the push step, so a failed check publishes nothing."#;

pub(super) const VERIFY: &str = r#"Writes every build file again in memory, and compares each one byte for byte
with what the repository holds: `generated/`, the declared workflows and
`.github/renovate.json5`. It names each file that differs, each file that is
missing, and each file under `generated/` that nothing writes. A build checks
the committed tree against the snapshot instead, and reads no source. It
resolves each cloned asset's tag as `generate` does, so a tag that moved
reports the plan as stale."#;

pub(super) const SUMMARY: &str = r#"Prints a markdown table of what one target is made of. Each row is a component
that the target builds, with its description and the options it resolved. A CI
build writes the table into its job summary."#;

pub(super) const SBOM: &str = r#"Prints the pinned payloads of one target, as SPDX packages and their
relationships. A scan of the built image cannot see where a downloaded asset
came from, so CI merges this into the SBOM that the scan produces."#;

pub(super) const SCAP: &str = r#"Reads a scan report against its datastream and prints the target's findings.
The report shows what each component claimed and what was measured, the score
against each profile of the datastream, and what stopped passing since the last
scan."#;

pub(super) const SCAP_NOTES: &str = r#"> [!NOTE]
> The datastream maps each benchmark number to a rule. A number that maps to no
> rule is a fault in the declaration, and the image is not at fault. A declared
> profile that the datastream does not carry is a finding.
>
> A claimed rule that the image fails names the component that claimed it. If
> another component replaced a file of the claiming component, then it names that
> component too.
>
> `--baseline` keeps a pass set. A rule that passed the last scan and fails now
> is a finding, and each run writes the new pass set, so a deliberate
> regression is one failed run.
>
> `--base-scan` adds a column for a scan of the bare base. A claim that the base
> already passes is reported as a notice, never as a finding.
>
> A finding fails the command only under `audit { enforce #true }`. The report
> goes to stdout either way."#;

pub(super) const SCAP_CONTENT: &str = r#"Prints the path of the datastream that the target is measured with, as
`generated/plan.json` records it. The build used that same file for
remediation. On a booted system, it prints the installed SSG file that the base
names, which can be another release. If the target declares no `conforms`, then
it prints nothing, and the CI scan job skips the image. `buildc set conforms`
writes a `conforms`."#;

pub(super) const SCAP_TAILORING: &str = r#"Prints the XCCDF tailoring that a scan of the target runs. The tailoring defines
the profile `xccdf_buildc_profile_measured`, which is the declared profile with
every group and rule selected. If the image declares no `conforms`, then it
prints nothing."#;

pub(super) const SCAP_TAILORING_NOTES: &str = r#"> [!NOTE]
> `--profile '(all)'` scores every variable at its default, so a rule set to the
> profile's value reads as failing. The tailoring keeps the profile's values and
> still checks every claim outside the profile."#;

pub(super) const REGISTRY_NAMESPACE: &str = r#"Prints where the images publish. If `$IMAGE_REGISTRY` is set, then it prints
that value. Otherwise it prints the `registry` that `repo.kdl` names, and it
fails if `repo.kdl` names none."#;

pub(super) const REGISTRY_REF: &str = r#"Prints the full reference that one target publishes under. The reference joins
the namespace, the name the target publishes under and its tag. A `publish`
value that holds `/` is the reference without its tag. With no target, it uses
the default image."#;

pub(super) const RECIPE: &str = r#"Prints the part of an install recipe that the repository answers, as JSON that
the installer reads. The recipe holds the reference to install, the reference
the machine updates from, and properties of the image. The properties are the
boot chain, the bootloader, the root filesystem and whether the initramfs can
unlock LUKS. The disk, the account and the encryption choice belong to the
user, so the recipe holds none of them."#;

pub(super) const RECIPE_NOTES: &str = r#"> [!NOTE]
> The base family settles each value. For a family with no measured answer it
> stops, because a wrong value erases a disk that then does not boot.
>
> `--image` installs a local build while the machine still updates from the
> published reference.
>
> `hostname` is the published name. It is the one value that the user is
> expected to replace."#;

pub(super) const WHY: &str = r#"Prints what one component is and where each input came from. It lists the
targets that build it, what it provides and requires, and what it claims to
harden. It also lists the library it is read from, or the origin of a copy and
whether the copy was edited since. It lists what it fetches, and whether it adds
a package repository."#;

pub(super) const WHY_NOTES: &str = r#"> [!NOTE]
> In a repository it reads the resolved plan. On a booted image it reads the
> two documents that the build baked into the image, and it also prints the
> source repository, the commit and the `git clone` that reaches them.
>
> On a booted image, it refuses a document written for a schema version that
> this `buildc` does not read, and names both versions. `buildc plan --json` still
> prints the document.
>
> A copy edited since it was copied is said plainly, and it is not an error.
>
> `buildc` does not read a `repo.sh` file, so it points at the file and prints the
> URLs it found."#;

pub(super) const BUILD_RECORD: &str = r#"Writes `/usr/share/buildc/build.json`, the record of what the build resolved.
The baked `manifest.json` beside it holds what the repository declared. The
record holds the digest of the base, the base's SPDX attestation (its
predicate, subject, envelope hash and decoded payload hash), the SBOM package
count, whether the base declares a signature, whether a verifier checked the
signature and that the checked attestation carries the recorded payload, the
version the build measured for each capability from its package, the commit of
each cloned asset, the source commit, the `buildc` release, the target, the
content hash of each component, and whether enforcement was on. The versions
sit in `base.versions`. That key is absent when the build measured none. A base
that published no attestation records null there, and `verified` says whether a
verifier checked it. A verifier that checked a different payload refuses the
build, so `verified.base_attestation` is true only for the payload in the
record.

No file under `generated/` holds the record, so `buildc verify` never compares
it. The record changes with each build."#;

pub(super) const FETCH: &str = r#"Downloads one payload, checks it against its hash, and places it by its kind.
It runs inside a build:

- `file` keeps the download
- `tree` unpacks it
- `bin` installs one executable
- `rpm` installs the package, on an rpm family
- `deb` installs the package, on a deb family"#;

pub(super) const VALIDATE_IMAGE: &str = r#"Runs every check that a built image has to pass. The build supplies the
presets. The command fails for each preset that the image lacks."#;
