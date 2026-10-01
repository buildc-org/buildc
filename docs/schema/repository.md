# The repository

A repository is a git repository with `repo.kdl` at its root, which is how `buildc` finds the root. `buildc create repo` scaffolds the layout below. Every path that `buildc` reads or writes has a fixed place, so every repository looks alike.

```text
<repository>/
├── .github/
│   └── workflows/
├── generated/
├── keys/
│   ├── private/  (not in git)
│   └── public/
├── modules/
│   └── .remote/  (not in git)
├── out/  (not in git)
│   └── sources/  (not in git)
├── scripts/
├── .gitignore
├── <name>.image.kdl
├── image.kdl
└── repo.kdl
```

| Path | Written by | In git | Description |
| --- | --- | --- | --- |
| `repo.kdl` | `buildc create repo`, then the user | yes | The settings of the whole repository. See [`repo.kdl`](repo.md). |
| `image.kdl` | `buildc create image`, then the user | yes | The images of the repository. See [`image.kdl`](image.md). |
| `<name>.image.kdl` | `buildc create image`, then the user | yes | More images. The name in front of the suffix is decorative. |
| `modules/` | the user, `buildc create module` or `buildc copy module` | yes | The modules of the repository, one directory each. See [Modules](modules.md). |
| `modules/.remote/` | `buildc` | no | The modules that an image pins from another repository, fetched and verified against their hash. |
| `keys/public/` | `buildc create key` or `buildc set key` | yes | The public half of each key, at the path it has in the image. |
| `keys/private/` | `buildc create key` | no | The private half of each key. The build never copies it into the image. |
| `generated/` | `buildc generate` | yes | Everything that `buildc generate` writes. `buildc verify` compares it byte for byte, so the user does not edit it. |
| `scripts/` | `buildc create scripts`, then the user | yes | The scripts that the repository keeps in place of the ones `buildc generate` writes into `generated/scripts/`. |
| `.github/workflows/` | `buildc generate` | yes | The CI workflows that `workflows` in `repo.kdl` names. |
| `out/` | `buildc` | no | Local exports, caches and scratch. `buildc` reads nothing here as a declaration. |
| `out/sources/` | `buildc` | no | The collections that `buildc` fetches. An imported module is read from here. |
| `.gitignore` | `buildc create repo` | yes | Keeps `keys/private/`, `out/` and `modules/.remote/` out of git. |
