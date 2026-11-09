use crate::harness::{assert_golden, copy, crate_dir, given, seed_bases, seed_library, walk};

use std::path::{Path, PathBuf};

use buildc::Command;

fn capture(name: &str, root: &Path) {
    let here = given(root);
    for (command, file) in [
        (Command::Plan, "plan.json"),
        (Command::Section, "section.txt"),
        (Command::Summary, "summary.md"),
        (Command::Sbom, "sbom.json"),
    ] {
        assert_golden(name, file, &buildc::run(command, None, &here).report.stdout);
    }
    assert_golden(
        name,
        "issues.txt",
        &buildc::run(Command::Check, None, &here)
            .problems
            .issues
            .plain(),
    );

    let mut generated = String::new();
    let run = buildc::run(Command::Generate, None, &here);
    for (path, body) in &run.files {
        let covered = path == std::path::Path::new("generated/plan.json")
            || path.starts_with("generated/lib")
            || path.starts_with("scripts/")
            || path.starts_with(buildc::layout::WORKFLOW_DIR);
        match covered {
            true => generated.push_str(&format!("==== {}\n", path.display())),
            false => generated.push_str(&format!("==== {}\n{body}", path.display())),
        }
    }
    for copied in &run.copies {
        generated.push_str(&format!("==== {}\n", copied.path.display()));
    }
    assert_golden(name, "generated.txt", &generated);
}

fn init_repo(name: &str) -> PathBuf {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("BUILDC_ASSETS", crate_dir().join("assets"));
    std::env::set_var("GIT_CONFIG_GLOBAL", "/dev/null");
    std::env::set_var("GIT_CONFIG_SYSTEM", "/dev/null");
    seed_bases(&root);
    buildc::create::Repo::collect(
        Some("Example".into()),
        None,
        Some("someone".into()),
        Some("Example".into()),
        Some("quay.io/fedora/fedora-bootc:44".into()),
        Some(root.clone()),
        &common::prompt::Prompt::silent(),
    )
    .unwrap()
    .expect("a silent run draws no review screen")
    .apply()
    .unwrap();
    root
}

fn verify(name: &str, root: &Path) {
    let here = given(root);
    let run = buildc::run(Command::Generate, None, &here);
    for (path, body) in &run.files {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().expect("a file under generated/")).unwrap();
        std::fs::write(path, body).unwrap();
    }
    let issues = || {
        buildc::run(Command::Verify, None, &here)
            .problems
            .issues
            .plain()
    };

    let mut out = format!("==== as generated\n{}", issues());

    let manifest = root.join("generated/plan.json");
    let edited = std::fs::read_to_string(&manifest)
        .unwrap()
        .replace("\"schema_version\": 1", "\"schema_version\": 0");
    std::fs::write(&manifest, edited).unwrap();
    out.push_str(&format!("==== edited by hand\n{}", issues()));

    std::fs::remove_file(&manifest).unwrap();
    std::fs::write(root.join("generated/leftover"), "").unwrap();
    out.push_str(&format!("==== gone, and one nobody claims\n{}", issues()));

    let dropped = root.join(".github/workflows/smoke-test.yml");
    let repo = root.join("repo.kdl");
    let text = std::fs::read_to_string(&repo).unwrap();
    std::fs::write(&repo, text.replace("    smoke-test\n", "")).unwrap();
    out.push_str(&format!("==== one workflow undeclared\n{}", issues()));

    let run = buildc::run(Command::Generate, None, &here);
    buildc::write_generated(&here, &run.files, &run.copies).unwrap();
    out.push_str(&format!(
        "==== and generated again\n{}smoke-test.yml is {}\n",
        issues(),
        match dropped.exists() {
            true => "still there",
            false => "gone",
        }
    ));

    std::fs::remove_dir_all(root.join("generated")).unwrap();
    out.push_str(&format!("==== never generated\n{}", issues()));

    assert_golden(name, "verify.txt", &out);
}

