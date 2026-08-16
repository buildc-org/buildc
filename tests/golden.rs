use std::path::{Path, PathBuf};
use buildc::Command;

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn compare(name: &str, file: &str, actual: &str) {
    let actual = actual.replace(env!("CARGO_PKG_VERSION"), "{version}");
    let path = crate_dir().join("tests/golden").join(name).join(file);
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("{}: {err}\nrun UPDATE_GOLDEN=1 cargo test", path.display()));
    assert!(
        expected == actual,
        "{} changed. Rerun with UPDATE_GOLDEN=1 and read the diff",
        path.display()
    );
}

fn capture(name: &str, root: &Path) {
    std::env::set_current_dir(root).expect("fixture root exists");
    let here = Path::new(".");
    for (command, file) in [
        (Command::Plan, "plan.json"),
        (Command::Section, "section.txt"),
        (Command::Summary, "summary.md"),
        (Command::Sbom, "sbom.json"),
    ] {
        compare(name, file, &buildc::run(command, None, here).stdout);
    }
    compare(
        name,
        "issues.txt",
        &buildc::run(Command::Check, None, here).issues.plain(),
    );

    let mut generated = String::new();
    for (path, body) in &buildc::run(Command::Generate, None, here).files {
        if path == std::path::Path::new("generated/plan.json") {
            generated.push_str(&format!("==== {}\n", path.display()));
        } else {
            generated.push_str(&format!("==== {}\n{body}", path.display()));
        }
    }
    compare(name, "generated.txt", &generated);
}

fn init_repo(name: &str) -> PathBuf {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("BUILDC_ASSETS", crate_dir().join("assets"));
    std::env::set_var("GIT_CONFIG_GLOBAL", "/dev/null");
    std::env::set_var("GIT_CONFIG_SYSTEM", "/dev/null");
    buildc::create::Repo::collect(
        Some("Example".into()),
        None,
        Some("someone".into()),
        Some("Example".into()),
        None,
        Some(root.clone()),
        &buildc::prompt::Prompt::silent(),
    )
    .unwrap()
    .apply()
    .unwrap();
    root
}

fn verify(name: &str, root: &Path) {
    std::env::set_current_dir(root).expect("fixture root exists");
    let here = Path::new(".");
    let run = buildc::run(Command::Generate, None, here);
    for (path, body) in &run.files {
        std::fs::create_dir_all(path.parent().expect("a file under generated/")).unwrap();
        std::fs::write(path, body).unwrap();
    }
    let issues = || buildc::run(Command::Verify, None, here).issues.plain();

    let mut out = format!("==== as generated\n{}", issues());

    let manifest = Path::new("generated/plan.json");
    let edited = std::fs::read_to_string(manifest)
        .unwrap()
        .replace("\"schema_version\": 1", "\"schema_version\": 0");
    std::fs::write(manifest, edited).unwrap();
    out.push_str(&format!("==== edited by hand\n{}", issues()));

    std::fs::remove_file(manifest).unwrap();
    std::fs::write("generated/leftover", "").unwrap();
    out.push_str(&format!("==== gone, and one nobody claims\n{}", issues()));

    compare(name, "verify.txt", &out);
}

fn create(name: &str, root: &Path) {
    std::env::set_current_dir(root).expect("the created-into repository exists");
    let here = Path::new(".");
    let silent = buildc::prompt::Prompt::silent();

    buildc::create::Image::collect(
        here,
        Some("Server".into()),
        None,
        "example",
        None,
        "a name argument",
        &silent,
    )
    .unwrap()
    .apply(here)
    .unwrap();
    buildc::create::Module::collect(
        here,
        Some("My Editor".into()),
        vec!["nano".into()],
        vec![("provides".into(), "editor".into())],
        vec!["example".into(), "server".into()],
        &silent,
    )
    .unwrap()
    .apply()
    .unwrap();
    buildc::create::Module::collect(
        here,
        Some("plain".into()),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        &silent,
    )
    .unwrap()
    .apply()
    .unwrap();

    let mut out = String::new();
    for file in [
        "modules/my-editor/module.kdl",
        "modules/plain/module.kdl",
        "example.image.kdl",
        "server.image.kdl",
    ] {
        out.push_str(&format!(
            "==== {file}\n{}",
            std::fs::read_to_string(file).unwrap()
        ));
    }
    out.push_str(&format!(
        "==== check\n{}",
        buildc::run(Command::Check, None, here).issues.plain()
    ));
    compare(name, "create.txt", &out);
}

