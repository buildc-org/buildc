<a id="repo.kdl"></a>

# `repo.kdl`

Location: The root of the repository.

The file holds at most one of each repository setting, and `buildc` reports an unknown node rather than ignoring it. It must declare `schema-version`, and each image lives in an image file.

```kdl
schema-version 1
name "Workstation"
base-url "https://github.com/owner/workstation"
registry "ghcr.io/owner"
builds default="workstation" {
    workstation "./workstation" {
        install {
            bootloader "grub2"
        }
    }
}
github {
    workflows {
        image-build {
            push
            pr
            schedule frequency="daily" time="12:30"
        }
    }
}
renovate rebaseWhen="never" {
    schedule "* 0-5 * * *"
}
libraries default="stable" {
    local {
        mine {
            source "."
        }
    }
    remote {
        stable {
            source "https://github.com/buildc-org/library"
        }
    }
}
```

| Field | Accepts | Description |
| --- | --- | --- |
| [`schema-version`](#schema-version) (required) | *number* | The schema release that this repository is written against. |
| [`buildc-version`](#buildc-version) (optional) | *string* | The `buildc` release that builds this repository. |
| [`name`](#name) (optional) | *string* | An optional human-readable label for the repository. |
| [`base-url`](#base-url) (optional) | *string* | The HTTPS URL of the repository, which an image option value reads as `{base-url}`. |
| [`registry`](#registry) (optional) | *string* | The namespace that every build publishes under. |
| [`builds`](#builds) (optional) | {&nbsp;[fields](#builds-fields)&nbsp;} | The build targets of the repository. |
| [`backend`](#backend) (optional) | `buildah` or `buildx` | The container build tool that `buildc build` runs and that the generated `image-build.yml` sets up. |
| [`seed`](#seed) (optional) | *string* | The image that this repository publishes as a starting point for a new repository. |
| [`github`](#github) (optional) | optionally {&nbsp;[fields](#github-fields)&nbsp;} | Everything that runs on GitHub. |
| [`renovate`](#renovate) (optional) | optionally {&nbsp;[fields](#renovate-fields)&nbsp;} | The Renovate policy that `buildc generate` writes into `.github/renovate.json5`. |
| [`libraries`](#libraries) (optional) | {&nbsp;[fields](#libraries-fields)&nbsp;} | The libraries this repository reads components from. |
| [`library`](#library) (optional) | {&nbsp;[fields](#library-fields)&nbsp;} | Where this repository keeps the components it offers as a library. |
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

<a id="registry"></a>

## `registry` (optional)

A build publishes to `<registry>/<name>`. A repository with no `registry` builds and does not push, unless a build's `publish` holds a full reference. `IMAGE_REGISTRY` in the environment overrides the value.

```kdl
registry "ghcr.io/owner"
```

Accepts: *string*

<a id="builds"></a>

## `builds` (optional)

Each child is one build. `buildc` refuses two builds with one name.

```kdl
builds default="workstation" {
    workstation "./workstation"
}
```

Accepts: {&nbsp;[fields](#builds-fields)&nbsp;}

| Property | Accepts | Description |
| --- | --- | --- |
| `default=` (optional) | *string* | The build that a command uses when it names none. The name must name a declared build. If the repository declares one build, then that build is the default. |

<a id="builds-fields"></a>

| Field | Accepts | Description |
| --- | --- | --- |
| [`<name>`](#builds-name) (optional,&nbsp;repeatable) | optionally a *string*, then optionally {&nbsp;[fields](#builds-name-fields)&nbsp;} | One build target, which builds one component and names what it publishes. |

<a id="builds-name"></a>

### `<name>` (optional, repeatable)

The node name is the target, cache and published name, and an option value reads it as `{image-id}`. The argument names the component that the build builds. A path that starts with `./` reads `<path>.kdl` or `<path>/component.kdl` from the repository root, and any other path names a component in the components folder. With no argument, the build uses the component of its own name in the components folder.

```kdl
builds default="workstation" {
    workstation "./workstation"
}
```

Accepts: optionally a *string*, then optionally {&nbsp;[fields](#builds-name-fields)&nbsp;}

<a id="builds-name-fields"></a>

| Field | Accepts | Description |
| --- | --- | --- |
| [`publish`](#builds-name-publish) (optional) | *string* | The name the build publishes under, in place of its own name. |
| [`install`](#builds-name-install) (optional) | {&nbsp;[fields](#builds-name-install-fields)&nbsp;} | What the installer lays down if the answer of the base family is not the one that the build wants. |

<a id="builds-name-publish"></a>

#### `publish` (optional)

A bare name renames the artifact under `registry`. A value that holds `/` is a full reference. Neither holds a tag or a digest, because the build sets the tags.

```kdl
builds default="workstation" {
    workstation "./workstation" {
        publish "workstation-nightly"
    }
}
```

Accepts: *string*

<a id="builds-name-install"></a>

#### `install` (optional)

The block answers the install questions that the base family leaves open. If neither the family nor a UKI seal settles an answer, then the build has no install media unless the block gives it.

```kdl
builds default="workstation" {
    workstation "./workstation" {
        install {
            filesystem "btrfs"
        }
    }
}
```

Accepts: {&nbsp;[fields](#builds-name-install-fields)&nbsp;}

<a id="builds-name-install-fields"></a>

| Field | Accepts | Description |
| --- | --- | --- |
| [`filesystem`](#builds-name-install-filesystem) (optional) | `xfs` or `ext4` or `btrfs` | The root filesystem that the installer formats in place of the one that the base family settles. |
| [`composefs`](#builds-name-install-composefs) (optional) | *boolean* | Whether the install seals the deployment, which replaces the answer of the base family. |
| [`generic-image`](#builds-name-install-generic-image) (optional) | *boolean* | Whether the install skips the bootupd check, which replaces the answer of the base family. |
| [`admin-group`](#builds-name-install-admin-group) (optional) | *string* | The group that the installer creates an administrator in, which replaces the group of the base family. |
| [`bootloader`](#builds-name-install-bootloader) (optional) | `grub2` or `systemd` | The bootloader that the installer installs. The catalog entry of the base lists the bootloaders that the base carries. |

<a id="builds-name-install-filesystem"></a>

##### `filesystem` (optional)

The root filesystem that the installer formats in place of the one that the base family settles.

```kdl
builds default="workstation" {
    workstation "./workstation" {
        install {
            filesystem "btrfs"
        }
    }
}
```

Accepts: `xfs` or `ext4` or `btrfs`

<a id="builds-name-install-composefs"></a>

##### `composefs` (optional)

Whether the install seals the deployment, which replaces the answer of the base family.

```kdl
builds default="workstation" {
    workstation "./workstation" {
        install {
            composefs #true
        }
    }
}
```

Accepts: *boolean*

<a id="builds-name-install-generic-image"></a>

##### `generic-image` (optional)

Whether the install skips the bootupd check, which replaces the answer of the base family.

```kdl
builds default="workstation" {
    workstation "./workstation" {
        install {
            generic-image #false
        }
    }
}
```

Accepts: *boolean*

<a id="builds-name-install-admin-group"></a>

##### `admin-group` (optional)

The group that the installer creates an administrator in, which replaces the group of the base family.

```kdl
builds default="workstation" {
    workstation "./workstation" {
        install {
            admin-group "wheel"
        }
    }
}
```

Accepts: *string*

<a id="builds-name-install-bootloader"></a>

##### `bootloader` (optional)

No family answers this row, so the block declares a bootloader unless a UKI seal settles it as `systemd`.

```kdl
builds default="workstation" {
    workstation "./workstation" {
        install {
            bootloader "grub2"
        }
    }
}
```

Accepts: `grub2` or `systemd`

<a id="backend"></a>

## `backend` (optional)

The value selects the one tool that `buildc build` drives, and `buildah` applies when the node is absent. `buildx` needs a default build to name the layer cache, and it cannot build a build that seals a UKI.

```kdl
backend "buildx"
```

Accepts: `buildah` or `buildx`

| Value | Effect |
| --- | --- |
| `buildah` | `buildc build` runs `podman build`. A build that seals a UKI needs it. |
| `buildx` | `buildc build` runs `docker buildx build`. The generated `image-build.yml` loads its OCI archive into the podman store that the later steps read, and it can use a registry layer cache. |

> [!NOTE]
> `--backend` or `$BUILD_BACKEND` overrides it for one build. The flag wins over the variable, and the variable wins over this node.
>
> `buildc check` refuses `buildx` for a build that seals a UKI. A sealed UKI carries the composefs digest, and `buildc build` reads that digest from the local buildah store.

<a id="seed"></a>

## `seed` (optional)

The image must be declared in the repository, and `collection=` must name a library that `libraries` declares. Every component in the seed is fetched through that collection, so an inline definition leaves the seed unbuildable.

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
> The seed names each component through the collection it is fetched from, so `collection=` names a library in `libraries`. A repository is seedable only if it publishes its own `components/` as a collection.
>
> If the seed image lists a component that no collection can import, then `buildc` reports it, because a new repository could not build that seed.

<a id="github"></a>

## `github` (optional)

`buildc generate` writes the workflows this block lists into `.github/workflows/`.

```kdl
github
```

Accepts: optionally {&nbsp;[fields](#github-fields)&nbsp;}

<a id="github-fields"></a>

| Field | Accepts | Description |
| --- | --- | --- |
| [`workflows`](#github-workflows) (optional) | {&nbsp;[fields](#github-workflows-fields)&nbsp;} | The CI workflows that `buildc generate` writes into `.github/workflows/`. |

> [!NOTE]
> `image-build`, `smoke-test` and `build-disk` run once for each build they select. `cleanup-registry`, `update-components`, `kernel-freshness` and `base-sig-probe` run once for the repository, and `buildc check` refuses `builds` on them.
>
> The shipped schedules run at whole hours in UTC, in the order the work depends on: `update-components` at 02:00, `base-sig-probe` and `kernel-freshness` at 03:00, `image-build` at 04:00, `smoke-test` at 06:00 and `cleanup-registry` at 08:00.
>
> `buildc generate` writes only the workflows named here. After a tool upgrade, it rewrites each named workflow.
>
> `buildc set workflows` edits this block.

<a id="github-workflows"></a>

### `workflows` (optional)

A workflow that is not listed does not run. A workflow with no trigger runs on its shipped triggers, and a listed trigger replaces them. Every workflow also runs by hand.

```kdl
github {
    workflows {
        image-build
    }
}
```

Accepts: {&nbsp;[fields](#github-workflows-fields)&nbsp;}

<a id="github-workflows-fields"></a>

| Field | Accepts | Description |
| --- | --- | --- |
| [`<name>`](#github-workflows-name) (optional,&nbsp;repeatable) | optionally any of `push` or `pr` or `schedule`, then optionally {&nbsp;[fields](#github-workflows-name-fields)&nbsp;} | One workflow, named by the stem of its file. |

<a id="github-workflows-name"></a>

#### `<name>` (optional, repeatable)

One workflow, named by the stem of its file.

```kdl
github {
    workflows {
        image-build
    }
}
```

Accepts: optionally any of `push` or `pr` or `schedule`, then optionally {&nbsp;[fields](#github-workflows-name-fields)&nbsp;}

<a id="github-workflows-name-fields"></a>

| Field | Accepts | Description |
| --- | --- | --- |
| [`push`](#github-workflows-name-push) (optional) | optionally {&nbsp;[fields](#github-workflows-name-push-fields)&nbsp;} | Runs the workflow on a push to `main`. |
| [`pr`](#github-workflows-name-pr) (optional) | optionally {&nbsp;[fields](#github-workflows-name-pr-fields)&nbsp;} | Runs the workflow on a pull request to `main`. A pull request build never publishes. |
| [`schedule`](#github-workflows-name-schedule) (optional) | optionally {&nbsp;[fields](#github-workflows-name-schedule-fields)&nbsp;} | Runs the workflow on a schedule. |
| [`tag`](#github-workflows-name-tag) (optional) | *string*, then optionally {&nbsp;[fields](#github-workflows-name-tag-fields)&nbsp;} | Runs the workflow on a pushed Git tag that matches the pattern. A tag build publishes under the tag. |
| [`builds`](#github-workflows-name-builds) (optional) | optionally a *list of strings*, then optionally {&nbsp;[fields](#github-workflows-name-builds-fields)&nbsp;} | The builds a workflow or a trigger runs for. |

<a id="github-workflows-name-push"></a>

##### `push` (optional)

Runs the workflow on a push to `main`.

```kdl
github {
    workflows {
        image-build {
            push
        }
    }
}
```

Accepts: optionally {&nbsp;[fields](#github-workflows-name-push-fields)&nbsp;}

<a id="github-workflows-name-push-fields"></a>

| Field | Accepts | Description |
| --- | --- | --- |
| [`builds`](#github-workflows-name-builds) (optional) | optionally a *list of strings*, then optionally {&nbsp;[fields](#github-workflows-name-builds-fields)&nbsp;} | The builds a workflow or a trigger runs for. |

<a id="github-workflows-name-pr"></a>

##### `pr` (optional)

A bare `pr` builds the default build, and `builds` names others.

```kdl
github {
    workflows {
        image-build {
            pr
        }
    }
}
```

Accepts: optionally {&nbsp;[fields](#github-workflows-name-pr-fields)&nbsp;}

<a id="github-workflows-name-pr-fields"></a>

| Field | Accepts | Description |
| --- | --- | --- |
| [`builds`](#github-workflows-name-builds) (optional) | optionally a *list of strings*, then optionally {&nbsp;[fields](#github-workflows-name-builds-fields)&nbsp;} | The builds a workflow or a trigger runs for. |

<a id="github-workflows-name-schedule"></a>

##### `schedule` (optional)

A schedule without `frequency` or `time` takes the workflow's shipped one.

```kdl
github {
    workflows {
        image-build {
            schedule frequency="daily" time="12:30"
        }
    }
}
```

Accepts: optionally {&nbsp;[fields](#github-workflows-name-schedule-fields)&nbsp;}

| Property | Accepts | Description |
| --- | --- | --- |
| `frequency=` (optional) | `daily` or `weekly` | How often the schedule runs. `weekly` runs on Monday. |
| `time=` (optional) | *string* | The hour and minute in UTC that the schedule runs at. |

<a id="github-workflows-name-schedule-fields"></a>

| Field | Accepts | Description |
| --- | --- | --- |
| [`builds`](#github-workflows-name-builds) (optional) | optionally a *list of strings*, then optionally {&nbsp;[fields](#github-workflows-name-builds-fields)&nbsp;} | The builds a workflow or a trigger runs for. |

<a id="github-workflows-name-tag"></a>

##### `tag` (optional)

Runs the workflow on a pushed Git tag that matches the pattern. A tag build publishes under the tag.

```kdl
github {
    workflows {
        image-build {
            tag "v*"
        }
    }
}
```

Accepts: *string*, then optionally {&nbsp;[fields](#github-workflows-name-tag-fields)&nbsp;}

<a id="github-workflows-name-tag-fields"></a>

| Field | Accepts | Description |
| --- | --- | --- |
| [`builds`](#github-workflows-name-builds) (optional) | optionally a *list of strings*, then optionally {&nbsp;[fields](#github-workflows-name-builds-fields)&nbsp;} | The builds a workflow or a trigger runs for. |

<a id="github-workflows-name-builds"></a>

##### `builds` (optional)

If `builds` is absent, then a workflow runs for every build. Named builds replace the builds that the enclosing workflow runs for, and `remove` drops a build from them.

```kdl
github {
    workflows {
        image-build {
            builds "workstation"
        }
    }
}
```

Accepts: optionally a *list of strings*, then optionally {&nbsp;[fields](#github-workflows-name-builds-fields)&nbsp;}

<a id="github-workflows-name-builds-fields"></a>

| Field | Accepts | Description |
| --- | --- | --- |
| [`remove`](#github-workflows-name-builds-remove) (optional,&nbsp;repeatable) | *list of strings* | Builds that leave the selection. |

<a id="github-workflows-name-builds-remove"></a>

###### `remove` (optional, repeatable)

Builds that leave the selection.

```kdl
github {
    workflows {
        image-build {
            builds "workstation" {
                remove "workstation-dev"
            }
        }
    }
}
```

Accepts: *list of strings*

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

<a id="libraries"></a>

## `libraries` (optional)

`local` holds the libraries that the user edits in place, and `remote` holds the libraries that buildc reads at a commit. Each child of a block is one library, named by its node. Declaring a library makes its components available, and a build uses one only when the repository names it. If no `local` block is declared, then the repository root is the local library. If a `local` block is declared, then buildc reads the repository's own components only through a local library whose source is `.`.

```kdl
libraries default="stable" {
    local
}
```

Accepts: {&nbsp;[fields](#libraries-fields)&nbsp;}

| Property | Accepts | Description |
| --- | --- | --- |
| `default=` (optional) | *string* | The library that a path from a library root reads when several libraries hold it. The name must name a declared library. If `default=` is absent, then the only local library is the default. |

<a id="libraries-fields"></a>

| Field | Accepts | Description |
| --- | --- | --- |
| [`local`](#libraries-local) (optional) | optionally {&nbsp;[fields](#libraries-local-fields)&nbsp;} | The libraries that the user edits in place. |
| [`remote`](#libraries-remote) (optional) | optionally {&nbsp;[fields](#libraries-remote-fields)&nbsp;} | The libraries that buildc reads at a commit. |

> [!NOTE]
> `buildc` never reads the `libraries` block of a library, so the repository declares every library its builds use.

<a id="libraries-local"></a>

### `local` (optional)

`buildc` reads a local library from its folder as it stands, so a local library takes no pin and no lock.

```kdl
libraries default="stable" {
    local
}
```

Accepts: optionally {&nbsp;[fields](#libraries-local-fields)&nbsp;}

<a id="libraries-local-fields"></a>

| Field | Accepts | Description |
| --- | --- | --- |
| [`<name>`](#libraries-local-name) (optional,&nbsp;repeatable) | optionally {&nbsp;[fields](#libraries-local-name-fields)&nbsp;} | One local library, named by its node. |

<a id="libraries-local-name"></a>

#### `<name>` (optional, repeatable)

The node name is the library's local name, and `library=` on a tree node names the library by its value. A source with a `repo.kdl` keeps its components where its `library` node says, else in `components/`. A source without a `repo.kdl` is a folder of components.

```kdl
libraries default="stable" {
    local {
        mine
    }
}
```

Accepts: optionally {&nbsp;[fields](#libraries-local-name-fields)&nbsp;}

<a id="libraries-local-name-fields"></a>

| Field | Accepts | Description |
| --- | --- | --- |
| [`source`](#libraries-local-name-source) (optional) | *string* | Where the library lives, as a path from the repository root. |

<a id="libraries-local-name-source"></a>

##### `source` (optional)

The user edits a local library in place. If `source` is absent, then the library is the folder `./<name>`. A source of `.` is this repository itself.

```kdl
libraries default="stable" {
    local {
        mine {
            source "."
        }
    }
}
```

Accepts: *string*

<a id="libraries-remote"></a>

### `remote` (optional)

`buildc` reads a remote library from its cache at a commit. `repo.lock.kdl` records the commit and the content hash of each component that a build reads.

```kdl
libraries default="stable" {
    remote
}
```

Accepts: optionally {&nbsp;[fields](#libraries-remote-fields)&nbsp;}

<a id="libraries-remote-fields"></a>

| Field | Accepts | Description |
| --- | --- | --- |
| [`<name>`](#libraries-remote-name) (optional,&nbsp;repeatable) | {&nbsp;[fields](#libraries-remote-name-fields)&nbsp;} | One remote library, named by its node. |

<a id="libraries-remote-name"></a>

#### `<name>` (optional, repeatable)

The node name is the library's local name, and `library=` on a tree node names the library by its value. A source with a `repo.kdl` keeps its components where its `library` node says, else in `components/`. A source without a `repo.kdl` is a folder of components.

```kdl
libraries default="stable" {
    remote {
        stable {
            source "https://github.com/buildc-org/library"
        }
    }
}
```

Accepts: {&nbsp;[fields](#libraries-remote-name-fields)&nbsp;}

<a id="libraries-remote-name-fields"></a>

| Field | Accepts | Description |
| --- | --- | --- |
| [`source`](#libraries-remote-name-source) (required) | *string* | Where the library lives, as an HTTPS Git URL or a path to a Git checkout. |
| [`version`](#libraries-remote-name-version) (optional) | *string* | The branch, tag or commit that a remote library is read at. |

<a id="libraries-remote-name-source"></a>

##### `source` (required)

`buildc` reads a remote library at a commit from its cache. A path names a Git checkout on this machine, and `buildc` reads its commits as it reads a URL.

```kdl
libraries default="stable" {
    remote {
        stable {
            source "https://github.com/buildc-org/library"
        }
    }
}
```

Accepts: *string*

<a id="libraries-remote-name-version"></a>

##### `version` (optional)

If `version` is absent, then the library follows the default branch of the remote.

```kdl
libraries default="stable" {
    remote {
        stable {
            version "v1"
        }
    }
}
```

Accepts: *string*

<a id="library"></a>

## `library` (optional)

A repository that another repository declares as a library offers the components in this folder. The repository's own builds read them from there too.

```kdl
library {
    components
}
```

Accepts: {&nbsp;[fields](#library-fields)&nbsp;}

<a id="library-fields"></a>

| Field | Accepts | Description |
| --- | --- | --- |
| [`components`](#library-components) (optional) | optionally a *string* | The folder that holds the components, relative to the repository root. |

<a id="library-components"></a>

### `components` (optional)

If `components` is absent, then the components live in `components/`. With no argument, or with a path that resolves to `.`, the block names the repository root, so every component directory sits at the top level. The path is relative to the repository root; `.` and `..` segments are resolved, and a path that leaves the repository is refused.

```kdl
library {
    components
}
```

Accepts: optionally a *string*

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
> `buildc` refuses a remote component whose cached content no longer matches its entry in `repo.lock.kdl`. This refusal does not depend on `enforce`.

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
| `strict` | A script step reaches the network only if its component declares `permissions { scripts { network #true } }`. |
| `deny` | Every script step runs under `--network=none`. |

> [!NOTE]
> If the rule is `strict` or `deny`, then `buildc check` names each component that ships a Containerfile fragment. The rule does not reach the `RUN` lines of a fragment.
