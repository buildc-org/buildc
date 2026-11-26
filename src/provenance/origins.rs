use crate::diag::{Issue, Issues, Source};
use crate::layout;
use crate::model::remote::{At, Collection};
use crate::parse::schema::{check_doc, Arg, Kind, Node, Prop, Say};
use crate::parse::{kids, prop, string_arg, syntax_issue};
use kdl::KdlDocument;
use std::path::Path;

pub const FILE: &str = "origins.kdl";

#[rustfmt::skip]
const COPIED: Node = Node::new("component",
    "One copy in this folder, by its path in the library it was copied from.")
    .example("\"browser/firefox\" as=\"firefox\" pin=\"0123456789abcdef0123456789abcdef01234567\" hash=\"b6c232b0…\"").minimal()
    .arg(Arg::Str, Say::new("`component` needs the path of the component", "no path given",
        "`component \"browser\"`, the path in the library it was copied from"))
    .unique(Say::new("`{}` is recorded twice from one origin", "a second entry would disagree",
        "an origin holds one entry for each copy; run `buildc copy component` again to rewrite it"))
    .props(&[
        Prop { name: "as", kind: Kind::Str,
            desc: "The path of the copy in this folder, when it differs from the path in the library.",
            say: Say::new("`as` must be a string", "not a string", ""),
            missing: Say::NONE },
        Prop { name: "pin", kind: Kind::Str,
            desc: "The commit the component was copied at. A copy from a checkout that gives no pin has none.",
            say: Say::new("`pin` must be a string", "not a string", ""),
            missing: Say::NONE },
        Prop { name: "hash", kind: Kind::Str,
            desc: "The hash that the component directory had when it was copied.",
            say: Say::new("`hash` must be a string", "not a string", ""),
            missing: Say::new("an origin entry records no `hash`", "nothing to compare against",
                "run `buildc copy component` again to rewrite it") },
    ], Say::new("unknown origin entry property `{}`", "not part of the schema",
        "an origin entry takes `as`, `pin` and `hash`"));

#[rustfmt::skip]
const ORIGIN: Node = Node::new("origin", "The copies taken from one library.")
    .example("\"https://github.com/buildc-org/library\"").minimal()
    .arg(Arg::Str, Say::new("`origin` needs the location of a library", "no location given",
        "the URL of the library, its `base-url`, or its path"))
    .unique(Say::new("`{}` is an origin twice", "a second block would disagree",
        "the file holds one block for each origin; run `buildc copy component` again to rewrite it"))
    .children(&[COPIED], Say::new("unknown node `{}` in an origin", "not part of the schema",
        "an origin holds one `component` entry for each copy"));

#[rustfmt::skip]
pub(crate) const ORIGINS_FILE: Node = Node::new("origins.kdl",
    "Where each copy in a folder of components came from.")
    .about("`buildc copy component` writes this file in the folder of components that holds the copy. \
        The copy is the repository's own, and the user may edit it. The entry keeps where it came from \
        and what its content hashed to, so `buildc why` can tell whether the copy was edited since.")
    .children(&[ORIGIN], Say::new("unknown node `{}` in the origins", "not part of the schema",
        "the file holds one `origin` block for each library"))
    .notes(&[
        "The user does not edit the file. `buildc copy component` rewrites it.",
        "The origin of a remote library is its URL, and the pin is the commit that was fetched.",
        "The origin of a local library is the `base-url` of its `repo.kdl`, else its path. The pin \
         is its Git `HEAD`. A checkout gives no pin if it holds uncommitted or ignored files or a \
         submodule, if its own config defines a filter, or if another user owns it.",
        "A copy renamed by hand loses its entry, and `buildc why` then says it was written in \
         this repository.",
    ]);

#[derive(Clone, Debug, PartialEq)]
pub struct Origin {
    pub origin: String,
    pub path: String,
    pub name: String,
    pub pin: Option<String>,
    pub hash: String,
}

