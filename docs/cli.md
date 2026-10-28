# The `buildc` CLI

`buildc` is the one tool that manages a bootc image repository. The user describes each image in KDL files, and `buildc` turns those files into the Containerfiles and the CI workflows that build the image. `buildc` works out the component order, the options, the names and the tags in one place, so the build scripts hold no logic of their own and the user can see what goes into each image.

## Where it runs

`buildc` runs in three places:

- On the user's machine, it scaffolds the repository and its images, components and keys. It checks the KDL files, writes the build files, and builds and boots an image.
- In CI, the generated workflows call it to write the build files, build and publish each image, and scan it.
- Inside a build, the generated Containerfile calls it to write the image identity, record what the build resolved and check the finished image. A component script can call `buildc fetch` to download a payload and check it against its hash.

## Global options

Every command takes these options, before or after its own words. [`buildc`](#buildc) says how a command asks for an answer, and what `--no-tui` changes.

| Option | Description |
| --- | --- |
| `--root <dir>` | the repository, else the nearest `repo.kdl` at or above the working directory |
| `--no-tui` | ask nothing, using defaults or failing when an answer has no default |

## Commands

### Repository

| Command | Description | Schema |
| --- | --- | --- |
| [`buildc create repo [name]`](#buildc-create-repo) | start a repository of images | [repository&nbsp;layout](schema/repository.md) |
| [`buildc create git`](#buildc-create-git) | initialise git and the ignore file in a directory |  |
| [`buildc create scripts [name]...`](#buildc-create-scripts) | keep a copy of a script buildc supplies in scripts/ | [`scripts/`](schema/repository.md) |
| [`buildc set library`](#buildc-set-library) | add a component library | [`repo.kdl`&nbsp;›&nbsp;`sources`](schema/repo.md#sources) |

### Image

| Command | Description | Schema |
| --- | --- | --- |
| [`buildc create image [name]`](#buildc-create-image) | add an image, with its name and the base it builds on | [`image.kdl`](schema/image.md) |
| [`buildc create flavour [name]`](#buildc-create-flavour) | add a gated component set an image also publishes | [`image.kdl`&nbsp;›&nbsp;`flavours`](schema/image.md#image-flavours) |

### Components

| Command | Description | Schema |
| --- | --- | --- |
| [`buildc create component [name]`](#buildc-create-component) | write a component, and offer to list it in an image | [`manifest.kdl`](schema/component.md) |
| [`buildc import component [name]`](#buildc-import-component) | copy a component from a library repo.kdl declares into this repository | [`components.lock.kdl`](schema/lock.md) |
| [`buildc set component [path]`](#buildc-set-component) | make an imported component the repository's own | [`components.lock.kdl`](schema/lock.md) |
| [`buildc fetch components [path]...`](#buildc-fetch-components) | refresh each tracked component from its library | [`components.lock.kdl`](schema/lock.md) |

### Keys

| Command | Description | Schema |
| --- | --- | --- |
| [`buildc create key [kind]`](#buildc-create-key) | generate a key one of this repository's components declares | [`manifest.kdl`&nbsp;›&nbsp;`key`](schema/component.md#key) |
| [`buildc set key [kind]`](#buildc-set-key) | record a key that already exists, in place of generating one | [`manifest.kdl`&nbsp;›&nbsp;`key`](schema/component.md#key) |

### Check and generate

| Command | Description | Schema |
| --- | --- | --- |
| [`buildc check`](#buildc-check) | read every manifest and say what is wrong with it |  |
| [`buildc generate`](#buildc-generate) | write the build files, and list what was written | [`generated/`](schema/repository.md) |
| [`buildc verify`](#buildc-verify) | compare the build files byte for byte with what buildc writes |  |

### Build and boot

| Command | Description | Schema |
| --- | --- | --- |
| [`buildc build [target]`](#buildc-build) | verify the build files, then build the image |  |
| [`buildc vm build [type]`](#buildc-vm-build) | convert the built image into a qcow2, raw or iso |  |
| [`buildc vm run [type]`](#buildc-vm-run) | boot that disk under qemu, building it if missing |  |
| [`buildc vm spawn [type]`](#buildc-vm-spawn) | boot a qcow2 or raw disk with systemd-vmspawn |  |

### CI

| Command | Description | Schema |
| --- | --- | --- |
| [`buildc set workflows`](#buildc-set-workflows) | choose the CI this repository generates | [`repo.kdl`&nbsp;›&nbsp;`workflows`](schema/repo.md#workflows) |

### Inspect

| Command | Description | Schema |
| --- | --- | --- |
| [`buildc section [image]`](#buildc-section) | print the Containerfile section an image generates |  |
| [`buildc graph`](#buildc-graph) | print what provides what, and what the base carries |  |
| [`buildc why [component]`](#buildc-why) | print one component's trust read-out, byte by byte |  |
| [`buildc plan`](#buildc-plan) | print every fact this repository derives, as json |  |
| [`buildc summary [target]`](#buildc-summary) | print what one target is made of, as a markdown table |  |
| [`buildc sbom [target]`](#buildc-sbom) | print the pinned payloads one target carries, as SPDX |  |
| [`buildc registry namespace`](#buildc-registry-namespace) | print where images publish |  |
| [`buildc registry ref`](#buildc-registry-ref) | print the full reference one target publishes under |  |
| [`buildc recipe`](#buildc-recipe) | print the installer recipe one target installs from |  |

### Audit

| Command | Description | Schema |
| --- | --- | --- |
| [`buildc set conforms [image]`](#buildc-set-conforms) | choose the benchmark profile an image is measured by | [`image.kdl`&nbsp;›&nbsp;`conforms`](schema/image.md#image-conforms) |
| [`buildc set claims [component]`](#buildc-set-claims) | choose the benchmark rules a component claims to cover | [`manifest.kdl`&nbsp;›&nbsp;`satisfies`](schema/component.md#satisfies) |
| [`buildc coverage [image]`](#buildc-coverage) | print who claims each rule the image conforms to | [`image.kdl`&nbsp;›&nbsp;`conforms`](schema/image.md#image-conforms) |
| [`buildc scap content`](#buildc-scap-content) | print the datastream the target is measured with |  |
| [`buildc scap tailoring`](#buildc-scap-tailoring) | print the tailoring the target is scanned with |  |
| [`buildc scap rules [number]...`](#buildc-scap-rules) | print the rule each benchmark number reaches, one per line |  |
| [`buildc scap [arf.xml]`](#buildc-scap) | print what one scan says about the target |  |

### Inside a build

| Command | Description | Schema |
| --- | --- | --- |
| [`buildc fetch <what> <url> <sha256> [target] [extra]...`](#buildc-fetch) | download one payload, verify it, and place it | [`manifest.kdl`&nbsp;›&nbsp;`asset`](schema/component.md#asset) |
| [`buildc build-record`](#buildc-build-record) | write the record of what the build resolved |  |
| [`buildc validate-image`](#buildc-validate-image) | run every check a built image has to pass | [`manifest.kdl`&nbsp;›&nbsp;`allow-verify`](schema/component.md#allow-verify) |

### CLI tool

| Command | Description | Schema |
| --- | --- | --- |
| [`buildc`](#buildc) | open a picker of the commands that run here, or list them where no terminal is watching |  |
| [`buildc upgrade`](#buildc-upgrade) | replace this buildc and its assets with the latest |  |

## `buildc`

The build tool for a bootc image repository. With no command, it opens a picker of the commands that run where it is typed. If no terminal is watching, then it lists those commands instead.

**Usage:** `buildc [OPTIONS] [COMMAND]`


| Option | Description |
| --- | --- |
| `--root <dir>` | the repository, else the nearest `repo.kdl` at or above the working directory |
| `--no-tui` | ask nothing, using defaults or failing when an answer has no default |

> [!NOTE]
> In a terminal, a command asks for each answer that its arguments and flags
> did not give. `buildc create repo` then shows every answer on a review screen.
> Enter on an answer asks it again, Create writes, and Esc leaves without
> writing.
>
> With `--no-tui`, or with no terminal, a command asks nothing. A typed answer
> comes from its flag, else from its default, else the command fails and names
> the flag. A yes or no question answers no, and a choice keeps the options
> that it opens with.

## `buildc upgrade`

Replaces this `buildc`, and the assets it scaffolds from, with the latest
published release. It prints the running version and the latest one. If they
are the same, or if the running build is newer than the latest tag, then it
stops.

**Usage:** `buildc upgrade`

> [!NOTE]
> The binary and its assets move together. A binary without its assets
> scaffolds from a stale `assets/` copy and says nothing.
>
> Run as root, it installs to `/usr/local/bin` and
> `/usr/local/share/buildc/assets`. Run as another user, it installs to
> `~/.local/bin` and `$XDG_DATA_HOME/buildc/assets`, which is
> `~/.local/share/buildc/assets` by default. If it cannot write there, then
> it stops and names the other location. It never falls back to it.
>
> It checks the download against the `.sha256` published beside it before it
> changes a file. If the check fails, then the old install stays.
>
> It replaces the assets as a whole, so a file that a release removed does not
> survive.
>
> An `assets` directory beside the binary takes priority over the installed
> assets, so it stops before it downloads. A set `BUILDC_ASSETS` also takes
> priority, and it warns about that.
>
> Releases exist for x86_64 and aarch64 Linux. On another platform it stops and
> names the platform.
>
> On a machine with no `buildc`, the install script does the same job:
> `curl -fsSL https://raw.githubusercontent.com/buildc-org/buildc/main/install.sh | sh`.

## `buildc create repo`

Starts a repository for the user's images. Every other command reads the
`repo.kdl` that it writes, so it runs first. It creates a directory named after
the repository, or uses the directory that `--root` names.

It asks, in order:

1. Name the repository.
2. Choose whether to sync the repository to a host. Then choose the host and
   give the account.
3. On GitHub, choose whether to create the remote repository now.
4. Choose whether to add a first image. Then name it and choose its base.
5. If the base needs components, choose whether to add them.
6. If the repository syncs to a host, choose the CI workflows. Then choose
   whether images publish and scan only on scheduled builds, and give the daily
   build time.
7. Choose the scripts to keep in `scripts/`.
8. Review every answer, change any of them, and choose Create.

**Usage:** `buildc create repo [OPTIONS] [name]`

| Argument | Description |
| --- | --- |
| `<name>` | the repository name, which also names its directory |

| Option | Description |
| --- | --- |
| `--host <domain>` | where the repository is hosted, github.com by default; with `--owner`, it forms the address every image URL starts from |
| `--owner <name>` | the account or organisation on the repository host |
| `--image <name>` | write a first image too; `create image` adds one later |
| `--base <ref>` | the bootc image the first image is based on |

**Schema:** [repository&nbsp;layout](schema/repository.md)

**What it writes:**

```text
example/
├── components/
│   ├── base-image/
│   │   └── fedora/
│   │       └── + manifest.kdl
│   ├── fedora-family/
│   │   ├── lib/
│   │   │   └── + family.sh
│   │   └── + manifest.kdl
│   ├── + .gitkeep
│   └── + components.lock.kdl
├── disk_config/
│   └── + disk.toml
├── scripts/
│   └── + Containerfile.skeleton
├── + .dockerignore
├── + .gitattributes
├── + .gitignore
├── + .shellcheckrc
├── + README.md
├── + desktop.image.kdl
└── + repo.kdl
```

> [!WARNING]
> The `buildc-components` source that `repo.kdl` declares is `unpinned`. Each
> fetch takes the head of its branch, and no hash checks what arrives. To pin
> it, add `version` and `sha256` beside its `url`.

> [!NOTE]
> It runs `git init` and does no other git step. It prints the commands for the
> first commit, the remote and the push.
>
> On GitHub, it offers to create the remote repository with `gh`. If `gh` is
> missing or logged out, then it says what to do.
>
> A repository cannot sit inside another repository, so it refuses such a
> directory.

## `buildc create git`

Puts the directory under git control, so a hand-written repository is under
version control before its first commit. It writes the `.gitignore` that
ignores `keys/private/`, `out/` and `.cache/`, unless one is already there.

It refuses a directory that git already tracks, because a repository does not
nest.

**Usage:** `buildc create git`

> [!NOTE]
> `buildc create repo` already does this for a repository it scaffolds. This
> command is for a directory written by hand or copied from somewhere, and it
> leaves every file that is already there alone.

## `buildc create image`

Adds an image to the repository, in a new image file at the root. The file
name derives from the image name, so "My Desktop" writes
`my-desktop.image.kdl`.

It asks, in order:

1. Name the image. The repository name is the default.
2. Choose the base from the catalog. If `--base` names a base outside the
   catalog, then choose its family.
3. If the base imports no build environment, choose whether to add the
   family adapter. If the base requires a component that the image does not
   list, choose whether to add its providers. Each one comes from the declared
   collections, is copied into `components/` and is recorded in
   `components/components.lock.kdl`. A component the repository already holds
   is listed as it is.

The components a library base imports are copied into `components/` beside
it, and the image file lists none of them.

**Usage:** `buildc create image [OPTIONS] [name]`

| Argument | Description |
| --- | --- |
| `<name>` | the image name, which also names its file |

| Option | Description |
| --- | --- |
| `--owner <name>` | the account or organisation, where no image already carries one |
| `--base <ref>` | any bootc image reference, in the catalog or not; skips the picker |

**Schema:** [`image.kdl`](schema/image.md)

**What it writes:**

```text
example/
├── components/
│   ├── base-image/
│   │   └── collected/
│   │       └── + manifest.kdl
│   └── ~ components.lock.kdl  base-image/collected recorded
├── + beta.image.kdl
└── ~ repo.kdl  example set as the default image
```

> [!NOTE]
> A base outside the catalog asks for its family and writes no `provides`.
> `buildc check` then reports each capability that no component provides.
>
> If the repository declares no `default-image`, then a second image adds one
> to `repo.kdl` that names the first image. A bare `buildc build` then builds what
> it built before.
>
> To offer the base's components, it reads the declared collections, and fetches a
> collection that is not on this machine. With no terminal, it fetches
> nothing.
>
> The base's components are the family adapter and a provider of each capability
> that the base requires. The family adapter is the component that `targets` the
> family and `provides "build-environment"`.

## `buildc create flavour`

Adds a flavour to an image. A flavour is a second build of the image with more
components, published as `<image>-<flavour>` beside the image. This command
declares the flavour in the image's `flavours` block. To add a component to the
flavour, run `buildc create component` or `buildc import component` and choose the
flavour.

It asks, in order:

1. Name the flavour.
2. Choose the image that publishes it.

**Usage:** `buildc create flavour [OPTIONS] [name]`

| Argument | Description |
| --- | --- |
| `<name>` | the flavour name |

| Option | Description |
| --- | --- |
| `--image <name>` | the image that publishes the flavour |

**Schema:** [`image.kdl`&nbsp;›&nbsp;`flavours`](schema/image.md#image-flavours)

**What it writes:**

```text
example/
└── ~ example.image.kdl  dx added to flavours
```

`example.image.kdl` after it runs:

```kdl
schema-version 1

image {
    name "Example"
    url "https://github.com/someone/example"
    issues-url "https://github.com/someone/example/issues"

    layout {
        bootloader "grub2"
    }

    base-image {
        fedora {
            version "44"
            signed #false
        }
    }
    flavours {
        dx
    }
}
```

> [!NOTE]
> It writes neither `default` nor `pr-build`. Each one changes what a build
> does, so the user writes it.
>
> It refuses a name that the image already declares, and `none`, which names
> the image's own build.

## `buildc create component`

Writes a new component, `components/<name>/manifest.kdl`, and offers to list it in an
image. The name can be a path, so `apps/firefox` writes
`components/apps/firefox/manifest.kdl`.

It asks, in order:

1. Name the component.
2. Optionally describe it.
3. Optionally restrict it to named base families.
4. Choose whether it installs packages, then name them.
5. Choose the images and flavours that list it.

**Usage:** `buildc create component [OPTIONS] [name]`

| Argument | Description |
| --- | --- |
| `<name>` | the component name, which can be a path under `components/` |

| Option | Description |
| --- | --- |
| `--image <name>` | list the component in this image or flavour; repeatable |
| `--pkg <name>` | a package the component installs; repeatable |
| `--with <verb=value>` | one more line in the manifest, such as `--with provides=browser`; repeatable |

**Schema:** [`manifest.kdl`](schema/component.md)

**What it writes:**

```text
example/
├── components/
│   └── my-editor/
│       └── + manifest.kdl
└── ~ example.image.kdl  my-editor added to components
```

> [!NOTE]
> With no `targets` declaration, a component targets every base family. It
> writes packages outside a family gate so every targeted family installs
> them. A package name that differs by family goes in a `family` block.
> The build installs them through the family adapter, with `dnf install -y` on
> Fedora and RHEL, and with `apt-get update`, `apt-get install -y` and
> `apt-get clean` on Debian and Ubuntu.
>
> An image and one of its own flavours cannot both list the component. The image
> entry already reaches every flavour of that image.
>
> No image is an answer. The component exists, and no image lists it yet. Leaving
> the picker writes nothing.

## `buildc create key`

Generates a key that a component in the repository declares. The public half goes
under `keys/public/`, at the path it has in the image. The private half goes
under `keys/private/`, which git ignores.

It makes each kind with its own tool:

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

It asks for one step:

1. Choose the kind.

**Usage:** `buildc create key [OPTIONS] [kind]`

| Argument | Description |
| --- | --- |
| `<kind>` | the kind of key, as a component declares it |

| Option | Description |
| --- | --- |
| `--component <name>` | which component, where two of them declare the same kind |
| `--cn <name>` | the certificate common name; the repository directory name by default |

**Schema:** [`manifest.kdl`&nbsp;›&nbsp;`key`](schema/component.md#key)

> [!WARNING]
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
> collections that does, and the `buildc import component` line that adds it.

## `buildc create scripts`

Copies a script that `buildc` supplies into `scripts/`, where the repository
owns it. Git tracks the copy, the user can review and change it, and a `buildc`
upgrade does not replace it. The scripts are:

- `Containerfile.skeleton`, the Containerfile around the generated component layers
- `lint.sh`, `buildc.sh`, `smoke.sh` and `vm.sh`, which `buildc generate` otherwise
  writes into `generated/scripts/`

It asks for one step:

1. Choose the scripts. The list holds only the scripts that `scripts/` does
   not hold yet.

**Usage:** `buildc create scripts [name]...`

| Argument | Description |
| --- | --- |
| `<name>` | each script to keep |

**Schema:** [`scripts/`](schema/repository.md)

> [!NOTE]
> Run `buildc generate` after it. Every generated script and workflow then calls
> the copy in `scripts/`.
>
> A kept script that still matches what `buildc` supplied is updated to call the
> new copies. A kept script that the user changed stays as it is.
>
> `buildc check` says when a kept script differs from what the running release
> supplies.
>
> `buildc create repo` asks the same question, with the skeleton chosen.

## `buildc import component`

Copies a component from a library into `components/<name>`, and records it in
`components/components.lock.kdl`: the library, the component's path there, the
library's revision and the content hash. The repository holds the copy, so a
fetch never changes it.

It asks, in order:

1. Choose the image or flavour the components are for, or none.
2. Choose the components. A component that the image's base family cannot build,
   and a base component, are listed as unavailable, with the reason beside
   them.
3. If the repository already holds a component at that path, choose to list the
   local component or to import the library's under another name. The name
   offered is `<name>@1`.
4. If several library components provide one capability that they require,
   choose one. Then choose whether to add the components that they require. A
   requirement whose `provider` reads an option takes the answer as that
   option's value on the component's entry, and the provider is copied in
   without an entry of its own.
5. If a component provides a capability that an entry in the same image or
   flavour declares exclusive, or the other way round, choose to replace that
   entry or cancel.
6. Choose whether to generate the CI workflows that they make runnable.
7. If they claim benchmark rules, choose a profile to measure the image
   against.
8. Choose whether the copies track upstream.

**Usage:** `buildc import component [OPTIONS] [name]`

| Argument | Description |
| --- | --- |
| `<name>` | one component, as `<name>` or `<owner>/<name>` |

| Option | Description |
| --- | --- |
| `--image <name>` | list the component in this image or flavour |
| `--datastream <file>` | the SCAP content the profile offer is read out of; the family's installed copy by default, and no content is no offer |
| `--dependencies` | add provider components for dependencies the selected components require |

**Schema:** [`components.lock.kdl`](schema/lock.md)

**What it writes:**

```text
example/
├── components/
│   ├── browser/
│   │   └── + manifest.kdl
│   └── ~ components.lock.kdl  browser recorded
└── ~ example.image.kdl  browser added to components
```

> [!WARNING]
> A pinned library is downloaded once and checked against its hash. An
> `unpinned` library is downloaded each time with no check, and
> `audit { enforce #true }` refuses it.

> [!NOTE]
> If `repo.kdl` declares no `sources`, then the command refuses and prints the
> `sources` block that `buildc create repo` writes.
>
> A bare name is searched for in every library. `<owner>/<name>` picks
> between two libraries that both hold it.
>
> It refuses a component that the image already lists, and a component that the
> image's base family cannot build.
>
> A tracked copy carries `track` in its lock entry. An untracked copy is the
> repository's own, and its entry keeps where it came from.
>
> Without a terminal, the copies are tracked, and an image lists them only
> where `--image` names one. A named component at a path the repository already
> holds is refused, and so is an exclusive clash. A provider that
> `--dependencies` brings is listed as the repository holds it. If several
> library components provide one requirement, then `--dependencies` refuses and
> names each.
>
> If the user declines a required component, then the file stays valid, and
> `buildc check` names the import that would satisfy it.

## `buildc set workflows`

Chooses the CI workflows that the repository generates, and writes the choice
into `workflows` in `repo.kdl`. Run `buildc generate` next to write the workflow
files. A workflow that the repository cannot run shows the reason, and the user
cannot choose it. With no terminal, it changes nothing and says to edit
`repo.kdl`.

It asks, in order:

1. Choose the workflows.
2. Choose whether images publish only on scheduled builds.
3. Choose whether image scans run only on scheduled builds.
4. If a chosen workflow has a schedule, give the daily build time in UTC.

**Usage:** `buildc set workflows`

**Schema:** [`repo.kdl`&nbsp;›&nbsp;`workflows`](schema/repo.md#workflows)

**What it writes:**

```text
example/
└── ~ repo.kdl  the workflows it generates
```

> [!NOTE]
> No workflow removes the `workflows` block, and the repository then generates
> no CI. Leaving the picker changes nothing.
>
> Each schedule is an offset from the daily build time, so a new daily time
> moves every schedule.

## `buildc set library`

Adds a library to the `sources` block in `repo.kdl`: the default library, or
a source the user types. A library holds components and base components alike. A
source with the same alias is replaced, so the declaration moves to what the
user chose.

It asks, in order:

1. Choose the default library, or another source.
2. For another source, give its alias, its HTTPS Git URL and the directory
   inside the repository.

**Usage:** `buildc set library`

**Schema:** [`repo.kdl`&nbsp;›&nbsp;`sources`](schema/repo.md#sources)

## `buildc set conforms`

Chooses the benchmark profile that a scan measures an image against, and writes
it into the image's `conforms`. A `conforms` turns on the image scan for every
build. With no terminal, it changes nothing and says to write the line into the
image file.

It asks, in order:

1. If the repository holds several images, choose one.
2. Choose the profile.
3. Choose whether to import the collection components that claim rules of the
   profile that nothing in the image claims yet.

**Usage:** `buildc set conforms [OPTIONS] [image]`

| Argument | Description |
| --- | --- |
| `<image>` | the image; the only image if absent, else a picker |

| Option | Description |
| --- | --- |
| `--datastream <file>` | the SCAP content the profile is chosen out of; the installed copy for the image's family by default |

**Schema:** [`image.kdl`&nbsp;›&nbsp;`conforms`](schema/image.md#image-conforms)

**What it writes:**

```text
example/
├── components/
│   ├── sshd/
│   │   └── + manifest.kdl
│   └── ~ components.lock.kdl  sshd recorded
└── ~ example.image.kdl  measured against `standard`, and sshd added to components
```

> [!NOTE]
> A `conforms` is not a claim that the image passes. It names what the image is
> measured against. If the repository sets `audit { enforce #true }`, then a
> failed rule fails the build.
>
> A second run replaces the profile.
>
> If no `--datastream` is given and no SCAP content is installed for the
> image's family, then it stops and names `scap-security-guide`.
>
> The import offer covers collection components only. A component in the repository
> needs its path segments in the image, and `buildc check` names it.

## `buildc set claims`

Chooses the benchmark rules that a component in the repository claims to cover,
and writes their numbers into its `satisfies` block. A claim says what the
component supplies, and `buildc scap` measures whether the built image keeps it.

It asks, in order:

1. Choose the profile that the rules come from.
2. Choose the rules that the component claims.

**Usage:** `buildc set claims [OPTIONS] [component]`

| Argument | Description |
| --- | --- |
| `<component>` | the component in the repository |

| Option | Description |
| --- | --- |
| `--datastream <file>` | the SCAP content the rules are read out of; the installed copy for the component's first declared family by default, and required when the component declares no family restriction |

**Schema:** [`manifest.kdl`&nbsp;›&nbsp;`satisfies`](schema/component.md#satisfies)

**What it writes:**

```text
example/
└── components/
    └── sshd/
        └── ~ manifest.kdl  claiming one rule of `standard`
```

> [!NOTE]
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
> number resolves against the datastream, never against the benchmark name.

## `buildc set key`

Records the public half of a key that the user already holds, in place of
generating one. Use it for an existing cosign key, MOK certificate or SSH key.
It writes to the path that the component's `public` declaration gives.

It asks, in order:

1. Choose the kind.
2. Give the file to read.

**Usage:** `buildc set key [OPTIONS] [kind]`

| Argument | Description |
| --- | --- |
| `<kind>` | the kind of key, as a component declares it |

| Option | Description |
| --- | --- |
| `--component <name>` | which component, where two of them declare the same kind |
| `--from <path>` | the public half to record |

**Schema:** [`manifest.kdl`&nbsp;›&nbsp;`key`](schema/component.md#key)

> [!WARNING]
> It never replaces an existing key.

> [!NOTE]
> It takes the public half only. The private half stays with the user, or in a
> CI secret.
>
> It checks the form of the file first, so a wrong form stops here and no later
> build fails on it. Each kind takes one form:
>
> - `cosign` takes a PEM public key.
> - `module-signing` takes a PEM or DER certificate.
> - `pcr-signing` takes a bare PEM public key.
> - `ssh-keygen` takes an OpenSSH key line.

## `buildc set component`

Makes an imported component the repository's own copy. The component stops tracking
its library, so `buildc fetch components` passes it over, and the user may edit it.
With no `--untracked`, it asks first. With no terminal, it needs the flag.

**Usage:** `buildc set component [OPTIONS] [path]`

| Argument | Description |
| --- | --- |
| `<path>` | the component, by its path under `components/` |

| Option | Description |
| --- | --- |
| `--untracked` | stop tracking its library, without asking |

**Schema:** [`components.lock.kdl`](schema/lock.md)

> [!NOTE]
> The lock entry keeps the library, the path and the content hash, so `buildc
> why` still says where the copy came from and reads out each later edit.

## `buildc check`

Reads every file in the repository and reports each problem at the line that
caused it. Run it after an edit. The last line counts the images, the components,
the flavours and the listed components that the base already provides.

**Usage:** `buildc check [OPTIONS]`

| Option | Description |
| --- | --- |
| `--datastream <file>` | the SSG content, for the conformance read-out |

> [!NOTE]
> It also reports these findings, which do not change the exit code:
>
> - A collection describes a base differently from the catalog.
> - A collection is `unpinned`.
> - An image `conforms` to a profile, and no listed component claims a rule of it.
> - A `manifest.kdl` sits inside the directory of another collection component.
>
> With `--datastream`, it also counts the rules of each declared profile that
> no listed component claims, and names the components that would claim them.
> `buildc scap content` prints the path to give it.

## `buildc generate`

Writes the build files, and the CI workflows that `repo.kdl` names. Run it
after a change to a KDL file. `buildc build` refuses build files that are not
current. It reads only the repository's own components. It writes:

- `generated/<image>/Containerfile`, one for each image
- `generated/<image>/components/<component>.sh`, the build script of each component
- `generated/<image>/finalize.sh`
- `generated/<image>/graph.md` and `graph.json`, the capability graph
- `generated/plan.json`
- `generated/seed.kdl`, if `repo.kdl` names a seedable image
- each workflow that `workflows` names, under `.github/workflows/`
- `.github/renovate.json5`, if `repo.kdl` holds a `renovate` block

**Usage:** `buildc generate`

**Schema:** [`generated/`](schema/repository.md)

> [!NOTE]
> It clears `generated/` first, so the files of a removed image or component go
> too. It removes only the workflows that `buildc` ships, and keeps the
> repository's own.
>
> A terminal gets a tree of the files, and a pipe gets one path per line.
>
> The Containerfile copies `plan.json` into the image at
> `/usr/share/buildc/manifest.json`, so a built image can say what it is made
> of.
>
> It resolves the base tag to a digest once, and passes it as the `BASE` build
> argument. If `$BASE` is set, then it uses that value as already resolved.

## `buildc build`

Builds one target with the container backend. A target is `<image>` for the
image, or `<image>/<flavour>` for a flavour. With no target, it builds the
default image. It runs `buildc verify` first, so it builds only from build files
that are current.

**Usage:** `buildc build [OPTIONS] [target]`

| Argument | Description |
| --- | --- |
| `<target>` | the image, or `<image>/<flavour>` for a flavour; the default image if absent |

| Option | Description |
| --- | --- |
| `--target <target>` | the target, where the positional argument is not used |
| `--tag <tag>` | tag the result; repeatable, and $TAGS adds to it |
| `--kernel <name>` | the KERNEL build arg |
| `--backend <name>` | buildx or buildah, else $BUILD_BACKEND, else buildah |
| `--oci-output <path>` | write an OCI archive instead of loading the image |
| `--secret <id=path>` | mount <path> as the build secret <id>; repeatable |
| `--cache-to` | export the layer cache to the registry cache repository |
| `--no-cache-from` | do not import the layer cache |

> [!NOTE]
> It fetches nothing and writes no build file. Run `buildc generate` first, or
> `buildc vm run --rebuild` to run both.
>
> `$TAGS` adds tags and `$LABELS` adds OCI labels. `$IMAGE_VERSION` is stamped
> into the image, and is today's date in UTC by default.
>
> A UKI target builds in two passes. The first builds the image, and the second
> seals it with the storage digest of the first. The digest comes from the local
> buildah store, so it refuses `--backend buildx` for a UKI target. It checks the
> sealed image against the embedded digest before it applies a tag, so a failed
> check publishes nothing.

## `buildc vm`

Turns the built image into a disk and boots it, to try the image before it is
published. `<type>` is `qcow2`, `raw` or `iso`, and a terminal asks for it.
`build` makes the disk. `run` boots it under qemu, and makes it first if it is
missing. `spawn` boots it with systemd-vmspawn, which cannot boot an `iso`.

**Usage:** `buildc vm [COMMAND]`

> [!NOTE]
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
> An `iso` is a live installer for the target, which installs from the recipe
> that `buildc recipe` prints. It needs a published reference, because the
> installed machine updates from `$IMAGE_REGISTRY`. `run iso` keeps the installed disk
> under `out/bootiso/storage/`, so a reboot tests what was installed.
>
> If a component imports `passwd.hashed-password.*`, then `run` and `spawn` add a
> `buildc` account for the console and ask for its password. `VM_USER` changes the
> account, and `VM_PASSWORD_HASH` gives a crypt(5) hash for a run with no
> terminal. The account is set on the first boot only.

## `buildc vm build`

convert the built image into a qcow2, raw or iso

**Usage:** `buildc vm build [OPTIONS] [type]`

| Argument | Description |
| --- | --- |
| `<type>` | the disk type, which is `qcow2`, `raw` or `iso` |

| Option | Description |
| --- | --- |
| `--target <target>` | what a rebuild builds, and what an iso installs |
| `--image <ref>` | the container image to convert, without its tag |
| `--tag <tag>` | its tag, else $DEFAULT_TAG, else latest |
| `--ram <size>` | memory for the virtual machine |
| `--rebuild` | fetch, generate and build the container image first |

## `buildc vm run`

boot that disk under qemu, building it if missing

**Usage:** `buildc vm run [OPTIONS] [type]`

| Argument | Description |
| --- | --- |
| `<type>` | the disk type, which is `qcow2`, `raw` or `iso` |

| Option | Description |
| --- | --- |
| `--target <target>` | what a rebuild builds, and what an iso installs |
| `--image <ref>` | the container image to convert, without its tag |
| `--tag <tag>` | its tag, else $DEFAULT_TAG, else latest |
| `--ram <size>` | memory for the virtual machine |
| `--rebuild` | fetch, generate and build the container image first |

## `buildc vm spawn`

boot a qcow2 or raw disk with systemd-vmspawn

**Usage:** `buildc vm spawn [OPTIONS] [type]`

| Argument | Description |
| --- | --- |
| `<type>` | the disk type, which is `qcow2` or `raw` |

| Option | Description |
| --- | --- |
| `--target <target>` | what a rebuild builds, and what an iso installs |
| `--image <ref>` | the container image to convert, without its tag |
| `--tag <tag>` | its tag, else $DEFAULT_TAG, else latest |
| `--ram <size>` | memory for the virtual machine |
| `--rebuild` | fetch, generate and build the container image first |

## `buildc section`

Prints the generated Containerfile component section of one image, or of the
default image if none is named.

**Usage:** `buildc section [image]`

| Argument | Description |
| --- | --- |
| `<image>` | the image; the default image if absent |

## `buildc graph`

Prints the capability graph of the default image. The graph shows what provides
each capability, what requires it, what only orders against it, and what the
base already carries.

**Usage:** `buildc graph [OPTIONS]`

| Option | Description |
| --- | --- |
| `--format <md|json>` | markdown holding a mermaid diagram by default, or json |

## `buildc why`

Prints what one component is and where every byte of it came from. It lists the
targets that build it, what it provides and requires, and what it claims to
harden. It also lists the collection and pin it came from, whether it was
edited since, what it fetches, and whether it adds a package repository.

**Usage:** `buildc why [OPTIONS] [component]`

| Argument | Description |
| --- | --- |
| `<component>` | the component name |

| Option | Description |
| --- | --- |
| `--format <md|json>` | markdown, the default, or JSON |

> [!NOTE]
> In a repository it reads the resolved plan. On a booted image it reads the
> two documents that the build baked into the image, and it also prints the
> source repository, the commit and the `git clone` that reaches them.
>
> On a booted image, it refuses a document written for a schema version that
> this `buildc` does not read, and names both versions. `buildc plan --json` still
> prints the document.
>
> A component that tracks its library and was edited since its import is refused,
> so `buildc why` stops. An untracked component edited since its import is said
> plainly, and it is not an error.
>
> `buildc` does not read a `repo.sh` file, so it points at the file and prints the
> URLs it found.

## `buildc coverage`

Prints each rule of the profile that an image `conforms` to, with the component
that claims it. If no component claims a rule, then it names a component in the
repository or its collections that would. It uses the default image if none is
named, and a terminal picks one.

**Usage:** `buildc coverage [OPTIONS] [image]`

| Argument | Description |
| --- | --- |
| `<image>` | the image; the default image if absent, or a picker in a terminal |

| Option | Description |
| --- | --- |
| `--format <md|json>` | markdown, the default, or json |
| `--datastream <file>` | the SSG content the profile is read out of |

**Schema:** [`image.kdl`&nbsp;›&nbsp;`conforms`](schema/image.md#image-conforms)

> [!NOTE]
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
> so no component can claim it.

## `buildc plan`

Prints every fact that the repository derives as one JSON document. The
document holds the images, the targets of each image, and what each target is
made of. A script reads a field from it, and derives nothing from a name.

**Usage:** `buildc plan [OPTIONS]`

| Option | Description |
| --- | --- |
| `--json` | the output is JSON with or without it |

## `buildc verify`

Writes every build file again in memory, and compares each one byte for byte
with what the repository holds: `generated/`, the declared workflows and
`.github/renovate.json5`. It names each file that differs, each file that is
missing, and each file under `generated/` that nothing writes. Every build runs
it first.

**Usage:** `buildc verify`

## `buildc summary`

Prints a markdown table of what one target is made of. Each row is a component
that the target builds, with its description and the options it resolved. A CI
build writes the table into its job summary.

**Usage:** `buildc summary [target]`

| Argument | Description |
| --- | --- |
| `<target>` | the image, or `<image>/<flavour>` for a flavour |

## `buildc sbom`

Prints the pinned payloads of one target, as SPDX packages and their
relationships. A scan of the built image cannot see where a downloaded asset
came from, so CI merges this into the SBOM that the scan produces.

**Usage:** `buildc sbom [target]`

| Argument | Description |
| --- | --- |
| `<target>` | the image, or `<image>/<flavour>` for a flavour |

## `buildc scap`

Reads the report of one scan against the datastream that produced it, and
prints what they say about the target as markdown. The report shows what each
component claimed and what was measured, the score against each profile of the
datastream, and what stopped passing since the last scan.

**Usage:** `buildc scap [OPTIONS] [arf.xml] [COMMAND]`


| Argument | Description |
| --- | --- |
| `<arf.xml>` | the ARF report that the scan wrote |

| Option | Description |
| --- | --- |
| `--target <target>` | the target, else the ungated one |
| `--datastream <file>` | the SSG content, else the one `scap content` names |
| `--baseline <file>` | the last scan's pass set, read then rewritten |
| `--base-scan <file>` | what the bare base passed alone, read only |

> [!NOTE]
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
> goes to stdout either way.

## `buildc scap content`

Prints the path of the datastream that the target is measured with. If the image
declares no `conforms`, then it prints nothing, and the CI scan job skips the
image. `buildc set conforms` writes a `conforms`.

**Usage:** `buildc scap content [OPTIONS]`

| Option | Description |
| --- | --- |
| `--target <target>` | the target, else the ungated one |

## `buildc scap tailoring`

Prints the XCCDF tailoring that a scan of the target runs. The tailoring defines
the profile `xccdf_tect_profile_measured`, which is the declared profile with
every group and rule selected. If the image declares no `conforms`, then it
prints nothing.

**Usage:** `buildc scap tailoring [OPTIONS]`

| Option | Description |
| --- | --- |
| `--target <target>` | the target, else the ungated one |
| `--datastream <file>` | the SSG content, else the one `scap content` names |

> [!NOTE]
> `--profile '(all)'` scores every variable at its default, so a rule set to the
> profile's value reads as failing. The tailoring keeps the profile's values and
> still checks every claim outside the profile.

## `buildc scap rules`

print the rule each benchmark number reaches, one per line

**Usage:** `buildc scap rules [OPTIONS] [number]...`

| Argument | Description |
| --- | --- |
| `<number>` | a benchmark number, such as `1.1.1.1` |

| Option | Description |
| --- | --- |
| `--datastream <file>` | the SSG content, else the one `scap content` names |

## `buildc fetch`

Downloads one payload, checks it against its hash, and places it by its kind.
It runs inside a build:

- `file` keeps the download
- `tree` unpacks it
- `bin` installs one executable
- `rpm` installs the package, on an rpm family
- `deb` installs the package, on a deb family

**Usage:** `buildc fetch <what> <url> <sha256> [target] [extra]...
       fetch [what] [url] [sha256] [target] [extra]... <COMMAND>`


| Argument | Description |
| --- | --- |
| `<what>` | the kind of payload, which is `file`, `tree`, `bin`, `rpm` or `deb` |
| `<url>` | the URL to download |
| `<sha256>` | the hash that the download must match |
| `<target>` | the path, the directory or the executable name that the kind places |
| `<extra>` | for `tree`, more arguments to tar; for `bin`, the file inside the archive |

**Schema:** [`manifest.kdl`&nbsp;›&nbsp;`asset`](schema/component.md#asset)

## `buildc fetch components`

Refreshes each tracked component from the library that its lock entry names, as
`sources` in `repo.kdl` declares that library now. It replaces the component's
files, and records the new content hash and revision in the lock. A component
whose library has not moved stays as it is. The scheduled `update-components`
workflow runs it and opens a pull request.

**Usage:** `buildc fetch components [path]...`

| Argument | Description |
| --- | --- |
| `<path>` | the components, by their path under `components/`; every tracked component if absent |

**Schema:** [`components.lock.kdl`](schema/lock.md)

> [!WARNING]
> A tracked component whose content no longer matches its lock entry stops the
> update before anything is written. Undo the edit, or make the copy the
> repository's own with `buildc set component <path> --untracked`.

> [!NOTE]
> A tracked component whose directory is missing is copied again. A component that
> another component's directory holds has its own lock entry, so the refresh of
> the outer component leaves it in place.

## `buildc registry namespace`

Prints where the images publish. If `$IMAGE_REGISTRY` is set, then it prints
that value. Otherwise it prints `ghcr.io/<owner>`, with the owner read from the
GitHub origin remote.

**Usage:** `buildc registry namespace`

## `buildc registry ref`

Prints the full reference that one target publishes under. The reference joins
the namespace, the name of the target and its tag. With no target, it uses the
default image.

**Usage:** `buildc registry ref [OPTIONS]`

| Option | Description |
| --- | --- |
| `--target <target>` | the target, else the ungated one |
| `--tag <tag>` | the tag, else $DEFAULT_TAG, else latest |

## `buildc recipe`

Prints the part of an install recipe that the repository answers, as JSON that
the installer reads. The recipe holds the reference to install, the reference
the machine updates from, and properties of the image. The properties are the
boot chain, the bootloader, the root filesystem and whether the initramfs can
unlock LUKS. The disk, the account and the encryption choice belong to the
user, so the recipe holds none of them.

**Usage:** `buildc recipe [OPTIONS]`

| Option | Description |
| --- | --- |
| `--target <target>` | the target, else the ungated one |
| `--tag <tag>` | the tag, else $DEFAULT_TAG, else latest |
| `--image <ref>` | the bytes installed, else the published reference |

> [!NOTE]
> The base family settles each value. For a family with no measured answer it
> stops, because a wrong value erases a disk that then does not boot.
>
> `--image` installs a local build while the machine still updates from the
> published reference.
>
> `hostname` is the published name. It is the one value that the user is
> expected to replace.

## `buildc build-record`

Writes `/usr/share/buildc/build.json`, the record of what the build resolved.
The baked `manifest.json` beside it holds what the repository declared. The
record holds the digest of the base, the commit of each cloned asset, the source
commit, the `buildc` release, the target, the content hash of each component, and
whether enforcement was on.

No file under `generated/` holds the record, so `buildc verify` never compares
it. The record changes with each build.

**Usage:** `buildc build-record`

## `buildc validate-image`

Runs every check that a built image has to pass. The build gives it the preset
files of the enabled components, and it fails on each one that the image does not
have.

**Usage:** `buildc validate-image`

**Schema:** [`manifest.kdl`&nbsp;›&nbsp;`allow-verify`](schema/component.md#allow-verify)
