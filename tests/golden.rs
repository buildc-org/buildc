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
        "{} changed. Rerun with UPDATE_GOLDEN=1 and read the diff.\n{}",
        path.display(),
        first_difference(&expected, &actual)
    );
}

fn first_difference(expected: &str, actual: &str) -> String {
    let at = expected
        .bytes()
        .zip(actual.bytes())
        .position(|(a, b)| a != b)
        .unwrap_or(expected.len().min(actual.len()));
    let window = |s: &str| {
        let from = at.saturating_sub(60);
        let to = (at + 60).min(s.len());
        s.get(from..to)
            .unwrap_or("<not a char boundary>")
            .escape_debug()
            .to_string()
    };
    format!(
        "first difference at byte {at} of {} expected, {} actual\n  expected: {}\n    actual: {}",
        expected.len(),
        actual.len(),
        window(expected),
        window(actual)
    )
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
        let covered = path == std::path::Path::new("generated/plan.json")
            || path.starts_with("generated/lib")
            || path.starts_with("scripts/")
            || path.starts_with(buildc::layout::WORKFLOW_DIR);
        match covered {
            true => generated.push_str(&format!("==== {}\n", path.display())),
            false => generated.push_str(&format!("==== {}\n{body}", path.display())),
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
    .expect("a silent run draws no review screen")
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

    let dropped = Path::new(".github/workflows/smoke-test.yml");
    let repo = Path::new("repo.kdl");
    let text = std::fs::read_to_string(repo).unwrap();
    std::fs::write(repo, text.replace("    smoke-test\n", "")).unwrap();
    out.push_str(&format!("==== one workflow undeclared\n{}", issues()));

    buildc::write_generated(here, &buildc::run(Command::Generate, None, here).files).unwrap();
    out.push_str(&format!(
        "==== and generated again\n{}smoke-test.yml is {}\n",
        issues(),
        match dropped.exists() {
            true => "still there",
            false => "gone",
        }
    ));

    std::fs::remove_dir_all("generated").unwrap();
    out.push_str(&format!("==== never generated\n{}", issues()));

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
        buildc::create::Field::Image,
        None,
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
    .apply(here)
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
    .apply(here)
    .unwrap();

    let repo = Path::new("repo.kdl");
    let collections = crate_dir().join("tests/collections");
    let text = std::fs::read_to_string(repo).unwrap().replace(
        "sources {\n",
        &format!(
            "sources {{\n    one {:?}\n",
            collections.join("one").display()
        ),
    );
    std::fs::write(repo, text).unwrap();
    let (list, issues, _) = buildc::declarations(here);
    assert!(issues.is_empty(), "{}", issues.plain());
    buildc::import::Module::collect(
        Some("one/fedora-family".into()),
        here,
        &list.sources,
        false,
        vec!["example".into(), "server".into()],
        None,
        buildc::import::Place::Reference,
        &silent,
    )
    .unwrap_or_else(|err| panic!("{}", err.message()))
    .apply(here, &list.sources)
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

fn copied(name: &str, root: &Path) {
    let mut out = String::new();
    let collections = crate_dir().join("tests/collections");
    std::fs::write(
        root.join("repo.kdl"),
        format!(
            "schema-version 1\nname \"Imported\"\nsources {{\n    one {:?}\n    two {:?}\n}}\n",
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
    for module in buildc::import::catalog(here, &sources, true).unwrap().0 {
        out.push_str(&format!("{}  {}\n", module.qualified(), module.about()));
    }

    for wanted in [
        "flatpak",
        "browser",
        "one/browser",
        "two/browser",
        "nosuch",
        "one/nosuch",
        "flatpak",
    ] {
        out.push_str(&format!("==== copy {wanted}\n"));
        let module = buildc::import::split(wanted).1;
        match buildc::import::find(here, &sources, wanted, false) {
            Err(message) => out.push_str(&format!("{message}\n")),
            Ok(found) if found.len() > 1 => {
                let owners: Vec<&str> = found.iter().map(|f| f.owner.as_str()).collect();
                out.push_str(&format!("ambiguous: {}\n", owners.join(", ")));
            }
            Ok(found) => match buildc::import::destination(here, &found[0], module).and_then(|dest| {
                buildc::import::vendor(here, &sources, &found[0], &dest).map(|_| dest)
            }) {
                Ok(dest) => out.push_str(&format!("copied {}\n", dest.display())),
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
        std::fs::read_to_string("modules/flatpak/provenance.kdl").unwrap()
    ));
    out.push_str(&format!(
        "==== check\n{}",
        buildc::run(Command::Check, None, here).issues.plain()
    ));
    out.push_str(&format!(
        "==== modified\n{:?}\n",
        buildc::provenance::record::modified(here)
    ));

    std::fs::write("modules/flatpak/module.sh", "echo forked\n").unwrap();
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
        out.push_str(&buildc::run(command, module.rsplit('/').next(), here).stdout);
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
        buildc::emit::why::display(&buildc::emit::why::known_on_host(&manifest, None)).join(", ")
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

fn summary_on_host(root: &Path) {
    std::env::set_current_dir(root).expect("fixture root exists");
    let here = Path::new(".");
    let manifest = buildc::emit::json::Json::parse(&buildc::run(Command::Plan, None, here).stdout)
        .expect("the plan is a document");

    let (every, _) = buildc::emit::why::built_as(&manifest, None);
    assert!(!every.is_empty(), "{} publishes nothing", root.display());
    for target in every {
        let record = buildc::emit::json::Json::parse(&format!(
            "{{\"target\": {:?}}}",
            buildc::emit::json::text(target, "name").expect("a target is named")
        ))
        .expect("the record is a document");
        let (scoped, _) = buildc::emit::why::built_as(&manifest, Some(&record));
        let [scoped] = scoped.as_slice() else {
            panic!("the record names exactly one target");
        };
        assert_eq!(
            buildc::run(
                Command::Summary,
                buildc::emit::json::text(target, "name").as_deref(),
                here
            )
            .stdout,
            buildc::emit::summary::on_host(scoped),
            "{} disagrees with itself",
            root.display()
        );
    }
}

fn why_unbuilt(root: &Path) {
    let temp = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("why-unbuilt");
    let _ = std::fs::remove_dir_all(&temp);
    copy(root, &temp);
    std::env::set_current_dir(&temp).expect("the copy exists");
    let here = Path::new(".");
    let mut out = String::new();

    out.push_str("==== a module the base suppresses\n");
    let bare = buildc::run(Command::Why, Some("flatpak"), here).stdout;
    out.push_str(&bare);

    let full = buildc::run(Command::Why, Some("apps/flatpak"), here).stdout;
    assert_eq!(bare, full, "the full path is a name like any other");

    std::fs::write(
        temp.join("also.image.kdl"),
        "image {\n    name \"Also\"\n\n    base \"ghcr.io/ublue-os/bazzite:stable\" {\n        \
         family \"fedora\"\n    }\n\n    modules {\n        module \"apps/flatpak\"\n    }\n}\n",
    )
    .unwrap();
    out.push_str("==== and the same module in an image that does build it\n");
    out.push_str(
        buildc::run(Command::Why, Some("flatpak"), here)
            .stdout
            .split("## What it exchanges")
            .next()
            .unwrap_or_default(),
    );
    std::fs::remove_file(temp.join("also.image.kdl")).unwrap();

    std::fs::remove_dir_all(temp.join("modules/apps/flatpak")).unwrap();
    out.push_str("==== and one whose manifest never loaded\n");
    let run = buildc::run(Command::Why, Some("flatpak"), here);
    out.push_str(
        run.issues
            .plain()
            .split("`apps/flatpak` is listed")
            .nth(1)
            .map(|rest| format!("`apps/flatpak` is listed{rest}"))
            .expect("the unread module is reported")
            .as_str(),
    );

    compare("suppressed", "why.txt", &out);
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
    let paths = std::env::var("PATH").unwrap_or_default();
    for tool in ["git", "sha256sum"] {
        let at = paths
            .split(':')
            .map(|at| Path::new(at).join(tool))
            .find(|at| at.is_file())
            .unwrap_or_else(|| panic!("{tool} on PATH"));
        std::os::unix::fs::symlink(at, dir.join(tool)).unwrap();
    }
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

fn flow_offering(name: &str, dir: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let work = tmp().join(format!("{name}-src"));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).unwrap();

    let tarball = work.join("one.tar.gz");
    let status = std::process::Command::new("tar")
        .args(["czf", &tarball.display().to_string(), "one"])
        .current_dir(crate_dir().join("tests/collections"))
        .status()
        .unwrap();
    assert!(status.success(), "tar the fixture collection");
    let out = std::process::Command::new("sha256sum")
        .arg(&tarball)
        .output()
        .unwrap();
    let sha256 = String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();

    let assets = work.join("assets");
    std::fs::create_dir_all(&assets).unwrap();
    let status = std::process::Command::new("cp")
        .arg("-r")
        .arg(crate_dir().join("assets").join("."))
        .arg(&assets)
        .status()
        .unwrap();
    assert!(status.success(), "copy the shipped assets");
    std::fs::write(
        assets.join(buildc::init::SOURCES_FILE),
        format!(
            "sources {{\n    one {{\n        pin {{\n            version \"1\"\n\
             \x20           url \"file://{}\"\n            sha256 \"{sha256}\"\n\
             \x20       }}\n    }}\n}}\n",
            tarball.display()
        ),
    )
    .unwrap();

    let path = bin(name, None);
    for tool in ["curl", "tar", "gzip"] {
        let at = std::env::var("PATH")
            .unwrap_or_default()
            .split(':')
            .map(|at| Path::new(at).join(tool))
            .find(|at| at.is_file())
            .unwrap_or_else(|| panic!("{tool} on PATH"));
        std::os::unix::fs::symlink(at, path.join(tool)).unwrap();
    }

    let fixture = crate_dir().join("tests/golden").join(name);
    let log = tmp().join(format!("{name}.log"));
    let file = std::fs::File::create(&log).unwrap();
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_tect"));
    let status = command
        .env("PATH", &path)
        .env("HOME", tmp())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("BUILDC_ASSETS", &assets)
        .env("BUILDC_ANSWERS", fixture.join("answers.txt"))
        .args(["create", "repo"])
        .current_dir(dir)
        .stdout(std::process::Stdio::from(file.try_clone().unwrap()))
        .stderr(std::process::Stdio::from(file))
        .status()
        .unwrap();
    let transcript = format!(
        "{}==== exit {}\n",
        std::fs::read_to_string(&log).unwrap(),
        status.code().unwrap_or_default()
    );
    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755));
    compare(name, "transcript.txt", &transcript);

    let root = dir.join("example");
    assert!(!root.join("out").exists(), "the fetch cached into the repo");
    let strays: Vec<PathBuf> = std::fs::read_dir(std::env::temp_dir())
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("buildc-bases."))
        })
        .collect();
    assert!(strays.is_empty(), "scratch left behind: {strays:?}");

    let image = std::fs::read_to_string(root.join("example.image.kdl")).unwrap();
    assert!(
        image.contains("source \"one\" {\n            module \"fedora-family\"\n"),
        "{image}"
    );
}

fn drawn_flow(name: &str, dir: &Path, command: &str, after: &str, steps: &[&[u8]]) {
    use std::io::{Read, Write};
    use std::process::Stdio;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    let mut child = std::process::Command::new("script")
        .args(["-qfec", command, "/dev/null"])
        .current_dir(dir)
        .env("BUILDC_ASSETS", crate_dir().join("assets"))
        .env("COLUMNS", "80")
        .env("BUILDC_TPM", "/nonexistent")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("script from util-linux");
    let input = Arc::new(Mutex::new(child.stdin.take().unwrap()));
    let mut output = child.stdout.take().unwrap();
    let raw = Arc::new(Mutex::new(Vec::new()));
    let reader = {
        let (input, raw) = (input.clone(), raw.clone());
        std::thread::spawn(move || {
            let mut byte = [0];
            while output.read_exact(&mut byte).is_ok() {
                let mut held = raw.lock().unwrap();
                held.push(byte[0]);
                if held.ends_with(b"\x1b[6n") {
                    let mut input = input.lock().unwrap();
                    let _ = input.write_all(b"\x1b[1;1R");
                    let _ = input.flush();
                }
            }
        })
    };
    for keys in steps {
        std::thread::sleep(Duration::from_millis(400));
        let mut input = input.lock().unwrap();
        input.write_all(keys).unwrap();
        input.flush().unwrap();
    }
    let status = child.wait().unwrap();
    reader.join().unwrap();
    let mut errors = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut errors)
        .unwrap();
    let raw = raw.lock().unwrap();
    assert!(
        status.success(),
        "{errors}{}",
        String::from_utf8_lossy(&raw)
    );

    let text = String::from_utf8_lossy(&raw);
    let stable = text.rsplit_once(after).unwrap().1;
    compare(
        name,
        "transcript.txt",
        &format!("{after}{stable}==== exit 0\n"),
    );
}

