# Buildc

Buildc is a framework for building bootc Linux images. It consists of a
repository schema and `buildc`, the command-line tool that turns image
declarations into reproducible local and CI build workflows.

A Buildc repository declares its images, modules, inputs, and policies.
`buildc` checks their composition, resolves them into an inspectable build plan,
generates reviewable build files and workflows, builds the images, and records
what each build resolved.

> [!NOTE]
> Buildc is in active development. The schema and command line may change
> between minor releases. Only the latest release is supported.

## Why Buildc

- **From declaration to installed system.** Build images locally or in CI,
  boot them in local virtual machines, and create installation media from the
  same declarations.
- **Ready-to-build defaults.** A scaffolded repository contains an image that
  can be generated and built immediately, and adapted as its requirements
  grow.
- **Reusable and extensible modules.** Modules can add image features or
  provide build capabilities for other modules. They can assemble one file
  from contributions across the image or request purpose-specific keys that
  `buildc` generates and places safely.
- **Use only the abstraction you need.** A local module can use plain shell
  scripts, file-tree overlays, or verbatim Containerfile fragments. A module
  manifest adds quality-of-life declarations for packages, options,
  capabilities, family differences, assets, and other common needs. A module
  can mix these forms.
- **Capability-based composition.** Modules declare what they provide and
  require. `buildc` resolves those declarations into a capability graph and
  catches missing requirements and incompatible combinations before the build
  starts.
- **One reviewable build plan.** Containerfiles, module scripts, capability
  graphs, workflows, and summaries derive from the same resolved plan. Every
  build verifies generated files byte for byte before using them.
- **Inspectable images.** Built images retain both the repository manifest and
  a record of what the build resolved, so their composition remains available
  after deployment.
- **Repository-wide control.** `repo.kdl` provides one place for rules and
  requirements that apply across the repository. Modules declare what they
  need, while the repository owner decides what the build permits. Defaults
  remain permissive unless the owner opts into stricter policy.
- **Verifiable provenance.** Imported and copied modules retain their origin
  and content hash. Declared downloads add relationships to CI-generated SBOMs
  that ordinary image scanners cannot infer.
- **Audit support.** Modules can map SCAP claims to the features that implement
  them. Generated reports keep declared coverage separate from results
  verified by a scan.

## Install

```sh
curl -fsSL https://raw.githubusercontent.com/buildc-org/buildc/main/install.sh | sh
```

The installer places `buildc` and its matching scaffolding assets under
`~/.local` by default, or under `/usr/local` when run as root. Upgrade both
together with:

```sh
buildc upgrade
```

Prebuilt releases are available for x86_64 and aarch64 Linux. On other
platforms, build `buildc` from source.

## Start a repository

Create the repository and choose its first image:

```sh
buildc create repo workstation
cd workstation
```

`buildc create repo` gathers its answers before writing anything, so cancelling
leaves no partial repository behind. A repository whose first image is also
named `workstation` starts with these key files:

```text
workstation/
├── .github/
│   └── renovate.json5
├── disk_config/
│   └── disk.toml
├── modules/
├── scripts/
│   └── Containerfile.skeleton
├── repo.kdl
└── workstation.image.kdl
```

Optionally import a module from a collection declared by the repository:

```sh
buildc import module
```

Generate the build files and configured CI workflows:

```sh
buildc generate
```

The generated tree includes:

```text
generated/
├── workstation/
│   ├── modules/
│   ├── Containerfile
│   ├── finalize.sh
│   ├── graph.md
│   └── graph.json
├── scripts/
└── plan.json
```

The generated Containerfile and capability graph can be reviewed before the
build. Generated files are committed but not edited by hand; `buildc` verifies
them against the declarations before every build.

Build the default image:

```sh
buildc build
```

With `--no-tui`, the same workflow runs in a script: a missing answer comes
from its flag, else from its default, else the command fails and names the
flag.

## Documentation

The full documentation is at
<https://buildc-org.github.io/buildc/>.

- [CLI reference](docs/cli.md)
- [KDL syntax](docs/kdl.md)
- [Repository layout](docs/schema/repository.md)
- [Repository schema](docs/schema/repo.md)
- [Image schema](docs/schema/image.md)
- [Module layout](docs/schema/modules.md)
- [Module schema](docs/schema/module.md)
- [Provenance schema](docs/schema/provenance.md)
- [Base catalog schema](docs/schema/bases.md)

The schema references are generated from the same definitions that `buildc`
uses to validate repositories.

## Releases

[CHANGELOG.md](CHANGELOG.md) lists the changes in each release.

Each release tarball has an accompanying SHA-256 checksum and CycloneDX SBOM.
The binary embeds dependency metadata for scanners, and GitHub publishes build
provenance attestations:

```sh
gh attestation verify buildc-v<version>-x86_64-linux-musl.tar.gz \
    -R buildc-org/buildc
```

## Building from source

```sh
cargo build --release
```

A local build is not an installation. Set `BUILDC_ASSETS` to this repository's
`assets/` directory when running it, or `buildc` may use assets already installed
on the host.

[CONTRIBUTING.md](CONTRIBUTING.md) documents the development checks.

## Licence

Apache 2.0. See [LICENSE](LICENSE).
