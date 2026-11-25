use crate::layout;
use crate::model::remote::{At, Collection, DefaultLibrary};
use std::path::{Path, PathBuf};

pub const DEFAULTS_FILE: &str = "libraries.kdl";

pub fn defaults(assets: &Path) -> Vec<DefaultLibrary> {
    std::fs::read_to_string(assets.join(DEFAULTS_FILE))
        .map(|text| crate::parse::remote::defaults(&text))
        .unwrap_or_default()
}

pub(crate) fn members(tree: &Path) -> Vec<String> {
    layout::member_dirs(tree)
        .iter()
        .map(|path| {
            path.strip_prefix(tree)
                .unwrap_or(path)
                .display()
                .to_string()
        })
        .collect()
}

pub fn cached(root: &Path, collection: &Collection) -> Option<PathBuf> {
    match &collection.at {
        At::Dir(dir) => Some(root.join(dir)).filter(|dir| dir.is_dir()),
        At::Git(remote) => {
            let url = remote.url.as_deref()?;
            let commit = cached_commit(root, url, remote.selector())?;
            Some(commit_path(root, url, &commit).ok()?).filter(|dir| dir.is_dir())
        }
    }
}

pub fn cached_commit(root: &Path, url: &str, selector: &str) -> Option<String> {
    let pointer = pointer(root, url, selector).ok()?;
    let commit = std::fs::read_to_string(pointer).ok()?.trim().to_string();
    is_commit(&commit).then_some(commit)
}

pub fn fetch(root: &Path, sources: &[Collection]) -> Result<(), String> {
    for collection in sources {
        if cached(root, collection).is_none() {
            tree(root, collection)?;
        }
    }
    Ok(())
}

pub fn refresh(root: &Path, libraries: &[Collection]) -> Vec<String> {
    libraries
        .iter()
        .filter(|library| library.pin().is_some())
        .filter_map(|library| tree(root, library).err())
        .map(|err| format!("{err}; `buildc generate` reads the commit that the cache holds"))
        .collect()
}

pub(crate) fn tree(root: &Path, collection: &Collection) -> Result<PathBuf, String> {
    let remote = match &collection.at {
        At::Dir(dir) => {
            let dir = root.join(dir);
            if dir.is_dir() {
                return Ok(dir);
            }
            return Err(format!(
                "`{}` is {}, which is not a directory on this machine",
                collection.name,
                dir.display()
            ));
        }
        At::Git(remote) => remote,
    };
    let url = remote.url.as_deref().ok_or("source has no Git URL")?;
    let selector = remote.selector();
    let (_, dir) =
        refreshed(root, url, selector).map_err(|err| format!("`{}`: {err}", collection.name))?;
    Ok(dir)
}

/// The run stamp holds the process id so that buildc fetches a library at most once in each run.
fn refreshed(root: &Path, url: &str, selector: &str) -> Result<(String, PathBuf), String> {
    let run = pointer(root, url, selector)?.with_extension("run");
    let this_run = std::process::id().to_string();
    if std::fs::read_to_string(&run).is_ok_and(|found| found.trim() == this_run) {
        if let Some(held) = cached_at(root, url, selector)? {
            return Ok(held);
        }
    }
    let found = resolve(root, url, selector)?;
    crate::init::put(&run, &format!("{this_run}\n"))?;
    Ok(found)
}

pub fn resolve(root: &Path, url: &str, selector: &str) -> Result<(String, PathBuf), String> {
    let (commit, dir) = fetched(root, url, selector)?;
    crate::init::put(&pointer(root, url, selector)?, &format!("{commit}\n"))?;
    Ok((commit, dir))
}

pub fn at_commit(root: &Path, url: &str, commit: &str) -> Result<PathBuf, String> {
    let dir = commit_path(root, url, commit)?;
    if dir.is_dir() {
        return Ok(dir);
    }
    let (fetched, dir) = fetched(root, url, commit)?;
    if fetched != commit {
        return Err(format!("{url} gave {fetched} for commit {commit}"));
    }
    Ok(dir)
}