#[test]
fn narrow_readouts_fall_back() {
    let root = crate_dir().join("tests/repos/enforced");
    let buildc = env!("CARGO_BIN_EXE_tect");
    let run = |command: &str, cols: Option<&str>, clear: bool| {
        let mut child = std::process::Command::new("script");
        child
            .args(["-qfec", command, "/dev/null"])
            .current_dir(&root)
            .env("BUILDC_ASSETS", crate_dir().join("assets"))
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        if clear {
            child.env_remove("COLUMNS");
        }
        if let Some(cols) = cols {
            child.env("COLUMNS", cols);
        }
        let out = child
            .spawn()
            .expect("script from util-linux")
            .wait_with_output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    };

    let graph = run(
        &format!("stty cols 40 rows 24; '{buildc}' --root . graph"),
        None,
        true,
    );
    assert!(graph.contains("# Enforced capability graph"), "{graph}");
    assert!(!graph.contains('\u{250c}'), "{graph}");
    let why = run(
        &format!("stty cols 40 rows 24; '{buildc}' --root . why one/hello"),
        None,
        true,
    );
    assert!(why.contains("## Where it is built"), "{why}");
    assert!(!why.contains('\u{250c}'), "{why}");

    let graph = run(&format!("'{buildc}' --root . graph"), Some("40"), false);
    assert!(
        graph.contains("# Enforced capability graph") && !graph.contains('\u{250c}'),
        "{graph}"
    );
    let why = run(
        &format!("'{buildc}' --root . why one/hello"),
        Some("40"),
        false,
    );
    assert!(
        why.contains("## Where it is built") && !why.contains('\u{250c}'),
        "{why}"
    );

    for (name, command) in [
        ("graph", format!("'{buildc}' --root . graph")),
        ("why", format!("'{buildc}' --root . why one/hello")),
    ] {
        let drawn = run(&command, Some("200"), false);
        assert!(
            drawn.contains('\u{250c}'),
            "{name} drew no table at 200 columns"
        );
    }
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

fn flow_repo_with(name: &str, collection: &str) -> PathBuf {
    let root = flow_repo(name);
    let mut repo = std::fs::read_to_string(root.join("repo.kdl"))
        .unwrap()
        .replace(&buildc::init::sources(&crate_dir().join("assets")), "");
    repo.push_str(&format!(
        "sources {{\n    {collection} {:?}\n}}\n",
        crate_dir()
            .join("tests/collections")
            .join(collection)
            .display()
    ));
    std::fs::write(root.join("repo.kdl"), repo).unwrap();
    root
}

fn flow_repo_claiming(name: &str) -> PathBuf {
    flow_repo_with(name, "three")
}

fn flow_repo_enforcing(name: &str) -> PathBuf {
    let root = flow_repo_claiming(name);
    let file = root.join("repo.kdl");
    let mut repo = std::fs::read_to_string(&file).unwrap();
    repo.push_str("\naudit {\n    enforce #true\n}\n");
    std::fs::write(&file, repo).unwrap();
    root
}

fn flow_repo_claiming_two(name: &str) -> PathBuf {
    let root = flow_repo_claiming(name);
    buildc(
        &root,
        &["--no-tui", "--root", ".", "create", "image", "Server"],
    );
    root
}

fn lists_sshd(root: &Path) {
    let file = root.join("example.image.kdl");
    let listed = std::fs::read_to_string(&file).unwrap().replace(
        "    modules {\n    }",
        "    modules {\n        module \"sshd\"\n    }",
    );
    assert!(listed.contains("module \"sshd\""), "{listed}");
    std::fs::write(file, listed).unwrap();
}

const CLAIMANT: &str = "description \"SSH daemon hardening\"\n\nsupports \"fedora\"\n";

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

    flow_offering(
        "flow-create-repo-offer",
        &empty("flow-create-repo-offer-in"),
    );

    let root = flow_repo_sourced("flow-image");
    flow(
        "flow-create-image",
        &root,
        None,
        &["--root", ".", "create", "image"],
    );
    let image = std::fs::read_to_string(root.join("beta.image.kdl")).unwrap();
    assert!(
        image.contains(
            "    modules {\n        source \"one\" {\n            module \"fedora-family\"\n        }\n    }"
        ),
        "{image}"
    );
    buildc(&root, &["--no-tui", "--root", ".", "fetch", "modules"]);
    buildc(&root, &["--no-tui", "--root", ".", "check"]);
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

    flow(
        "flow-check-nested",
        &flow_repo_with("flow-nested", "six"),
        None,
        &["--root", ".", "check"],
    );

    let conforms = flow_repo("flow-conforms");
    std::fs::create_dir_all(conforms.join("modules/hardening")).unwrap();
    std::fs::write(
        conforms.join("modules/hardening/module.kdl"),
        "description \"Claims a rule the profile selects\"\n\nsupports \"fedora\"\n\n\
         satisfies {\n    cis-fedora \"5.2.20\"\n}\n",
    )
    .unwrap();
    let image = conforms.join("example.image.kdl");
    let declared = std::fs::read_to_string(&image).unwrap().replace(
        "    modules {",
        "    conforms \"standard\"\n\n    modules {",
    );
    assert!(declared.contains("conforms \"standard\""), "{declared}");
    std::fs::write(&image, declared).unwrap();
    flow(
        "flow-check-conforms",
        &conforms,
        None,
        &["--root", ".", "check"],
    );
    let stream = crate_dir()
        .join("tests/scap/datastream.xml")
        .display()
        .to_string();
    flow(
        "flow-check-claims",
        &conforms,
        None,
        &["--root", ".", "check", "--datastream", &stream],
    );
    flow(
        "flow-coverage",
        &conforms,
        None,
        &["--root", ".", "coverage", "--datastream", &stream],
    );

    let stranger = flow_repo("flow-conforms-stranger");
    let image = stranger.join("example.image.kdl");
    let declared = std::fs::read_to_string(&image).unwrap().replace(
        "    modules {",
        "    conforms \"cusp_fedora\"\n\n    modules {",
    );
    std::fs::write(&image, declared).unwrap();
    flow(
        "flow-check-conforms-stranger",
        &stranger,
        None,
        &["--root", ".", "check", "--datastream", &stream],
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
        "flow-create-flavour",
        &flow_repo("flow-flavour"),
        None,
        &["--root", ".", "create", "flavour"],
    );

    let root = flow_repo("flow-gated");
    buildc(
        &root,
        &[
            "--no-tui", "--root", ".", "create", "flavour", "dx", "--image", "example",
        ],
    );
    flow(
        "flow-module-in-flavour",
        &root,
        None,
        &["--root", ".", "create", "module"],
    );
    let image = std::fs::read_to_string(root.join("example.image.kdl")).unwrap();
    assert!(
        image.contains(
            "    modules {\n        flavour \"dx\" {\n            module \"dev-tools\"\n        }\n    }"
        ),
        "{image}"
    );

    buildc(
        &root,
        &[
            "--no-tui", "--root", ".", "create", "flavour", "gaming", "--image", "example",
        ],
    );
    let listed = |at: &str| {
        let (list, _, _) = buildc::declarations(&root);
        buildc::create::Listing::collect(&root, vec![at.into()], &buildc::prompt::Prompt::silent())
            .and_then(|listing| listing.refuse_duplicate(&list, "dev-tools", None))
            .err()
            .unwrap_or_default()
    };
    assert_eq!(
        listed("example/dx"),
        "`example/dx` already lists `dev-tools`"
    );
    assert_eq!(
        listed("example"),
        "`example/dx` already lists `dev-tools`, so `example` lists it twice"
    );
    assert_eq!(listed("example/gaming"), "");

    let measured = flow_repo_claiming("flow-set-conforms-in");
    for name in ["flow-set-conforms", "flow-set-conforms-again"] {
        flow(
            name,
            &measured,
            None,
            &["--root", ".", "set", "conforms", "--datastream", &stream],
        );
    }
    let declared = std::fs::read_to_string(measured.join("example.image.kdl")).unwrap();
    assert_eq!(declared.matches("conforms ").count(), 1, "{declared}");
    assert!(
        declared.contains("    conforms \"ospp\"\n")
            && declared.contains("source \"three\" {\n            module \"sshd\""),
        "{declared}"
    );

    let import_sshd = [
        "--root",
        ".",
        "import",
        "module",
        "three/sshd",
        "--datastream",
        stream.as_str(),
    ];
    let claiming = flow_repo_claiming("flow-import-conforms-in");
    flow("flow-import-conforms", &claiming, None, &import_sshd);
    let taken = std::fs::read_to_string(claiming.join("example.image.kdl")).unwrap();
    assert!(
        taken.contains("    conforms \"standard\"\n")
            && taken.contains("source \"three\" {\n            module \"sshd\""),
        "{taken}"
    );

    let unmeasured = flow_repo_claiming("flow-import-conforms-none");
    flow(
        "flow-import-conforms-declined",
        &unmeasured,
        None,
        &import_sshd,
    );
    let left = std::fs::read_to_string(unmeasured.join("example.image.kdl")).unwrap();
    assert!(
        !left.contains("conforms") && left.contains("module \"sshd\""),
        "{left}"
    );

    let enforcing = flow_repo_enforcing("flow-set-conforms-enforced-in");
    flow(
        "flow-set-conforms-enforced",
        &enforcing,
        None,
        &["--root", ".", "set", "conforms", "--datastream", &stream],
    );
    let importing = flow_repo_enforcing("flow-import-conforms-enforced-in");
    flow(
        "flow-import-conforms-enforced",
        &importing,
        None,
        &import_sshd,
    );

    let both = flow_repo_claiming_two("flow-import-conforms-two-in");
    flow("flow-import-conforms-two", &both, None, &import_sshd);
    for named in ["example", "server"] {
        let written = std::fs::read_to_string(both.join(format!("{named}.image.kdl"))).unwrap();
        assert!(
            written.contains("    conforms \"standard\"\n") && written.contains("module \"sshd\""),
            "{named}: {written}"
        );
    }

    let typo = flow_repo_claiming("flow-import-datastream-in");
    flow(
        "flow-import-datastream",
        &typo,
        None,
        &[
            "--root",
            ".",
            "import",
            "module",
            "three/sshd",
            "--datastream",
            "no-such-datastream.xml",
        ],
    );
    let untouched = std::fs::read_to_string(typo.join("example.image.kdl")).unwrap();
    assert!(
        !untouched.contains("module \"sshd\"") && !typo.join("modules/sshd").exists(),
        "the refusal leaves the repository as it was: {untouched}"
    );

    let family = flow_repo_with("flow-import-family-in", "one");
    flow(
        "flow-import-family",
        &family,
        None,
        &["--root", ".", "import", "module", "one/needs-family"],
    );
    let listed = std::fs::read_to_string(family.join("example.image.kdl")).unwrap();
    assert!(
        listed.contains("module \"fedora-family\"") && !listed.contains("debian-family"),
        "the adapter brought has to support the base's family: {listed}"
    );

    let vendored = flow_repo_claiming("flow-copy-conforms-in");
    flow(
        "flow-copy-conforms",
        &vendored,
        None,
        &[
            "--root",
            ".",
            "copy",
            "module",
            "three/sshd",
            "--datastream",
            stream.as_str(),
        ],
    );
    let copied = std::fs::read_to_string(vendored.join("example.image.kdl")).unwrap();
    assert!(
        copied.contains("conforms \"standard\"") && copied.contains("module \"sshd\""),
        "{copied}"
    );

    let claimed = flow_repo("flow-set-claims-in");
    lists_sshd(&claimed);
    std::fs::create_dir_all(claimed.join("modules/sshd")).unwrap();
    std::fs::write(
        claimed.join("modules/sshd/module.kdl"),
        format!("{CLAIMANT}\nsatisfies {{\n    cis-fedora \"5.5.2\"\n}}\n"),
    )
    .unwrap();
    let claims = [
        "--root",
        ".",
        "set",
        "claims",
        "sshd",
        "--datastream",
        stream.as_str(),
    ];
    for name in ["flow-set-claims", "flow-set-claims-again"] {
        flow(name, &claimed, None, &claims);
    }

    let two = flow_repo("flow-set-claims-two-in");
    lists_sshd(&two);
    std::fs::create_dir_all(two.join("modules/sshd")).unwrap();
    std::fs::write(
        two.join("modules/sshd/module.kdl"),
        format!(
            "{CLAIMANT}\nsatisfies {{\n    cis-fedora \"5.5.2\"\n    \
             stig-fedora \"RHEL-09-232010\"\n}}\n"
        ),
    )
    .unwrap();
    flow(
        "flow-set-claims-two",
        &two,
        None,
        &[
            "--root",
            ".",
            "set",
            "claims",
            "sshd",
            "--datastream",
            stream.as_str(),
        ],
    );
    let merged = std::fs::read_to_string(two.join("modules/sshd/module.kdl")).unwrap();
    assert_eq!(merged.matches("satisfies ").count(), 1, "{merged}");
    assert!(
        !merged.contains("cis-fedora") && !merged.contains("stig-fedora"),
        "both benchmark names collapse into the derived one: {merged}"
    );
    buildc(&two, &["--no-tui", "--root", ".", "check"]);

    std::fs::create_dir_all(claimed.join("modules/.remote/one/sshd")).unwrap();
    std::fs::write(
        claimed.join("modules/.remote/one/sshd/module.kdl"),
        CLAIMANT,
    )
    .unwrap();
    flow(
        "flow-set-claims-fetched",
        &claimed,
        None,
        &[
            "--root",
            ".",
            "set",
            "claims",
            ".remote/one/sshd",
            "--datastream",
            stream.as_str(),
        ],
    );
    let declared = std::fs::read_to_string(claimed.join("modules/sshd/module.kdl")).unwrap();
    assert_eq!(declared.matches("satisfies ").count(), 1, "{declared}");
    assert!(
        declared
            .contains("    standard \"1.1.1.1\" \\\n        \"5.2.20\" \\\n        \"5.5.2\"\n"),
        "{declared}"
    );
    buildc(&claimed, &["--no-tui", "--root", ".", "check"]);

    let drawn = flow_repo("flow-set-claims-drawn-in");
    lists_sshd(&drawn);
    std::fs::create_dir_all(drawn.join("modules/sshd")).unwrap();
    std::fs::write(drawn.join("modules/sshd/module.kdl"), CLAIMANT).unwrap();
    drawn_flow(
        "flow-set-claims-drawn",
        &drawn,
        &format!(
            "'{}' --root . set claims sshd --datastream '{stream}'",
            env!("CARGO_BIN_EXE_tect")
        ),
        "Which rules does `sshd` claim?:",
        &[b"\r", b"aide \x1b[B\r"],
    );
    let picked = std::fs::read_to_string(drawn.join("modules/sshd/module.kdl")).unwrap();
    assert!(picked.contains("    standard \"1.1.1.1\"\n"), "{picked}");

    let prompted = flow_repo("flow-set");
    flow(
        "flow-set-workflows",
        &prompted,
        None,
        &["--root", ".", "set", "workflows"],
    );
    let declaration =
        "workflows at=\"05:45\" scan=\"scheduled\" {\n    build\n    base-sig-probe\n}";
    let prompted_repo = std::fs::read_to_string(prompted.join("repo.kdl")).unwrap();
    assert!(prompted_repo.contains(declaration), "{prompted_repo}");

    let drawn = flow_repo("flow-set-workflows-drawn");
    let repo_path = drawn.join("repo.kdl");
    let repo = std::fs::read_to_string(&repo_path).unwrap().replace(
        "workflows {",
        "workflows publish=\"scheduled\" scan=\"scheduled\" {",
    );
    std::fs::write(&repo_path, repo).unwrap();
    drawn_flow(
        "flow-set-workflows-drawn",
        &drawn,
        &format!("'{}' --root . set workflows", env!("CARGO_BIN_EXE_tect")),
        "Which workflows?:",
        &[b"\x1b[B\x1b[B\x1b[B\x1b[B\x1b[B\x1b[B\r", b"\r", b"\r"],
    );
    let repo = std::fs::read_to_string(&repo_path).unwrap();
    assert!(
        repo.contains("workflows publish=\"scheduled\" scan=\"scheduled\" {"),
        "{repo}"
    );

    let direct = flow_repo("flow-cadence-direct");
    let repo_path = direct.join("repo.kdl");
    let mut direct_repo = std::fs::read_to_string(&repo_path).unwrap();
    let span = buildc::parse::repo::workflows_span(&direct_repo).unwrap();
    direct_repo.replace_range(span.offset..span.offset + span.len, declaration);
    std::fs::write(&repo_path, direct_repo).unwrap();

    let generated_build = |root: &Path| {
        let run = buildc::run(Command::Generate, None, root);
        assert!(run.issues.is_empty(), "{}", run.issues.plain());
        buildc::write_generated(root, &run.files).unwrap();
        assert!(
            buildc::run(Command::Verify, None, root).issues.is_empty(),
            "verify rejected its generated workflow"
        );
        run.files
            .into_iter()
            .find(|(path, _)| path == Path::new(".github/workflows/build.yml"))
            .unwrap()
            .1
    };
    let prompted_build = generated_build(&prompted);
    let direct_build = generated_build(&direct);
    let drawn_build = generated_build(&drawn);
    assert_eq!(prompted_build, direct_build);
    assert!(prompted_build.contains(
        "    if: needs.build_push.outputs.publish == 'true' && (github.event_name == 'schedule' || github.event_name == 'workflow_dispatch') && needs.compute-matrix.outputs.scanned != '[]'\n"
    ));
    assert!(!prompted_build.contains(
        "    if: needs.build_push.outputs.publish == 'true' && github.event_name != 'pull_request' && needs.compute-matrix.outputs.scanned != '[]'\n"
    ));

    let cadence = flow_repo("flow-publish-cadence");
    let repo_path = cadence.join("repo.kdl");
    let push_build = generated_build(&cadence);
    assert!(push_build.contains(
        "    if: needs.build_push.outputs.publish == 'true' && github.event_name != 'pull_request' && needs.compute-matrix.outputs.scanned != '[]'\n"
    ));
    let repo = std::fs::read_to_string(&repo_path).unwrap();
    std::fs::write(
        &repo_path,
        repo.replace("workflows {", "workflows publish=\"scheduled\" {"),
    )
    .unwrap();
    let scheduled_build = generated_build(&cadence);
    let publish_gate = r#"          if [ "${{ github.event_name }}" != "schedule" ] \
             && [ "${{ github.event_name }}" != "workflow_dispatch" ]; then
            publish=false
          fi
"#;
    assert!(scheduled_build.contains(publish_gate), "{scheduled_build}");
    assert_eq!(drawn_build, scheduled_build);
    assert_eq!(
        scheduled_build.replace(publish_gate, "").replace(
            "    if: needs.build_push.outputs.publish == 'true' && (github.event_name == 'schedule' || github.event_name == 'workflow_dispatch') && needs.compute-matrix.outputs.scanned != '[]'\n",
            "    if: needs.build_push.outputs.publish == 'true' && github.event_name != 'pull_request' && needs.compute-matrix.outputs.scanned != '[]'\n",
        ),
        push_build,
    );

    let requires = flow_repo_sourced("flow-requires");
    flow(
        "flow-import-requires",
        &requires,
        None,
        &["--root", ".", "import", "module", "two/browser"],
    );
    let image = std::fs::read_to_string(requires.join("example.image.kdl")).unwrap();
    assert!(
        image.contains("source \"one\" {\n            module \"flatpak\"")
            && image.contains("source \"two\" {\n            module \"browser\""),
        "{image}"
    );
    buildc(
        &requires,
        &[
            "--no-tui",
            "--root",
            ".",
            "import",
            "module",
            "one/fedora-family",
            "--image",
            "example",
        ],
    );
    buildc(&requires, &["--no-tui", "--root", ".", "check"]);
    let declined = flow_repo_sourced("flow-declined");
    buildc(
        &declined,
        &[
            "--no-tui",
            "--root",
            ".",
            "import",
            "module",
            "two/browser",
            "--image",
            "example",
        ],
    );
    buildc(
        &declined,
        &[
            "--no-tui",
            "--root",
            ".",
            "import",
            "module",
            "one/fedora-family",
            "--image",
            "example",
        ],
    );
    flow(
        "flow-check-unmet",
        &declined,
        None,
        &["--root", ".", "check"],
    );

    let skip = flow_repo_sourced("flow-skip");
    buildc(
        &skip,
        &["--no-tui", "--root", ".", "create", "image", "Server"],
    );
    buildc(
        &skip,
        &[
            "--no-tui",
            "--root",
            ".",
            "import",
            "module",
            "one/flatpak",
            "--image",
            "example",
        ],
    );
    flow(
        "flow-import-skip",
        &skip,
        None,
        &[
            "--root",
            ".",
            "import",
            "module",
            "two/browser",
            "--image",
            "example",
            "--image",
            "server",
        ],
    );
    let image = std::fs::read_to_string(skip.join("example.image.kdl")).unwrap();
    assert_eq!(image.matches("module \"flatpak\"").count(), 1, "{image}");
    assert!(image.contains("module \"browser\""), "{image}");
    let server = std::fs::read_to_string(skip.join("server.image.kdl")).unwrap();
    assert!(
        server.contains("module \"flatpak\"") && server.contains("module \"browser\""),
        "{server}"
    );
    buildc(
        &skip,
        &[
            "--no-tui",
            "--root",
            ".",
            "import",
            "module",
            "one/fedora-family",
            "--image",
            "example",
        ],
    );
    buildc(&skip, &["--no-tui", "--root", ".", "fetch", "modules"]);
    buildc(&skip, &["--no-tui", "--root", ".", "check"]);

    let unfetched = flow_repo("flow-unfetched");
    std::fs::create_dir_all(unfetched.join("modules/core/one")).unwrap();
    std::fs::write(
        unfetched.join("modules/core/one/module.kdl"),
        "description \"Builds things\"\n\nsupports \"fedora\"\n\nrequires \"build-environment\"\n",
    )
    .unwrap();
    let image = unfetched.join("example.image.kdl");
    let listed = std::fs::read_to_string(&image).unwrap().replace(
        "    modules {\n    }",
        "    modules {\n        module \"core/one\"\n    }",
    );
    assert!(listed.contains("module \"core/one\""), "{listed}");
    std::fs::write(&image, listed).unwrap();
    flow(
        "flow-check-unfetched",
        &unfetched,
        None,
        &["--root", ".", "check"],
    );

    let why = flow_repo("flow-why-picker");
    std::fs::create_dir_all(why.join("modules/core/one")).unwrap();
    std::fs::write(
        why.join("modules/core/one/module.kdl"),
        "description \"Builds things\"\n\nsupports \"fedora\"\n",
    )
    .unwrap();
    let image = why.join("example.image.kdl");
    let listed = std::fs::read_to_string(&image).unwrap().replace(
        "    modules {\n    }",
        "    modules {\n        module \"core/one\"\n    }",
    );
    assert!(listed.contains("module \"core/one\""), "{listed}");
    std::fs::write(image, listed).unwrap();
    drawn_flow(
        "flow-why-picker",
        &why,
        &format!(
            "stty cols 80; '{}' --root . why",
            env!("CARGO_BIN_EXE_tect")
        ),
        "Which module?:",
        &[b"\r"],
    );

    let kernel = flow_repo_sourced("flow-kernel");
    flow(
        "flow-import-kernel",
        &kernel,
        None,
        &["--root", ".", "import", "module", "one/custom-kernel"],
    );
    assert!(std::fs::read_to_string(kernel.join("repo.kdl"))
        .unwrap()
        .contains("    kernel-freshness\n"));

    let root = flow_repo_sourced("flow-import");
    flow(
        "flow-import-module",
        &root,
        None,
        &["--root", ".", "import", "module"],
    );
    let image = std::fs::read_to_string(root.join("example.image.kdl")).unwrap();
    assert!(image.contains("source \"one\" {\n            module \"browser\"\n        }"));
    assert!(root
        .join("modules/.remote/one/browser/module.kdl")
        .is_file());
    assert!(!root.join("modules/browser").exists());

    let (list, issues, _) = buildc::declarations(&root);
    assert!(issues.is_empty(), "{}", issues.plain());
    let declined = buildc::import::Module::collect(
        Some("one/flatpak".into()),
        &root,
        &list.sources,
        false,
        Vec::new(),
        None,
        buildc::import::Place::Reference,
        &buildc::prompt::Prompt::silent(),
    )
    .unwrap_or_else(|err| panic!("{}", err.message()))
    .apply(&root, &list.sources)
    .unwrap_err();
    assert!(declined.contains("--image"), "{declined}");

    buildc::import::Module::collect(
        Some("one/flatpak".into()),
        &root,
        &list.sources,
        false,
        vec!["example".into()],
        None,
        buildc::import::Place::Reference,
        &buildc::prompt::Prompt::silent(),
    )
    .unwrap_or_else(|err| panic!("{}", err.message()))
    .apply(&root, &list.sources)
    .unwrap();
    let image = std::fs::read_to_string(root.join("example.image.kdl")).unwrap();
    assert_eq!(image.matches("source \"one\"").count(), 1);
    assert!(image.contains("module \"browser\"\n            module \"flatpak\""));

    let twice = buildc::import::Module::collect(
        Some("one/flatpak".into()),
        &root,
        &list.sources,
        false,
        vec!["example".into()],
        None,
        buildc::import::Place::Reference,
        &buildc::prompt::Prompt::silent(),
    )
    .err()
    .map(|err| err.message().to_string())
    .unwrap_or_default();
    assert_eq!(twice, "`example` already lists `flatpak`");

    let several = flow_repo_sourced("flow-several");
    flow(
        "flow-import-several",
        &several,
        None,
        &["--root", ".", "import", "module"],
    );
    let image = std::fs::read_to_string(several.join("example.image.kdl")).unwrap();
    for module in ["flatpak", "browser", "custom-kernel"] {
        assert!(image.contains(&format!("module \"{module}\"")), "{image}");
    }
    assert!(std::fs::read_to_string(several.join("repo.kdl"))
        .unwrap()
        .contains("    kernel-freshness\n"));
    buildc(
        &several,
        &[
            "--no-tui",
            "--root",
            ".",
            "import",
            "module",
            "one/fedora-family",
            "--image",
            "example",
        ],
    );
    buildc(&several, &["--no-tui", "--root", ".", "check"]);

    let nested = flow_repo_with("flow-nested", "four");
    flow(
        "flow-import-nested",
        &nested,
        None,
        &["--root", ".", "import", "module"],
    );
    let image = std::fs::read_to_string(nested.join("example.image.kdl")).unwrap();
    assert!(
        image.contains("source \"four\" {\n            module \"hardening/coredumps\"\n        }"),
        "{image}"
    );
    assert!(nested
        .join("modules/.remote/four/hardening/coredumps/module.kdl")
        .is_file());
    buildc(&nested, &["--no-tui", "--root", ".", "fetch", "modules"]);
    buildc(&nested, &["--no-tui", "--root", ".", "check"]);
    buildc(&nested, &["--no-tui", "--root", ".", "generate"]);
    assert!(nested
        .join("generated/example/modules/four/hardening/coredumps.sh")
        .is_file());

    let suffix = flow_repo_with("flow-suffix", "four");
    flow(
        "flow-import-suffix",
        &suffix,
        None,
        &[
            "--root",
            ".",
            "import",
            "module",
            "coredumps",
            "--image",
            "example",
        ],
    );
    let image = std::fs::read_to_string(suffix.join("example.image.kdl")).unwrap();
    assert!(
        image.contains("source \"four\" {\n            module \"hardening/coredumps\"\n        }"),
        "{image}"
    );
    assert!(!image.contains("module \"coredumps\""), "{image}");
    buildc(&suffix, &["--no-tui", "--root", ".", "fetch", "modules"]);
    buildc(&suffix, &["--no-tui", "--root", ".", "check"]);
    buildc(&suffix, &["--no-tui", "--root", ".", "generate"]);
    assert!(suffix
        .join("generated/example/modules/four/hardening/coredumps.sh")
        .is_file());

    let both = flow_repo("flow-suffix-ambiguous");
    let collections = crate_dir().join("tests/collections");
    let mut repo = std::fs::read_to_string(both.join("repo.kdl"))
        .unwrap()
        .replace(&buildc::init::sources(&crate_dir().join("assets")), "");
    repo.push_str(&format!(
        "sources {{\n    four {:?}\n    five {:?}\n}}\n",
        collections.join("four").display(),
        collections.join("five").display()
    ));
    std::fs::write(both.join("repo.kdl"), repo).unwrap();
    flow(
        "flow-import-suffix-ambiguous",
        &both,
        None,
        &[
            "--root",
            ".",
            "import",
            "module",
            "coredumps",
            "--image",
            "example",
        ],
    );
    let image = std::fs::read_to_string(both.join("example.image.kdl")).unwrap();
    assert!(
        image.contains("source \"four\" {\n            module \"hardening/coredumps\"\n        }"),
        "{image}"
    );
    assert!(!image.contains("five"), "{image}");
    assert!(!image.contains("sandbox"), "{image}");

    let (list, _, _) = buildc::declarations(&nested);
    let refused = buildc::import::find(&nested, &list.sources, "four/hardening//coredumps", false)
        .err()
        .expect("an empty part of a path is refused");
    assert_eq!(
        refused,
        "`four/hardening//coredumps` is not a module: a module is named by a path of names, \
         as `<path>`, or `<owner>/<path>` to name one collection, and no part of it may be \
         empty or start with a dot"
    );

    let collide = flow_repo_sourced("flow-collides");
    let remotes = "modules/editor/files/usr/share/example";
    std::fs::create_dir_all(collide.join(remotes)).unwrap();
    std::fs::write(
        collide.join("modules/editor/module.kdl"),
        "description \"Editor shipping its own flatpak remotes\"\n\nsupports \"fedora\"\n",
    )
    .unwrap();
    std::fs::write(collide.join(remotes).join("remotes.list"), "editor\n").unwrap();
    let image = collide.join("example.image.kdl");
    let listed = std::fs::read_to_string(&image).unwrap().replace(
        "    modules {\n    }",
        "    modules {\n        module \"editor\"\n    }",
    );
    assert!(listed.contains("module \"editor\""), "{listed}");
    std::fs::write(&image, listed).unwrap();
    buildc(
        &collide,
        &[
            "--no-tui",
            "--root",
            ".",
            "import",
            "module",
            "one/fedora-family",
            "--image",
            "example",
        ],
    );
    flow(
        "flow-import-collides",
        &collide,
        None,
        &[
            "--root",
            ".",
            "import",
            "module",
            "one/flatpak",
            "--image",
            "example",
        ],
    );
    assert!(
        buildc::run(Command::Check, None, &collide)
            .issues
            .plain()
            .contains("`one/flatpak` overwrites `/usr/share/example/remotes.list`"),
        "check reports the collision the import said"
    );
    flow(
        "flow-check-collides",
        &collide,
        None,
        &["--root", ".", "check"],
    );

    let copied_nested = flow_repo_with("flow-copy-nested", "four");
    flow(
        "flow-copy-nested",
        &copied_nested,
        None,
        &["--root", ".", "copy", "module"],
    );
    assert!(copied_nested
        .join("modules/hardening/coredumps/provenance.kdl")
        .is_file());
    let image = std::fs::read_to_string(copied_nested.join("example.image.kdl")).unwrap();
    assert!(
        image.contains("    modules {\n        module \"hardening/coredumps\"\n    }"),
        "{image}"
    );
    buildc(&copied_nested, &["--no-tui", "--root", ".", "check"]);
    buildc(&copied_nested, &["--no-tui", "--root", ".", "generate"]);
    assert!(copied_nested
        .join("generated/example/modules/hardening/coredumps.sh")
        .is_file());

    let copied = flow_repo_sourced("flow-copy-collides");
    std::fs::create_dir_all(copied.join(remotes)).unwrap();
    std::fs::write(
        copied.join("modules/editor/module.kdl"),
        "description \"Editor shipping its own flatpak remotes\"\n\nsupports \"fedora\"\n",
    )
    .unwrap();
    std::fs::write(copied.join(remotes).join("remotes.list"), "editor\n").unwrap();
    let image = copied.join("example.image.kdl");
    let listed = std::fs::read_to_string(&image).unwrap().replace(
        "    modules {\n    }",
        "    modules {\n        module \"editor\"\n    }",
    );
    assert!(listed.contains("module \"editor\""), "{listed}");
    std::fs::write(&image, listed).unwrap();
    flow(
        "flow-copy-collides",
        &copied,
        None,
        &[
            "--root",
            ".",
            "copy",
            "module",
            "one/flatpak",
            "--image",
            "example",
        ],
    );
    assert!(
        buildc::run(Command::Check, None, &copied)
            .issues
            .plain()
            .contains("`flatpak` overwrites `/usr/share/example/remotes.list`"),
        "check reports the collision the copy said"
    );

    let root = flow_repo_sourced("flow-copy");
    flow(
        "flow-copy-module",
        &root,
        None,
        &["--root", ".", "copy", "module"],
    );
    assert!(root.join("modules/browser/provenance.kdl").is_file());
    assert!(!root.join("modules/one").exists());

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

    flow(
        "flow-key-undeclared",
        &flow_repo_sourced("flow-key-undeclared"),
        None,
        &["--root", ".", "create", "key", "sbom"],
    );

    flow(
        "flow-key-no-kind",
        &flow_repo("flow-key-no-kind"),
        None,
        &["--root", ".", "create", "key"],
    );
}