fn import(name: &str, root: &Path) {
    let mut out = String::new();
    let collections = crate_dir().join("tests/collections");
    std::fs::write(
        root.join("repo.kdl"),
        format!(
            "schema-version 1\n\nsources {{\n    one {:?}\n    two {:?}\n}}\n",
            collections.join("one").display(),
            collections.join("two").display()
        ),
    )
    .unwrap();
    std::env::set_current_dir(root).expect("the imported-into repository exists");
    let here = Path::new(".");

    let (list, issues, _) = buildc::declarations(here);
    let sources = list.sources;
    out.push_str(&format!("==== the registry\n{}", issues.plain()));
    for collection in &sources {
        out.push_str(&format!("{}\n", collection.name));
    }

    out.push_str("==== the catalog\n");
    for module in buildc::import::catalog(here, &sources).unwrap() {
        out.push_str(&format!("{}  {}\n", module.qualified, module.about()));
    }

    for wanted in [
        "flatpak",
        "browser",
        "one/browser",
        "nosuch",
        "one/nosuch",
        "flatpak",
    ] {
        out.push_str(&format!("==== import {wanted}\n"));
        let module = buildc::import::split(wanted).1;
        match buildc::import::find(here, &sources, wanted, false) {
            Err(message) => out.push_str(&format!("{message}\n")),
            Ok(found) if found.len() > 1 => {
                let owners: Vec<&str> = found.iter().map(|f| f.owner.as_str()).collect();
                out.push_str(&format!("ambiguous: {}\n", owners.join(", ")));
            }
            Ok(found) => match buildc::import::destination(here, &found[0], module).and_then(|dest| {
                buildc::import::vendor(here, &sources, &found[0], &dest).map(|()| dest)
            }) {
                Ok(dest) => out.push_str(&format!("imported {}\n", dest.display())),
                Err(message) => out.push_str(&format!("{message}\n")),
            },
        }
    }

    out.push_str("==== the tree\n");
    let mut written: Vec<String> = walk(Path::new("modules"))
        .iter()
        .map(|p| format!("{}\n", p.display()))
        .collect();
    written.sort();
    out.extend(written);

    out.push_str(&format!(
        "==== the record\n{}",
        std::fs::read_to_string("modules/one/flatpak/provenance.kdl").unwrap()
    ));
    out.push_str(&format!(
        "==== check\n{}",
        buildc::run(Command::Check, None, here).issues.plain()
    ));
    out.push_str(&format!(
        "==== modified\n{:?}\n",
        buildc::provenance::record::modified(here)
    ));

    std::fs::write("modules/one/flatpak/module.sh", "echo forked\n").unwrap();
    out.push_str(&format!(
        "==== modified after an edit\n{:?}\n{}",
        buildc::provenance::record::modified(here),
        buildc::run(Command::Check, None, here).issues.plain()
    ));
    compare(name, "import.txt", &out);
}

fn edited_module(root: &Path) {
    std::env::set_current_dir(root).expect("the created-into repository exists");
    let here = Path::new(".");
    for (path, body) in &buildc::run(Command::Generate, None, here).files {
        std::fs::create_dir_all(path.parent().expect("a file under generated/")).unwrap();
        std::fs::write(path, body).unwrap();
    }
    let issues = || buildc::run(Command::Verify, None, here).issues.plain();
    let clean = issues();
    assert!(
        clean.is_empty(),
        "verify was not green to begin with: {clean}"
    );

    let module = Path::new("modules/my-editor/module.sh");
    std::fs::write(module, "echo edited\n").unwrap();
    let dirty = issues();
    assert!(
        dirty.contains("generated/plan.json"),
        "an edited module left plan.json current: {dirty}"
    );
}