pub fn read(root: &Path) -> Vec<Origin> {
    read_at(&layout::origins(root))
}

pub fn read_at(file: &Path) -> Vec<Origin> {
    let Ok(raw) = std::fs::read_to_string(file) else {
        return Vec::new();
    };
    let Ok(doc) = raw.parse::<KdlDocument>() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for block in doc
        .nodes()
        .iter()
        .filter(|n| n.name().value() == ORIGIN.name)
    {
        let Some(origin) = string_arg(block) else {
            continue;
        };
        for node in kids(block)
            .iter()
            .filter(|n| n.name().value() == COPIED.name)
        {
            let (Some(path), Some(hash)) = (string_arg(node), prop(node, "hash")) else {
                continue;
            };
            out.push(Origin {
                origin: origin.to_string(),
                path: path.to_string(),
                name: prop(node, "as").unwrap_or(path).to_string(),
                pin: prop(node, "pin").map(str::to_string),
                hash: hash.to_string(),
            });
        }
    }
    out
}

pub fn with(root: &Path, added: &[Origin]) -> Result<String, String> {
    with_at(&layout::origins(root), added)
}

pub fn with_at(file: &Path, added: &[Origin]) -> Result<String, String> {
    parses_at(file).map_err(|err| {
        format!("{err}; a rewrite would drop its other entries, so resolve it, then run the command again")
    })?;
    let mut held: Vec<Origin> = read_at(file)
        .into_iter()
        .filter(|held| !added.iter().any(|new| new.name == held.name))
        .chain(added.iter().cloned())
        .collect();
    held.sort_by(|a, b| (&a.origin, &a.path, &a.name).cmp(&(&b.origin, &b.path, &b.name)));
    let mut out = String::new();
    for (at, copy) in held.iter().enumerate() {
        if at == 0 || held[at - 1].origin != copy.origin {
            if at > 0 {
                out.push_str("}\n\n");
            }
            out.push_str(&format!("origin {} {{\n", quoted(&copy.origin)));
        }
        out.push_str(&format!("    component {}", quoted(&copy.path)));
        if copy.name != copy.path {
            out.push_str(&format!(" as={}", quoted(&copy.name)));
        }
        if let Some(pin) = &copy.pin {
            out.push_str(&format!(" pin={}", quoted(pin)));
        }
        out.push_str(&format!(" hash={}\n", quoted(&copy.hash)));
    }
    if !held.is_empty() {
        out.push_str("}\n");
    }
    Ok(out)
}

fn parses_at(path: &Path) -> Result<(), String> {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Ok(());
    };
    raw.parse::<KdlDocument>()
        .map(drop)
        .map_err(|err| format!("{} does not parse: {err}", path.display()))
}

pub fn source(root: &Path, library: &Collection) -> (String, Option<String>) {
    match &library.at {
        At::Git(pin) => {
            let url = pin.url.clone().unwrap_or_default();
            let commit = crate::collection::cached_commit(root, &url, pin.selector());
            (url, commit)
        }
        At::Dir(dir) => {
            let tree = root.join(dir);
            let location = std::fs::read_to_string(tree.join(layout::REPO_FILE))
                .ok()
                .and_then(|text| crate::parse::repo::base_url(&text))
                .unwrap_or_else(|| dir.clone());
            (location, clean_head(&tree))
        }
    }
}