#[test]
fn install_screens() {
    let dir = empty("flow-install-drawn");
    std::fs::write(
        dir.join(buildc::install::RECIPE),
        r#"{
  "image": "ghcr.io/buildc-org/deb2:latest",
  "targetImgref": "ghcr.io/buildc-org/deb2:latest",
  "composeFsBackend": true,
  "genericImage": true,
  "bootloader": "grub2",
  "filesystem": "ext4",
  "hostname": "deb2",
  "user": { "groups": ["sudo"] }
}
"#,
    )
    .unwrap();
    let sys = dir.join("sys-block");
    for (name, size, removable, model) in [
        ("vda", "134217728", "0", "QEMU HARDDISK"),
        ("sdb", "31457280", "1", "Cruzer Blade"),
    ] {
        let disk = sys.join(name);
        std::fs::create_dir_all(disk.join("device")).unwrap();
        std::fs::write(disk.join("size"), size).unwrap();
        std::fs::write(disk.join("removable"), removable).unwrap();
        std::fs::write(disk.join("device/model"), model).unwrap();
    }
    drawn_flow(
        "flow-install-drawn",
        &dir,
        &format!(
            "BUILDC_SYS_BLOCK='{}' '{}' installer --from .",
            sys.display(),
            env!("CARGO_BIN_EXE_tect")
        ),
        "buildc: ghcr.io/buildc-org/deb2:latest, from .\r\n",
        &[
            b"\r",
            b"\x1b[B\r",
            b"\r",
            b"buildc\r",
            b"hunter2\r",
            b"hunter2\r",
            b"\r",
            b"\r",
            b"\x1b[B\r",
            b"\x1b",
            b"\x1b[B\x1b[B\r",
        ],
    );
    let transcript =
        std::fs::read_to_string(crate_dir().join("tests/golden/flow-install-drawn/transcript.txt"))
            .unwrap();
    assert!(!transcript.contains("hunter2"), "{transcript}");
    assert!(transcript.contains("Are you sure"), "{transcript}");
    assert!(transcript.contains(buildc::copy::CONTINUE), "{transcript}");
    assert!(transcript.contains("/dev/sdb"), "{transcript}");
    assert!(transcript.contains("/dev/vda"), "{transcript}");
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
    assert!(
        !init.join("lib").exists(),
        "lib is generated; it must not land in the editable scaffold"
    );
    for script in ["buildc.sh", "lint.sh", "vm.sh"] {
        assert!(
            !init.join("scripts").join(script).exists(),
            "{script} is generated; it must not land in the editable scaffold"
        );
    }
    for name in names {
        capture(&name, &dir.join(&name));
    }
    no_default(&dir.join("no-default"));
    unenforced(&dir.join("enforced"));
    unpinned_import(&dir.join("unpinned-source"));
    why("enforced", &dir.join("enforced"), "one/hello");
    why("minimal", &dir.join("minimal"), "core/hello");
    why("seeding", &dir.join("seeding"), "buildc-org/flatpak");
    why_unbuilt(&dir.join("suppressed"));
    for name in [
        "minimal",
        "multi-image",
        "suppressed",
        "collecting",
        "pinned",
    ] {
        summary_on_host(&dir.join(name));
    }
    capture("init", &init);
    verify("init", &init);
    let created = init_repo("create");
    create("create", &created);
    edited_module(&created);
    copied("copy", &init_repo("copy"));
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

