use crate::diag::{Issues, Source};
use crate::layout;
use crate::parse::schema::{check_doc, Arg, Kind, Node, Prop, Say};
use crate::parse::{kids, prop, string_arg, syntax_issue};
use crate::provenance::origins::quoted;
use kdl::KdlDocument;
use std::collections::BTreeMap;
use std::path::Path;

pub const FILE: &str = "repo.lock.kdl";

#[rustfmt::skip]
const LOCKED: Node = Node::new("component",
    "One remote component that the build reads, by its path in its library.")
    .example("\"hardware/nvidia/cuda\" library=\"stable\" source=\"https://github.com/buildc-org/library\" pin=\"v2\" commit=\"0123456789abcdef0123456789abcdef01234567\" content=\"sha256:…\"").minimal()
    .arg(Arg::Str, Say::new("`component` needs the path of the component", "no path given",
        "`component \"browser\"`, the path in its library"))
    .unique(Say::new("`{}` is locked twice in one build", "a second entry would disagree",
        "a build holds one entry for each component; run `buildc generate` to rewrite the lock"))
    .props(&[
        Prop { name: "library", kind: Kind::Str,
            desc: "The library the build reads the component from, as `libraries` in `repo.kdl` names it.",
            say: Say::new("`library` must be a string", "not a string", ""),
            missing: Say::new("a lock entry records no `library`", "library unknown", "run `buildc generate` to rewrite the lock") },
        Prop { name: "source", kind: Kind::Str,
            desc: "The Git repository that holds the component.",
            say: Say::new("`source` must be a string", "not a string", ""),
            missing: Say::new("a lock entry records no `source`", "repository unknown", "run `buildc generate` to rewrite the lock") },
        Prop { name: "pin", kind: Kind::Str,
            desc: "The `pin=` that the build's tree sets on the component. The entry applies only while the tree sets this pin.",
            say: Say::new("`pin` must be a string", "not a string", ""),
            missing: Say::NONE },
        Prop { name: "commit", kind: Kind::Str,
            desc: "The commit the build reads the component at.",
            say: Say::new("`commit` must be a string", "not a string", ""),
            missing: Say::new("a lock entry records no `commit`", "revision unknown", "run `buildc generate` to rewrite the lock") },
        Prop { name: "content", kind: Kind::Str,
            desc: "The hash of the component directory at that commit.",
            say: Say::new("`content` must be a string", "not a string", ""),
            missing: Say::new("a lock entry records no `content` hash", "nothing to compare against", "run `buildc generate` to rewrite the lock") },
    ], Say::new("unknown lock entry property `{}`", "not part of the schema",
        "a lock entry takes `library`, `source`, `pin`, `commit` and `content`"));

#[rustfmt::skip]
const BUILD: Node = Node::new("build", "The remote components of one build.")
    .example("\"workstation\"").minimal()
    .arg(Arg::Str, Say::new("`build` needs the name of a build", "no name given",
        "`build \"workstation\"`, a name in `builds` in `repo.kdl`"))
    .unique(Say::new("`{}` is locked twice", "a second block would disagree",
        "the lock holds one block for each build; run `buildc generate` to rewrite it"))
    .children(&[LOCKED], Say::new("unknown node `{}` in a build's lock", "not part of the schema",
        "a build's lock holds one `component` entry for each remote component"));

#[rustfmt::skip]
pub(crate) const LOCK_FILE: Node = Node::new("repo.lock.kdl",
    "The commit and content hash of every remote component that each build reads.")
    .about("`buildc generate` writes this file at the repository root. It moves each unpinned \
        remote component to the commit of its library `version` and records the commit. Every \
        other command reads a remote component at the commit its entry records.")
    .children(&[BUILD], Say::new("unknown node `{}` in the lock", "not part of the schema",
        "the lock holds one `build` block for each build"))
    .notes(&[
        "The user does not edit the file. `buildc generate` rewrites it from the builds.",
        "If a component's content at its commit no longer matches its entry, then every command \
         that reads the build refuses it.",
        "A component that `pin=` names at a commit stays at that commit. `buildc generate` reads \
         a tag that `pin=` names again, and every other command reads the commit its entry \
         records.",
        "An entry recorded under another `pin=` does not apply, so the component counts as \
         unlocked until `buildc generate` records it.",
        "If a library's remote does not answer, then `buildc generate` keeps the commit that the \
         cache holds and says so.",
        "A component of a local library has no entry, because the user edits it in place.",
    ]);