fn clean_head(tree: &Path) -> Option<String> {
    // `git status` keeps the user's global config because a global filter such as git-lfs is the user's own tool.
    // A `core.fsmonitor` hook in the checkout's config would run a program on `git status`.
    // `--no-optional-locks` stops `git status` from rewriting the index of the user's checkout.
    let git = |args: &[&str]| {
        let mut command =
            crate::collection::git(tree, &["--no-optional-locks", "-c", "core.fsmonitor=false"]);
        command.args(args).env("GIT_CONFIG_NOSYSTEM", "1");
        command.output().ok()
    };
    let alone = |args: &[&str]| {
        crate::collection::git(tree, args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .ok()
    };
    if !alone(&["rev-parse", "--git-dir"])?.status.success() {
        return None;
    }
    // `git status` runs a clean filter that the checkout's own config defines.
    // `git config` exits with 1 when no key matches.
    let filters = alone(&["config", "--get-regexp", r"^filter\."])?;
    if filters.status.code() != Some(1) || !filters.stdout.is_empty() {
        return None;
    }
    let read = |args: &[&str]| {
        let out = git(args)?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    };
    let staged = read(&["ls-files", "--stage", "--", "."])?;
    // Mode 160000 marks a submodule.
    // `HEAD` does not pin the files of a submodule checkout and the status below ignores them.
    if staged.lines().any(|line| line.starts_with("160000 ")) {
        return None;
    }
    let changed = read(&[
        "status",
        "--porcelain",
        "--ignored",
        "--ignore-submodules=all",
        "--",
        ".",
    ])?;
    let head = read(&["rev-parse", "HEAD"])?;
    (changed.is_empty() && crate::collection::is_commit(&head)).then_some(head)
}

pub fn hash(dir: &Path) -> Option<String> {
    let held = layout::held_components(dir);
    let mut files: Vec<String> = Vec::new();
    let mut links: Vec<String> = Vec::new();
    listed(
        dir,
        Path::new(""),
        &held,
        &mut vec![dir.canonicalize().ok()?],
        &mut files,
        &mut links,
    )?;
    if files.is_empty() && links.is_empty() {
        return None;
    }
    let mut lines = match files.is_empty() {
        true => Vec::new(),
        false => crate::runtime::sha256_listing(dir, &files).ok()?,
    };
    for link in links {
        lines.extend(link.into_bytes());
    }
    crate::runtime::sha256_stdin(&lines).ok()
}

fn listed(
    dir: &Path,
    under: &Path,
    held: &[std::path::PathBuf],
    followed: &mut Vec<std::path::PathBuf>,
    files: &mut Vec<String>,
    links: &mut Vec<String>,
) -> Option<()> {
    let here = dir.join(under);
    let mut entries: Vec<std::fs::DirEntry> = std::fs::read_dir(&here).ok()?.flatten().collect();
    // `read_dir` gives no fixed order and one tree must always give one hash.
    entries.sort_by_key(std::fs::DirEntry::path);
    for entry in entries {
        let rel = under.join(entry.file_name());
        if held.iter().any(|component| rel.starts_with(component)) {
            continue;
        }
        let path = entry.path();
        let link = entry.file_type().ok()?.is_symlink();
        // An overlay link reaches the image as a link and its target text is its content.
        if link && layout::in_overlay(dir, &rel) {
            let target = std::fs::read_link(&path).ok()?;
            links.push(format!("link {} {}\n", rel.display(), target.display()));
            continue;
        }
        if link {
            let real = path.canonicalize().ok()?;
            let looped = followed.iter().any(|at| at.starts_with(&real));
            if looped || layout::leaves(dir, &path).is_some() {
                return None;
            }
        }
        if path.is_dir() {
            let real = path.canonicalize().ok()?;
            followed.push(real);
            listed(dir, &rel, held, followed, files, links)?;
            followed.pop();
        } else {
            files.push(rel.to_string_lossy().into_owned());
        }
    }
    Some(())
}

pub fn check(root: &Path, libraries: &[Collection], issues: &mut Issues) {
    let mut files = vec![layout::origins(root)];
    for library in libraries {
        if let At::Dir(dir) = &library.at {
            let file = layout::library_components(&root.join(dir)).join(layout::ORIGINS_FILE);
            if !files.contains(&file) {
                files.push(file);
            }
        }
    }
    for file in files {
        check_at(&file, issues);
    }
}

fn check_at(path: &Path, issues: &mut Issues) {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return;
    };
    let name = path.display().to_string();
    let src = Source::new(&name, raw.clone());
    let doc = match raw.parse::<KdlDocument>() {
        Ok(doc) => doc,
        Err(err) => {
            issues.push(syntax_issue(&err, &name, &src));
            return;
        }
    };
    check_doc(&doc, &ORIGINS_FILE, &src, issues);
    for copy in read_at(path) {
        if !inside(&copy.name) {
            issues.push(
                Issue::new(
                    format!("`{}` is not a path in the folder of components", copy.name),
                    &src,
                )
                .help(
                    "an origin entry names a copy by its path in this folder; run `buildc copy \
                     component` again to rewrite it",
                ),
            );
        }
    }
}