fn scap_run(root: &Path, args: &[&str]) -> (String, String, i32) {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_tect"))
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        out.status.code().unwrap_or_default(),
    )
}

#[test]
fn coverage() {
    let enforced = crate_dir().join("tests/repos/enforced");
    let stream = crate_dir()
        .join("tests/scap/datastream.xml")
        .display()
        .to_string();
    let read_out = |at: &Path, args: &[&str]| {
        let mut all = vec!["--root", ".", "coverage"];
        all.extend_from_slice(args);
        all.extend(["--datastream", &stream]);
        scap_run(at, &all)
    };

    let mut out = String::new();
    for (heading, args) in [
        ("the read-out, with no scan behind it", &[][..]),
        ("and as json", &["--format", "json"][..]),
    ] {
        let (report, said, code) = read_out(&enforced, args);
        out.push_str(&format!("==== {heading}\n{report}{said}==== exit {code}\n"));
    }

    let root = tmp().join("coverage");
    let _ = std::fs::remove_dir_all(&root);
    copy(&enforced, &root);
    let image = root.join("example.image.kdl");
    let text = std::fs::read_to_string(&image).unwrap();
    std::fs::write(
        &image,
        text.replace("conforms \"standard\"", "conforms \"nosuch\""),
    )
    .unwrap();
    for (heading, at) in [
        ("conforming to a profile the content does not carry", &root),
        (
            "and measured against nothing at all",
            &crate_dir().join("tests/repos/minimal"),
        ),
    ] {
        let (report, said, code) = read_out(at, &[]);
        out.push_str(&format!("==== {heading}\n{report}{said}==== exit {code}\n"));
    }
    let (report, said, code) = scap_run(
        &enforced,
        &[
            "--root",
            ".",
            "coverage",
            "--datastream",
            "example.image.kdl",
        ],
    );
    out.push_str(&format!(
        "==== and a datastream that is not one\n{report}{said}==== exit {code}\n"
    ));

    let (_, said, code) = scap_run(&enforced, &["--root", ".", "coverage"]);
    out.push_str(&format!(
        "==== and with nothing to measure it against\n{said}==== exit {code}\n"
    ));

    compare("coverage", "report.txt", &out);
}