fn why(name: &str, root: &Path, module: &str) {
    std::env::set_current_dir(root).expect("fixture root exists");
    let here = Path::new(".");
    let mut out = String::new();

    for (command, heading) in [(Command::Why, "markdown"), (Command::WhyJson, "json")] {
        out.push_str(&format!("==== {heading}\n"));
        out.push_str(&buildc::run(command, Some(module), here).stdout);
    }

    out.push_str("==== unknown\n");
    out.push_str(&buildc::run(Command::Why, Some("nosuch"), here).issues.plain());

    let manifest = buildc::emit::json::Json::parse(&buildc::run(Command::Plan, None, here).stdout)
        .expect("the plan is a document");
    let host = buildc::emit::why::on_host(&manifest, None, module).expect("the manifest names it");
    out.push_str("==== from the baked manifest, with no repository\n");
    out.push_str(&host.markdown());
    out.push_str(&format!(
        "==== the names it knows\n{}\n",
        buildc::emit::why::known_on_host(&manifest).join(", ")
    ));

    let record = buildc::emit::json::Json::parse(&format!(
        "{{\"modules\": [{{\"path\": {module:?}, \"content\": \"not what was declared\"}}]}}"
    ))
    .expect("the record is a document");
    let observed =
        buildc::emit::why::on_host(&manifest, Some(&record), module).expect("the manifest names it");
    out.push_str("==== against a build record that disagrees\n");
    out.push_str(
        observed
            .markdown()
            .split("## Where it came from")
            .nth(1)
            .unwrap_or_default()
            .split("## What it pulls in")
            .next()
            .unwrap_or_default(),
    );

    compare(name, "why.txt", &out);
}

fn unenforced(root: &Path) {
    let temp = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("unenforced");
    let _ = std::fs::remove_dir_all(&temp);
    copy(root, &temp);
    let repo = temp.join("repo.kdl");
    let text = std::fs::read_to_string(&repo).unwrap();
    let (before, rest) = text.split_once("audit {").expect("the fixture enforces");
    let after = rest.split_once('}').expect("a closed block").1;
    std::fs::write(&repo, format!("{before}{after}")).unwrap();

    std::env::set_current_dir(&temp).expect("the copied repository exists");
    let issues = buildc::run(Command::Check, None, Path::new("."))
        .issues
        .plain();
    assert!(
        issues.is_empty(),
        "the same repository has to check clean unenforced: {issues}"
    );
}

fn unpinned_import(root: &Path) {
    std::env::set_current_dir(root).expect("fixture root exists");
    let (list, _, _) = buildc::declarations(Path::new("."));
    assert!(
        list.sources.iter().any(|c| c.unpinned()),
        "the fixture has to declare an unpinned collection"
    );
    let refused = buildc::import::find(Path::new("."), &list.sources, "anything", true)
        .err()
        .expect("enforcement refuses an unpinned collection");
    assert!(refused.contains("follows a moving ref"), "{refused}");
}

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let (src, dest) = (entry.path(), to.join(entry.file_name()));
        match src.is_dir() {
            true => copy(&src, &dest),
            false => drop(std::fs::copy(&src, &dest).unwrap()),
        }
    }
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        match path.is_dir() {
            true => out.extend(walk(&path)),
            false => out.push(path),
        }
    }
    out
}

fn no_default(root: &Path) {
    std::env::set_current_dir(root).expect("fixture root exists");
    let here = Path::new(".");
    for quiet in [Command::Check, Command::Generate] {
        let issues = buildc::run(quiet, None, here).issues.plain();
        assert!(issues.is_empty(), "{quiet:?} reported {issues}");
    }
    for quiet in [Command::Section, Command::Graph] {
        let issues = buildc::run(quiet, Some("server"), here).issues.plain();
        assert!(issues.is_empty(), "{quiet:?} reported {issues}");
    }
    for loud in [
        Command::Plan,
        Command::Section,
        Command::Graph,
        Command::Summary,
        Command::Sbom,
    ] {
        let issues = buildc::run(loud, None, here).issues.plain();
        assert!(
            issues.contains("2 images are declared and none is the default"),
            "{loud:?} reported {issues}"
        );
    }
}