#[derive(Clone, Debug, PartialEq)]
pub struct Locked {
    pub path: String,
    pub library: String,
    pub source: String,
    pub pin: Option<String>,
    pub commit: String,
    pub content: String,
}

#[derive(Default, Debug, PartialEq)]
pub struct Lock {
    pub builds: BTreeMap<String, Vec<Locked>>,
}

impl Lock {
    pub fn read(root: &Path) -> Self {
        let Ok(raw) = std::fs::read_to_string(root.join(FILE)) else {
            return Self::default();
        };
        let Ok(doc) = raw.parse::<KdlDocument>() else {
            return Self::default();
        };
        let mut builds = BTreeMap::new();
        for block in doc
            .nodes()
            .iter()
            .filter(|n| n.name().value() == BUILD.name)
        {
            let Some(build) = string_arg(block) else {
                continue;
            };
            let entries = kids(block)
                .iter()
                .filter(|n| n.name().value() == LOCKED.name)
                .filter_map(|node| {
                    Some(Locked {
                        path: string_arg(node)?.to_string(),
                        library: prop(node, "library")?.to_string(),
                        source: prop(node, "source")?.to_string(),
                        pin: prop(node, "pin").map(str::to_string),
                        commit: prop(node, "commit")?.to_string(),
                        content: prop(node, "content")?.to_string(),
                    })
                })
                .collect();
            builds.insert(build.to_string(), entries);
        }
        Self { builds }
    }

    pub fn find(
        &self,
        build: &str,
        library: &str,
        path: &str,
        pin: Option<&str>,
    ) -> Option<&Locked> {
        self.builds
            .get(build)?
            .iter()
            .find(|held| held.library == library && held.path == path && held.pin.as_deref() == pin)
    }

    pub fn render(&self) -> String {
        let mut out = String::new();
        for (build, entries) in self.builds.iter().filter(|(_, held)| !held.is_empty()) {
            let mut entries = entries.clone();
            entries.sort_by(|a, b| (&a.path, &a.library).cmp(&(&b.path, &b.library)));
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&format!("build {} {{\n", quoted(build)));
            for held in &entries {
                let pin = held
                    .pin
                    .as_deref()
                    .map(|pin| format!(" pin={}", quoted(pin)))
                    .unwrap_or_default();
                out.push_str(&format!(
                    "    component {} library={} source={}{pin} commit={} content={}\n",
                    quoted(&held.path),
                    quoted(&held.library),
                    quoted(&held.source),
                    quoted(&held.commit),
                    quoted(&held.content),
                ));
            }
            out.push_str("}\n");
        }
        out
    }

    pub fn write(&self, root: &Path) -> Result<bool, String> {
        let path = root.join(FILE);
        let text = self.render();
        let held = std::fs::read_to_string(&path).ok();
        if held.as_deref() == Some(text.as_str()) || (held.is_none() && text.is_empty()) {
            return Ok(false);
        }
        match text.is_empty() {
            true => crate::init::remove(&path)?,
            false => crate::init::put(&path, &text)?,
        }
        Ok(true)
    }
}

fn remote(
    image: &crate::model::image::Image,
) -> impl Iterator<
    Item = (
        &crate::model::image::Entry,
        &crate::model::image::Held,
        &crate::model::image::Revision,
    ),
> {
    image
        .base_entry
        .iter()
        .map(|base| &base.entry)
        .chain(&image.entries)
        .filter_map(|entry| {
            let held = entry.held.as_ref()?;
            Some((entry, held, held.remote.as_ref()?))
        })
}

pub fn of(images: &[crate::model::image::Image]) -> Lock {
    let builds = images
        .iter()
        .map(|image| {
            let entries = remote(image)
                .filter_map(|(entry, held, revision)| {
                    Some(Locked {
                        path: entry.path.clone(),
                        library: held.library.clone(),
                        source: revision.source.clone(),
                        pin: entry.pin.clone(),
                        commit: revision.commit.clone(),
                        content: content(&held.components.join(&entry.path))?,
                    })
                })
                .collect();
            (image.id.clone(), entries)
        })
        .collect();
    Lock { builds }
}