#[test]
fn scap() {
    let enforced = crate_dir().join("tests/repos/enforced");
    let fixtures = crate_dir().join("tests/scap");
    let root = tmp().join("scap");
    let _ = std::fs::remove_dir_all(&root);
    copy(&enforced, &root);

    let repo = root.join("repo.kdl");
    let text = std::fs::read_to_string(&repo).unwrap();
    let (before, rest) = text.split_once("audit {").expect("the fixture enforces");
    let after = rest.split_once('}').expect("a closed block").1;
    std::fs::write(&repo, format!("{before}{after}")).unwrap();

    let baseline = tmp().join("scap-baseline.json");
    let _ = std::fs::remove_file(&baseline);
    let datastream = fixtures.join("datastream.xml");
    let claim_only = tmp().join("scap-claim-only");
    let _ = std::fs::remove_dir_all(&claim_only);
    copy(&enforced, &claim_only);
    let image = claim_only.join("example.image.kdl");
    let text = std::fs::read_to_string(&image).unwrap();
    std::fs::write(&image, text.replace("    conforms \"standard\"\n", "")).unwrap();

    let (report, said, code) = scap_run(
        &claim_only,
        &[
            "--root",
            ".",
            "scap",
            &fixtures.join("arf.xml").display().to_string(),
            "--datastream",
            &datastream.display().to_string(),
        ],
    );
    assert!(
        code == 2,
        "explicit datastream did not evaluate claims: {said}"
    );
    assert!(report.contains("| one/hello | cis-fedora | 1.1.1.1 | pass |"));

    let mut out = String::new();
    for (heading, arf) in [
        ("the first scan, which is the baseline", "arf.xml"),
        ("and one where a rule stopped passing", "arf-regressed.xml"),
    ] {
        let (report, said, code) = scap_run(
            &root,
            &[
                "--root",
                ".",
                "scap",
                &fixtures.join(arf).display().to_string(),
                "--datastream",
                &datastream.display().to_string(),
                "--baseline",
                &baseline.display().to_string(),
            ],
        );
        out.push_str(&format!("==== {heading}\n{report}{said}==== exit {code}\n"));
    }

    let (tailoring, said, code) = scap_run(
        &root,
        &[
            "--root",
            ".",
            "scap",
            "tailoring",
            "--datastream",
            &datastream.display().to_string(),
        ],
    );
    out.push_str(&format!(
        "==== the tailoring\n{tailoring}{said}==== exit {code}\n"
    ));

    let base = fixtures.join("base.json");
    let against_base = [
        "--root",
        ".",
        "scap",
        &fixtures.join("arf.xml").display().to_string(),
        "--datastream",
        &datastream.display().to_string(),
        "--base-scan",
        &base.display().to_string(),
    ]
    .map(String::from);
    let borrowed: Vec<&str> = against_base.iter().map(String::as_str).collect();
    let (report, said, code) = scap_run(&root, &borrowed);
    out.push_str(&format!(
        "==== and against what the bare base passes alone\n{report}{said}==== exit {code}\n"
    ));
    let (_, said, code) = scap_run(&enforced, &borrowed);
    assert!(code == 2, "enforcement did not fail the scan: {said}");
    assert!(
        said.contains("quay.io/fedora/fedora-bootc:44` alone passed this rule"),
        "the base-regression help did not name the base: {said}"
    );

    let image = root.join("example.image.kdl");
    let text = std::fs::read_to_string(&image).unwrap();
    std::fs::write(
        &image,
        text.replace("conforms \"standard\"", "conforms \"nosuch\""),
    )
    .unwrap();
    let (report, said, code) = scap_run(
        &root,
        &[
            "--root",
            ".",
            "scap",
            &fixtures.join("arf.xml").display().to_string(),
            "--datastream",
            &datastream.display().to_string(),
        ],
    );
    out.push_str(&format!(
        "==== conforming to a profile nothing carries\n{}{said}==== exit {code}\n",
        report.split("## Measured").nth(1).unwrap_or_default()
    ));

    let (report, said, code) = scap_run(
        &enforced,
        &[
            "--root",
            ".",
            "scap",
            &fixtures.join("arf.xml").display().to_string(),
            "--datastream",
            &datastream.display().to_string(),
        ],
    );
    assert!(code == 2, "enforcement did not fail the scan: {said}");
    assert!(said.contains("cis-fedora 5.2.20"), "{said}");
    out.push_str(&format!(
        "==== enforced, and the report is the same\n{report}"
    ));

    for (heading, at) in [
        ("the content it is measured with", &enforced),
        (
            "and for an image declaring claims but no profile",
            &claim_only,
        ),
        (
            "and for an image declaring neither a claim nor a profile",
            &crate_dir().join("tests/repos/minimal"),
        ),
    ] {
        let (path, _, code) = scap_run(at, &["--root", ".", "scap", "content"]);
        out.push_str(&format!("==== {heading}\n{path}==== exit {code}\n"));
    }

    compare("scap", "report.txt", &out);
}

#[test]
fn drawn_create_repo() {
    let dir = empty("flow-create-repo-drawn-in");
    drawn_flow(
        "flow-create-repo-drawn",
        &dir,
        &format!(
            "env PATH='{}' HOME='{}' GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null \
             '{}' create repo",
            bin("flow-create-repo-drawn", None).display(),
            tmp().display(),
            env!("CARGO_BIN_EXE_tect")
        ),
        "Creating example...",
        &[
            b"Example\r",
            b"\r",
            b"\r",
            b"someone\r",
            b"\x1b[B\r",
            b"\x1b[B\r",
            b"\r",
            b"\r",
            b"\r",
            b"\r",
            b"\x1b[B\x1b[B\x1b[B\x1b[B\x1b[B\x1b[B\x1b[B\x1b[B\r",
        ],
    );
    assert!(dir.join("example/repo.kdl").is_file());
}
