use std::path::{Path, PathBuf};

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn compare(name: &str, file: &str, actual: &str) {
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
        ("plan", "plan.json"),
        ("section", "section.txt"),
        ("summary", "summary.md"),
        ("sbom", "sbom.json"),
    ] {
        compare(name, file, &buildc::run(command, None, here).stdout);
    }
    compare(
        name,
        "issues.txt",
        &buildc::run("check", None, here).issues.plain(),
    );

    let mut generated = String::new();
    for (path, body) in &buildc::run("generate", None, here).files {
        generated.push_str(&format!("==== {}\n{body}", path.display()));
    }
    compare(name, "generated.txt", &generated);
}

fn init_repo(name: &str) -> PathBuf {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("BUILDC_ASSETS", crate_dir().join("assets"));
    buildc::create::repo(
        Some("Example".into()),
        Some("someone".into()),
        Some("Example".into()),
        None,
        Some(root.clone()),
        &buildc::prompt::Prompt::silent(),
    )
    .unwrap();
    root
}

fn verify(name: &str, root: &Path) {
    std::env::set_current_dir(root).expect("fixture root exists");
    let here = Path::new(".");
    let run = buildc::run("generate", None, here);
    for (path, body) in &run.files {
        std::fs::create_dir_all(path.parent().expect("a file under generated/")).unwrap();
        std::fs::write(path, body).unwrap();
    }
    let issues = || buildc::run("verify", None, here).issues.plain();

    let mut out = format!("==== as generated\n{}", issues());

    let containerfile = &run.files[0].0;
    let edited = std::fs::read_to_string(containerfile)
        .unwrap()
        .replace("ARG IMAGE_VERSION=dev", "ARG IMAGE_VERSION=by-hand");
    std::fs::write(containerfile, edited).unwrap();
    out.push_str(&format!("==== edited by hand\n{}", issues()));

    std::fs::remove_file(containerfile).unwrap();
    std::fs::write("generated/leftover", "").unwrap();
    out.push_str(&format!("==== gone, and one nobody claims\n{}", issues()));

    compare(name, "verify.txt", &out);
}

fn create(name: &str, root: &Path) {
    std::env::set_current_dir(root).expect("the created-into repository exists");
    let here = Path::new(".");
    let silent = buildc::prompt::Prompt::silent();

    buildc::create::module(
        here,
        Some("My Editor".into()),
        vec!["nano".into()],
        vec![("provides".into(), "editor".into())],
        Some("example".into()),
        &silent,
    )
    .unwrap();
    buildc::create::module(
        here,
        Some("plain".into()),
        Vec::new(),
        Vec::new(),
        None,
        &silent,
    )
    .unwrap();

    let mut out = String::new();
    for file in [
        "modules/my-editor/module.kdl",
        "modules/plain/module.kdl",
        "example.kdl",
    ] {
        out.push_str(&format!(
            "==== {file}\n{}",
            std::fs::read_to_string(file).unwrap()
        ));
    }
    out.push_str(&format!(
        "==== check\n{}",
        buildc::run("check", None, here).issues.plain()
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

    let (sources, issues, _) = buildc::sources(here);
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
        match buildc::import::find(here, &sources, wanted) {
            Err(message) => out.push_str(&format!("{message}\n")),
            Ok(found) if found.len() > 1 => {
                let owners: Vec<&str> = found.iter().map(|f| f.owner.as_str()).collect();
                out.push_str(&format!("ambiguous: {}\n", owners.join(", ")));
            }
            Ok(found) => match buildc::import::vendor(here, &found[0], module) {
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
        "==== check\n{}",
        buildc::run("check", None, here).issues.plain()
    ));
    compare(name, "import.txt", &out);
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
    for quiet in ["check", "generate"] {
        let issues = buildc::run(quiet, None, here).issues.plain();
        assert!(issues.is_empty(), "{quiet} reported {issues}");
    }
    for quiet in [("section", "server"), ("graph", "server")] {
        let issues = buildc::run(quiet.0, Some(quiet.1), here).issues.plain();
        assert!(issues.is_empty(), "{} reported {issues}", quiet.0);
    }
    for loud in ["plan", "section", "graph", "summary", "sbom"] {
        let issues = buildc::run(loud, None, here).issues.plain();
        assert!(
            issues.contains("2 images are declared and none is the default"),
            "{loud} reported {issues}"
        );
    }
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
    for name in names {
        capture(&name, &dir.join(&name));
    }
    no_default(&dir.join("no-default"));
    capture("init", &init);
    verify("init", &init);
    create("create", &init_repo("create"));
    import("import", &init_repo("import"));
}