fn bin(name: &str, gh: Option<&str>) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let dir = tmp().join(format!("{name}-bin"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let git = std::env::var("PATH")
        .unwrap_or_default()
        .split(':')
        .map(|at| Path::new(at).join("git"))
        .find(|at| at.is_file())
        .expect("git on PATH");
    std::os::unix::fs::symlink(git, dir.join("git")).unwrap();
    if let Some(script) = gh {
        let at = dir.join("gh");
        std::fs::write(&at, script).unwrap();
        std::fs::set_permissions(&at, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    dir
}

const SIGNED_OUT: &str = "#!/bin/sh\ntest \"$1\" = auth && exit 1\nexit 0\n";
const SIGNED_IN: &str = "#!/bin/sh\nexit 0\n";

fn sealed<'a>(
    command: &'a mut std::process::Command,
    path: &Path,
) -> &'a mut std::process::Command {
    command
        .env("PATH", path)
        .env("HOME", tmp())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("BUILDC_ASSETS", crate_dir().join("assets"))
}

fn flow(name: &str, dir: &Path, gh: Option<&str>, args: &[&str]) {
    let fixture = crate_dir().join("tests/golden").join(name);
    let log = tmp().join(format!("{name}.log"));
    let file = std::fs::File::create(&log).unwrap();
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_tect"));
    let status = sealed(&mut command, &bin(name, gh))
        .args(args)
        .current_dir(dir)
        .env("BUILDC_ANSWERS", fixture.join("answers.txt"))
        .stdout(std::process::Stdio::from(file.try_clone().unwrap()))
        .stderr(std::process::Stdio::from(file))
        .status()
        .unwrap();
    let transcript = format!(
        "{}==== exit {}\n",
        std::fs::read_to_string(&log).unwrap(),
        status.code().unwrap_or_default()
    );
    compare(name, "transcript.txt", &transcript);
}

fn tmp() -> PathBuf {
    PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
}

fn buildc(dir: &Path, args: &[&str]) {
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_tect"));
    let out = sealed(&mut command, &bin("buildc", None))
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn empty(name: &str) -> PathBuf {
    let dir = tmp().join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn flow_repo(name: &str) -> PathBuf {
    let dir = empty(name);
    buildc(
        &dir,
        &[
            "--no-tui", "create", "repo", "Example", "--owner", "someone", "--image", "Example",
        ],
    );
    dir.join("example")
}

fn flow_repo_two(name: &str) -> PathBuf {
    let root = flow_repo(name);
    buildc(
        &root,
        &["--no-tui", "--root", ".", "create", "image", "Server"],
    );
    root
}

fn flow_repo_sourced(name: &str) -> PathBuf {
    let root = flow_repo(name);
    let collections = crate_dir().join("tests/collections");
    let mut repo = std::fs::read_to_string(root.join("repo.kdl"))
        .unwrap()
        .replace(&buildc::init::sources(&crate_dir().join("assets")), "");
    repo.push_str(&format!(
        "sources {{\n    one {:?}\n    two {:?}\n}}\n",
        collections.join("one").display(),
        collections.join("two").display()
    ));
    std::fs::write(root.join("repo.kdl"), repo).unwrap();
    root
}

const KEYHOLDER: &str = "description \"Signs the modules it builds\"\n\n\
     supports \"fedora\"\n\n\
     key \"secureboot\" {\n\
     \x20   generator \"openssl\" profile=\"module-signing\" bits=4096\n\
     \x20   public \"/usr/share/secureboot/sb_cert.der\" format=\"der\"\n\
     \x20   private \"MOK.priv\"\n\
     }\n";

#[test]
fn flows() {
    let repo = ["create", "repo"];
    for (name, gh) in [
        ("flow-create-repo", None),
        ("flow-create-repo-no-gh", None),
        ("flow-create-repo-signed-out", Some(SIGNED_OUT)),
        ("flow-create-repo-signed-in", Some(SIGNED_IN)),
        ("flow-create-repo-forgejo", None),
        ("flow-image-default", None),
    ] {
        flow(name, &empty(&format!("{name}-in")), gh, &repo);
    }

    flow(
        "flow-create-image",
        &flow_repo_sourced("flow-image"),
        None,
        &["--root", ".", "create", "image"],
    );
    flow(
        "flow-check-shadow",
        &flow_repo_sourced("flow-shadow"),
        None,
        &["--root", ".", "check"],
    );
    flow(
        "flow-check-unpinned",
        &flow_repo("flow-unpinned"),
        None,
        &["--root", ".", "check"],
    );

    let root = flow_repo("flow-module");
    let module = ["--root", ".", "create", "module"];
    flow("flow-create-module", &root, None, &module);
    flow(
        "flow-module-taken",
        &root,
        None,
        &[&module[..], &["My Editor"]].concat(),
    );
    flow("flow-unanswered", &root, None, &module);
    flow(
        "flow-module-two-images",
        &flow_repo_two("flow-module-both"),
        None,
        &module,
    );

    flow(
        "flow-import-module",
        &flow_repo_sourced("flow-import"),
        None,
        &["--root", ".", "import", "module"],
    );

    flow(
        "flow-key-absent",
        &flow_repo_sourced("flow-key-none"),
        None,
        &["--root", ".", "create", "key", "cosign"],
    );

    let root = flow_repo("flow-key");
    std::fs::create_dir_all(root.join("modules/signed-kernel")).unwrap();
    std::fs::write(root.join("modules/signed-kernel/module.kdl"), KEYHOLDER).unwrap();
    flow(
        "flow-key-kinds",
        &root,
        None,
        &["--root", ".", "create", "key"],
    );
}

#[test]
fn schema_doc() {
    let path = crate_dir().join("docs/schema.md");
    let doc = std::fs::read_to_string(&path).expect("docs/schema.md exists");
    let rendered = buildc::emit::schema_md::render(&doc).unwrap_or_else(|err| panic!("{err}"));
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&path, rendered).unwrap();
        return;
    }
    assert!(
        doc == rendered,
        "docs/schema.md is stale. Rerun with UPDATE_GOLDEN=1 and read the diff"
    );
}

#[test]
fn golden() {
    let dir = crate_dir().join("tests/repos");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .expect("tests/repos exists")
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert!(!names.is_empty());

    let init = init_repo("init");
    assert!(
        !init.join("bases.kdl").exists(),
        "bases.kdl is a control asset; it must not land at a scaffolded repository root"
    );
    for name in names {
        capture(&name, &dir.join(&name));
    }
    no_default(&dir.join("no-default"));
    unenforced(&dir.join("enforced"));
    unpinned_import(&dir.join("unpinned-source"));
    why("enforced", &dir.join("enforced"), "one/hello");
    capture("init", &init);
    verify("init", &init);
    let created = init_repo("create");
    create("create", &created);
    edited_module(&created);
    import("import", &init_repo("import"));
}

#[test]
fn every_written_document_reads_back() {
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        return;
    }
    let dir = crate_dir().join("tests/golden");
    let mut read = 0;
    for path in walk(&dir) {
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        if text.trim().is_empty() {
            continue;
        }
        let parsed = buildc::emit::json::Json::parse(&text)
            .unwrap_or_else(|err| panic!("{}: {err}", path.display()));
        assert!(
            parsed.render() == text,
            "{} did not read back as what was written",
            path.display()
        );
        read += 1;
    }
    assert!(read >= 20, "only {read} documents were read");
}

#[test]
fn the_scan_fixtures_are_the_goldens_they_came_from() {
    for (golden, shipped) in [
        ("enforced/plan.json", "plan.json"),
        ("suppressed/plan.json", "suppressed.json"),
        ("broken-overlay/plan.json", "overlay.json"),
    ] {
        let from = crate_dir().join("tests/golden").join(golden);
        let to = crate_dir().join("assets/tests/scan-fixtures").join(shipped);
        let wanted = std::fs::read_to_string(&from).unwrap();
        if std::env::var_os("UPDATE_GOLDEN").is_some() {
            std::fs::write(&to, &wanted).unwrap();
            continue;
        }
        assert!(
            std::fs::read_to_string(&to).unwrap_or_default() == wanted,
            "{} is no longer {}; rerun with UPDATE_GOLDEN=1",
            to.display(),
            from.display()
        );
    }
}