pub fn record(
    root: &Path,
    images: &[crate::model::image::Image],
) -> Result<Option<crate::create::Change>, String> {
    let existed = root.join(FILE).exists();
    let before = Lock::read(root);
    let lock = of(images);
    let mut moved: Vec<String> = lock
        .builds
        .iter()
        .flat_map(|(build, entries)| {
            entries
                .iter()
                .filter(|entry| {
                    before.find(build, &entry.library, &entry.path, entry.pin.as_deref())
                        != Some(*entry)
                })
                .map(|entry| format!("`{}`", entry.path))
        })
        .collect();
    moved.sort();
    moved.dedup();
    if !lock.write(root)? {
        return Ok(None);
    }
    Ok(Some(match (existed, moved.is_empty()) {
        (false, _) => crate::create::Change::Created,
        (true, true) => crate::create::Change::Updated(String::new()),
        (true, false) => {
            crate::create::Change::Updated(format!("{} recorded", crate::said(&moved)))
        }
    }))
}

pub fn unhashable(images: &[crate::model::image::Image]) -> Vec<String> {
    let mut out = Vec::new();
    for image in images {
        for (entry, held, revision) in remote(image) {
            if content(&held.components.join(&entry.path)).is_none() {
                out.push(format!(
                    "`{}` reads `{}` from `{}` at {}, whose content cannot be hashed, so {FILE} \
                     cannot record it",
                    image.id,
                    entry.path,
                    held.library,
                    short(&revision.commit)
                ));
            }
        }
    }
    out
}

pub fn unlocked(root: &Path, images: &[crate::model::image::Image]) -> Vec<String> {
    let held = Lock::read(root);
    let now = of(images);
    let mut out = unhashable(images);
    for (build, entries) in &now.builds {
        for entry in entries {
            if held.find(build, &entry.library, &entry.path, entry.pin.as_deref()) != Some(entry) {
                out.push(format!(
                    "`{build}` reads `{}` from `{}` at {}, which {FILE} does not record; `buildc \
                     generate` records it",
                    entry.path,
                    entry.library,
                    short(&entry.commit)
                ));
            }
        }
    }
    out
}

/// Each call asks the remote of every locked library and every locked tag pin over the network.
/// One question serves every component that reads one library at one tag or at its version.
pub fn moved(root: &Path, list: &crate::model::image::List) -> Vec<String> {
    let lock = Lock::read(root);
    let mut asked: BTreeMap<(String, Option<String>, String), Vec<&str>> = BTreeMap::new();
    for image in &list.images {
        for (entry, held, revision) in remote(image) {
            if entry
                .pin
                .as_deref()
                .is_some_and(crate::collection::is_commit)
            {
                continue;
            }
            let Some(locked) =
                lock.find(&image.id, &held.library, &entry.path, entry.pin.as_deref())
            else {
                continue;
            };
            asked
                .entry((
                    held.library.clone(),
                    entry.pin.clone(),
                    revision.source.clone(),
                ))
                .or_default()
                .push(locked.commit.as_str());
        }
    }
    let mut out = Vec::new();
    for ((library, tag, url), locked) in asked {
        let selector = match &tag {
            Some(tag) => format!("refs/tags/{tag}"),
            None => match list
                .libraries
                .iter()
                .find(|held| held.name == library)
                .and_then(crate::model::remote::Collection::pin)
            {
                Some(pin) => pin.selector().to_string(),
                None => continue,
            },
        };
        let Some(remote) = crate::collection::remote_commit(root, &url, &selector) else {
            out.push(format!(
                "`{library}` could not be compared, because {url} did not answer for `{selector}`"
            ));
            continue;
        };
        if locked.iter().all(|commit| *commit == remote) {
            continue;
        }
        out.push(match &tag {
            Some(tag) => format!(
                "`{library}` is locked at {} for `pin=\"{tag}\"`, and the tag is at {} at the \
                 remote; `buildc generate` moves the components that pin it",
                crate::said(&dedup(&locked)),
                short(&remote)
            ),
            None => format!(
                "`{library}` is locked at {}, and its `{selector}` is at {} at the remote; `buildc \
                 generate` moves the unpinned components",
                crate::said(&dedup(&locked)),
                short(&remote)
            ),
        });
    }
    out
}