pub(crate) fn inside(key: &str) -> bool {
    !key.is_empty()
        && Path::new(key)
            .components()
            .all(|part| matches!(part, std::path::Component::Normal(_)))
}

pub(crate) fn quoted(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn copy(origin: &str, path: &str, name: &str) -> Origin {
        Origin {
            origin: origin.into(),
            path: path.into(),
            name: name.into(),
            pin: Some("a".repeat(40)),
            hash: "abc".into(),
        }
    }

    fn temp(name: &str) -> std::path::PathBuf {
        let root =
            std::env::temp_dir().join(format!("buildc-origins-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        root
    }

    #[test]
    fn a_written_entry_reads_back_as_what_was_written() {
        let root = temp("read");
        let held = [
            copy("https://host/library", "browser/firefox", "firefox"),
            copy("https://host/library", "editor", "editor"),
            Origin {
                pin: None,
                ..copy("../library", "shell", "shell")
            },
        ];
        crate::init::put(&layout::origins(&root), &with(&root, &held).unwrap()).unwrap();
        let text = std::fs::read_to_string(layout::origins(&root)).unwrap();
        assert_eq!(
            text.matches("origin \"https://host/library\"").count(),
            1,
            "{text}"
        );
        assert!(
            text.contains("component \"browser/firefox\" as=\"firefox\" pin="),
            "{text}"
        );
        assert!(text.contains("component \"shell\" hash=\"abc\""), "{text}");

        let mut issues = Issues::default();
        check(&root, &[], &mut issues);
        assert!(issues.is_empty(), "{}", issues.plain());
        let mut read = read(&root);
        read.sort_by(|a, b| a.name.cmp(&b.name));
        let mut expected = held.to_vec();
        expected.sort_by(|a, b| a.name.cmp(&b.name));
        assert_eq!(read, expected);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_second_copy_replaces_only_the_entry_of_its_own_name() {
        let root = temp("replace");
        let first = [
            copy("one", "browser", "browser"),
            copy("one", "editor", "editor"),
        ];
        crate::init::put(&layout::origins(&root), &with(&root, &first).unwrap()).unwrap();
        let again = copy("two", "web/browser", "browser");
        crate::init::put(
            &layout::origins(&root),
            &with(&root, std::slice::from_ref(&again)).unwrap(),
        )
        .unwrap();
        let held = read(&root);
        assert_eq!(
            held.iter().find(|held| held.name == "browser"),
            Some(&again)
        );
        assert_eq!(
            held.iter().find(|held| held.name == "editor"),
            Some(&first[1])
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn origins_that_do_not_parse_are_refused_and_kept() {
        let root = temp("broken");
        let broken = "<<<<<<< HEAD\norigin \"one\" {\n";
        crate::init::put(&layout::origins(&root), broken).unwrap();
        let refused = with(&root, &[copy("one", "editor", "editor")]).unwrap_err();
        assert!(refused.contains("does not parse"), "{refused}");
        let mut issues = Issues::default();
        check(&root, &[], &mut issues);
        assert!(!issues.is_empty());
        assert_eq!(
            std::fs::read_to_string(layout::origins(&root)).unwrap(),
            broken
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn an_entry_without_its_hash_or_outside_the_folder_is_reported() {
        let root = temp("grammar");
        crate::init::put(
            &layout::origins(&root),
            "origin \"one\" {\n    component \"browser\"\n    component \"x\" as=\"../..\" hash=\"0\"\n}\n",
        )
        .unwrap();
        let mut issues = Issues::default();
        check(&root, &[], &mut issues);
        let said = issues.plain();
        assert!(said.contains("records no `hash`"), "{said}");
        assert!(said.contains("`../..` is not a path"), "{said}");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_library_path_with_an_underscore_is_a_key() {
        assert!(inside("tools/node_exporter"));
        assert!(inside("browser@1"));
        assert!(!inside("../browser"));
        assert!(!inside("/etc"));
        assert!(!inside(""));
    }

    #[test]
    fn a_held_component_stays_out_of_the_hash_and_the_overlay_does_not() {
        let root = temp("held");
        let dir = layout::component(&root, "nvidia");
        crate::init::put(&dir.join(layout::MANIFEST_FILE), "description \"x\"\n").unwrap();
        let before = hash(&dir).unwrap();

        crate::init::put(&dir.join("cuda").join(layout::MANIFEST_FILE), "").unwrap();
        crate::init::put(&dir.join("cuda/component.sh"), "true\n").unwrap();
        assert_eq!(hash(&dir).unwrap(), before);

        // A `component.kdl` inside the overlay is an image file and marks no nested component.
        let overlay = dir
            .join(layout::OVERLAY)
            .join("usr/share/x")
            .join(layout::MANIFEST_FILE);
        crate::init::put(&overlay, "").unwrap();
        assert_ne!(hash(&dir).unwrap(), before);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn an_overlay_link_hashes_as_its_target_and_a_looping_link_has_no_hash() {
        let root = temp("links");
        let dir = layout::component(&root, "masks");
        crate::init::put(&dir.join(layout::MANIFEST_FILE), "description \"x\"\n").unwrap();
        let etc = dir.join(layout::OVERLAY).join("etc");
        std::fs::create_dir_all(&etc).unwrap();
        std::os::unix::fs::symlink("/dev/null", etc.join("a.service")).unwrap();
        let masked = hash(&dir);
        std::fs::remove_file(etc.join("a.service")).unwrap();
        std::os::unix::fs::symlink("/dev/zero", etc.join("a.service")).unwrap();
        let retargeted = hash(&dir);
        crate::init::put(&dir.join("lib/x.sh"), "true\n").unwrap();
        std::os::unix::fs::symlink(".", dir.join("lib/again")).unwrap();
        let looped = hash(&dir);
        std::fs::remove_dir_all(root).unwrap();
        assert!(masked.is_some());
        assert_ne!(masked, retargeted);
        assert_eq!(looped, None);
    }

    fn git(dir: &Path, args: &[&str]) {
        let out = std::process::Command::new("git")
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgSign=false",
            ])
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[test]
    fn a_local_library_pins_its_clean_head_and_names_its_base_url() {
        let root = temp("local");
        let tree = root.join("library");
        crate::init::put(&tree.join("browser/component.kdl"), "").unwrap();
        let library = Collection {
            name: "mine".into(),
            at: At::Dir("library".into()),
            span: crate::diag::Span::default(),
        };
        assert_eq!(source(&root, &library), ("library".to_string(), None));

        git(&tree, &["init", "--quiet"]);
        crate::init::put(
            &tree.join(layout::REPO_FILE),
            "base-url \"https://example.org/mine\"\n",
        )
        .unwrap();
        git(&tree, &["add", "."]);
        git(&tree, &["commit", "--quiet", "-m", "first"]);
        let (location, pin) = source(&root, &library);
        assert_eq!(location, "https://example.org/mine");
        assert!(
            pin.as_deref().is_some_and(crate::collection::is_commit),
            "{pin:?}"
        );

        crate::init::put(&tree.join("browser/component.kdl"), "description \"x\"\n").unwrap();
        assert_eq!(source(&root, &library).1, None);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_checkout_that_holds_a_submodule_records_no_pin() {
        let root = temp("submodule");
        let tree = root.join("library");
        crate::init::put(&tree.join("browser/component.kdl"), "").unwrap();
        // A gitlink entry stands in for a submodule without a second repository.
        git(&tree, &["init", "--quiet"]);
        git(&tree, &["add", "."]);
        git(&tree, &["commit", "--quiet", "-m", "first"]);
        let head = String::from_utf8(
            std::process::Command::new("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(&tree)
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap();
        git(
            &tree,
            &[
                "update-index",
                "--add",
                "--cacheinfo",
                &format!("160000,{},browser/sub", head.trim()),
            ],
        );
        git(&tree, &["commit", "--quiet", "-m", "second"]);
        let library = Collection {
            name: "mine".into(),
            at: At::Dir("library".into()),
            span: crate::diag::Span::default(),
        };

        let pin = source(&root, &library).1;
        std::fs::remove_dir_all(root).unwrap();
        assert_eq!(pin, None);
    }

    #[test]
    fn a_checkout_that_defines_a_filter_records_no_pin_and_runs_no_filter() {
        planted_filter("filter", "local");
        planted_filter("filter-included", "include");
        planted_filter("filter-worktree", "worktree");
        planted_filter("filter-trusted", "trusted");
    }

    fn planted_filter(case: &str, how: &str) {
        let root = temp(case);
        let tree = root.join("library");
        crate::init::put(&tree.join("browser/component.kdl"), "").unwrap();
        crate::init::put(&tree.join(".gitattributes"), "* filter=planted\n").unwrap();
        git(&tree, &["init", "--quiet"]);
        git(&tree, &["add", "."]);
        git(&tree, &["commit", "--quiet", "-m", "first"]);
        let planted = root.join("planted");
        let clean = format!("touch {} && cat", planted.display());
        match how {
            "include" => {
                let extra = root.join("extra.gitconfig");
                crate::init::put(
                    &extra,
                    &format!("[filter \"planted\"]\n\tclean = {clean}\n"),
                )
                .unwrap();
                git(
                    &tree,
                    &["config", "include.path", &extra.display().to_string()],
                );
            }
            "worktree" => {
                git(&tree, &["config", "extensions.worktreeConfig", "true"]);
                git(
                    &tree,
                    &["config", "--worktree", "filter.planted.clean", &clean],
                );
            }
            _ => git(&tree, &["config", "filter.planted.clean", &clean]),
        }
        let trusted = how == "trusted";
        if trusted {
            let global = root.join("global.gitconfig");
            crate::init::put(&global, "[safe]\n\tdirectory = *\n").unwrap();
            // The global config trusts a checkout of another owner.
            // The checkout must still record no pin.
            std::env::set_var("GIT_CONFIG_GLOBAL", &global);
            std::env::set_var("GIT_TEST_ASSUME_DIFFERENT_OWNER", "1");
        }
        // The rewrite gets a later mtime than the index entry.
        // `git status` then runs the clean filter to compare the file.
        std::thread::sleep(std::time::Duration::from_millis(1100));
        crate::init::put(&tree.join("browser/component.kdl"), "").unwrap();
        let library = Collection {
            name: "mine".into(),
            at: At::Dir("library".into()),
            span: crate::diag::Span::default(),
        };

        let pin = source(&root, &library).1;
        if trusted {
            std::env::remove_var("GIT_CONFIG_GLOBAL");
            std::env::remove_var("GIT_TEST_ASSUME_DIFFERENT_OWNER");
        }
        let ran = planted.exists();
        std::fs::remove_dir_all(root).unwrap();
        assert!(!ran, "{case} ran the filter");
        assert_eq!(pin, None, "{case}");
    }
}