pub fn at_tag(
    root: &Path,
    url: &str,
    tag: &str,
    follow: bool,
) -> Result<(String, PathBuf), String> {
    let selector = format!("refs/tags/{tag}");
    let held = cached_at(root, url, &selector)?;
    match (follow, held) {
        (true, held) => refreshed(root, url, &selector).or_else(|err| {
            let (commit, dir) = held.ok_or_else(|| err.clone())?;
            eprintln!(
                "buildc: tag `{tag}` of {url} was not read again ({err}); the cache keeps it at {}",
                crate::provenance::repo_lock::short(&commit)
            );
            Ok((commit, dir))
        }),
        (false, Some(held)) => Ok(held),
        (false, None) => resolve(root, url, &selector),
    }
}

fn cached_at(root: &Path, url: &str, selector: &str) -> Result<Option<(String, PathBuf)>, String> {
    let Some(commit) = cached_commit(root, url, selector) else {
        return Ok(None);
    };
    let dir = commit_path(root, url, &commit)?;
    Ok(dir.is_dir().then_some((commit, dir)))
}

pub fn is_commit(text: &str) -> bool {
    text.len() == 40
        && text
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}

pub fn remote_commit(root: &Path, url: &str, selector: &str) -> Option<String> {
    if is_commit(selector) {
        return Some(selector.to_string());
    }
    let peeled = format!("{selector}^{{}}");
    let url = git_source(root, url).ok()?;
    // `ls-remote` runs at the filesystem root so that the config of no checkout applies.
    let out = git_ok(
        Path::new("/"),
        &["ls-remote", "--", &url, selector, &peeled],
    )
    .ok()?;
    let listed = String::from_utf8_lossy(&out.stdout);
    let rows: Vec<(&str, &str)> = listed
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .collect();
    // `ls-remote` lists an annotated tag twice.
    // The row with the `^{}` suffix holds the commit and the plain row holds the tag object.
    let ranked = [
        peeled.clone(),
        selector.to_string(),
        format!("refs/tags/{peeled}"),
        format!("refs/tags/{selector}"),
        format!("refs/heads/{selector}"),
    ];
    let (commit, _) = ranked
        .iter()
        .find_map(|want| rows.iter().find(|(_, name)| name == want))?;
    let commit = commit.to_lowercase();
    is_commit(&commit).then_some(commit)
}

fn fetched(root: &Path, url: &str, selector: &str) -> Result<(String, PathBuf), String> {
    let cache = root.join(layout::SOURCES_CACHE);
    std::fs::create_dir_all(&cache).map_err(|err| format!("{}: {err}", cache.display()))?;
    let work = cache.join(format!(".fetching.{}", std::process::id()));
    crate::init::remove(&work)?;
    let work = crate::init::Scratch(work);
    let commit = fetch_git(&git_source(root, url)?, selector, &work.0)?;
    let dir = commit_path(root, url, &commit)?;
    if !dir.is_dir() {
        std::fs::rename(work.0.join("tree"), &dir)
            .map_err(|err| format!("{}: {err}", dir.display()))?;
    }
    Ok((commit, dir))
}

pub fn git_source(root: &Path, source: &str) -> Result<String, String> {
    if source.contains("://") {
        return Ok(source.to_string());
    }
    let path = root.join(source);
    // Git runs in the cache work folder or at the filesystem root.
    // A relative path would name another folder there.
    match std::fs::canonicalize(&path) {
        Ok(found) if found.join(".git").exists() => Ok(found.display().to_string()),
        _ => Err(format!("{} is not a Git checkout", path.display())),
    }
}

fn pointer(root: &Path, url: &str, selector: &str) -> Result<PathBuf, String> {
    Ok(cache_path(root, url, selector)?.with_extension("commit"))
}

fn commit_path(root: &Path, url: &str, commit: &str) -> Result<PathBuf, String> {
    cache_path(root, url, commit)
}

pub fn cache_key(url: &str, selector: &str) -> Result<String, String> {
    crate::runtime::sha256_stdin(format!("{url}\0{selector}").as_bytes())
}

pub fn cache_path(root: &Path, url: &str, selector: &str) -> Result<PathBuf, String> {
    Ok(root
        .join(layout::SOURCES_CACHE)
        .join(cache_key(url, selector)?))
}

