# Threat model

This page is for a security team that is deciding whether to build a fleet
with `buildc`. It names each trust boundary, the control at that boundary, the
risks the project accepts and the gaps that are still open. Each control cites
the file that enforces it. The model covers three repositories: `buildc`
(the `buildc` CLI), `installer` (`bootc-anywhere`) and `common`, the library
that both link.

Report a vulnerability as the [security policy](https://github.com/buildc-org/buildc/security/policy) says.

## What an adopter trusts

The image repository is trusted. Its components run as root inside the build
containers. So does every component that a build reads from a library, and
every copy that `buildc copy component` made from one.
`buildc` controls where a component comes from and records what it was. `buildc` does
not sandbox what a component does. The review of the repository and its pins is
the control for that code.

## Trust boundaries

### The image repository and `repo.kdl`

- `audit { enforce #true }` turns each provenance check into an error in the
  command that runs it. The [`repo.kdl`](schema/repo.md#audit-enforce) page
  lists each check and its command. `src/provenance/mod.rs` `enforce_build`
  refuses a build whose base did not resolve to a digest, and a build from a
  tree with no commit.
- Every command that reads the resolved images refuses a remote component
  whose cached content no longer matches its entry in `repo.lock.kdl`
  (`src/resolve/library.rs` `Libraries::revise`). The refusal does not depend
  on `audit`.
- A copy that `buildc copy component` makes belongs to the repository.
  `components/origins.kdl` records its origin, its commit and its content hash,
  and `buildc why` reads out a later edit. No command refuses an edited copy.
- The `security-policy { network }` rule decides which generated component steps
  reach the network. `src/emit/containerfile.rs` runs a closed step with
  `--network=none`. If the repository declares no rule, then every step
  reaches the network.
- A component's `Containerfile.inc` fragment is copied into the build as written
  (`src/emit/containerfile.rs` `fragment`), so the network rule does not reach
  its own `RUN` lines. If the script rule is not `allow`, then `buildc check`
  names each component that ships a fragment (`src/provenance/mod.rs`
  `ungoverned`).
- Component secrets enter a step as BuildKit `--mount=type=secret`
  (`src/emit/containerfile.rs`), so no secret is copied into a layer.

### Libraries

A library is a tree of components and base components that `repo.kdl`
declares under `libraries`. A `local` library is a folder of the repository,
and `buildc` edits it in place. A `remote` library is a Git repository that
`buildc` fetches and never writes.

- `src/parse/repo.rs` `check_local_paths` refuses a `local` source that
  resolves outside the repository, through a symlink or a `..` segment, and
  the refusal blocks every edit.
- `src/parse/remote.rs` `parse_library` refuses a remote URL that is not
  `https://`, and a URL that holds a template. Any other source is a path to a
  Git checkout on this machine.
- Every git call on a library removes the variables that point git at a
  repository, such as `GIT_DIR` and `GIT_INDEX_FILE` (`src/collection.rs`
  `git`). A hook that runs `buildc` exports them, and git would otherwise fetch
  into the user's own repository.
- A fetch from a path source runs `git-upload-pack` in that checkout, and
  upload-pack reads the checkout's own git config. Git takes
  `uploadpack.packObjectsHook` only from the global, system and command-line
  config, so the checkout's config runs no command during the fetch.
- The fetch writes a commit's files into `.cache/sources/` through `git
  archive` with the host's global and system git config switched off
  (`src/collection.rs` `fetch_git`). A library's `.gitattributes` therefore
  cannot select a filter driver that the host config defines. The cache
  repository's own attributes file unsets `export-subst`, `export-ignore`,
  `text`, `eol`, `ident` and `working-tree-encoding` for every path. It
  overrides the fetched tree's attributes and the user's, so the cached files
  hold the bytes of their commit.
- `buildc generate` is the one command that moves a remote component. It
  fetches each library's `version`, or its default branch when it names none.
  It records the commit and the content hash of each remote component a build
  reads in `repo.lock.kdl` (`src/provenance/repo_lock.rs`). It also reads each
  tag that a `pin=` names again. Every other command reads a remote component
  at its locked commit from `.cache/sources/` and fetches nothing to resolve
  it. An entry applies only while the tree sets the `pin=` that the entry
  records.
- `buildc check` reports each remote component that `repo.lock.kdl` does not
  record, or whose content cannot be hashed (`repo_lock::unlocked`). With
  network, it asks each remote where a library `version` and a pinned tag point
  now, and it reports each one that moved away from the lock
  (`repo_lock::moved`).
- A library is never signature-verified. The content hash in `repo.lock.kdl`
  and the review of the change that moves it are the controls. See *Accepted
  risks*.
- Resolution refuses a component whose path resolves through a link outside
  its library, or onto the path of another component (`src/parse/component.rs`
  `Component::load`).
- `buildc copy component` refuses a component that resolves outside its
  library root, also through a linked components folder (`src/layout.rs`
  `contained`). Its pickers skip such a library.
- The copy refuses a link that leaves the component, and it keeps a link
  inside an overlay as a link, because that link is part of the image
  (`src/init.rs` `copy_below`).
- A copy from a local library records the checkout's commit only when the
  checkout is clean. It records no commit if the checkout's own config defines
  a filter driver or the checkout holds a submodule, because `git status`
  would run those drivers on the host (`src/provenance/origins.rs`
  `clean_head`). The user's global config stays, so the user's own filters
  such as git-lfs run as they do for any `git status`.
- The `update-components` workflow runs `buildc generate` with a token that can
  push a branch and open a pull request. The commits it moves `repo.lock.kdl`
  to reach `main` only through the review of that pull request.

### Assets a component downloads

An `asset` in `manifest.kdl` pins a download that the component's own script
fetches while its layer builds.

- An asset whose URL ends in `.git` is cloned, and its commit is the
  verifier. `buildc generate` resolves that commit with
  `protocol.ext.allow=never` (`src/provenance/build.rs` `clone_commit`), so the
  resolution cannot run a command through the `ext::` transport. `buildc build`
  resolves the tag again and refuses the build if it names another commit. If
  the remote does not answer, then the build warns, and an enforced audit
  refuses it (`src/build.rs` `confirm_clones`).
- The clone itself runs in the component's layer, as trusted component code, under
  Git's own protocol policy. An asset URL is not held to `https://`.
- `buildc build` records the resolved commit of each cloned asset in the build
  record. If the commit does not resolve, then `buildc build` names the asset
  and leaves it out of the record. Under enforcement, `buildc generate` and
  `buildc build` refuse it (`src/provenance/mod.rs` `enforce_build`).

### The SCAP datastream `generate` fetches

- A conforming image's base pins its datastream in `scap-content`. `buildc
  generate` downloads an `https://` URL with curl into `.cache/scap/`, or
  reads a path inside the repository. A path that leaves the repository,
  through `..` or a symbolic link, is refused, and so is any other scheme
  (`src/scap/pinned.rs` `fetch`, `inside`).
- The pin hashes the datastream itself. A `.zip` is unpacked with `unzip`
  before that hash is checked, so the bytes `unzip` writes are capped while
  they are read, whatever size the archive declares (`src/scap/pinned.rs`
  `member_within`). The download itself is capped by curl's
  `--max-filesize`.
- The verified file is committed under `generated/<image>/scap/` beside a
  record of its URL. A later `generate` reuses it only when both the URL and
  the hash match.

### The `buildc` binary a build runs

- `buildc-version` in `repo.kdl` pins the release that `buildc.sh` fetches. With
  `sha256=`, the tarball is held to that hash. Without it, `buildc.sh` checks
  the `.sha256` published beside the tarball, which proves only that the
  download is complete. `buildc check` warns about a pin with no `sha256=`
  (`src/dispatch/read.rs` `reading`).
- If `repo.kdl` declares no `buildc-version`, then `buildc.sh` resolves the latest
  release on each run (`assets/scripts/buildc.sh`). A build then takes a new
  release as soon as it is published, and `buildc check` does not warn. See
  *Known gaps*.
- `buildc upgrade` runs only when the user runs it. It fetches the tarball and
  the `.sha256` from the same GitHub release (`src/upgrade.rs`). See
  *Accepted risks*.
- Each release of `buildc` and `bootc-anywhere` builds `--locked` from the
  committed `Cargo.lock`, embeds its dependency list with `cargo auditable`,
  ships a CycloneDX SBOM per tarball and carries a GitHub provenance
  attestation (`.github/workflows/release.yml` in each repository).

### The build host

- The generated workflows start from `permissions: {}`, grant each job its
  own permissions, and pin every action to a commit SHA
  (`assets/.github/workflows/build.yml`).
- The generated workflow runs `buildc build` as root, because every later step
  reads the root container store (`assets/.github/workflows/build.yml`). A
  local `buildc build` can run rootless.
- A UKI build runs two containers from the image under build
  (`src/build.rs` `build_uki`). The composefs digest runs the image's own
  `bootc` with `--privileged` and the host's container storage mounted
  (`src/build.rs` `digest_argv`). In the generated workflow that container is
  privileged root on the runner. The sealed image's validation runs under
  its SELinux confinement with no `--privileged`. It mounts the build's own
  copy of the `buildc` binary read-only with `z`, which relabels that one file
  (`src/build.rs` `validate_argv`).
- The local VM and media scripts run outside the pinned workflow path.
  `assets/scripts/vm.sh` runs `bootc-image-builder:latest` and an untagged
  `qemux/qemu` image with `--privileged --pull=newer`, so a moved tag runs as
  root on the user's machine. `BIB_IMAGE` replaces the builder image.

### The registry

The generated build pushes the image under the `unverified` tag alone. It
signs the image and its SPDX SBOM attestation with `cosign` and the
repository's key, and verifies both against
`keys/public/etc/pki/containers/cosign.pub`. Only then does it copy the digest
to the published tags (`assets/.github/workflows/build.yml`). If the
verification fails, then no published tag moves. The verification passes
`--insecure-ignore-tlog=true`, so a signature does not need a transparency log
entry.

### Installer media and the installer

- The media fetches `bootc-anywhere` by version and holds it to a sha256
  per architecture (`src/emit/installer.Containerfile`).
- The media carries the image in its own container storage. The installer
  runs each check of the image with `--net=none` (`installer/src/boot.rs`).
- If the media's install recipe names a UKI chain, then the installer refuses
  the image unless the image carries the signed-chain marker
  (`installer/src/boot.rs` `require_signed_boot_chain`). The check runs before
  the first write to the disk (`installer/src/run.rs` `run`).
- The media records the ID of the image it was built to install
  (`assets/scripts/vm.sh`). Before the first write to the disk, the installer
  refuses an image that the store holds under another ID
  (`installer/src/run.rs` `require_built_image`). The confirmation screen
  shows the short ID. A hand-written payload may record no ID, and the
  installer then says that the identity is unchecked.
- In both cases the installer then reads the whole image back out of the
  store. That read checks each layer file against its layer's record, so an
  altered byte refuses the install.
- The image ID and the image are both on the media, so a rewrite of the whole
  media passes the check. See *Known gaps*.
- The install runs the image's own `bootc` with `--privileged`, the host PID
  namespace and `/dev` mounted (`installer/src/run.rs` `bootc_command`). That
  `bootc` comes from the image, so installing the image trusts it with the
  target machine.

### The installer and the partition table

The installer runs as root. It reads the partition table through
`libfdisk.so.1`, which it opens read-only (`installer/src/fdisk.rs`). It
writes the table by passing a script built from the user's answers to the
`sfdisk` binary (`installer/src/table.rs` `sfdisk_script`). The base of the
media supplies both, because `sfdisk` links `libfdisk`. Each `unsafe` block
carries a `// SAFETY:` comment, and the `[lints]` table in
`installer/Cargo.toml` denies one without it.

### The dependency supply chain

- `lint.sh` checks bans, licences and sources against each repository's
  `deny.toml` with `cargo deny`. CI also checks advisories on every pull
  request, every push to `main` and every week (`.github/workflows/ci.yml`).
- Every cargo command in `lint.sh` that resolves the dependency graph passes
  `--locked`, so CI builds the committed lock and never resolves around it.
- In the three tool repositories, Renovate proposes a release only after it
  has stood for seven days (`.github/renovate.json5`). The Renovate
  policy that `buildc create repo` writes into an image repository's `repo.kdl`
  has no such wait. See *Known gaps*.

## Accepted risks

Each risk is accepted for the reason given. The last column names what would
reopen it.

| Risk | Why it is accepted | What reopens it |
| --- | --- | --- |
| A library is locked by commit and content hash and never signature-verified. | The hash binds the exact tree the reviewer read, and the review of `repo.lock.kdl` approves each move. | A library publisher that signs its releases, or an adopter that needs to trust a publisher instead of a tree. |
| A remote library with no `version` follows its default branch. | A simple repository can follow its library, and only `buildc generate` moves it. `buildc check` reports each remote component that the lock does not record. | An adopter that needs a `repo.kdl` rule that requires every library to name a version. |
| `buildc upgrade` trusts a `.sha256` from the same release as the tarball. | No pin exists before the tool does. The GitHub attestation can be checked with `gh attestation verify`. | A signed release manifest. |
| The composefs digest runs the image under build with `--privileged`. | The image's own `bootc` computes the digest from the container storage. The image is already trusted as the repository's own code. | An unprivileged digest command upstream. |
| The install runs the image's own `bootc` privileged on the target machine. | The image is what the user chose to install, so its `bootc` is trusted with the machine. | None. |

`buildc` sends no telemetry and runs no background update.

## Known gaps

Each gap is current behaviour that a control does not yet cover.

- A repository with no `buildc-version` takes each new `buildc` release on its
  next build, with no warning from `buildc check`.
- A `Containerfile.inc` fragment's `RUN` lines run outside the network rule.
- The Renovate policy that `buildc create repo` writes into `repo.kdl` merges a
  pin update with no wait after the release.
- A rewrite of the whole installer media, recorded ID included, passes the
  installer's check.

## Air-gapped builds

These parts work with no network:

- a `local` library, and a remote library whose source is a path;
- the library cache in `.cache/sources/`, which holds each remote library by commit;
- `buildc check` and `buildc build`, which read each remote component at the commit `repo.lock.kdl` records;
- `buildc generate`, which keeps the commit the cache holds for a library `version` or a tag when a remote does not answer;
- installer media that carries the image in its own storage.

These parts need the network:

- `buildc generate` to move a remote library to a new commit;
- `buildc add component` and `buildc create image` from a remote library that is not on this machine;
- a run of `buildc.sh` in a repository with no `buildc-version`;
- a registry pull of the base image, and the digest lookup of a base named by
  tag (`src/provenance/build.rs` `base`);
- the commit lookup of each cloned asset;
- a component step that the network rule leaves open.
