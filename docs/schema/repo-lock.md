<a id="repo.lock.kdl"></a>

# `repo.lock.kdl`

Location: The root of the repository.

`buildc generate` writes this file at the repository root. It moves each unpinned remote component to the commit of its library `version` and records the commit. Every other command reads a remote component at the commit its entry records.

```kdl
build "workstation" {
    component "hardware/nvidia/cuda" library="stable" source="https://github.com/buildc-org/library" pin="v2" commit="0123456789abcdef0123456789abcdef01234567" content="sha256:…"
}
```

| Field | Accepts | Description |
| --- | --- | --- |
| [`build`](#build) (required,&nbsp;unique) | *string*, then {&nbsp;[fields](#build-fields)&nbsp;} | The remote components of one build. |

> [!NOTE]
> The user does not edit the file. `buildc generate` rewrites it from the builds.
>
> If a component's content at its commit no longer matches its entry, then every command that reads the build refuses it.
>
> A component that `pin=` names at a commit stays at that commit. `buildc generate` reads a tag that `pin=` names again, and every other command reads the commit its entry records.
>
> An entry recorded under another `pin=` does not apply, so the component counts as unlocked until `buildc generate` records it.
>
> If a library's remote does not answer, then `buildc generate` keeps the commit that the cache holds and says so.
>
> A component of a local library has no entry, because the user edits it in place.

<a id="build"></a>

## `build` (required, unique)

The remote components of one build.

```kdl
build "workstation" {
    component "hardware/nvidia/cuda" library="stable" source="https://github.com/buildc-org/library" pin="v2" commit="0123456789abcdef0123456789abcdef01234567" content="sha256:…"
}
```

Accepts: *string*, then {&nbsp;[fields](#build-fields)&nbsp;}

<a id="build-fields"></a>

| Field | Accepts | Description |
| --- | --- | --- |
| [`component`](#build-component) (required,&nbsp;unique) | *string* | One remote component that the build reads, by its path in its library. |

<a id="build-component"></a>

### `component` (required, unique)

One remote component that the build reads, by its path in its library.

```kdl
build "workstation" {
    component "hardware/nvidia/cuda" library="stable" source="https://github.com/buildc-org/library" pin="v2" commit="0123456789abcdef0123456789abcdef01234567" content="sha256:…"
}
```

Accepts: *string*

| Property | Accepts | Description |
| --- | --- | --- |
| `library=` (required) | *string* | The library the build reads the component from, as `libraries` in `repo.kdl` names it. |
| `source=` (required) | *string* | The Git repository that holds the component. |
| `pin=` (optional) | *string* | The `pin=` that the build's tree sets on the component. The entry applies only while the tree sets this pin. |
| `commit=` (required) | *string* | The commit the build reads the component at. |
| `content=` (required) | *string* | The hash of the component directory at that commit. |
