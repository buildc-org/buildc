<a id="origins.kdl"></a>

# `origins.kdl`

Location: The root of the components folder of each local library. In this repository that is `components/` unless `library { components }` in `repo.kdl` moves it.

`buildc copy component` writes this file in the folder of components that holds the copy. The copy is the repository's own, and the user may edit it. The entry keeps where it came from and what its content hashed to, so `buildc why` can tell whether the copy was edited since.

```kdl
origin "https://github.com/buildc-org/library" {
    component "browser/firefox" as="firefox" pin="0123456789abcdef0123456789abcdef01234567" hash="b6c232b0…"
}
```

| Field | Accepts | Description |
| --- | --- | --- |
| [`origin`](#origin) (required,&nbsp;unique) | *string*, then {&nbsp;[fields](#origin-fields)&nbsp;} | The copies taken from one library. |

> [!NOTE]
> The user does not edit the file. `buildc copy component` rewrites it.
>
> The origin of a remote library is its URL, and the pin is the commit that was fetched.
>
> The origin of a local library is the `base-url` of its `repo.kdl`, else its path. The pin is its Git `HEAD`. A checkout gives no pin if it holds uncommitted or ignored files or a submodule, if its own config defines a filter, or if another user owns it.
>
> A copy renamed by hand loses its entry, and `buildc why` then says it was written in this repository.

<a id="origin"></a>

## `origin` (required, unique)

The copies taken from one library.

```kdl
origin "https://github.com/buildc-org/library" {
    component "browser/firefox" as="firefox" pin="0123456789abcdef0123456789abcdef01234567" hash="b6c232b0…"
}
```

Accepts: *string*, then {&nbsp;[fields](#origin-fields)&nbsp;}

<a id="origin-fields"></a>

| Field | Accepts | Description |
| --- | --- | --- |
| [`component`](#origin-component) (required,&nbsp;unique) | *string* | One copy in this folder, by its path in the library it was copied from. |

<a id="origin-component"></a>

### `component` (required, unique)

One copy in this folder, by its path in the library it was copied from.

```kdl
origin "https://github.com/buildc-org/library" {
    component "browser/firefox" as="firefox" pin="0123456789abcdef0123456789abcdef01234567" hash="b6c232b0…"
}
```

Accepts: *string*

| Property | Accepts | Description |
| --- | --- | --- |
| `as=` (optional) | *string* | The path of the copy in this folder, when it differs from the path in the library. |
| `pin=` (optional) | *string* | The commit the component was copied at. A copy from a checkout that gives no pin has none. |
| `hash=` (required) | *string* | The hash that the component directory had when it was copied. |
