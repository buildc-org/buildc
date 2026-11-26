# The repository

A repository is a git repository with `repo.kdl` at its root, which is how `buildc` finds the root. `buildc create repo` scaffolds the layout below. Every path that `buildc` reads or writes has a fixed place, so every repository looks alike.

```text
<repository>/
├── .cache/
│   └── sources/  (not in git)
├── .github/
│   ├── workflows/
│   └── renovate.json5
├── components/
│   └── origins.kdl
├── disk_config/
├── generated/
├── keys/
│   ├── private/  (not in git)
│   └── public/
├── out/  (not in git)
├── scripts/
├── .gitignore
├── <name>.kdl
├── repo.kdl
└── repo.lock.kdl
```

| Path | Written by | In git | Description |
| --- | --- | --- | --- |
| `repo.kdl` | `buildc create repo`, then the user | yes | The settings of the whole repository. See [`repo.kdl`](repo.md). |
| `repo.lock.kdl` | `buildc generate` | yes | The commit and content hash of each remote component that each build reads. See [`repo.lock.kdl`](repo-lock.md). |
| `components/` | the user, `buildc create component` or `buildc copy component` | yes | The components of the repository. A component is a directory, a `<name>.kdl` file beside where its directory would be, or an inline definition in another component. See [Components](components.md). |
| `<name>.kdl` | `buildc create image`, then the user | yes | A `.kdl` file at the repository root is an image or a component. `builds` in `repo.kdl` names an image as `./<name>`. Another root file reaches a component with `component "./<name>"`. A bare child of a root file reads from the library root. |
| `components/origins.kdl` | `buildc copy component` | yes | Where each copied component came from, and what its content hashed to. See [`origins.kdl`](origins.md). |
| `keys/public/` | `buildc create key` or `buildc set key` | yes | The public half of each key, at the path it has in the image. |
| `keys/private/` | `buildc create key` | no | The private half of each key. The build never copies it into the image. |
| `generated/` | `buildc generate` | yes | Everything that `buildc generate` writes. `buildc verify` compares it byte for byte, so the user does not edit it. |
| `scripts/` | `buildc create scripts`, then the user | yes | The scripts that the repository keeps in place of the ones `buildc generate` writes into `generated/scripts/`. |
| `.github/workflows/` | `buildc generate` | yes | The CI workflows that the `github` block in `repo.kdl` names. |
| `out/` | `buildc` | no | Local exports, caches and scratch. `buildc` reads nothing here as a declaration. |
| `.cache/sources/` | `buildc` | no | The remote libraries, one tree for each commit. A build reads a remote component from here at the commit that `repo.lock.kdl` records. |
| `disk_config/` | `buildc create repo`, then the user | yes | The settings that bootc-image-builder reads when it builds a Fedora or RHEL disk image. `disk.toml` sets the smallest size of `/`. |
| `.github/renovate.json5` | `buildc generate` | yes | The Renovate settings, written from the `renovate` block in `repo.kdl`. Its custom managers keep the `buildc` release, the base images, each pin that declares `renovate` and each image entry's tracked `version` current. |
| `.gitignore` | `buildc create repo` | yes | Keeps `keys/private/`, `out/` and `.cache/` out of git. |