fn dedup(commits: &[&str]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for commit in commits {
        let commit = short(commit);
        if !out.contains(&commit) {
            out.push(commit);
        }
    }
    out
}

pub(crate) fn short(commit: &str) -> String {
    commit.chars().take(12).collect()
}

pub fn content(dir: &Path) -> Option<String> {
    crate::provenance::origins::hash(dir).map(|hash| format!("sha256:{hash}"))
}

pub fn check(root: &Path, issues: &mut Issues) {
    let path = root.join(FILE);
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return;
    };
    let src = Source::new(FILE, raw.clone());
    match raw.parse::<KdlDocument>() {
        Ok(doc) => check_doc(&doc, &LOCK_FILE, &src, issues),
        Err(err) => issues.push(syntax_issue(&err, FILE, &src)),
    }
}

pub(crate) fn mismatch(build: &str, held: &Locked, found: Option<&str>) -> String {
    let said = match found {
        Some(found) => format!("its content is {found}"),
        None => "its content cannot be hashed".to_string(),
    };
    format!(
        "`{build}` locks `{}` from `{}` at {} with content {}, and {said}; the cached tree of {} \
         at that commit differs from what was recorded, so remove `{}` and run the command again \
         to fetch it",
        held.path,
        held.library,
        held.commit,
        held.content,
        held.source,
        layout::SOURCES_CACHE
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn locked(path: &str) -> Locked {
        Locked {
            path: path.into(),
            library: "stable".into(),
            source: "https://github.com/buildc-org/library".into(),
            pin: None,
            commit: "a".repeat(40),
            content: "sha256:abc".into(),
        }
    }

    #[test]
    fn a_written_lock_reads_back_as_what_was_written() {
        let root = std::env::temp_dir().join(format!("buildc-repo-lock-{}", std::process::id()));
        let mut lock = Lock::default();
        lock.builds.insert(
            "workstation".into(),
            vec![locked("browser"), locked("apps/editor")],
        );
        let pinned = Locked {
            pin: Some("v2".into()),
            ..locked("browser")
        };
        lock.builds.insert("server".into(), vec![pinned.clone()]);
        assert_eq!(lock.write(&root), Ok(true));
        assert_eq!(lock.write(&root), Ok(false));

        let read = Lock::read(&root);
        assert_eq!(read.builds["workstation"].len(), 2);
        assert_eq!(
            read.find("server", "stable", "browser", Some("v2")),
            Some(&pinned)
        );
        assert_eq!(read.find("server", "stable", "browser", None), None);
        assert_eq!(read.find("server", "testing", "browser", Some("v2")), None);
        let mut issues = Issues::default();
        check(&root, &mut issues);
        assert!(issues.is_empty(), "{}", issues.plain());
        let text = std::fs::read_to_string(root.join(FILE)).unwrap();
        let (_, workstation) = text.split_once("build \"workstation\"").unwrap();
        assert!(
            workstation.find("apps/editor") < workstation.find("\"browser\""),
            "{text}"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn an_empty_lock_is_removed() {
        let root =
            std::env::temp_dir().join(format!("buildc-repo-lock-empty-{}", std::process::id()));
        let mut lock = Lock::default();
        lock.builds
            .insert("workstation".into(), vec![locked("browser")]);
        lock.write(&root).unwrap();
        lock.builds.insert("workstation".into(), Vec::new());
        assert_eq!(lock.write(&root), Ok(true));
        assert!(!root.join(FILE).exists());
        assert_eq!(Lock::default().write(&root), Ok(false));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn an_entry_without_its_commit_is_reported() {
        let root =
            std::env::temp_dir().join(format!("buildc-repo-lock-bad-{}", std::process::id()));
        crate::init::put(
            &root.join(FILE),
            "build \"w\" {\n    component \"browser\" library=\"stable\" source=\"x\" content=\"sha256:0\"\n}\n",
        )
        .unwrap();
        let mut issues = Issues::default();
        check(&root, &mut issues);
        assert!(
            issues.plain().contains("records no `commit`"),
            "{}",
            issues.plain()
        );
        assert_eq!(Lock::read(&root).builds["w"], Vec::new());
        std::fs::remove_dir_all(root).unwrap();
    }
}