/// The returned commit folder stays empty until the calling code fills it.
/// The run stamp makes this process read the seeded commit without a fetch.
pub fn seed(root: &Path, url: &str, selector: &str, commit: &str) -> Result<PathBuf, String> {
    let pointer = pointer(root, url, selector)?;
    crate::init::put(&pointer, &format!("{commit}\n"))?;
    crate::init::put(
        &pointer.with_extension("run"),
        &format!("{}\n", std::process::id()),
    )?;
    let dir = commit_path(root, url, commit)?;
    std::fs::create_dir_all(&dir).map_err(|err| format!("{}: {err}", dir.display()))?;
    Ok(dir)
}

/// The list is what `git rev-parse --local-env-vars` prints, less the config variables that a user passes on purpose.
/// An inherited `GIT_DIR` would point the cache fetch at the user's repository.
const REPOSITORY_ENV: &[&str] = &[
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_COMMON_DIR",
    "GIT_DIR",
    "GIT_GRAFT_FILE",
    "GIT_IMPLICIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_INTERNAL_SUPER_PREFIX",
    "GIT_NO_REPLACE_OBJECTS",
    "GIT_OBJECT_DIRECTORY",
    "GIT_PREFIX",
    "GIT_REPLACE_REF_BASE",
    "GIT_SHALLOW_FILE",
    "GIT_WORK_TREE",
];

pub(crate) fn git(repo: &Path, args: &[&str]) -> std::process::Command {
    let mut command = std::process::Command::new("git");
    command
        .args(args)
        .current_dir(repo)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ASKPASS", "/bin/false")
        .env("SSH_ASKPASS", "/bin/false");
    for name in REPOSITORY_ENV {
        command.env_remove(name);
    }
    command
}

fn git_ok(repo: &Path, args: &[&str]) -> Result<std::process::Output, String> {
    ran(args, git(repo, args))
}

fn ran(args: &[&str], mut command: std::process::Command) -> Result<std::process::Output, String> {
    let out = command
        .output()
        .map_err(|err| format!("git {}: {err}", args.join(" ")))?;
    if out.status.success() {
        return Ok(out);
    }
    Err(format!(
        "git {}: {}\n{}",
        args.join(" "),
        out.status,
        String::from_utf8_lossy(&out.stderr).trim_end()
    ))
}