fn create_into(name: &str, root: &Path) {
    let here = given(root);
    let silent = common::prompt::Prompt::silent();

    buildc::create::Image::collect(
        buildc::create::Home {
            root: &here,
            repo: "example",
            url: None,
            sources_text: "",
        },
        Some("Server".into()),
        Some("quay.io/fedora/fedora-bootc:44".into()),
        "a name argument",
        buildc::setup::Field::Image,
        None,
        &silent,
    )
    .unwrap()
    .apply(&here)
    .unwrap();
    buildc::create::Component::collect(
        &here,
        Some("My Editor".into()),
        vec!["nano".into()],
        vec![("provides".into(), "editor".into())],
        vec!["example".into(), "server".into()],
        &silent,
    )
    .unwrap()
    .apply(&here)
    .unwrap();
    buildc::create::Component::collect(
        &here,
        Some("plain".into()),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        &silent,
    )
    .unwrap()
    .apply(&here)
    .unwrap();

    let mut out = String::new();
    for file in [
        "components/my-editor/manifest.kdl",
        "components/plain/manifest.kdl",
        "example.image.kdl",
        "server.image.kdl",
    ] {
        out.push_str(&format!(
            "==== {file}\n{}",
            std::fs::read_to_string(root.join(file)).unwrap()
        ));
    }
    out.push_str(&format!(
        "==== check\n{}",
        buildc::run(Command::Check, None, &here)
            .problems
            .issues
            .plain()
    ));
    assert_golden(name, "create.txt", &out);
}

fn copied(name: &str, root: &Path) {
    let mut out = String::new();
    let collections = crate_dir().join("tests/collections");
    std::fs::write(
        root.join("repo.kdl"),
        format!(
            "schema-version 1\nname \"Imported\"\nsources {{\n    components \"upstream\" {{ dir {:?} }}\n    components \"community\" {{ dir {:?} }}\n}}\n",
            collections.join("upstream").display(),
            collections.join("community").display()
        ),
    )
    .unwrap();
    let here = given(root);

    let (list, issues, _) = buildc::declarations(&here);
    let sources = list.sources;
    out.push_str(&format!("==== the registry\n{}", issues.plain()));
    for collection in &sources {
        out.push_str(&format!("{}\n", collection.name));
    }

    out.push_str("==== the catalog\n");
    for component in buildc::provider::catalog(&here, &sources, true).unwrap() {
        out.push_str(&format!(
            "{}  {}\n",
            component.qualified(),
            component.about()
        ));
    }

    for wanted in [
        "flatpak",
        "browser",
        "upstream/browser",
        "community/browser",
        "nosuch",
        "upstream/nosuch",
        "flatpak",
    ] {
        out.push_str(&format!("==== import {wanted}\n"));
        let imported = buildc::import::Component::collect(
            buildc::import::Request {
                name: Some(wanted.to_string()),
                image: None,
                datastream: None,
                dependencies: false,
            },
            &here,
            &sources,
            false,
            &common::prompt::Prompt::silent(),
            &mut |_| {},
        )
        .map_err(|err| err.message().to_string())
        .and_then(|component| component.apply(&here, &sources));
        match imported {
            Ok(()) => out.push_str("copied\n"),
            Err(message) => out.push_str(&format!("{message}\n")),
        }
    }

    out.push_str("==== the tree\n");
    let mut written: Vec<String> = walk(&root.join("components"))
        .iter()
        .map(|p| format!("{}\n", p.strip_prefix(root).unwrap().display()))
        .collect();
    written.sort();
    out.extend(written);

    out.push_str(&format!(
        "==== the lock\n{}",
        std::fs::read_to_string(root.join("components/components.lock.kdl")).unwrap()
    ));
    out.push_str(&format!(
        "==== check\n{}",
        buildc::run(Command::Check, None, &here)
            .problems
            .issues
            .plain()
    ));

    std::fs::write(
        root.join("components/flatpak/component.sh"),
        "echo forked\n",
    )
    .unwrap();
    out.push_str(&format!(
        "==== check after an edit\n{}",
        buildc::run(Command::Check, None, &here)
            .problems
            .issues
            .plain()
    ));
    assert_golden(name, "import.txt", &out);
}

