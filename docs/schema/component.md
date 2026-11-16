<a id="manifest.kdl"></a>

# `manifest.kdl`

Location: `components/<component-name>/manifest.kdl`, in the directory of the component.

The directory under `components/` is the component's name, and the manifest sits at `manifest.kdl` in it. An image that defines a component inline names it with the path that the image lists.

```kdl
schema-version 1
description "Traditional CLI utilities"
targets "fedora" "debian"
```

| Field | Accepts | Description |
| --- | --- | --- |
| [`schema-version`](#schema-version) (required) | *number* | The schema release that this file is written against. |
| [`description`](#description) (optional) | *string* | One line that names the component in the resolved build summary. |
| [`targets`](#targets) (optional,&nbsp;repeatable) | *list of strings* | The base families that this component builds on, which the image's `family` must match. Omit it to target every family. |
| [`provides`](#provides) (optional,&nbsp;repeatable) | *list of strings*, or {&nbsp;[one name per line](#provides-fields)&nbsp;} | A capability that this component provides for the components that require it. |
| [`requires`](#requires) (optional,&nbsp;repeatable) | *list of strings*, or {&nbsp;[one name per line](#requires-fields)&nbsp;} | A capability that another component must provide. It also orders the build. |
| [`imports`](#imports) (optional,&nbsp;repeatable) | *list of strings*, or {&nbsp;[one name per line](#imports-fields)&nbsp;} | A component that builds in every image that holds this component. |
| [`after`](#after) (optional,&nbsp;repeatable) | *list of strings*, or {&nbsp;[one name per line](#after-fields)&nbsp;} | A capability that this component builds after. The component does not require it. |
| [`removes`](#removes) (optional,&nbsp;repeatable) | *list of strings*, or {&nbsp;[one name per line](#removes-fields)&nbsp;} | A capability that this component withdraws from the image. |
| [`overrides`](#overrides) (optional,&nbsp;repeatable) | *list of strings* | An absolute path that this component replaces on purpose. |
| [`mode`](#mode) (optional,&nbsp;unique) | *path*, then *octal mode* | The octal file mode that one path in the overlay of this component takes. |
| [`key`](#key) (optional,&nbsp;unique) | *string*, then {&nbsp;[fields](#key-fields)&nbsp;} | A key that `buildc create key` generates for this component. |
| [`secret`](#secret) (optional,&nbsp;repeatable) | *list of strings* | A build secret that the layer of this component mounts. |
| [`arg`](#arg) (optional,&nbsp;repeatable) | *list of strings* | A build argument that the layer of this component reads. |
| [`helpers`](#helpers) (optional,&nbsp;repeatable) | *list of strings* | Files from this component that every component layer mounts by basename into `/ctx/lib`. |
| [`allow-verify`](#allow-verify) (optional,&nbsp;repeatable) | *string* | One `buildc validate-image` diagnostic that the check accepts on one unit, while the check still covers every other diagnostic and unit of the image. |
| [`refuses`](#refuses) (optional,&nbsp;unique) | *string* | One benchmark rule that this component leaves unsatisfied on purpose. |
| [`collects`](#collects) (optional,&nbsp;repeatable) | *string* | A filename that this component gathers from every component that ships one. A collector claims one filename across the image. |
| [`contributes`](#contributes) (optional,&nbsp;repeatable) | *string* | A file that this component ships for another component to collect. |
| [`fragment`](#fragment) (optional) |  | Where the component's `Containerfile.inc` goes in relation to the generated layer. |
| [`option`](#option) (optional,&nbsp;unique) | *string*, then {&nbsp;[fields](#option-fields)&nbsp;} | One value that an image can set on this component. |
| [`variant`](#variant) (optional,&nbsp;unique) | *string*, then optionally {&nbsp;[fields](#variant-fields)&nbsp;} | A named set of option values that an image selects with `variant=`. |
| [`asset`](#asset) (optional,&nbsp;unique) | *string*, then {&nbsp;[fields](#asset-fields)&nbsp;} | A pinned upstream payload that the component fetches during its own layer. |
| [`network`](#network) (optional) | `scripts` | The declaration that the script step of this component reaches the network. |
| [`packages`](#packages) (optional,&nbsp;repeatable) | *list of strings*, or {&nbsp;[one name per line](#packages-fields)&nbsp;} | The packages that this component installs. |
| [`package-groups`](#package-groups) (optional,&nbsp;repeatable) | *list of strings*, or {&nbsp;[one name per line](#package-groups-fields)&nbsp;} | The package groups that this component installs. Package groups work with dnf only, so an ungated `package-groups` needs a component that targets only `fedora` or `rhel`. |
| [`copr`](#copr) (optional,&nbsp;unique) | *string* | The `owner/project` name of a COPR repository that this component enables for its own installs. `copr` works on Fedora only. |
| [`satisfies`](#satisfies) (optional) | optionally {&nbsp;[fields](#satisfies-fields)&nbsp;} | An audit declaration of the benchmarks and rules that this component claims to harden. `buildc` records the claim and certifies nothing, and the scan checks it after the build. |
| [`on`](#on) (optional,&nbsp;repeatable) | *list of strings*, then optionally {&nbsp;[fields](#on-fields)&nbsp;} | The declarations that the build takes only on the named base families. |
| [`base`](#base) (optional) | *string*, then optionally {&nbsp;[fields](#base-fields)&nbsp;} | The image reference that an image on this base builds from. It makes the component a base. |

> [!NOTE]
> `requires` and `after` decide the build order. The order of the image list only breaks ties.
>
> If nothing provides a `requires`, then `buildc check` fails and names every component that would satisfy it.

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

<a id="description"></a>

## `description` (optional)

One line that names the component in the resolved build summary.

```kdl
description "Traditional CLI utilities"
```

Accepts: *string*

<a id="targets"></a>

## `targets` (optional, repeatable)

Each name must be `fedora`, `rhel`, `debian` or `ubuntu`. An image whose base family this node omits reports the component as unsupported.

```kdl
targets "fedora" "debian"
```

Accepts: *list of strings*

<a id="provides"></a>

## `provides` (optional, repeatable)

A name is one or more lower-case segments joined by `/`. Each segment starts with a lowercase letter. `base-image` is provided by a `base` node. A manifest cannot declare it.

```kdl
provides "flatpak"
```

Accepts: *list of strings*, or {&nbsp;[one name per line](#provides-fields)&nbsp;}

| Property | Accepts | Description |
| --- | --- | --- |
| `witness=` (optional) | *string* | The absolute path in the finished image that witnesses the capability. Without it, nothing witnesses the capability. The build checks the path at the end of the component's layer. If the component ships a finalize hook for the base family, then the build checks the path after the finalize hooks. |
| `build-only=` (optional) | *boolean* | Whether the `witness` exists only while the build runs. |
| `exclusive=` (optional) | *boolean* | Whether a second enabled provider of the capability conflicts with this one. |
| `version=` (optional) | *string* | The version that the base probe or the build measured for this capability. |
| `package=` (optional) | *string* | The package whose version measures this capability. |

<a id="provides-fields"></a>

| Field | Accepts | Description |
| --- | --- | --- |
| [`<name>`](#provides-name) (optional,&nbsp;repeatable) |  | One capability in a `provides` block, which the node name names. |

> [!NOTE]
> The build checks the finished image for the witness file.
>
> A `provides` without `witness`, such as `rechunking`, has no witness. `buildc` holds no default path for a name.
>
> A `version` is the measured result for this capability, not a value to write by hand. A `provides` that carries a `version` must also name the `package` it was measured from. A `version` without `package` is refused.
>
> `buildc validate-image` checks the finished image for the witness of each name that the base or a component provides with a `witness`. `base-sig-probe` reads those witnesses when it measures a base, except the ones of a component that meets a base requirement or that the base imports.
>
> The witness of `luks-initramfs` is inside an archive. If a target declares it, then the build validates the initramfs with `lsinitrd`. If no target declares it, then the installer does not offer root encryption.
>
> Declare `luks-initramfs` in a custom base or component only if its initramfs unlocks LUKS. `buildc build` then proves that the binary is present before the installer offers it.
>
> Many enabled components may provide one capability. If one of them declares it `exclusive=#true`, then `buildc check` refuses a second provider that builds into the same image or flavour. `buildc import component` asks whether to replace the first. Providers in sibling flavours never build together.
>
> A capability name is lower-case letters, digits and dashes, and starts with a letter. The generated graph writes the name unquoted into a mermaid label and a markdown table cell.

<a id="provides-name"></a>

### `<name>` (optional, repeatable)

One capability in a `provides` block, which the node name names.

```kdl
provides {
    "ssh" witness="/usr/sbin/sshd"
}
```

| Property | Accepts | Description |
| --- | --- | --- |
| `witness=` (optional) | *string* | The absolute path in the finished image that witnesses the capability. Without it, nothing witnesses the capability. The build checks the path at the end of the component's layer. If the component ships a finalize hook for the base family, then the build checks the path after the finalize hooks. |
| `build-only=` (optional) | *boolean* | Whether the `witness` exists only while the build runs. |
| `exclusive=` (optional) | *boolean* | Whether a second enabled provider of the capability conflicts with this one. |
| `version=` (optional) | *string* | The version that the base probe or the build measured for this capability. |
| `package=` (optional) | *string* | The package whose version measures this capability. |

<a id="requires"></a>

## `requires` (optional, repeatable)

A name is one or more lower-case segments joined by `/`. Each segment starts with a lowercase letter. `buildc check` fails when nothing in the image provides the name.

```kdl
requires "libostree" min="2025.3"
```

Accepts: *list of strings*, or {&nbsp;[one name per line](#requires-fields)&nbsp;}

| Property | Accepts | Description |
| --- | --- | --- |
| `min=` (optional) | *string* | The lowest version of the capability that satisfies this requirement. |

<a id="requires-fields"></a>

| Field | Accepts | Description |
| --- | --- | --- |
| [`<name>`](#requires-name) (optional,&nbsp;repeatable) |  | One capability in a `requires` block, which the node name names. |

> [!NOTE]
> A `min` is opt-in. A requirement without one is satisfied by any provider. A versioned requirement needs a provider that names the `package` whose version measures the capability.
>
> A pinned base is checked before a build. It is refused when its recorded version is below the minimum and when it records no version. A moving base is measured in the build before the first component layer runs. The build stops when the measured version is below the minimum.
>
> A component's own version is measured after its layer. `buildc validate-image` checks the floor on it.

<a id="requires-name"></a>

### `<name>` (optional, repeatable)

One capability in a `requires` block, which the node name names.

```kdl
requires {
    "libostree" min="2025.3"
}
```

| Property | Accepts | Description |
| --- | --- | --- |
| `min=` (optional) | *string* | The lowest version of the capability that satisfies this requirement. |

<a id="imports"></a>

## `imports` (optional, repeatable)

The value is a component path of lowercase segments joined by `/`, or a `{option}` reference to a string option that resolves to one. The block of an imported path sets the options that the importing component passes to the imported component.

```kdl
imports "container-runtime"
```

Accepts: *list of strings*, or {&nbsp;[one name per line](#imports-fields)&nbsp;}

<a id="imports-fields"></a>

| Field | Accepts | Description |
| --- | --- | --- |
| [`<name>`](#imports-name) (optional,&nbsp;repeatable) | optionally {&nbsp;[fields](#imports-name-fields)&nbsp;} | One component path in an `imports` block, which the node name names. Its block sets the options of that component. |

<a id="imports-name"></a>

### `<name>` (optional, repeatable)

One component path in an `imports` block, which the node name names. Its block sets the options of that component.

```kdl
imports {
    "container-runtime"
}
```

Accepts: optionally {&nbsp;[fields](#imports-name-fields)&nbsp;}

<a id="imports-name-fields"></a>

| Field | Accepts | Description |
| --- | --- | --- |
| [`<name>`](#imports-name-name) (optional,&nbsp;repeatable) |  | One option value that the imported component takes, which the node name names. |

<a id="imports-name-name"></a>

#### `<name>` (optional, repeatable)

One option value that the imported component takes, which the node name names.

```kdl
imports {
    "container-runtime" {
        storage-driver "{storage-driver}"
    }
}
```

<a id="after"></a>

## `after` (optional, repeatable)

The name orders the build only when something in the image provides it. A name with no provider is allowed and adds no order.

```kdl
after "vfio"
```

Accepts: *list of strings*, or {&nbsp;[one name per line](#after-fields)&nbsp;}

<a id="after-fields"></a>

| Field | Accepts | Description |
| --- | --- | --- |
| [`<name>`](#after-name) (optional,&nbsp;repeatable) |  | One capability in the block, which the node name names. |

<a id="after-name"></a>

### `<name>` (optional, repeatable)

One capability in the block, which the node name names.

```kdl
after {
    "kernel-devel"
}
```

<a id="removes"></a>

## `removes` (optional, repeatable)

A name is one or more lower-case segments joined by `/`. Each segment starts with a lowercase letter. `buildc check` reports a name that the base and the other enabled components do not provide.

```kdl
removes "bootupctl"
```

Accepts: *list of strings*, or {&nbsp;[one name per line](#removes-fields)&nbsp;}

<a id="removes-fields"></a>

| Field | Accepts | Description |
| --- | --- | --- |
| [`<name>`](#after-name) (optional,&nbsp;repeatable) |  | One capability in the block, which the node name names. |

<a id="overrides"></a>

## `overrides` (optional, repeatable)

The path is absolute, and it must name a file that an earlier component ships. `buildc check` reports an override that replaces nothing, so one cannot outlive the collision it was added for.

```kdl
overrides "/etc/containers/policy.json"
```

Accepts: *list of strings*

<a id="mode"></a>

## `mode` (optional, unique)

The path must name a regular file that the component ships under `files/`, and the mode is an octal number up to `07777`. A `fragment` with `standard-layer #false` refuses the mode, because the fragment replaces the layer that would apply it.

```kdl
mode "/etc/audit/rules.d/audit.rules" "0600"
```

Accepts: *path*, then *octal mode*

<a id="key"></a>

## `key` (optional, unique)

A complete block provides the `<kind>-key` capability and ships the public half as a witness. The public file must exist under `keys/public/` at the path that `public` names.

```kdl
key "secureboot" {
    generator "openssl" profile="module-signing"
    public "/usr/share/secureboot/sb_cert.der" format="der"
    private "MOK.priv"
}
```

Accepts: *string*, then {&nbsp;[fields](#key-fields)&nbsp;}

<a id="key-fields"></a>

| Field | Accepts | Description |
| --- | --- | --- |
| [`generator`](#key-generator) (required) | `cosign` or `openssl` or `ssh-keygen` | The generator that writes this key. |
| [`public`](#key-public) (required) | *string* | Where the public half ships, which witnesses the `<kind>-key` capability of this component. |
| [`private`](#key-private) (required) | *string* | The filename that the private half takes under `keys/private/`. |

<a id="key-generator"></a>

### `generator` (required)

`openssl` needs a `profile`, and `bits` is its RSA key size. `cosign` and `ssh-keygen` take no profile.

```kdl
key "secureboot" {
    generator "openssl" profile="module-signing"
}
```

Accepts: `cosign` or `openssl` or `ssh-keygen`

| Property | Accepts | Description |
| --- | --- | --- |
| `profile=` (optional) | `module-signing` or `pcr-signing` or `tls-ca` | The purpose that the generator serves, if the generator has more than one purpose. |
| `bits=` (optional) | *number* from 2048 to 16384 | The RSA key size, which is 4096 by default. |

<a id="key-public"></a>

### `public` (required)

The path is absolute and holds no `..` segment. The public file must exist under `keys/public/`, and a `pcr-signing` key takes PEM alone.

```kdl
key "secureboot" {
    public "/usr/share/secureboot/sb_cert.der" format="der"
}
```

Accepts: *string*

| Property | Accepts | Description |
| --- | --- | --- |
| `format=` (optional) | `pem` or `der` | The format of the public half, which is PEM by default. |

<a id="key-private"></a>

### `private` (required)

The value is a plain filename, so it carries no `/` and does not start with a dot. The scaffolded `.gitignore` keeps `keys/private/` out of the repository.

```kdl
key "secureboot" {
    private "MOK.priv"
}
```

Accepts: *string*

<a id="secret"></a>

## `secret` (optional, repeatable)

Each name is the ID of a BuildKit secret that the component layer mounts at `/run/secrets/<name>`. The value comes from the build command, and a secret the build does not supply is not an error.

```kdl
secret "mok_privkey"
```

Accepts: *list of strings*

<a id="arg"></a>

## `arg` (optional, repeatable)

Each name becomes an environment variable in the component's layer script. The value comes from a build argument, and the manifest does not hold it.

```kdl
arg "KERNEL"
```

Accepts: *list of strings*

<a id="helpers"></a>

## `helpers` (optional, repeatable)

The path is relative to the component directory, and the file must exist there. The basename is the mount name, so two enabled components cannot share a basename.

```kdl
helpers "lib/family.sh"
```

Accepts: *list of strings*

<a id="allow-verify"></a>

## `allow-verify` (optional, repeatable)

The class must be one that `buildc validate-image` reports, and the same class on the same unit is accepted once.

```kdl
allow-verify "man-page-missing" unit="plasmalogin.service"
```

Accepts: *string*

| Property | Accepts | Description |
| --- | --- | --- |
| `unit=` (optional) | *string* | The unit the exception applies to. |

<a id="refuses"></a>

## `refuses` (optional, unique)

The `because` reason states why the rule stays unsatisfied, and a refusal without one is reported. The same rule may be refused once in a component.

```kdl
refuses "grub2_nousb_argument" because="it removes the keyboard"
```

Accepts: *string*

| Property | Accepts | Description |
| --- | --- | --- |
| `because=` (optional) | *string* | Why the component leaves the rule unsatisfied. |

> [!NOTE]
> Remediation does not set a refused rule. If an image declares `allow-remediation` for the rule, then remediation can set it.

<a id="collects"></a>

## `collects` (optional, repeatable)

The filename is claimed once across the image, and `into` must be an absolute path. `buildc check` reports a shipped file that no enabled collector claims.

```kdl
collects "flatpaks.list" into="/usr/share/flatpak-defaults/apps.list" priority=500
```

Accepts: *string*

| Property | Accepts | Description |
| --- | --- | --- |
| `into=` (optional) | *string* | The absolute path that receives the assembled file. |
| `priority=` (optional) | *number* from 0 to 9999 | The position of a contribution that declares no priority. |

> [!NOTE]
> The build stages each contribution as `<into>.d/NNNN-<component>.part`. The finalize phase assembles the parts in that order, so the build order of the contributors does not change the assembled file.
>
> A component ships at most one copy of a collected file. The most specific copy wins. The order is `debian/`, then `deb/`, then the component root.

<a id="contributes"></a>

## `contributes` (optional, repeatable)

A file that this component ships for another component to collect.

```kdl
contributes "flatpaks.list"
```

Accepts: *string*

| Property | Accepts | Description |
| --- | --- | --- |
| `priority=` (optional) | *number* from 0 to 9999 | The position that this file takes in the assembled file. |

<a id="fragment"></a>

## `fragment` (optional)

Shipping `Containerfile.inc` is what adds a fragment, so a component that ships none is refused. A `fragment` with `standard-layer #false` refuses `position`, because the fragment is then the whole layer.

```kdl
fragment position="after"
```

| Property | Accepts | Description |
| --- | --- | --- |
| `position=` (optional) | `before` or `after` or `tail` | Where the fragment goes. `before`, the default, puts it above the generated block. `after` puts it below the block. `tail` puts it below the finalize layer, where the lineage stage has ended and can be named. If the component ships no finalize hook, then a witness of an `after` component is checked after the fragment, and a witness of a `tail` component is checked at the end of its own layer. |
| `standard-layer=` (optional) | *boolean* | Whether the build emits the generated block. |

> [!NOTE]
> The build places the fragment verbatim and replaces `@COMPONENT@` with `/generated/<image>/components/<dir>`, the component's resolved directory in the build context. A `COPY --from=ctx` reaches a file that the component ships through that path.
>
> Every `RUN` bind-mounts over `/etc/hostname`, `/etc/hosts` and `/etc/resolv.conf`. A fragment writes those three files with a `COPY --from=ctx`.

<a id="option"></a>

## `option` (optional, unique)

The type decides the value the layer reads, and `buildc check` refuses a default that does not match it. A string value may hold none of `"`, `\`, `$`, a backtick or a newline, and a list value may hold no whitespace, because values reach the layer through a `RUN` env prefix.

```kdl
option "version" type="string" {
    default "128.0"
}
```

Accepts: *string*, then {&nbsp;[fields](#option-fields)&nbsp;}

| Property | Accepts | Description |
| --- | --- | --- |
| `type=` (required) | `string` or `bool` or `list` | The type of the value. A `string` arrives verbatim, a `bool` arrives as `1` or `0`, and a `list` arrives as a bash array of its strings. |

<a id="option-fields"></a>

| Field | Accepts | Description |
| --- | --- | --- |
| [`description`](#option-description) (optional) | *string* | What the option does. |
| [`default`](#option-default) (required) | *list of strings* | The value that the component builds with if no image sets one. A string default may read `{image-id}` alone. `buildc generate` writes the id in place of the `{image-id}` reference. |
| [`renovate`](#option-renovate) (optional) |  | What keeps the value of a `version` option current. |

> [!NOTE]
> Every declared option reaches the layer of its component as `OPT_<NAME>`. `<NAME>` is the option name in upper case, and each dash becomes an underscore. A `finalize.sh` hook reads only its own component's options.
>
> A default reaches the layer too, so `component.sh` reads a variable and does not test whether one is set.
>
> A `string` option's value fills each `{<name>}` that names it in a `packages` or `package-groups` argument, as `packages "firefox={version}"` does. A `{<name>}` that names a `bool` or a `list` option, or no option, fails `buildc check`.

<a id="option-description"></a>

### `description` (optional)

What the option does.

```kdl
option "version" type="string" {
    description "The Firefox release to install"
}
```

Accepts: *string*

<a id="option-default"></a>

### `default` (required)

The value that the component builds with if no image sets one. A string default may read `{image-id}` alone. `buildc generate` writes the id in place of the `{image-id}` reference.

```kdl
option "version" type="string" {
    default "128.0"
}
```

Accepts: *list of strings*

<a id="option-renovate"></a>

### `renovate` (optional)

The block names the Renovate datasource and the dependency name that the datasource knows. Only an option named `version` of type `string` may carry it, because that is the value Renovate rewrites.

```kdl
option "version" type="string" {
    renovate datasource="repology" depName="fedora_rawhide/firefox"
}
```

| Property | Accepts | Description |
| --- | --- | --- |
| `datasource=` (required) | *string* | The Renovate datasource that publishes the versions. |
| `depName=` (required) | *string* | The name that the datasource knows the software by. |
| `extractVersion=` (optional) | *string* | The pattern that extracts the version from what the datasource reports. |

<a id="variant"></a>

## `variant` (optional, unique)

An image selects the variant by name with `variant=` on the component entry. Every option that a `set` child names must be declared in the same manifest, and a variant name may appear once.

```kdl
variant "wine-only"
```

Accepts: *string*, then optionally {&nbsp;[fields](#variant-fields)&nbsp;}

<a id="variant-fields"></a>

| Field | Accepts | Description |
| --- | --- | --- |
| [`description`](#variant-description) (optional) | *string* | What the variant is for. |
| [`set`](#variant-set) (optional,&nbsp;repeatable) | *string* | One option that this variant sets, where the option value follows the option name. A string value may read `{image-id}` alone. |

> [!NOTE]
> An image can still set an option on an entry that selects a variant. The option that the image sets wins.

<a id="variant-description"></a>

### `description` (optional)

What the variant is for.

```kdl
variant "wine-only" {
    description "Skip the metadata and .NET payloads"
}
```

Accepts: *string*

<a id="variant-set"></a>

### `set` (optional, repeatable)

One option that this variant sets, where the option value follows the option name. A string value may read `{image-id}` alone.

```kdl
variant "wine-only" {
    set "dotnet" #false
}
```

Accepts: *string*

<a id="asset"></a>

## `asset` (optional, unique)

The name becomes the `ASSET_<NAME>` prefix of the environment that the component layer reads. It takes lowercase letters, digits and dashes, and one component may hold one asset of each name.

```kdl
asset "starship" {
    pin {
        renovate datasource="github-tags" depName="owner/repo"
        version "v1.0.0"
        url "https://github.com/owner/repo/archive/refs/tags/{version}.tar.gz"
        sha256 "b7c232b0e8249d8e55a40beb79c5c43a7d370f3f9408bd215deb0170daeaadf3"
    }
}
```

Accepts: *string*, then {&nbsp;[fields](#asset-fields)&nbsp;}

The layer of the component gets three variables, where `<NAME>` is the asset name in upper case with each dash turned into an underscore:

- `ASSET_<NAME>_VERSION`;
- `ASSET_<NAME>_URL`, with `{version}` already expanded, so no shell code builds a URL;
- `ASSET_<NAME>_SHA256`.

<a id="asset-fields"></a>

| Field | Accepts | Description |
| --- | --- | --- |
| [`pin`](#asset-pin) (optional) | {&nbsp;[fields](#asset-pin-fields)&nbsp;} | Where content comes from, which version it is, what verifies it, and what keeps the version current. |

<a id="asset-pin"></a>

### `pin` (optional)

A pin fixes where a download comes from and which version it is, and says how the version stays current. `buildc` verifies each download against the pinned hash, so a changed upstream file is caught.

```kdl
asset "starship" {
    pin {
        renovate datasource="github-tags" depName="owner/repo"
        version "v1.0.0"
        url "https://github.com/owner/repo/archive/refs/tags/{version}.tar.gz"
        sha256 "b7c232b0e8249d8e55a40beb79c5c43a7d370f3f9408bd215deb0170daeaadf3"
    }
}
```

Accepts: {&nbsp;[fields](#asset-pin-fields)&nbsp;}

A pin holds exactly one of these, so every pin says how it stays current:

- `renovate`, if Renovate keeps the version current;
- `manual`, if nothing tracks the pin.

<a id="asset-pin-fields"></a>

| Field | Accepts | Description |
| --- | --- | --- |
| [`renovate`](#asset-pin-renovate) (optional) |  | The Renovate custom manager that keeps `version` current. |
| [`manual`](#asset-pin-manual) (optional) | *string* | Why nothing tracks this pin. |
| [`version`](#asset-pin-version) (required) | *string* | The version, tag or commit that `url` expands and Renovate rewrites. |
| [`url`](#asset-pin-url) (required) | *string* | Where the content comes from. |
| [`sha256`](#asset-pin-sha256) (optional) | *string* | The hash the fetched content must match. |

> [!NOTE]
> An `asset` holds a `pin`. A library in `sources` declares its own `version`, `sha256` and `unpinned`.
>
> A base reference holds no `pin`. Its image reference holds the location and the version, and `signed` records whether the base publishes a cosign signature. A base's `scap-content` holds a `pin`, whose `sha256` is the hash of the datastream file itself.
>
> Renovate matches `renovate` together with the line directly below it. Put `version` on that line.

<a id="asset-pin-renovate"></a>

#### `renovate` (optional)

The Renovate custom manager that keeps `version` current.

```kdl
asset "starship" {
    pin {
        renovate datasource="github-tags" depName="owner/repo"
    }
}
```

| Property | Accepts | Description |
| --- | --- | --- |
| `datasource=` (required) | `github-releases` or `github-tags` or `git-refs` | The Renovate datasource that tracks the pin. |
| `depName=` (required) | *string* | The name the datasource knows the dependency by. |
| `extractVersion=` (optional) | *string* | The pattern that extracts the version from a tag. |

<a id="asset-pin-manual"></a>

#### `manual` (optional)

Why nothing tracks this pin.

```kdl
asset "starship" {
    pin {
        manual "upstream publishes no releases"
    }
}
```

Accepts: *string*

<a id="asset-pin-version"></a>

#### `version` (required)

The version, tag or commit that `url` expands and Renovate rewrites.

```kdl
asset "starship" {
    pin {
        version "v1.0.0"
    }
}
```

Accepts: *string*

<a id="asset-pin-url"></a>

#### `url` (required)

Where the content comes from.

```kdl
asset "starship" {
    pin {
        url "https://github.com/owner/repo/archive/refs/tags/{version}.tar.gz"
    }
}
```

Accepts: *string*

<a id="asset-pin-sha256"></a>

#### `sha256` (optional)

The hash the fetched content must match.

```kdl
asset "starship" {
    pin {
        sha256 "b7c232b0e8249d8e55a40beb79c5c43a7d370f3f9408bd215deb0170daeaadf3"
    }
}
```

Accepts: *string*

| Property | Accepts | Description |
| --- | --- | --- |
| `from=` (optional) | `asset` or `sidecar` or `manual` | Where the hash is refreshed from. `asset`, the default, hashes the payload itself. `sidecar` reads the `<url>.sha256` file that upstream publishes. `manual` means nothing recomputes the hash. |

> [!NOTE]
> If a pin names a download and no `sha256`, then `buildc check` reports an error. A pin whose `url` is a Git repository is cloned at its `version` and declares none.

<a id="network"></a>

## `network` (optional)

The only value is `scripts`, because the package step already declares its network use. Only a `strict` script rule in `repo.kdl` reads the node.

```kdl
network "scripts"
```

Accepts: `scripts`

> [!NOTE]
> Only a `strict` script rule in `repo.kdl` reads this node.
>
> The package step needs no declaration, because `packages`, `copr` or a repo file already declares it.

<a id="packages"></a>

## `packages` (optional, repeatable)

A package name is a literal, or a string option reference that resolves to one. The resolved name takes letters, digits and `. _ + : -`, with at most one `=` before a version.

```kdl
packages "htop" "tmux"
```

Accepts: *list of strings*, or {&nbsp;[one name per line](#packages-fields)&nbsp;}

| Property | Accepts | Description |
| --- | --- | --- |
| `enablerepo=` (optional) | *string* | A repository that is enabled for this install only. It works with dnf only, so the batch must resolve to `fedora` or `rhel` alone. |

<a id="packages-fields"></a>

| Field | Accepts | Description |
| --- | --- | --- |
| [`<name>`](#packages-name) (optional,&nbsp;repeatable) |  | One name in the block, which the node name names. |

<a id="packages-name"></a>

### `<name>` (optional, repeatable)

One name in the block, which the node name names.

```kdl
packages {
    "htop"
}
```

<a id="package-groups"></a>

## `package-groups` (optional, repeatable)

The package groups that this component installs. Package groups work with dnf only, so an ungated `package-groups` needs a component that targets only `fedora` or `rhel`.

```kdl
package-groups "kde-desktop"
```

Accepts: *list of strings*, or {&nbsp;[one name per line](#package-groups-fields)&nbsp;}

| Property | Accepts | Description |
| --- | --- | --- |
| `enablerepo=` (optional) | *string* | A repository that is enabled for this install only. |

<a id="package-groups-fields"></a>

| Field | Accepts | Description |
| --- | --- | --- |
| [`<name>`](#packages-name) (optional,&nbsp;repeatable) |  | One name in the block, which the node name names. |

<a id="copr"></a>

## `copr` (optional, unique)

The `owner/project` name of a COPR repository that this component enables for its own installs. `copr` works on Fedora only.

```kdl
copr "owner/project"
```

Accepts: *string*

<a id="satisfies"></a>

## `satisfies` (optional)

Each child is one benchmark, and the child node name is the benchmark. Its string arguments are rule IDs, and a benchmark with none is refused.

```kdl
satisfies
```

Accepts: optionally {&nbsp;[fields](#satisfies-fields)&nbsp;}

`buildc generate` writes every claim into `generated/plan.json`. The compliance job in `.github/workflows/build.yml` then:

- resolves each number to an XCCDF rule ID through the SSG datastream;
- scans the pushed image and compares each claim with the scan result;
- reports a number that maps to no rule as a failed declaration;
- reports a rule that the image fails as a false claim;
- reports a failed rule as a composition failure, if the overlay of another component owns the final copy of a file that this component ships. `plan.json` carries `overlay_overridden` for that case;
- skips a target whose components declare nothing, and says so;
- reads `.components[]` only and never `.suppressed[]`, because a suppressed component adds no layer.

<a id="satisfies-fields"></a>

| Field | Accepts | Description |
| --- | --- | --- |
| [`<name>`](#satisfies-name) (optional,&nbsp;unique) | *list of strings* | One benchmark, which the node name names, with the rule numbers that it covers. The set of benchmarks is open. |

<a id="satisfies-name"></a>

### `<name>` (optional, unique)

One benchmark, which the node name names, with the rule numbers that it covers. The set of benchmarks is open.

```kdl
satisfies {
    stig "RHEL-09-232010"
}
```

Accepts: *list of strings*

<a id="on"></a>

## `on` (optional, repeatable)

A gate names one or more of `fedora`, `rhel`, `debian` and `ubuntu`, and every named family must appear in `targets` when the component names any. Every gate is read, so a bad declaration is reported even where the image does not build that family.

```kdl
on "debian" "ubuntu"
```

Accepts: *list of strings*, then optionally {&nbsp;[fields](#on-fields)&nbsp;}

<a id="on-fields"></a>

| Field | Accepts | Description |
| --- | --- | --- |
| [`packages`](#packages) (optional,&nbsp;repeatable) | *list of strings*, or {&nbsp;[one name per line](#packages-fields)&nbsp;} | The packages that this component installs. |
| [`package-groups`](#package-groups) (optional,&nbsp;repeatable) | *list of strings*, or {&nbsp;[one name per line](#package-groups-fields)&nbsp;} | The package groups that this component installs. Package groups work with dnf only, so an ungated `package-groups` needs a component that targets only `fedora` or `rhel`. |
| [`copr`](#copr) (optional,&nbsp;unique) | *string* | The `owner/project` name of a COPR repository that this component enables for its own installs. `copr` works on Fedora only. |
| [`requires`](#requires) (optional,&nbsp;repeatable) | *list of strings*, or {&nbsp;[one name per line](#requires-fields)&nbsp;} | A capability that another component must provide. It also orders the build. |
| [`imports`](#imports) (optional,&nbsp;repeatable) | *list of strings*, or {&nbsp;[one name per line](#imports-fields)&nbsp;} | A component that builds in every image that holds this component. |
| [`after`](#after) (optional,&nbsp;repeatable) | *list of strings*, or {&nbsp;[one name per line](#after-fields)&nbsp;} | A capability that this component builds after. The component does not require it. |
| [`satisfies`](#satisfies) (optional) | optionally {&nbsp;[fields](#satisfies-fields)&nbsp;} | An audit declaration of the benchmarks and rules that this component claims to harden. `buildc` records the claim and certifies nothing, and the scan checks it after the build. |

> [!NOTE]
> A declaration outside a gate applies to every family the component targets. A declaration inside a gate applies to the families that the gate names.
>
> One gate takes several family names, so the user writes a list that two families share once.
>
> `provides` stays outside a gate. A capability that one family offers and another does not belongs in a second component.
>
> `buildc` reads every gate whether or not the image builds for it, so the manifest gets the same checks on each family it claims.
>
> The build drops each gated declaration for a family that it does not build. A Fedora-only `after` does not order the component after a Debian provider, and a Debian scan does not map a Fedora rule number.
>
> If a gate names a family that the component does not target, then `buildc` refuses the gate.

<a id="base"></a>

## `base` (optional)

The reference may carry a `{option}` placeholder, and only a string option may fill it. A component with this node provides `base-image`, so a second base in one image is refused.

```kdl
base "quay.io/fedora/fedora-bootc:{version}"
```

Accepts: *string*, then optionally {&nbsp;[fields](#base-fields)&nbsp;}

<a id="base-fields"></a>

| Field | Accepts | Description |
| --- | --- | --- |
| [`family`](#base-family) (optional) | *string* | The family that an image on this base installs as. |
| [`bootloader`](#base-bootloader) (optional) | *list of strings* | The bootloaders that an image on this base can install, with the default first. |
| [`scap-content`](#base-scap-content) (optional) | *string*, then optionally {&nbsp;[fields](#base-scap-content-fields)&nbsp;} | The SSG datastream file that remediates and measures an image on this base, and the pin it is fetched by. |

<a id="base-family"></a>

### `family` (optional)

The family selects the package manager adapter and the installer recipe for the image. A component's `targets` is read against this value.

```kdl
base "quay.io/fedora/fedora-bootc:{version}" {
    family "fedora"
}
```

Accepts: *string*

<a id="base-bootloader"></a>

### `bootloader` (optional)

Each name must be `grub2` or `systemd`, the two bootloaders the installer writes.

```kdl
base "quay.io/fedora/fedora-bootc:{version}" {
    bootloader "grub2"
}
```

Accepts: *list of strings*

<a id="base-scap-content"></a>

### `scap-content` (optional)

The value is a bare filename. The `pin` child states the `url` and the `sha256` that the fetched datastream must match.

```kdl
base "quay.io/fedora/fedora-bootc:{version}" {
    scap-content "ssg-fedora-ds.xml"
}
```

Accepts: *string*, then optionally {&nbsp;[fields](#base-scap-content-fields)&nbsp;}

<a id="base-scap-content-fields"></a>

| Field | Accepts | Description |
| --- | --- | --- |
| [`pin`](#asset-pin) (optional) | {&nbsp;[fields](#asset-pin-fields)&nbsp;} | Where content comes from, which version it is, what verifies it, and what keeps the version current. |