fn fetch_git(url: &str, selector: &str, work: &Path) -> Result<String, String> {
    let work = {
        let repo = work.join("repo.git");
        std::fs::create_dir_all(&repo).map_err(|err| format!("{}: {err}", repo.display()))?;
        std::fs::canonicalize(work).map_err(|err| format!("{}: {err}", work.display()))?
    };
    let repo = work.join("repo.git");
    let tree = work.join("tree");
    let archive = work.join("source.tar");
    git_ok(&repo, &["init", "--bare", "--quiet"])?;
    // The bare repository's `info/attributes` outrank the `.gitattributes` of the fetched tree.
    // Each unset attribute stops `git archive` from rewriting a file or running a filter.
    let attributes = repo.join("info/attributes");
    crate::init::put(
        &attributes,
        "* -export-subst -export-ignore -text -eol -filter -ident -working-tree-encoding\n",
    )?;
    git_ok(
        &repo,
        &[
            "-c",
            "protocol.version=2",
            "fetch",
            "--quiet",
            "--depth=1",
            "--no-tags",
            "--",
            url,
            selector,
        ],
    )?;
    let commit = git_ok(&repo, &["rev-parse", "FETCH_HEAD^{commit}"])?;
    let commit = String::from_utf8_lossy(&commit.stdout).trim().to_string();
    let output = format!("--output={}", archive.to_string_lossy());
    let archive_args = ["archive", "--format=tar", &output, &commit];
    let mut archive_git = git(&repo, &archive_args);
    // A filter or `core.autocrlf` in the host git config would change the archived bytes.
    archive_git
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1");
    ran(&archive_args, archive_git)?;
    std::fs::create_dir_all(&tree).map_err(|err| format!("{}: {err}", tree.display()))?;
    let out = std::process::Command::new("tar")
        .args([
            "-xf",
            &archive.to_string_lossy(),
            "-C",
            &tree.to_string_lossy(),
        ])
        .output()
        .map_err(|err| format!("tar: {err}"))?;
    if !out.status.success() {
        return Err(format!(
            "tar: {}{}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(commit)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(dir: &Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "tag.gpgSign=false",
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
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn upstream(root: &Path) -> (String, String, String) {
        let repo = root.join("upstream");
        std::fs::create_dir_all(&repo).unwrap();
        run(&repo, &["init", "--quiet", "--initial-branch=main"]);
        std::fs::write(repo.join("marker"), "first\n").unwrap();
        run(&repo, &["add", "marker"]);
        run(&repo, &["commit", "--quiet", "-m", "first"]);
        run(&repo, &["tag", "-a", "v1", "-m", "v1"]);
        let first = run(&repo, &["rev-parse", "HEAD"]);
        std::fs::write(repo.join("marker"), "second\n").unwrap();
        run(&repo, &["commit", "--quiet", "-am", "second"]);
        let second = run(&repo, &["rev-parse", "HEAD"]);
        (format!("file://{}", repo.display()), first, second)
    }

    #[test]
    fn a_commit_is_forty_lowercase_hex_digits() {
        assert!(is_commit(&"0a".repeat(20)));
        assert!(!is_commit(&"0A".repeat(20)));
        assert!(!is_commit(&"g".repeat(40)));
        assert!(!is_commit("a1b2c3d"));
        assert!(!is_commit(&"a".repeat(41)));
    }

    #[test]
    fn a_tag_and_a_commit_read_their_own_trees() {
        let root = std::env::temp_dir().join(format!("buildc-cache-git-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (url, first, second) = upstream(&root);

        let (tagged, dir) = at_tag(&root, &url, "v1", false).unwrap();
        assert_eq!(tagged, first);
        assert_eq!(
            std::fs::read_to_string(dir.join("marker")).unwrap(),
            "first\n"
        );
        assert_eq!(remote_commit(&root, &url, "v1"), Some(first.clone()));

        let dir = at_commit(&root, &url, &second).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("marker")).unwrap(),
            "second\n"
        );
        let (branch, _) = resolve(&root, &url, "main").unwrap();
        assert_eq!(branch, second);
        assert_eq!(cached_commit(&root, &url, "main"), Some(second));
        assert!(at_tag(&root, &url, "main", false).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_path_source_reads_its_checkout_from_the_repository_root() {
        let root = std::env::temp_dir().join(format!("buildc-cache-path-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (_, first, _) = upstream(&root);

        let (tagged, dir) = at_tag(&root, "upstream", "v1", false).unwrap();
        assert_eq!(tagged, first);
        assert_eq!(
            std::fs::read_to_string(dir.join("marker")).unwrap(),
            "first\n"
        );
        assert_eq!(remote_commit(&root, "upstream", "v1"), Some(first));

        std::fs::create_dir_all(root.join("plain")).unwrap();
        let refused = at_tag(&root, "plain", "v1", false).unwrap_err();
        assert!(
            refused.ends_with("plain is not a Git checkout"),
            "{refused}"
        );
        assert_eq!(remote_commit(&root, "plain", "v1"), None);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_cached_tree_keeps_its_commit_bytes_under_the_host_git_config() {
        let root = std::env::temp_dir().join(format!("buildc-cache-attrs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let repo = root.join("upstream");
        std::fs::create_dir_all(&repo).unwrap();
        run(&repo, &["init", "--quiet", "--initial-branch=main"]);
        std::fs::write(repo.join(".gitattributes"), "marker filter=host text\n").unwrap();
        std::fs::write(repo.join("marker"), "first\n").unwrap();
        run(&repo, &["add", "."]);
        run(&repo, &["commit", "--quiet", "-m", "first"]);
        let first = run(&repo, &["rev-parse", "HEAD"]);
        let host = root.join("host.gitconfig");
        let planted = root.join("planted");
        std::fs::write(
            &host,
            format!(
                "[core]\n\tautocrlf = true\n[filter \"host\"]\n\tsmudge = touch {} && sed s/first/smudged/\n",
                planted.display()
            ),
        )
        .unwrap();
        // nextest runs each test in its own process.
        // The variable reaches no other test.
        std::env::set_var("GIT_CONFIG_GLOBAL", &host);

        let dir = at_commit(&root, "upstream", &first).unwrap();
        std::env::remove_var("GIT_CONFIG_GLOBAL");
        assert_eq!(std::fs::read(dir.join("marker")).unwrap(), b"first\n");
        assert!(!planted.exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_cached_tree_keeps_its_commit_bytes_under_the_tree_attributes() {
        let root = std::env::temp_dir().join(format!("buildc-cache-export-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let repo = root.join("upstream");
        std::fs::create_dir_all(&repo).unwrap();
        run(&repo, &["init", "--quiet", "--initial-branch=main"]);
        std::fs::write(
            repo.join(".gitattributes"),
            "subst export-subst\nident ident\nhidden export-ignore\n",
        )
        .unwrap();
        std::fs::write(repo.join("subst"), "$Format:%H$\n").unwrap();
        std::fs::write(repo.join("ident"), "$Id$\n").unwrap();
        std::fs::write(repo.join("hidden"), "kept\n").unwrap();
        run(&repo, &["add", "."]);
        run(&repo, &["commit", "--quiet", "-m", "first"]);
        let first = run(&repo, &["rev-parse", "HEAD"]);

        let dir = at_commit(&root, "upstream", &first).unwrap();
        let subst = std::fs::read_to_string(dir.join("subst")).unwrap();
        let ident = std::fs::read_to_string(dir.join("ident")).unwrap();
        let hidden = std::fs::read_to_string(dir.join("hidden"));
        std::fs::remove_dir_all(root).unwrap();
        assert_eq!(subst, "$Format:%H$\n");
        assert_eq!(ident, "$Id$\n");
        assert_eq!(hidden.unwrap(), "kept\n");
    }

    #[test]
    fn a_followed_tag_keeps_its_cached_commit_when_the_remote_is_gone() {
        let root = std::env::temp_dir().join(format!("buildc-cache-gone-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (_, first, _) = upstream(&root);
        at_tag(&root, "upstream", "v1", true).unwrap();
        // The removed run stamp sends the next read to the remote.
        // The empty `.git` folder passes as a checkout and fails the fetch.
        std::fs::remove_file(
            pointer(&root, "upstream", "refs/tags/v1")
                .unwrap()
                .with_extension("run"),
        )
        .unwrap();
        std::fs::remove_dir_all(root.join("upstream")).unwrap();
        std::fs::create_dir_all(root.join("upstream/.git")).unwrap();

        let read = at_tag(&root, "upstream", "v1", true);
        std::fs::remove_dir_all(root).unwrap();
        assert_eq!(read.map(|(commit, _)| commit), Ok(first));
    }

    #[test]
    fn a_fetch_leaves_the_repository_that_git_dir_names_alone() {
        let root = std::env::temp_dir().join(format!("buildc-cache-gitdir-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (url, first, _) = upstream(&root);
        let user = root.join("user");
        std::fs::create_dir_all(&user).unwrap();
        run(&user, &["init", "--quiet", "--initial-branch=main"]);
        let config = std::fs::read_to_string(user.join(".git/config")).unwrap();
        std::env::set_var("GIT_DIR", user.join(".git"));

        let fetched = at_commit(&root, &url, &first);
        std::env::remove_var("GIT_DIR");
        let after = std::fs::read_to_string(user.join(".git/config")).unwrap();
        let shallow = user.join(".git/shallow").exists();
        std::fs::remove_dir_all(root).unwrap();
        assert!(fetched.is_ok(), "{fetched:?}");
        assert_eq!(after, config);
        assert!(!shallow);
    }

    #[test]
    fn a_remote_commit_reads_the_tag_before_a_branch_of_its_name() {
        let root = std::env::temp_dir().join(format!("buildc-cache-rank-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (url, first, second) = upstream(&root);
        run(&root.join("upstream"), &["tag", "v2", &first]);
        run(&root.join("upstream"), &["branch", "v2", &second]);

        let read = remote_commit(&root, &url, "v2");
        let branch = remote_commit(&root, &url, "main");
        std::fs::remove_dir_all(root).unwrap();
        assert_eq!(read, Some(first));
        assert_eq!(branch, Some(second));
    }
}