fn edited_component(root: &Path) {
    let here = given(root);
    let run = buildc::run(Command::Generate, None, &here);
    for (path, body) in &run.files {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().expect("a file under generated/")).unwrap();
        std::fs::write(path, body).unwrap();
    }
    for copied in &run.copies {
        let path = root.join(&copied.path);
        std::fs::create_dir_all(path.parent().expect("a file under generated/")).unwrap();
        std::fs::write(&path, &copied.bytes).unwrap();
        std::fs::set_permissions(
            &path,
            std::os::unix::fs::PermissionsExt::from_mode(copied.mode & 0o7777),
        )
        .unwrap();
    }
    let issues = || {
        buildc::run(Command::Verify, None, &here)
            .problems
            .issues
            .plain()
    };
    let clean = issues();
    assert!(
        clean.is_empty(),
        "verify was not green to begin with: {clean}"
    );

    std::fs::write(
        root.join("components/my-editor/component.sh"),
        "echo edited\n",
    )
    .unwrap();
    let dirty = issues();
    assert!(
        dirty.contains("generated/plan.json"),
        "an edited component left plan.json current: {dirty}"
    );
}

fn why(name: &str, root: &Path, component: &str) {
    let here = given(root);
    let mut out = String::new();

    for (command, heading) in [(Command::Why, "markdown"), (Command::WhyJson, "json")] {
        out.push_str(&format!("==== {heading}\n"));
        out.push_str(
            &buildc::run(command, component.rsplit('/').next(), &here)
                .report
                .stdout,
        );
    }

    out.push_str("==== unknown\n");
    out.push_str(
        &buildc::run(Command::Why, Some("nosuch"), &here)
            .problems
            .issues
            .plain(),
    );

    let manifest =
        common::json::Json::parse(&buildc::run(Command::Plan, None, &here).report.stdout)
            .expect("the plan is a document");
    let host =
        buildc::emit::why::on_host(&manifest, None, component).expect("the manifest names it");
    out.push_str("==== from the baked manifest, with no repository\n");
    out.push_str(&host.markdown());
    out.push_str(&format!(
        "==== the names it knows\n{}\n",
        buildc::emit::why::display(&buildc::emit::why::known_on_host(&manifest, None)).join(", ")
    ));

    let record = common::json::Json::parse(&format!(
        "{{\"components\": [{{\"path\": {component:?}, \"content\": \"not what was declared\"}}]}}"
    ))
    .expect("the record is a document");
    let observed = buildc::emit::why::on_host(&manifest, Some(&record), component)
        .expect("the manifest names it");
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

    assert_golden(name, "why.txt", &out);
}

