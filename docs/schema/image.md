<a id="image.kdl"></a>

# `image.kdl`

Location: The root of the repository, as `image.kdl` or `<name>.image.kdl`.

A file at the repository root holds one or more `image` blocks, and `buildc` reads no other root `.kdl` file. It must declare `schema-version`, and it holds no node besides `schema-version` and `image`.

```kdl
schema-version 1
image "workstation" {
    layout {
        bootloader "grub2"
    }
    desktop-environment
}
```

| Field | Accepts | Description |
| --- | --- | --- |
| [`schema-version`](#schema-version) (required) | *number* | The schema release that this file is written against. |
| [`image`](#image) (required,&nbsp;repeatable) | *string*, then optionally {&nbsp;[fields](#image-fields)&nbsp;} | One image, named by its id, which declares every component that it is made of, its base among them. |

> [!NOTE]
> A root `.kdl` file is an image file only if it is named `image.kdl` or ends in `.image.kdl`. `buildc` reports each other root `.kdl` file and does not read it.
>
> The part of the file name in front of `.image.kdl` does not name the image. An image is called what it declares, so one file can hold as many images as suit the repository. `plan.json` records the file name only to say where each image is declared.

<a id="schema-version"></a>

## `schema-version` (required)

The number states the release the file is written against. `buildc` reports a file whose number differs from the one `repo.kdl` declares, and it reports a file that carries none.

```kdl
schema-version 1
```

Accepts: *number*

> [!NOTE]
> A file cannot declare a schema newer than `repo.kdl`.
>
> `buildc create repo`, `buildc create image` and `buildc create component` write the version, and `buildc` never changes it.

<a id="image"></a>

## `image` (required, repeatable)

The block is the whole declaration of one published image. `buildc` reports two images with one id, and a flavour name that publishes as another image's id. The `os-release` component sets the name that the boot menu and the desktop read. Its other options set the pretty name, hostname and URLs. An image that does not build `os-release` keeps the identity that its base image ships.

```kdl
image "workstation"
```

Accepts: *string*, then optionally {&nbsp;[fields](#image-fields)&nbsp;}

The id names:

- the published image;
- the build target;
- the cache tag.

<a id="image-fields"></a>

| Field | Accepts | Description |
| --- | --- | --- |
| [`description`](#image-description) (optional) | *string* | OCI label `org.opencontainers.image.description`. One line that summarises the image. |
| [`keywords`](#image-keywords) (optional,&nbsp;repeatable) | *list of strings* | OCI label `io.artifacthub.package.keywords`. The keywords of the image, which the build joins with commas. |
| [`logo-url`](#image-logo-url) (optional) | *string* | OCI label `io.artifacthub.package.logo-url`. The URL that points at the logo of the image. |
| [`conforms`](#image-conforms) (optional) | *string* | The benchmark profile that a scan measures the ungated target against. |
| [`boot-chain`](#image-boot-chain) (optional) | `uki-shim` or `uki-db` | The UKI boot chain, which is owner-signed through shim or enrolled directly into Secure Boot. |
| [`layout`](#image-layout) (optional) | {&nbsp;[fields](#image-layout-fields)&nbsp;} | What the installer lays down if the answer of the base family is not the one that the image wants. |
| [`allow-remediation`](#image-allow-remediation) (optional,&nbsp;unique) | *string* | One rule an installed component refuses that this image lets remediation set anyway. |
| [`flavours`](#image-flavours) (optional) | {&nbsp;[fields](#image-flavours-fields)&nbsp;} | The flavours that this image publishes beside its ungated build. |
| [`flavour`](#image-flavour) (optional,&nbsp;repeatable) | *string* | The components one flavour adds, which build only for that flavour. |
| [`<name>`](#image-name) (optional,&nbsp;repeatable) |  | One part of a component path, which lists the component at that path or holds further parts. |

> [!NOTE]
> An image that builds `os-release` takes the id as its default hostname. The image can set the component's `default-hostname` option to another value.

<a id="image-description"></a>

### `description` (optional)

OCI label `org.opencontainers.image.description`. One line that summarises the image.

```kdl
image "workstation" {
    description "A KDE desktop for daily work"
}
```

Accepts: *string*

<a id="image-keywords"></a>

### `keywords` (optional, repeatable)

OCI label `io.artifacthub.package.keywords`. The keywords of the image, which the build joins with commas.

```kdl
image "workstation" {
    keywords "desktop" "kde"
}
```

Accepts: *list of strings*

<a id="image-logo-url"></a>

### `logo-url` (optional)

OCI label `io.artifacthub.package.logo-url`. The URL that points at the logo of the image.

```kdl
image "workstation" {
    logo-url "https://github.com/owner/workstation/raw/main/logo.svg"
}
```

Accepts: *string*

<a id="image-conforms"></a>

### `conforms` (optional)

The name selects a profile in the SSG datastream that the base pins, and `buildc scap` measures the target against its rules. `buildc scap` refuses a name the datastream does not carry.

```kdl
image "workstation" {
    conforms "standard"
}
```

Accepts: *string*

> [!NOTE]
> A scan reports the result. A failed rule fails `buildc scap` only if `audit { enforce #true }` is set.

<a id="image-boot-chain"></a>

### `boot-chain` (optional)

A declared chain builds a UKI and settles the install layout, so `buildc check` refuses a `layout` row that contradicts it. A `buildx` backend cannot build the image, because `buildc build` reads the sealed composefs digest from the local buildah store.

```kdl
image "workstation" {
    boot-chain "uki-shim"
}
```

Accepts: `uki-shim` or `uki-db`

<a id="image-layout"></a>

### `layout` (optional)

A layout answers the install questions that the base family leaves open. If the family and a `boot-chain` both leave an answer open, then the image builds no install media unless the block gives it.

```kdl
image "workstation" {
    layout {
        filesystem "btrfs"
    }
}
```

Accepts: {&nbsp;[fields](#image-layout-fields)&nbsp;}

<a id="image-layout-fields"></a>

| Field | Accepts | Description |
| --- | --- | --- |
| [`filesystem`](#image-layout-filesystem) (optional) | `xfs` or `ext4` or `btrfs` | The root filesystem that the installer formats in place of the one that the base family settles. |
| [`composefs`](#image-layout-composefs) (optional) | *boolean* | Whether the install seals the deployment, which replaces the answer of the base family. |
| [`generic-image`](#image-layout-generic-image) (optional) | *boolean* | Whether the install skips the bootupd check, which replaces the answer of the base family. |
| [`admin-group`](#image-layout-admin-group) (optional) | *string* | The group that the installer creates an administrator in, which replaces the group of the base family. |
| [`bootloader`](#image-layout-bootloader) (optional) | `grub2` or `systemd` | The bootloader that the installer installs. The catalog entry of the base lists the bootloaders that the base carries. |

<a id="image-layout-filesystem"></a>

#### `filesystem` (optional)

The root filesystem that the installer formats in place of the one that the base family settles.

```kdl
image "workstation" {
    layout {
        filesystem "btrfs"
    }
}
```

Accepts: `xfs` or `ext4` or `btrfs`

<a id="image-layout-composefs"></a>

#### `composefs` (optional)

Whether the install seals the deployment, which replaces the answer of the base family.

```kdl
image "workstation" {
    layout {
        composefs #true
    }
}
```

Accepts: *boolean*

<a id="image-layout-generic-image"></a>

#### `generic-image` (optional)

Whether the install skips the bootupd check, which replaces the answer of the base family.

```kdl
image "workstation" {
    layout {
        generic-image #false
    }
}
```

Accepts: *boolean*

<a id="image-layout-admin-group"></a>

#### `admin-group` (optional)

The group that the installer creates an administrator in, which replaces the group of the base family.

```kdl
image "workstation" {
    layout {
        admin-group "wheel"
    }
}
```

Accepts: *string*

<a id="image-layout-bootloader"></a>

#### `bootloader` (optional)

No family answers this row, so a layout declares a bootloader unless a `boot-chain` settles it as `systemd`.

```kdl
image "workstation" {
    layout {
        bootloader "grub2"
    }
}
```

Accepts: `grub2` or `systemd`

<a id="image-allow-remediation"></a>

### `allow-remediation` (optional, unique)

The rule must be one that a component the image installs refuses, or the allowance lifts nothing and `buildc scap` reports it.

```kdl
image "workstation" {
    allow-remediation "grub2_nousb_argument" because="this machine has no USB keyboard"
}
```

Accepts: *string*

| Property | Accepts | Description |
| --- | --- | --- |
| `because=` (optional) | *string* | Why this image overrides the component's judgement. |

<a id="image-flavours"></a>

### `flavours` (optional)

The block publishes each child as `<id>-<flavour>`, and it reserves `none` for the ungated build. One flavour at most carries `default=#true`, and one flavour at most carries `pr-build=#true`.

```kdl
image "workstation" {
    flavours {
        dev
    }
}
```

Accepts: {&nbsp;[fields](#image-flavours-fields)&nbsp;}

<a id="image-flavours-fields"></a>

| Field | Accepts | Description |
| --- | --- | --- |
| [`<name>`](#image-flavours-name) (optional,&nbsp;repeatable) |  | One flavour that publishes a gated component set as `<id>-<flavour>`, where `<flavour>` is the node name. |

> [!NOTE]
> An image builds one target for its ungated component set and one more for each flavour. The targets publish as `<id>` and `<id>-<flavour>`.
>
> A flavour gates components inside one image. A component listed inside `flavour "dev"` builds only for that target, and each other component builds for every target.
>
> The `conforms=` of a flavour measures that flavour. The `conforms` of the image measures the ungated target only.

<a id="image-flavours-name"></a>

#### `<name>` (optional, repeatable)

One flavour that publishes a gated component set as `<id>-<flavour>`, where `<flavour>` is the node name.

```kdl
image "workstation" {
    flavours {
        dev
    }
}
```

| Property | Accepts | Description |
| --- | --- | --- |
| `default=` (optional) | *boolean* | Whether a build that names no flavour builds this one. |
| `pr-build=` (optional) | *boolean* | Whether a pull request builds this flavour in place of the default. |
| `conforms=` (optional) | *string* | The benchmark profile that a scan measures this flavour against. |

<a id="image-flavour"></a>

### `flavour` (optional, repeatable)

The argument names one flavour of the `flavours` block, and every component below the node is gated to that target. `buildc check` refuses a name that no flavour declares.

```kdl
image "workstation" {
    flavour "dev"
}
```

Accepts: *string*

<a id="image-name"></a>

### `<name>` (optional, repeatable)

A segment takes no argument. Its name takes lowercase letters, digits and dashes, with one `@<tag>` allowed on the last part. `variant=` is its only property, and every other setting is a child node. A child with an argument sets an option of the component. The option value may read `{image-id}` and `{base-url}`.

```kdl
image "workstation" {
    desktop-environment
}
```

> [!NOTE]
> A component that the image file defines inline must not also be a component under `components/`. `buildc check` refuses the pair and names both.
>
> `buildc import component` copies a component from a library into `components/` and records where it came from in `components/components.lock.kdl`.
>
> The base is the listed component that declares `base`. Its segment also takes `digest` and `signed`, which record the digest and the signature state of its reference.
