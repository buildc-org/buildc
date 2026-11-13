<a id="repo.kdl"></a>

# `repo.kdl`

Location: The root of the repository.

The file holds at most one of each repository setting, and `buildc` reports an unknown node rather than ignoring it. It must declare `schema-version`, and each image lives in an image file.

```kdl
schema-version 1
name "Workstation"
base-url "https://github.com/owner/workstation"
workflows at="12:30" {
    build
}
renovate rebaseWhen="never" {
    schedule "* 0-5 * * *"
}
sources {
    components "buildc" {
        url "https://github.com/buildc-org/library"
        path "components"
    }
}
```

| Field | Accepts | Description |
| --- | --- | --- |
| [`schema-version`](#schema-version) (required) | *number* | The schema release that this repository is written against. |
| [`buildc-version`](#buildc-version) (optional) | *string* | The `buildc` release that builds this repository. |
| [`name`](#name) (optional) | *string* | An optional human-readable label for the repository. |
| [`base-url`](#base-url) (optional) | *string* | The HTTPS URL of the repository, which an image option value reads as `{base-url}`. |
| [`default-image`](#default-image) (optional) | *string* | The image that a command with no image, or a build with no target, uses. |
| [`pr-image`](#pr-image) (optional) | *string* | The image a pull request builds. |
| [`backend`](#backend) (optional) | `buildah` or `buildx` | The container build tool that `buildc build` runs and that the generated `build.yml` sets up. |
| [`seed`](#seed) (optional) | *string* | The image that this repository publishes as a starting point for a new repository. |
| [`workflows`](#workflows) (optional) | {&nbsp;[fields](#workflows-fields)&nbsp;} | The CI workflows that `buildc generate` writes into `.github/workflows/`. |
| [`renovate`](#renovate) (optional) | optionally {&nbsp;[fields](#renovate-fields)&nbsp;} | The Renovate policy that `buildc generate` writes into `.github/renovate.json5`. |
| [`sources`](#sources) (optional) | {&nbsp;[fields](#sources-fields)&nbsp;} | The component libraries this repository reads. |
| [`manifest`](#manifest) (optional) | {&nbsp;[fields](#manifest-fields)&nbsp;} | Whether a build stamps the generated manifest onto the image as an OCI label. |
| [`audit`](#audit) (optional) | {&nbsp;[fields](#audit-fields)&nbsp;} | How the repository treats a provenance check that fails. |
| [`security-policy`](#security-policy) (optional) | optionally {&nbsp;[fields](#security-policy-fields)&nbsp;} | The security rules that the repository sets for every image it defines. |

<a id="schema-version"></a>

## `schema-version` (required)

The number names the schema release the repository uses, and `buildc` stops at any number it does not implement. It reports the difference and reads no repository node.

```kdl
schema-version 1
```

Accepts: *number*

> [!NOTE]
> The version picks the reader, so a repository written against an earlier release keeps building.
>
> If `buildc` does not read the version, then it says so once and reports no node it cannot place.
>
> `buildc` does not move a repository to a newer version. The release that the repository pins reads it.

<a id="buildc-version"></a>

## `buildc-version` (optional)

The value names the release that builds and reads the repository. `buildc` reports a pin that names another release, and `sha256=` must be 64 lowercase hexadecimal digits.

```kdl
buildc-version "0.6.49"
```

Accepts: *string*

If a different release of `buildc` reads the repository:

- `buildc` says once that the pin names a different release.
- `schema-version` decides whether that release can read the repository.
- `buildc verify` reports the difference as drift.
- `buildc generate` resolves the drift.

| Property | Accepts | Description |
| --- | --- | --- |
| `sha256=` (optional) | *string* | The sha256 that `buildc.sh` holds the release tarball to. |

> [!NOTE]
> `buildc.sh` fetches this release, so the build does not use the `buildc` installed on the machine.
>
> If `sha256=` is absent, then `buildc.sh` checks the checksum published beside the tarball. That check proves only that the download is complete.
>
> If the repository pins no release, then `buildc` says nothing about the release.

<a id="name"></a>

## `name` (optional)

The label is for people, and the directory may use another name. It names no image and no target.

```kdl
name "Workstation"
```

Accepts: *string*

<a id="base-url"></a>

## `base-url` (optional)

`buildc` writes the value in place of each `{base-url}` reference when it generates. The generated files hold the URL itself. The value must start with `https://` and hold none of `"`, `\`, `$`, a backtick or a brace. An image option value that reads `{base-url}` fails `buildc check` when the repository declares no `base-url`.

```kdl
base-url "https://github.com/owner/workstation"
```

Accepts: *string*

<a id="default-image"></a>

## `default-image` (optional)

The id must name an image declared in the repository, and `buildc` refuses an id that no image declares. A repository with more than one image and no `default-image` has no target for a command that names no image.

```kdl
default-image "workstation"
```

Accepts: *string*

<a id="pr-image"></a>

## `pr-image` (optional)

The image a pull request builds.

```kdl
pr-image "workstation"
```

Accepts: *string*

<a id="backend"></a>

## `backend` (optional)

The value selects the one tool that `buildc build` drives, and `buildah` applies when the node is absent. `buildx` needs a `default-image` to name the layer cache, and it cannot build an image whose `boot-chain` seals a UKI.

```kdl
backend "buildx"
```

Accepts: `buildah` or `buildx`

| Value | Effect |
| --- | --- |
| `buildah` | `buildc build` runs `podman build`. An image whose `boot-chain` builds a UKI needs it. |
| `buildx` | `buildc build` runs `docker buildx build`. The generated `build.yml` loads its OCI archive into the podman store that the later steps read, and it can use a registry layer cache. |

> [!NOTE]
> `--backend` or `$BUILD_BACKEND` overrides it for one build. The flag wins over the variable, and the variable wins over this node.
>
> `buildc check` refuses `buildx` for an image whose `boot-chain` builds a UKI. A sealed UKI carries the composefs digest, and `buildc build` reads that digest from the local buildah store.

<a id="seed"></a>

## `seed` (optional)

The image must be declared in the repository, and `collection=` must name a `components` source that `sources` declares. Every component in the seed is fetched through that collection, so an inline definition leaves the seed unbuildable.

```kdl
seed "workstation" collection="owner"
```

Accepts: *string*

`buildc generate` writes the seed to `generated/seed.kdl`. The seed carries:

- the base of the image;
- the component list of the image;
- the collections that those components come from.

| Property | Accepts | Description |
| --- | --- | --- |
| `collection=` (required) | *string* | The collection that this repository publishes its own components as. |

> [!NOTE]
> The seed carries no name, URL or owner of this repository.
>
> The seed names each component through the collection it is fetched from, so `collection=` names a collection in `sources`. A repository is seedable only if it publishes its own `components/` as a collection.
>
> If the seed image lists a component that no collection can import, then `buildc` reports it, because a new repository could not build that seed.

<a id="workflows"></a>

## `workflows` (optional)

The block selects the workflows that `buildc generate` writes, and a workflow named twice is refused. A child's `schedule=` needs a five-field cron line, and `at` needs an hour and minute in UTC.

```kdl
workflows at="12:30" {
    build
}
```

Accepts: {&nbsp;[fields](#workflows-fields)&nbsp;}

| Property | Accepts | Description |
| --- | --- | --- |
| `at=` (optional) | *string* | The hour and minute in UTC that the daily build runs at. Every other scheduled workflow runs at its own offset from it. |
| `publish=` (optional) | `scheduled` | If `scheduled`, then images publish from the daily build alone, and image scans move off pushes too, because a scan reads a published image. If absent, then images also publish on pushes. |
| `scan=` (optional) | `scheduled` | If `scheduled`, then image scans run on the daily build alone. If absent, then scans run on pushes and on scheduled builds. |

<a id="workflows-fields"></a>

| Field | Accepts | Description |
| --- | --- | --- |
| [`<name>`](#workflows-name) (optional,&nbsp;repeatable) |  | One workflow, which the stem of its file names. |

> [!NOTE]
> `buildc generate` writes only the workflows named here. After a tool upgrade, it rewrites each named workflow.
>
> `buildc set workflows` edits this block.
>
> The `conforms` of each image decides whether SCAP content exists to scan.

<a id="workflows-name"></a>

### `<name>` (optional, repeatable)

One workflow, which the stem of its file names.

```kdl
workflows at="12:30" {
    build
}
```

| Property | Accepts | Description |
| --- | --- | --- |
| `schedule=` (optional) | *string* | The cron line that this workflow runs on, in UTC. It replaces the schedule that `at` places. |

<a id="renovate"></a>

## `renovate` (optional)

The block maps member by member to Renovate's JSON, so a property is a scalar, arguments are an array and a child is an object. A node that holds no value, or both values and members, is refused.

```kdl
renovate rebaseWhen="never"
```

Accepts: optionally {&nbsp;[fields](#renovate-fields)&nbsp;}

<a id="renovate-fields"></a>

| Field | Accepts | Description |
| --- | --- | --- |
| [`<name>`](#renovate-name) (optional,&nbsp;repeatable) |  | One Renovate setting, named as Renovate names it. |

> [!NOTE]
> A property is a scalar setting. The arguments of a child are an array, because Renovate wants array syntax for a list of one.
>
> The properties and named children of a child are an object. A block of `-` children is an array, and each `-` holds one value or one object.
>
> `$schema`, `ignorePaths`, `dockerfile` and `customManagers` belong to `buildc generate`, and the block refuses them.
>
> If the block is absent, then `buildc generate` writes no `.github/renovate.json5`, and leaves one the user wrote in place.

<a id="renovate-name"></a>

### `<name>` (optional, repeatable)

One Renovate setting, named as Renovate names it.

```kdl
renovate rebaseWhen="never" {
    schedule "* 0-5 * * *"
}
```

<a id="sources"></a>

## `sources` (optional)

Each child declares a `components` library under the alias that references and the local cache use. The alias takes lowercase letters, digits and dashes, and one kind and alias identify one source.

```kdl
sources {
    components "buildc"
}
```

Accepts: {&nbsp;[fields](#sources-fields)&nbsp;}

<a id="sources-fields"></a>

| Field | Accepts | Description |
| --- | --- | --- |
| [`components`](#sources-components) (optional,&nbsp;repeatable) | *string*, then optionally {&nbsp;[fields](#sources-components-fields)&nbsp;} | A component library that imports and `buildc create image` read components and bases from. |

> [!NOTE]
> `buildc import component` copies a component from a `components` source to `components/<name>`, and `components/components.lock.kdl` records the source.

<a id="sources-components"></a>

### `components` (optional, repeatable)

A component library that imports and `buildc create image` read components and bases from.

```kdl
sources {
    components "buildc"
}
```

Accepts: *string*, then optionally {&nbsp;[fields](#sources-components-fields)&nbsp;}

<a id="sources-components-fields"></a>

| Field | Accepts | Description |
| --- | --- | --- |
| [`dir`](#sources-components-dir) (optional) | *string* | The local directory that holds this source. |
| [`url`](#sources-components-url) (optional) | *string* | The HTTPS Git repository that holds this source. |
| [`path`](#sources-components-path) (optional) | *string* | The directory inside the Git repository that holds this component library. |
| [`version`](#sources-components-version) (optional) | *string* | The branch, tag or commit that selects the repository tree. |
| [`sha256`](#sources-components-sha256) (optional) | *string* | The hash of the canonical tar archive for the selected commit. |
| [`unpinned`](#sources-components-unpinned) (optional) | *string* | Why this source follows a ref without a verified archive hash. |

<a id="sources-components-dir"></a>

#### `dir` (optional)

The local directory that holds this source.

```kdl
sources {
    components "buildc" {
        dir "../library/components"
    }
}
```

Accepts: *string*

<a id="sources-components-url"></a>

#### `url` (optional)

The HTTPS Git repository that holds this source.

```kdl
sources {
    components "buildc" {
        url "https://github.com/buildc-org/library"
    }
}
```

Accepts: *string*

<a id="sources-components-path"></a>

#### `path` (optional)

The directory inside the Git repository that holds this component library.

```kdl
sources {
    components "buildc" {
        path "components"
    }
}
```

Accepts: *string*

<a id="sources-components-version"></a>

#### `version` (optional)

The branch, tag or commit that selects the repository tree.

```kdl
sources {
    components "buildc" {
        version "v1"
    }
}
```

Accepts: *string*

<a id="sources-components-sha256"></a>

#### `sha256` (optional)

The hash of the canonical tar archive for the selected commit.

```kdl
sources {
    components "buildc" {
        sha256 "b7c232b0e8249d8e55a40beb79c5c43a7d370f3f9408bd215deb0170daeaadf3"
    }
}
```

Accepts: *string*

<a id="sources-components-unpinned"></a>

#### `unpinned` (optional)

Why this source follows a ref without a verified archive hash.

```kdl
sources {
    components "buildc" {
        unpinned "This repository follows the library's default ref"
    }
}
```

Accepts: *string*

<a id="manifest"></a>

## `manifest` (optional)

The block holds one `label`, and a build without the block stamps no label. `label #true` writes `org.buildc.manifest` with the path where the manifest sits in the image.

```kdl
manifest {
    label #true
}
```

Accepts: {&nbsp;[fields](#manifest-fields)&nbsp;}

<a id="manifest-fields"></a>

| Field | Accepts | Description |
| --- | --- | --- |
| [`label`](#manifest-label) (optional) | *boolean* | Whether the build stamps `org.buildc.manifest` with the path of the baked manifest file. |

<a id="manifest-label"></a>

### `label` (optional)

Whether the build stamps `org.buildc.manifest` with the path of the baked manifest file.

```kdl
manifest {
    label #true
}
```

Accepts: *boolean*

<a id="audit"></a>

## `audit` (optional)

The posture applies to every provenance check that `buildc` runs. The facts are recorded whether or not the block exists, and `enforce #true` is what makes a failed check stop the command.

```kdl
audit {
    enforce #true
}
```

Accepts: {&nbsp;[fields](#audit-fields)&nbsp;}

`buildc` records these provenance facts whether or not this block exists:

- the hash of each component;
- the source of each import;
- the digest of the base tag;
- the commit of each cloned asset.

<a id="audit-fields"></a>

| Field | Accepts | Description |
| --- | --- | --- |
| [`enforce`](#audit-enforce) (optional) | *boolean* | Whether a failed provenance check stops the command that runs it. |

<a id="audit-enforce"></a>

### `enforce` (optional)

Whether a failed provenance check stops the command that runs it.

```kdl
audit {
    enforce #true
}
```

Accepts: *boolean*

Each check runs in the command named before it:

- `buildc import component`, `buildc create image` and `buildc set conforms`: the library a component is copied from is pinned to a tag and its hash.
- `buildc build`: the base tag resolves to a manifest digest.
- `buildc build`: the repository is at a commit.
- `buildc build`: each cloned asset resolves to a commit.
- `buildc scap`: the scan report passes every rule of the profile.

| Value | Effect |
| --- | --- |
| `#true` | A failed check is an error, and the command stops. |
| `#false`, or no `enforce` | `buildc` reports a failed check, and the command continues. |

> [!NOTE]
> `buildc` also reports a `component.sh` or `finalize.sh` that reaches the network with no `asset` that declares the download. This report does not depend on `enforce`.
>
> `buildc` refuses a component that tracks its library and no longer matches its entry in the component lock. This refusal does not depend on `enforce`.

<a id="security-policy"></a>

## `security-policy` (optional)

The block holds one `network` rule for every image the repository declares. An image cannot set a rule of its own.

```kdl
security-policy
```

Accepts: optionally {&nbsp;[fields](#security-policy-fields)&nbsp;}

<a id="security-policy-fields"></a>

| Field | Accepts | Description |
| --- | --- | --- |
| [`network`](#security-policy-network) (optional) | optionally `allow` or `strict`, then optionally {&nbsp;[fields](#security-policy-network-fields)&nbsp;} | The rule that decides whether each step of a component layer reaches the network. |

> [!NOTE]
> If the `security-policy` block is absent, then every step keeps the network.

<a id="security-policy-network"></a>

### `network` (optional)

The value on the line applies to both kinds of step, and a `packages` or `scripts` child overrides one kind. A denied step runs under `--network=none` in the generated Containerfile.

```kdl
security-policy {
    network "strict"
}
```

Accepts: optionally `allow` or `strict`, then optionally {&nbsp;[fields](#security-policy-network-fields)&nbsp;}

A component layer runs two kinds of step:

- A package step installs the repo files, the COPRs and the packages of the component.
- A script step runs the `component.sh` of the component and its finalize hook.

| Value | Effect |
| --- | --- |
| `allow` | Both kinds of step take `allow`, unless `packages` or `scripts` sets another rule. |
| `strict` | Both kinds of step take `strict`, unless `packages` or `scripts` sets another rule. |
| No value | Both kinds of step take `allow`, unless `packages` or `scripts` sets another rule. |

<a id="security-policy-network-fields"></a>

| Field | Accepts | Description |
| --- | --- | --- |
| [`packages`](#security-policy-network-packages) (optional) | `allow` or `strict` or `deny` | The rule that every package step runs under. |
| [`scripts`](#security-policy-network-scripts) (optional) | `allow` or `strict` or `deny` | The rule that every script step runs under. |

<a id="security-policy-network-packages"></a>

#### `packages` (optional)

The rule that every package step runs under.

```kdl
security-policy {
    network "strict" {
        packages "allow"
    }
}
```

Accepts: `allow` or `strict` or `deny`

| Value | Effect |
| --- | --- |
| `allow` | Every package step reaches the network. |
| `strict` | Every package step reaches the network, because a package step runs only where the component declares packages, a COPR or a repo file. |
| `deny` | Every package step runs under `--network=none`. |

<a id="security-policy-network-scripts"></a>

#### `scripts` (optional)

The rule that every script step runs under.

```kdl
security-policy {
    network "strict" {
        scripts "strict"
    }
}
```

Accepts: `allow` or `strict` or `deny`

| Value | Effect |
| --- | --- |
| `allow` | Every script step reaches the network. |
| `strict` | A script step reaches the network only if its component declares `network "scripts"`. |
| `deny` | Every script step runs under `--network=none`. |

> [!NOTE]
> If the rule is `strict` or `deny`, then `buildc check` names each component that ships a Containerfile fragment. The rule does not reach the `RUN` lines of a fragment.