fn summary_on_host(root: &Path) {
    let here = given(root);
    let manifest =
        common::json::Json::parse(&buildc::run(Command::Plan, None, &here).report.stdout)
            .expect("the plan is a document");

    let (every, _) = buildc::emit::why::built_as(&manifest, None);
    assert!(!every.is_empty(), "{} publishes nothing", root.display());
    for target in every {
        let record = common::json::Json::parse(&format!(
            "{{\"target\": {:?}}}",
            common::json::text(target, "name").expect("a target is named")
        ))
        .expect("the record is a document");
        let (scoped, _) = buildc::emit::why::built_as(&manifest, Some(&record));
        let [scoped] = scoped.as_slice() else {
            panic!("the record names exactly one target");
        };
        assert_eq!(
            buildc::run(
                Command::Summary,
                common::json::text(target, "name").as_deref(),
                &here
            )
            .report
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
    let here = given(&temp);
    let mut out = String::new();

    out.push_str("==== a component the base suppresses\n");
    let bare = buildc::run(Command::Why, Some("flatpak"), &here)
        .report
        .stdout;
    out.push_str(&bare);

    let full = buildc::run(Command::Why, Some("apps/flatpak"), &here)
        .report
        .stdout;
    assert_eq!(bare, full, "the full path is a name like any other");

    std::fs::write(
        temp.join("also.image.kdl"),
        "schema-version 1\n\nimage {\n    name \"Also\"\n\n    base-image {\n        base \"ghcr.io/ublue-os/bazzite:stable\" { family \"fedora\"; }\n    }\n\n    apps {\n        flatpak\n    }\n}\n",
    )
    .unwrap();
    out.push_str("==== and the same component in an image that does build it\n");
    out.push_str(
        buildc::run(Command::Why, Some("flatpak"), &here)
            .report
            .stdout
            .split("## What it exchanges")
            .next()
            .unwrap_or_default(),
    );
    std::fs::remove_file(temp.join("also.image.kdl")).unwrap();

    std::fs::remove_dir_all(temp.join("components/apps/flatpak")).unwrap();
    out.push_str("==== and one whose manifest never loaded\n");
    let run = buildc::run(Command::Why, Some("flatpak"), &here);
    out.push_str(
        run.problems
            .issues
            .plain()
            .split("`apps/flatpak` is listed")
            .nth(1)
            .map(|rest| format!("`apps/flatpak` is listed{rest}"))
            .expect("the unread component is reported")
            .as_str(),
    );

    assert_golden("suppressed", "why.txt", &out);
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

    let here = given(&temp);
    let issues = buildc::run(Command::Check, None, &here)
        .problems
        .issues
        .plain();
    assert!(
        issues.is_empty(),
        "the same repository has to check clean unenforced: {issues}"
    );
}

fn unpinned_import(root: &Path) {
    let here = given(root);
    let (list, _, _) = buildc::declarations(&here);
    assert!(
        list.sources.iter().any(|c| c.unpinned()),
        "the fixture has to declare an unpinned collection"
    );
    let refused = buildc::import::find(&here, &list.sources, "anything", true)
        .err()
        .expect("enforcement refuses an unpinned collection");
    assert!(refused.contains("follows a moving ref"), "{refused}");
}

fn refused_everywhere(root: &Path) {
    let here = given(root);
    let checked = buildc::run(Command::Check, None, &here)
        .problems
        .issues
        .plain();
    assert!(checked.contains("tracks its library"), "{checked}");
    for command in [Command::Generate, Command::Verify] {
        let said = buildc::run(command, None, &here).problems.issues.plain();
        assert!(said.starts_with(&checked), "{command:?}: {said}");
    }
}

fn assert_no_default(root: &Path) {
    let here = given(root);
    for quiet in [Command::Check, Command::Generate] {
        let issues = buildc::run(quiet, None, &here).problems.issues.plain();
        assert!(issues.is_empty(), "{quiet:?} reported {issues}");
    }
    for quiet in [Command::Section, Command::Graph] {
        let issues = buildc::run(quiet, Some("server"), &here)
            .problems
            .issues
            .plain();
        assert!(issues.is_empty(), "{quiet:?} reported {issues}");
    }
    for loud in [
        Command::Plan,
        Command::Section,
        Command::Graph,
        Command::Summary,
        Command::Sbom,
    ] {
        let issues = buildc::run(loud, None, &here).problems.issues.plain();
        assert!(
            issues.contains("2 images are declared and none is the default"),
            "{loud:?} reported {issues}"
        );
    }
}

fn repo_case(name: &str) {
    let dir = crate_dir().join("tests/repos");
    let root = dir.join(name);
    capture(name, &root);
    match name {
        "collecting" | "multi-image" => summary_on_host(&root),
        "enforced" => {
            unenforced(&root);
            why(name, &root, "one/hello");
        }
        "minimal" => {
            why(name, &root, "core/hello");
            summary_on_host(&root);
        }
        "broken-lock" => refused_everywhere(&root),
        "no-default" => assert_no_default(&root),
        "seeding" => why(name, &root, "flatpak"),
        "suppressed" => {
            why_unbuilt(&root);
            summary_on_host(&root);
        }
        "unpinned-source" => unpinned_import(&root),
        _ => {}
    }
}

macro_rules! repo_cases {
    ($($test:ident => $fixture:literal),+ $(,)?) => {
        const REPO_CASES: &[&str] = &[$($fixture),+];

        $(
            #[test]
            fn $test() {
                repo_case($fixture);
            }
        )+
    };
}

repo_cases! {
    bare_component => "bare-component",
    broken_asset => "broken-asset",
    broken_bootc => "broken-bootc",
    broken_collect => "broken-collect",
    broken_cycle => "broken-cycle",
    broken_fetch => "broken-fetch",
    broken_flavours => "broken-flavours",
    broken_fragment => "broken-fragment",
    broken_graph => "broken-graph",
    broken_image => "broken-image",
    broken_key => "broken-key",
    broken_layout => "broken-layout",
    broken_lock => "broken-lock",
    broken_component => "broken-component",
    broken_network => "broken-network",
    broken_options => "broken-options",
    broken_overlay => "broken-overlay",
    broken_pin => "broken-pin",
    broken_root => "broken-root",
    broken_seed => "broken-seed",
    broken_sources => "broken-sources",
    broken_syntax => "broken-syntax",
    collecting => "collecting",
    deb_families => "deb-families",
    enforced => "enforced",
    future_schema => "future-schema",
    media_recipe => "media-recipe",
    minimal => "minimal",
    multi_image => "multi-image",
    network => "network",
    no_default => "no-default",
    no_image => "no-image",
    non_bootc => "non-bootc",
    one_file => "one-file",
    optional_metadata => "optional-metadata",
    past_schema => "past-schema",
    refused => "refused",
    renovate => "renovate",
    seeding => "seeding",
    signing => "signing",
    suppressed => "suppressed",
    uki => "uki",
    unmeasured_family => "unmeasured-family",
    unpinned_source => "unpinned-source",
    versioned => "versioned",
    wrong_pin => "wrong-pin",
}

#[test]
fn every_repo_fixture_has_a_case() {
    let mut found: Vec<String> = std::fs::read_dir(crate_dir().join("tests/repos"))
        .expect("tests/repos exists")
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    found.sort();
    let mut expected = REPO_CASES.to_vec();
    expected.sort();
    assert_eq!(
        found, expected,
        "each repository fixture needs a named test"
    );
}

#[test]
fn the_library_cache_changes_no_output() {
    let root = init_repo("cache-blind");
    seed_library(&root);
    let run = || {
        (
            buildc::run(Command::Check, None, &root)
                .problems
                .issues
                .plain(),
            buildc::run(Command::Generate, None, &root).files,
        )
    };
    let cached = run();
    std::fs::remove_dir_all(root.join(buildc::layout::SOURCES_CACHE)).unwrap();
    assert_eq!(run(), cached);
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn init() {
    let init = init_repo("init");
    assert!(
        !init.join("lib").exists(),
        "lib is generated; it must not land in the editable scaffold"
    );
    for script in ["buildc.sh", "lint.sh", "vm.sh", "media.sh"] {
        assert!(
            !init.join("scripts").join(script).exists(),
            "{script} is generated; it must not land in the editable scaffold"
        );
    }
    capture("init", &init);
    verify("init", &init);
}

#[test]
fn create() {
    let created = init_repo("create");
    create_into("create", &created);
    edited_component(&created);
}

#[test]
fn copy_component() {
    copied("copy", &init_repo("copy"));
}

#[test]
fn a_strict_script_rule_names_the_fragments_it_does_not_reach() {
    let root = crate_dir().join("tests/repos/network");
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_buildc"))
        .arg("check")
        .current_dir(&root)
        .env("BUILDC_ASSETS", crate_dir().join("assets"))
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("`core/spliced` ships a Containerfile fragment"),
        "{stderr}"
    );
}
