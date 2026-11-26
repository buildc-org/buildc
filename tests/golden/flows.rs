use crate::harness::{assert_golden, contents, copy, crate_dir, seed_bases, seed_library, tmp};

use std::cell::Cell;

use std::path::{Path, PathBuf};

use buildc::Command;

struct Flow {
    target: &'static str,
    expected: usize,
    asserted: Cell<usize>,
}

impl Flow {
    fn new(target: &'static str) -> Self {
        let expected = match target {
            name if name.starts_with("flow-") => 1,
            _ => 0,
        };
        Self {
            target,
            expected,
            asserted: Cell::new(0),
        }
    }

    fn done(&self) {
        assert_eq!(
            self.asserted.get(),
            self.expected,
            "{} must assert each of its snapshots",
            self.target
        );
    }

    fn at(&self, name: &str) -> PathBuf {
        tmp().join(format!("{}--{name}", self.target))
    }

    fn note_assertion(&self) {
        self.asserted.set(self.asserted.get() + 1);
    }

    fn bin(&self, name: &str, gh: Option<&str>) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let dir = self.at(&format!("{name}-bin"));
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

    fn run(&self, name: &str, dir: &Path, gh: Option<&str>, args: &[&str]) {
        self.run_with_assets(name, dir, gh, args, &crate_dir().join("assets"));
    }

    fn run_silent(&self, name: &str, dir: &Path, args: &[&str]) {
        let log = self.at(&format!("{name}.log"));
        let file = std::fs::File::create(&log).unwrap();
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_buildc"));
        let status = sealed(&mut command, &self.bin(name, None))
            .env("BUILDC_ASSETS", crate_dir().join("assets"))
            .args(args)
            .current_dir(dir)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::from(file.try_clone().unwrap()))
            .stderr(std::process::Stdio::from(file))
            .status()
            .unwrap();
        let transcript = format!(
            "{}==== exit {}\n",
            std::fs::read_to_string(&log).unwrap(),
            status.code().unwrap_or_default()
        );
        if name == self.target {
            self.note_assertion();
            assert_golden(name, "transcript.txt", &transcript);
        }
    }

    fn run_with_assets(
        &self,
        name: &str,
        dir: &Path,
        gh: Option<&str>,
        args: &[&str],
        assets: &Path,
    ) {
        let fixture = crate_dir().join("tests/answers").join(name);
        let log = self.at(&format!("{name}.log"));
        let file = std::fs::File::create(&log).unwrap();
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_buildc"));
        let status = sealed(&mut command, &self.bin(name, gh))
            .env("BUILDC_ASSETS", assets)
            .args(args)
            .current_dir(dir)
            .env("BUILDC_ANSWERS", answers(&fixture))
            .stdout(std::process::Stdio::from(file.try_clone().unwrap()))
            .stderr(std::process::Stdio::from(file))
            .status()
            .unwrap();
        let transcript = format!(
            "{}==== exit {}\n",
            std::fs::read_to_string(&log).unwrap(),
            status.code().unwrap_or_default()
        );
        if name == self.target {
            self.note_assertion();
            assert_golden(name, "transcript.txt", &transcript);
        }
    }
    fn offering(&self, name: &str, dir: &Path) {
        use std::os::unix::fs::PermissionsExt;
        let path = self.bin(name, None);

        let fixture = crate_dir().join("tests/answers").join(name);
        let log = self.at(&format!("{name}.log"));
        let root = dir.join("example");
        seed_library(&root);
        let file = std::fs::File::create(&log).unwrap();
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_buildc"));
        let status = command
            .env("PATH", &path)
            .env("HOME", tmp())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("BUILDC_ASSETS", crate_dir().join("assets"))
            .env("BUILDC_ANSWERS", answers(&fixture))
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
        if name == self.target {
            self.note_assertion();
            assert_golden(name, "transcript.txt", &transcript);
        }

        let image = std::fs::read_to_string(root.join("example.kdl")).unwrap();
        assert!(!image.contains("fedora-family"), "{image}");
        assert!(!root.join("components/fedora-family").exists());
    }
    fn buildc(&self, dir: &Path, args: &[&str]) {
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_buildc"));
        let out = sealed(&mut command, &self.bin("buildc", None))
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

    fn empty(&self, name: &str) -> PathBuf {
        let dir = self.at(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn repo(&self, name: &str) -> PathBuf {
        let dir = self.empty(name);
        let root = dir.join("example");
        seed_bases(&root);
        self.buildc(
            &dir,
            &[
                "--no-tui",
                "create",
                "repo",
                "Example",
                "--owner",
                "someone",
                "--image",
                "Example",
                "--base",
                "quay.io/fedora/fedora-bootc:44",
            ],
        );
        root
    }

    fn repo_two(&self, name: &str) -> PathBuf {
        let root = self.repo(name);
        self.buildc(
            &root,
            &["--no-tui", "--root", ".", "create", "image", "Server"],
        );
        root
    }

    fn repo_sourced(&self, name: &str) -> PathBuf {
        let root = self.repo(name);
        let collections = crate_dir().join("tests/collections");
        let mut repo = std::fs::read_to_string(root.join("repo.kdl"))
            .unwrap()
            .replace(&scaffolded_sources(), "");
        repo.push_str(&local_libraries(
            &root,
            &[
                ("upstream", &collections.join("upstream")),
                ("community", &collections.join("community")),
            ],
        ));
        std::fs::write(root.join("repo.kdl"), repo).unwrap();
        root
    }

    fn repo_with(&self, name: &str, collection: &str) -> PathBuf {
        let root = self.repo(name);
        let mut repo = std::fs::read_to_string(root.join("repo.kdl"))
            .unwrap()
            .replace(&scaffolded_sources(), "");
        repo.push_str(&local_libraries(
            &root,
            &[(
                collection,
                &crate_dir().join("tests/collections").join(collection),
            )],
        ));
        std::fs::write(root.join("repo.kdl"), repo).unwrap();
        let upstream = crate_dir().join("tests/collections/upstream");
        for held in ["base-image/fedora", "fedora-family"] {
            copy(&upstream.join(held), &root.join("components").join(held));
        }
        root
    }

    fn repo_claiming(&self, name: &str) -> PathBuf {
        self.repo_with(name, "claims")
    }

    fn repo_enforcing(&self, name: &str) -> PathBuf {
        let root = self.repo_claiming(name);
        let file = root.join("repo.kdl");
        let mut repo = std::fs::read_to_string(&file).unwrap();
        repo.push_str("\naudit {\n    enforce #true\n}\n");
        std::fs::write(&file, repo).unwrap();
        root
    }
}

const SIGNED_OUT: &str = "#!/bin/sh\ntest \"$1\" = auth && exit 1\nexit 0\n";

const SIGNED_IN: &str =
    "#!/bin/sh\ntest \"$1\" = auth && { echo \"account someone\"; exit 0; }\nexit 0\n";

fn answers(fixture: &Path) -> PathBuf {
    let file = fixture.join("answers.txt");
    match file.exists() {
        true => file,
        false => PathBuf::from("/dev/null"),
    }
}

fn sealed<'a>(
    command: &'a mut std::process::Command,
    path: &Path,
) -> &'a mut std::process::Command {
    command
        .env("PATH", path)
        .env("HOME", tmp())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_ALLOW_PROTOCOL", "file")
        .env("BUILDC_ASSETS", crate_dir().join("assets"))
}

fn drawn_run(
    dir: &Path,
    command: &str,
    steps: &[(&[u8], &[u8])],
) -> (std::process::ExitStatus, String, Vec<u8>) {
    use std::io::{Read, Write};
    use std::process::Stdio;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    let mut child = std::process::Command::new("script")
        .args(["-qfec", command, "/dev/null"])
        .current_dir(dir)
        .env("BUILDC_ASSETS", crate_dir().join("assets"))
        .env("COLUMNS", "80")
        .env_remove("NO_COLOR")
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

                // The test answers the cursor-position request because a pseudo-terminal has no terminal to answer it.
                if held.ends_with(b"\x1b[6n") {
                    let mut input = input.lock().unwrap();
                    let _ = input.write_all(b"\x1b[1;1R");
                    let _ = input.flush();
                }
            }
        })
    };
    for (marker, keys) in steps {
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        loop {
            let seen = raw
                .lock()
                .unwrap()
                .windows(marker.len())
                .any(|window| window == *marker);
            if seen {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the terminal did not draw {:?}: {}",
                String::from_utf8_lossy(marker),
                String::from_utf8_lossy(&raw.lock().unwrap())
            );
            std::thread::sleep(Duration::from_millis(10));
        }
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
    (status, errors, raw.clone())
}

fn listing(text: &str, segments: &str) -> String {
    format!("{text}{segments}")
}

fn lists_sshd(root: &Path) {
    let file = root.join("example.kdl");
    let listed = listing(&std::fs::read_to_string(&file).unwrap(), "sshd\n");
    assert!(
        buildc::parse::image::listed(&listed, "sshd") == 1,
        "{listed}"
    );
    std::fs::write(file, listed).unwrap();
}

const CLAIMANT: &str =
    "schema-version 1\n\ndescription \"SSH daemon hardening\"\n\ntargets \"fedora\"\n";

const KEYHOLDER: &str = r#"schema-version 1

description "Signs the modules it builds"

targets "fedora"

key "secureboot" {
    generator "openssl" profile="module-signing" bits=4096
    public "/usr/share/secureboot/sb_cert.der" format="der"
    private "MOK.priv"
}
"#;
fn written(file: &Path) -> Vec<String> {
    fn walk(nodes: &[kdl::KdlNode], prefix: &str, out: &mut Vec<String>) {
        for node in nodes {
            let path = match prefix.is_empty() {
                true => node.name().value().to_string(),
                false => format!("{prefix}/{}", node.name().value()),
            };
            if let Some(children) = node.children() {
                walk(children.nodes(), &path, out);
            }
            out.push(path);
        }
    }
    let text =
        std::fs::read_to_string(file).unwrap_or_else(|err| panic!("{}: {err}", file.display()));
    let doc: kdl::KdlDocument = text.parse().expect("the scaffold is KDL");
    let mut out = Vec::new();
    walk(doc.nodes(), "", &mut out);
    out
}

fn matches(pattern: &str, path: &str) -> bool {
    let path: Vec<&str> = path.split('/').collect();
    for (at, part) in pattern.split('/').enumerate() {
        if part == "**" {
            return path.len() >= at;
        }
        if path.get(at).is_none_or(|seg| part != "*" && part != *seg) {
            return false;
        }
    }
    pattern.split('/').count() == path.len()
}

fn scaffolded(area: buildc::emit::schema_md::Area, file: &Path) {
    let written = written(file);
    let marked = buildc::emit::schema_md::scaffold(area);
    for path in &written {
        assert!(
            marked.iter().any(|pattern| matches(pattern, path)),
            "{}: `buildc create` writes `{path}`, which the grammar does not mark as scaffolded",
            file.display()
        );
    }
    for pattern in &marked {
        assert!(
            written.iter().any(|path| matches(pattern, path)),
            "{}: the grammar marks `{pattern}` as scaffolded, and `buildc create` does not write it",
            file.display()
        );
    }
}

fn known(area: buildc::emit::schema_md::Area, file: &Path) {
    let grammar = buildc::emit::schema_md::grammar(area);
    for path in written(file) {
        assert!(
            grammar.iter().any(|pattern| matches(pattern, &path)),
            "{}: `buildc create` writes `{path}`, which the grammar does not hold",
            file.display()
        );
    }
}

fn flow_case(flow: &Flow) {
    match flow.target {
        "flow-create-repo"
        | "flow-create-repo-no-gh"
        | "flow-create-repo-signed-out"
        | "flow-create-repo-signed-in"
        | "flow-create-repo-forgejo"
        | "flow-image-default"
        | "flow-create-repo-minimal" => setup_create_repo(flow),
        "flow-create-repo-offer" => setup_create_repo_offer(flow),
        "flow-create-image" => setup_create_image(flow),
        "flow-check-unpinned" => setup_check_unpinned(flow),
        "flow-check-conforms" | "flow-check-claims" | "flow-coverage" => setup_conforms(flow),
        "flow-check-conforms-stranger" => setup_check_conforms_stranger(flow),
        "flow-create-component" | "flow-component-taken" | "flow-unanswered" => {
            setup_component(flow)
        }
        "flow-add-package" => setup_add_package(flow),
        "flow-component-two-images" => setup_component_two_images(flow),
        "flow-set-conforms" | "flow-set-conforms-again" => setup_set_conforms(flow),
        "flow-set-conforms-enforced" => setup_set_conforms_enforced(flow),
        "flow-set-conforms-copy" => setup_set_conforms_copy(flow),
        "flow-set-conforms-copy-cancelled" => setup_set_conforms_copy_cancelled(flow),
        "flow-set-library-named" => setup_set_library_named(flow),
        "flow-set-conforms-pinned" => setup_set_conforms_pinned(flow),
        "flow-add-component-declined" => setup_add_component_declined(flow),
        "flow-add-component-family" => setup_add_component_family(flow),
        "flow-set-claims" | "flow-set-claims-again" => setup_set_claims(flow),
        "flow-set-claims-two" => setup_set_claims_two(flow),
        "flow-set-workflows" => setup_workflows(flow),
        "flow-add-component-requires" => setup_add_component_requires(flow),
        "flow-add-component-requires-no-tui" => setup_add_component_requires_no_tui(flow),
        "flow-add-component-requires-no-terminal" => setup_add_component_requires_no_terminal(flow),
        "flow-add-component-requires-namesakes" => setup_add_component_requires_namesakes(flow),
        "flow-add-component-dependencies" => setup_add_component_dependencies(flow),
        "flow-check-unmet" => setup_check_unmet(flow),
        "flow-add-component-skip" => setup_add_component_skip(flow),
        "flow-check-unfetched" => setup_check_unfetched(flow),
        "flow-add-component" => setup_add_component(flow),
        "flow-add-component-exclusive" | "flow-add-component-exclusive-no-tui" => {
            setup_add_component_exclusive(flow)
        }
        "flow-add-component-unavailable" => setup_add_component_unavailable(flow),
        "flow-add-component-default" | "flow-create-component-with-schema-issues" => {
            setup_add_component_default(flow)
        }
        "import-edit-guards" => setup_import_edit_guards(flow),
        "add-component-pin" => setup_add_component_pin(flow),
        "flow-add-component-several" => setup_add_component_several(flow),
        "flow-add-component-nested" => setup_add_component_nested(flow),
        "flow-check-collides" => setup_collides(flow),
        "flow-key-absent" => setup_key_absent(flow),
        "flow-key-kinds" => setup_key_kinds(flow),
        "flow-key-undeclared" => setup_key_undeclared(flow),
        "flow-key-no-kind" => setup_key_no_kind(flow),
        "flow-key-refused" => {
            setup_key_refused(flow, &["--root", ".", "create", "key", "secureboot"])
        }
        "flow-key-refused-no-kind" => setup_key_refused(flow, &["--root", ".", "create", "key"]),
        target => panic!("no flow case for {target}"),
    }
}

fn datastream() -> String {
    crate_dir()
        .join("tests/scap/datastream.xml")
        .display()
        .to_string()
}

// `example-library` names the root library because a `local` block replaces the implicit one. Each test library is copied into `libraries/` because a local library is a folder of the repository.
fn local_libraries(root: &Path, libraries: &[(&str, &Path)]) -> String {
    let mut out = String::from(
        "libraries default=\"example-library\" {\n    local {\n        example-library { source \".\" }\n",
    );
    for (name, source) in libraries {
        copy(source, &root.join("libraries").join(name));
        out.push_str(&format!(
            "        {name} {{ source \"libraries/{name}\" }}\n"
        ));
    }
    out + "    }\n}\n"
}

fn scaffolded_sources() -> String {
    buildc::init::libraries_block(&buildc::setup::Libraries::every(
        &crate_dir().join("assets"),
    ))
}

fn claims(stream: &str) -> [&str; 7] {
    [
        "--root",
        ".",
        "set",
        "claims",
        "sshd",
        "--datastream",
        stream,
    ]
}

fn setup_create_repo(flow: &Flow) {
    let target = flow.target;
    let gh = match target {
        "flow-create-repo-signed-out" => Some(SIGNED_OUT),
        "flow-create-repo-signed-in" => Some(SIGNED_IN),
        _ => None,
    };
    let dir = flow.empty(&format!("{target}-in"));
    seed_bases(&dir.join("example"));
    let mut args = vec!["create", "repo"];
    if target == "flow-create-repo-forgejo" {
        args.extend(["--host", "forge.example.org"]);
    }
    flow.run(target, &dir, gh, &args);
    if target == "flow-create-repo" {
        let created = flow.at("flow-create-repo-in").join("example");
        let repo = std::fs::read_to_string(created.join("repo.kdl")).unwrap();
        assert!(
            repo.contains("base-url \"https://github.com/someone/example\""),
            "{repo}"
        );
        assert!(repo.contains("registry \"ghcr.io/someone\""), "{repo}");
        scaffolded(
            buildc::emit::schema_md::Area::Repo,
            &created.join("repo.kdl"),
        );
        known(
            buildc::emit::schema_md::Area::Component,
            &created.join("example.kdl"),
        );
    }
    if target == "flow-create-repo-minimal" {
        let created = flow.at("flow-create-repo-minimal-in").join("example");
        let repo = std::fs::read_to_string(created.join("repo.kdl")).unwrap();
        assert!(repo.contains("buildc { source "), "{repo}");
        assert!(!repo.contains("workflows"), "{repo}");
        assert!(!repo.contains("runner"), "{repo}");
        assert!(!created.join("example.kdl").exists());
        assert!(!created.join("runners").exists());
    }
}

fn setup_create_repo_offer(flow: &Flow) {
    let target = flow.target;
    flow.offering(target, &flow.empty("flow-create-repo-offer-in"));
}

fn setup_create_image(flow: &Flow) {
    let target = flow.target;
    let root = flow.repo_sourced("flow-image");
    flow.run(target, &root, None, &["--root", ".", "create", "image"]);
    let image = std::fs::read_to_string(root.join("beta.kdl")).unwrap();
    assert!(!image.contains("fedora-family"), "{image}");
    assert!(!root.join("components/fedora-family").exists());
    flow.buildc(&root, &["--no-tui", "--root", ".", "check"]);

    let manifest = root.join("components/fedora-family/component.kdl");
    let fork = "schema-version 1\n\ndescription \"The user's own family\"\n";
    std::fs::create_dir_all(manifest.parent().unwrap()).unwrap();
    std::fs::write(&manifest, fork).unwrap();
    flow.buildc(
        &root,
        &["--no-tui", "--root", ".", "create", "image", "Gamma"],
    );
    assert_eq!(std::fs::read_to_string(&manifest).unwrap(), fork);
    let gamma = std::fs::read_to_string(root.join("gamma.kdl")).unwrap();
    assert!(!gamma.contains("fedora-family"), "{gamma}");
}

fn setup_check_unpinned(flow: &Flow) {
    let target = flow.target;
    flow.run(
        target,
        &flow.repo("flow-unpinned"),
        None,
        &["--root", ".", "check"],
    );
}

fn setup_conforms(flow: &Flow) {
    let target = flow.target;
    let stream = datastream();
    let conforms = flow.repo("flow-conforms");
    std::fs::create_dir_all(conforms.join("components/hardening")).unwrap();
    std::fs::write(
        conforms.join("components/hardening/component.kdl"),
        "schema-version 1\n\ndescription \"Claims a rule the profile selects\"\n\ntargets \"fedora\"\n\n\
             satisfies {\n    cis-fedora \"5.2.20\"\n}\n",
    )
    .unwrap();
    let image = conforms.join("example.kdl");
    let declared = std::fs::read_to_string(&image)
        .unwrap()
        .replace("\nbase-image {", "\nconforms \"standard\"\n\nbase-image {");
    assert!(declared.contains("conforms \"standard\""), "{declared}");
    std::fs::write(&image, declared).unwrap();
    flow.run(
        "flow-check-conforms",
        &conforms,
        None,
        &["--root", ".", "check"],
    );
    if target == "flow-check-conforms" {
        return;
    }
    flow.run(
        "flow-check-claims",
        &conforms,
        None,
        &["--root", ".", "check", "--datastream", &stream],
    );
    if target == "flow-check-claims" {
        return;
    }
    flow.run(
        "flow-coverage",
        &conforms,
        None,
        &["--root", ".", "coverage", "--datastream", &stream],
    );
}

fn setup_check_conforms_stranger(flow: &Flow) {
    let target = flow.target;
    let stream = datastream();
    let stranger = flow.repo("flow-conforms-stranger");
    let image = stranger.join("example.kdl");
    let declared = std::fs::read_to_string(&image).unwrap().replace(
        "\nbase-image {",
        "\nconforms \"cusp_fedora\"\n\nbase-image {",
    );
    std::fs::write(&image, declared).unwrap();
    flow.run(
        target,
        &stranger,
        None,
        &["--root", ".", "check", "--datastream", &stream],
    );
}

fn setup_component(flow: &Flow) {
    let target = flow.target;
    let root = flow.repo("flow-component");
    let component = ["--root", ".", "create", "component"];
    flow.run("flow-create-component", &root, None, &component);
    scaffolded(
        buildc::emit::schema_md::Area::Component,
        &root.join("components/my-editor/component.kdl"),
    );
    if target == "flow-create-component" {
        return;
    }
    if target == "flow-component-taken" {
        flow.run(
            target,
            &root,
            None,
            &[&component[..], &["My Editor"]].concat(),
        );
        return;
    }
    flow.run(target, &root, None, &component);
}

fn setup_component_two_images(flow: &Flow) {
    let target = flow.target;
    flow.run(
        target,
        &flow.repo_two("flow-component-both"),
        None,
        &["--root", ".", "create", "component"],
    );
}

fn setup_add_package(flow: &Flow) {
    let target = flow.target;
    let root = flow.repo("flow-add-package");
    flow.buildc(
        &root,
        &[
            "--no-tui",
            "--root",
            ".",
            "create",
            "component",
            "apps",
            "--inline",
            "--build",
            "example",
            "--with",
            "description=Applications",
        ],
    );
    flow.run(
        target,
        &root,
        None,
        &[
            "--no-tui",
            "--root",
            ".",
            "add",
            "package",
            "firefox",
            "wireguard-tools",
            "--component",
            "apps",
            "--build",
            "example",
        ],
    );
    let image = std::fs::read_to_string(root.join("example.kdl")).unwrap();
    assert!(
        image.contains("    packages \"firefox\" \"wireguard-tools\"\n"),
        "{image}"
    );
    flow.buildc(
        &root,
        &[
            "--no-tui",
            "--root",
            ".",
            "add",
            "package",
            "htop",
            "--component",
            "apps",
            "--build",
            "example",
        ],
    );
    flow.buildc(
        &root,
        &[
            "--no-tui",
            "--root",
            ".",
            "add",
            "package",
            "firefox",
            "--component",
            "apps",
            "--build",
            "example",
        ],
    );
    let image = std::fs::read_to_string(root.join("example.kdl")).unwrap();
    assert!(
        image.contains("    packages \"firefox\" \"wireguard-tools\" \"htop\"\n"),
        "{image}"
    );
    flow.buildc(
        &root,
        &[
            "--no-tui",
            "--root",
            ".",
            "create",
            "component",
            "local-tools",
            "--with",
            "description=Local tools",
        ],
    );
    flow.buildc(
        &root,
        &[
            "--no-tui",
            "--root",
            ".",
            "add",
            "package",
            "nano",
            "--component",
            "local-tools",
        ],
    );
    let manifest =
        std::fs::read_to_string(root.join("components/local-tools/component.kdl")).unwrap();
    assert!(manifest.ends_with("\npackages \"nano\"\n"), "{manifest}");
    let example = std::fs::read_to_string(root.join("example.kdl")).unwrap();
    std::fs::write(root.join("second.kdl"), example).unwrap();
    let repo = std::fs::read_to_string(root.join("repo.kdl")).unwrap();
    std::fs::write(
        root.join("repo.kdl"),
        repo.replacen(
            "builds default=\"example\" {\n",
            "builds default=\"example\" {\n    second \"./second\"\n",
            1,
        ),
    )
    .unwrap();
    flow.buildc(
        &root,
        &[
            "--no-tui",
            "--root",
            ".",
            "add",
            "package",
            "htop",
            "--component",
            "tools",
            "--build",
            "example",
            "--build",
            "second",
        ],
    );
    for file in ["example.kdl", "second.kdl"] {
        let text = std::fs::read_to_string(root.join(file)).unwrap();
        assert!(
            text.contains("\ntools {\n    packages \"htop\"\n}\n"),
            "{file}: {text}"
        );
    }
    flow.buildc(&root, &["--root", ".", "check"]);
    flow.buildc(&root, &["--root", ".", "generate"]);
    for image in ["example", "second"] {
        let script =
            std::fs::read_to_string(root.join(format!("generated/{image}/components/tools.sh")))
                .unwrap();
        assert!(
            script.contains("dnf install -y 'htop'"),
            "{image}: {script}"
        );
    }
}

fn setup_set_conforms(flow: &Flow) {
    let target = flow.target;
    let stream = datastream();
    let measured = flow.repo_claiming("flow-set-conforms-in");
    for name in ["flow-set-conforms", "flow-set-conforms-again"] {
        flow.run(
            name,
            &measured,
            None,
            &["--root", ".", "set", "conforms", "--datastream", &stream],
        );
        if target == "flow-set-conforms" {
            return;
        }
    }
    let declared = std::fs::read_to_string(measured.join("example.kdl")).unwrap();
    assert_eq!(declared.matches("conforms ").count(), 1, "{declared}");
    assert!(
        declared.contains("\nconforms \"ospp\"\n")
            && buildc::parse::image::listed(&declared, "sshd") == 1
            && !measured.join("components/sshd").exists(),
        "{declared}"
    );
}

fn setup_set_conforms_copy(flow: &Flow) {
    let target = flow.target;
    let stream = datastream();
    let copied = flow.repo_claiming("flow-set-conforms-copy-in");
    flow.run(
        target,
        &copied,
        None,
        &["--root", ".", "set", "conforms", "--datastream", &stream],
    );
    let image = std::fs::read_to_string(copied.join("example.kdl")).unwrap();
    let origins = std::fs::read_to_string(copied.join("components/origins.kdl")).unwrap();
    assert!(
        image.contains("\nsshd\n") && copied.join("components/sshd/component.kdl").is_file(),
        "{image}"
    );
    assert!(origins.contains("component \"sshd\""), "{origins}");
    flow.buildc(&copied, &["--no-tui", "--root", ".", "check"]);
}

fn setup_set_conforms_copy_cancelled(flow: &Flow) {
    let stream = datastream();
    let copied = flow.repo_claiming("flow-set-conforms-copy-cancelled-in");
    let before = std::fs::read_to_string(copied.join("example.kdl")).unwrap();
    flow.run(
        flow.target,
        &copied,
        None,
        &["--root", ".", "set", "conforms", "--datastream", &stream],
    );
    let image = std::fs::read_to_string(copied.join("example.kdl")).unwrap();
    assert_eq!(image, before);
    assert!(!copied.join("components/sshd").exists());
}

fn setup_set_library_named(flow: &Flow) {
    let dir = flow.empty("flow-set-library-named-in");
    std::fs::write(dir.join("repo.kdl"), "schema-version 1\n").unwrap();
    flow.run_silent(
        flow.target,
        &dir,
        &["--no-tui", "--root", ".", "set", "library", "buildc"],
    );
    let text = std::fs::read_to_string(dir.join("repo.kdl")).unwrap();
    assert!(
        text.contains("buildc { source \"https://github.com/buildc-org/library\" }"),
        "{text}"
    );
}

fn setup_set_conforms_pinned(flow: &Flow) {
    let target = flow.target;
    let stream = datastream();
    let pinned = flow.repo("flow-set-conforms-pinned-in");
    seed_library(&pinned);
    let keyed = buildc::collection::seed(
        &pinned,
        &crate::harness::library_url(),
        "HEAD",
        crate::harness::LIBRARY_COMMIT,
    )
    .unwrap();
    copy(
        &crate_dir().join("tests/collections/claims"),
        &keyed.join("components"),
    );
    flow.run(
        target,
        &pinned,
        None,
        &["--root", ".", "set", "conforms", "--datastream", &stream],
    );
    let image = std::fs::read_to_string(pinned.join("example.kdl")).unwrap();
    let pin = crate::harness::LIBRARY_COMMIT;
    assert!(
        image.contains(&format!("\nsshd pin=\"{pin}\"\n")),
        "{image}"
    );
    flow.buildc(&pinned, &["--no-tui", "--root", ".", "check"]);
}

fn setup_set_conforms_enforced(flow: &Flow) {
    let target = flow.target;
    let stream = datastream();
    let enforcing = flow.repo_enforcing("flow-set-conforms-enforced-in");
    flow.run(
        target,
        &enforcing,
        None,
        &["--root", ".", "set", "conforms", "--datastream", &stream],
    );
}

fn setup_add_component_family(flow: &Flow) {
    let target = flow.target;
    let family = flow.repo_with("flow-add-component-family-in", "upstream");
    std::fs::create_dir_all(family.join("components/base-image/plain")).unwrap();
    std::fs::write(
        family.join("components/base-image/plain/component.kdl"),
        "schema-version 1\n\nbase \"quay.io/fedora/fedora-bootc:44\" { family \"fedora\"; }\n",
    )
    .unwrap();
    let image_file = family.join("example.kdl");
    let image = std::fs::read_to_string(&image_file)
        .unwrap()
        .replace(
            "    fedora signed=#false {\n        version \"44\"\n",
            "    plain signed=#false {\n",
        )
        .replace("\nfedora-family\n", "\n");
    assert!(
        buildc::parse::image::listed(&image, "base-image/plain") == 1
            && buildc::parse::image::listed(&image, "fedora-family") == 0,
        "{image}"
    );
    std::fs::write(&image_file, image).unwrap();
    std::fs::remove_dir_all(family.join("components/fedora-family")).unwrap();
    flow.run(
        target,
        &family,
        None,
        &[
            "--root",
            ".",
            "add",
            "component",
            "needs-family",
            "--library",
            "upstream",
        ],
    );
    let listed = std::fs::read_to_string(family.join("example.kdl")).unwrap();
    assert!(
        buildc::parse::image::listed(&listed, "fedora-family") == 1
            && !listed.contains("debian-family"),
        "the adapter brought has to support the base's family: {listed}"
    );
}

fn setup_set_claims(flow: &Flow) {
    let target = flow.target;
    let stream = datastream();
    let claimed = flow.repo("flow-set-claims-in");
    lists_sshd(&claimed);
    std::fs::create_dir_all(claimed.join("components/sshd")).unwrap();
    std::fs::write(
        claimed.join("components/sshd/component.kdl"),
        format!("{CLAIMANT}\nsatisfies {{\n    cis-fedora \"5.5.2\"\n}}\n"),
    )
    .unwrap();
    for name in ["flow-set-claims", "flow-set-claims-again"] {
        flow.run(name, &claimed, None, &claims(&stream));
        if target == "flow-set-claims" {
            return;
        }
    }
}

fn setup_set_claims_two(flow: &Flow) {
    let target = flow.target;
    let stream = datastream();
    let two = flow.repo("flow-set-claims-two-in");
    lists_sshd(&two);
    std::fs::create_dir_all(two.join("components/sshd")).unwrap();
    std::fs::write(
        two.join("components/sshd/component.kdl"),
        format!(
            "{CLAIMANT}\nsatisfies {{\n    cis-fedora \"5.5.2\"\n    \
                 stig-fedora \"RHEL-09-232010\"\n}}\n"
        ),
    )
    .unwrap();
    flow.run(
        target,
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
    let merged = std::fs::read_to_string(two.join("components/sshd/component.kdl")).unwrap();
    assert_eq!(merged.matches("satisfies ").count(), 1, "{merged}");
    assert!(
        !merged.contains("cis-fedora") && !merged.contains("stig-fedora"),
        "both benchmark names collapse into the derived one: {merged}"
    );
    flow.buildc(&two, &["--no-tui", "--root", ".", "check"]);
}

fn setup_workflows(flow: &Flow) {
    let prompted = flow.repo("flow-set");
    flow.run(
        "flow-set-workflows",
        &prompted,
        None,
        &["--root", ".", "set", "workflows"],
    );
    let declaration = "github {\n    workflows {\n        image-build {\n            pr\n            push\n            schedule frequency=\"daily\" time=\"12:30\"\n        }\n        base-sig-probe\n    }\n}";
    let prompted_repo = std::fs::read_to_string(prompted.join("repo.kdl")).unwrap();
    assert!(prompted_repo.contains(declaration), "{prompted_repo}");

    let generated_build = |root: &Path| {
        let run = buildc::run(Command::Generate, None, root);
        assert!(
            run.problems.issues.is_empty(),
            "{}",
            run.problems.issues.plain()
        );
        buildc::write_generated(root, &run.files, &run.copies).unwrap();
        assert!(
            buildc::run(Command::Verify, None, root)
                .problems
                .issues
                .is_empty(),
            "verify rejected its generated workflow"
        );
        run.files
            .into_iter()
            .find(|(path, _)| path == Path::new(".github/workflows/image-build.yml"))
            .unwrap()
            .1
    };
    let scaffolded = generated_build(&prompted);
    assert!(
        scaffolded.contains("    - cron: '30 12 * * *'\n"),
        "{scaffolded}"
    );
    assert!(scaffolded.contains("  pull_request:\n"), "{scaffolded}");

    let direct = flow.repo("flow-triggers-direct");
    flow.buildc(
        &direct,
        &[
            "--no-tui",
            "--root",
            ".",
            "set",
            "workflows",
            "image-build",
            "--on",
            "push",
            "--on",
            "tag=v*",
        ],
    );
    let direct_repo = std::fs::read_to_string(direct.join("repo.kdl")).unwrap();
    assert!(
        direct_repo.contains(
            "        image-build {\n            push\n            tag \"v*\"\n        }\n"
        ),
        "{direct_repo}"
    );
    let tagged = generated_build(&direct);
    assert!(tagged.contains("    tags:\n      - 'v*'\n"), "{tagged}");
    assert!(!tagged.contains("  pull_request:\n"), "{tagged}");
    assert!(!tagged.contains("- cron:"), "{tagged}");
}

fn setup_add_component_requires(flow: &Flow) {
    let target = flow.target;
    let requires = flow.repo_sourced("flow-requires");
    flow.run(
        target,
        &requires,
        None,
        &[
            "--root",
            ".",
            "add",
            "component",
            "browser",
            "--library",
            "community",
        ],
    );
    let image = std::fs::read_to_string(requires.join("example.kdl")).unwrap();
    assert!(
        image.contains("\nflatpak\nbrowser library=\"community\"\n"),
        "{image}"
    );
    assert!(!requires.join("components/flatpak").exists());
    flow.buildc(&requires, &["--no-tui", "--root", ".", "check"]);
}

fn setup_add_component_requires_no_tui(flow: &Flow) {
    let requires = flow.repo_sourced("flow-requires-no-tui");
    let image = requires.join("example.kdl");
    flow.run_silent(
        flow.target,
        &requires,
        &[
            "--no-tui",
            "--root",
            ".",
            "add",
            "component",
            "browser",
            "--library",
            "community",
        ],
    );
    let image = std::fs::read_to_string(image).unwrap();
    assert!(
        buildc::parse::image::listed(&image, "browser") == 1,
        "{image}"
    );
    assert!(
        buildc::parse::image::listed(&image, "flatpak") == 0,
        "{image}"
    );
}

fn setup_add_component_requires_no_terminal(flow: &Flow) {
    let requires = flow.repo_sourced("flow-requires-no-terminal");
    let image = requires.join("example.kdl");
    flow.run_silent(
        flow.target,
        &requires,
        &[
            "--root",
            ".",
            "add",
            "component",
            "browser",
            "--library",
            "community",
        ],
    );
    let image = std::fs::read_to_string(image).unwrap();
    assert!(
        buildc::parse::image::listed(&image, "browser") == 1,
        "{image}"
    );
    assert!(
        buildc::parse::image::listed(&image, "flatpak") == 0,
        "{image}"
    );
}

fn setup_add_component_dependencies(flow: &Flow) {
    let requires = flow.repo_sourced("flow-add-component-dependencies");
    let image = requires.join("example.kdl");
    flow.run_silent(
        flow.target,
        &requires,
        &[
            "--no-tui",
            "--root",
            ".",
            "add",
            "component",
            "browser",
            "--library",
            "community",
            "--dependencies",
        ],
    );
    let image = std::fs::read_to_string(image).unwrap();
    assert!(
        buildc::parse::image::listed(&image, "browser") == 1,
        "{image}"
    );
    assert!(
        buildc::parse::image::listed(&image, "flatpak") == 1,
        "{image}"
    );
    assert!(!requires.join("components/browser").exists());
    assert!(!requires.join("components/flatpak").exists());
}

fn setup_add_component_requires_namesakes(flow: &Flow) {
    let requires = flow.repo_with("flow-requires-namesakes", "namesakes");
    flow.run(
        flow.target,
        &requires,
        None,
        &["--root", ".", "add", "component"],
    );
    let image = std::fs::read_to_string(requires.join("example.kdl")).unwrap();
    for component in ["apps/client", "desktop/client", "flatpak"] {
        assert!(
            buildc::parse::image::listed(&image, component) == 1,
            "{image}"
        );
    }
}

fn add_declined(flow: &Flow) -> PathBuf {
    let declined = flow.repo_sourced("flow-declined");
    flow.run(
        "flow-add-component-declined",
        &declined,
        None,
        &[
            "--root",
            ".",
            "add",
            "component",
            "browser",
            "--library",
            "community",
        ],
    );
    let image = std::fs::read_to_string(declined.join("example.kdl")).unwrap();
    assert!(
        buildc::parse::image::listed(&image, "browser") == 1,
        "{image}"
    );
    assert!(
        buildc::parse::image::listed(&image, "flatpak") == 0,
        "{image}"
    );
    declined
}

fn setup_add_component_declined(flow: &Flow) {
    let _ = add_declined(flow);
}

fn setup_check_unmet(flow: &Flow) {
    let target = flow.target;
    let declined = add_declined(flow);
    flow.run(target, &declined, None, &["--root", ".", "check"]);
}

fn setup_add_component_skip(flow: &Flow) {
    let skip = flow.repo_sourced("flow-skip");
    flow.buildc(
        &skip,
        &["--no-tui", "--root", ".", "create", "image", "Server"],
    );
    copy(
        &crate_dir().join("tests/collections/upstream/flatpak"),
        &skip.join("components/flatpak"),
    );
    let held = contents(&skip.join("components/flatpak"));
    flow.run(
        "flow-add-component-skip",
        &skip,
        None,
        &[
            "--root",
            ".",
            "add",
            "component",
            "browser",
            "--library",
            "community",
            "--in",
            "server",
        ],
    );
    let server = std::fs::read_to_string(skip.join("server.kdl")).unwrap();
    assert!(
        buildc::parse::image::listed(&server, "flatpak") == 1
            && buildc::parse::image::listed(&server, "browser") == 1,
        "{server}"
    );
    assert_eq!(contents(&skip.join("components/flatpak")), held);
    flow.buildc(&skip, &["--no-tui", "--root", ".", "check"]);
}

fn setup_check_unfetched(flow: &Flow) {
    let target = flow.target;
    let unfetched = flow.repo("flow-unfetched");
    std::fs::create_dir_all(unfetched.join("components/core/one")).unwrap();
    std::fs::write(
        unfetched.join("components/core/one/component.kdl"),
        "schema-version 1\n\ndescription \"Builds things\"\n\ntargets \"fedora\"\n\nrequires \"flatpak\"\n",
    )
    .unwrap();
    let image = unfetched.join("example.kdl");
    let listed = listing(
        &std::fs::read_to_string(&image).unwrap(),
        "core {\n    one\n}\n",
    );
    assert!(
        buildc::parse::image::listed(&listed, "core/one") == 1,
        "{listed}"
    );
    std::fs::write(&image, listed).unwrap();
    flow.run(target, &unfetched, None, &["--root", ".", "check"]);
}

fn on_debian(root: &Path) -> PathBuf {
    std::fs::create_dir_all(root.join("components/base-image/debian")).unwrap();
    std::fs::write(
        root.join("components/base-image/debian/component.kdl"),
        "schema-version 1\n\nbase \"docker.io/library/debian:forky\" { family \"debian\"; }\n",
    )
    .unwrap();
    let image_file = root.join("example.kdl");
    let image = std::fs::read_to_string(&image_file).unwrap().replace(
        "    fedora signed=#false {\n        version \"44\"\n",
        "    debian signed=#false {\n",
    );
    assert!(
        buildc::parse::image::listed(&image, "base-image/debian") == 1,
        "{image}"
    );
    std::fs::write(&image_file, image).unwrap();
    image_file
}

fn setup_add_component(flow: &Flow) {
    let target = flow.target;
    let root = flow.repo_sourced("flow-add");
    flow.run(target, &root, None, &["--root", ".", "add", "component"]);
    let image = std::fs::read_to_string(root.join("example.kdl")).unwrap();
    assert!(
        image.contains("\nbrowser library=\"upstream\"\n"),
        "{image}"
    );
    assert!(!root.join("components/browser").exists());
    assert!(!root.join("components/origins.kdl").exists());
}

fn setup_add_component_exclusive(flow: &Flow) {
    let target = flow.target;
    let root = flow.repo_sourced(target);
    std::fs::create_dir_all(root.join("components/flathub")).unwrap();
    std::fs::write(
        root.join("components/flathub/component.kdl"),
        "schema-version 1\n\ndescription \"Flathub remote\"\n\nprovides \"flatpak\" exclusive=#true\n",
    )
    .unwrap();
    let image_file = root.join("example.kdl");
    let listed = listing(&std::fs::read_to_string(&image_file).unwrap(), "flathub\n");
    assert!(
        buildc::parse::image::listed(&listed, "flathub") == 1,
        "{listed}"
    );
    std::fs::write(&image_file, &listed).unwrap();
    let args = [
        "--root",
        ".",
        "add",
        "component",
        "flatpak",
        "--library",
        "upstream",
    ];
    match target {
        "flow-add-component-exclusive" => {
            flow.run(target, &root, None, &args);
            let image = std::fs::read_to_string(&image_file).unwrap();
            assert!(
                buildc::parse::image::listed(&image, "flatpak") == 1,
                "{image}"
            );
            assert!(
                buildc::parse::image::listed(&image, "flathub") == 0,
                "{image}"
            );
            assert!(root.join("components/flathub/component.kdl").is_file());
        }
        _ => {
            let silent = [&["--no-tui"][..], &args[..]].concat();
            flow.run_silent(target, &root, &silent);
            assert_eq!(std::fs::read_to_string(&image_file).unwrap(), listed);
            assert!(!root.join("components/flatpak").exists());
        }
    }
}

fn setup_add_component_unavailable(flow: &Flow) {
    let target = flow.target;
    let root = flow.repo_with("flow-add-component-unavailable-in", "upstream");
    let image_file = on_debian(&root);
    flow.run(target, &root, None, &["--root", ".", "add", "component"]);
    let image = std::fs::read_to_string(&image_file).unwrap();
    assert!(
        buildc::parse::image::listed(&image, "debian-family") == 1,
        "{image}"
    );
    assert!(!root.join("components/flatpak").exists());
}

fn setup_add_component_default(flow: &Flow) {
    let target = flow.target;
    let defaulted = flow.repo("flow-add-component-default-in");
    let repo = defaulted.join("repo.kdl");
    let text = std::fs::read_to_string(&repo)
        .unwrap()
        .replace(&scaffolded_sources(), "")
        + "unfinished-repo-property \"kept for buildc check\"\n"
        + &local_libraries(
            &defaulted,
            &[("upstream", &crate_dir().join("tests/collections/upstream"))],
        );
    std::fs::write(&repo, text).unwrap();
    let image_file = defaulted.join("example.kdl");
    let image = std::fs::read_to_string(&image_file).unwrap().replace(
        "\nbase-image {",
        "\nunfinished-image-property \"kept for buildc check\"\nbase-image {",
    );
    std::fs::write(&image_file, image).unwrap();
    let (_, issues, _) = buildc::declarations(&defaulted);
    assert!(
        !issues.is_empty(),
        "the fixture must prove edits do not depend on a clean check"
    );
    flow.run(
        "flow-add-component-default",
        &defaulted,
        None,
        &[
            "--no-tui",
            "--root",
            ".",
            "add",
            "component",
            "browser",
            "--library",
            "upstream",
        ],
    );
    let declared = std::fs::read_to_string(&repo).unwrap();
    let image = std::fs::read_to_string(defaulted.join("example.kdl")).unwrap();
    assert!(
        declared.contains("\n        upstream { source")
            && buildc::parse::image::listed(&image, "browser") == 1,
        "{declared}\n{image}"
    );
    if target == "flow-add-component-default" {
        return;
    }
    flow.run(
        "flow-create-component-with-schema-issues",
        &defaulted,
        None,
        &[
            "--no-tui",
            "--root",
            ".",
            "create",
            "component",
            "local-tool",
            "--with",
            "description=Local tools",
            "--with",
            "targets=",
            "--build",
            "example",
        ],
    );
    let image = std::fs::read_to_string(&image_file).unwrap();
    assert!(
        buildc::parse::image::listed(&image, "browser") == 1
            && buildc::parse::image::listed(&image, "local-tool") == 1,
        "{image}"
    );
}

fn setup_import_edit_guards(flow: &Flow) {
    let malformed = flow.repo_sourced("flow-malformed-edit-in");
    let image_file = malformed.join("example.kdl");
    let mut image = std::fs::read_to_string(&image_file).unwrap();
    image.push_str("image {\n");
    std::fs::write(&image_file, image).unwrap();
    let before = contents(&malformed);
    for args in [
        vec![
            "--no-tui",
            "--root",
            ".",
            "add",
            "component",
            "browser",
            "--library",
            "upstream",
        ],
        vec![
            "--no-tui",
            "--root",
            ".",
            "create",
            "component",
            "local-tool",
            "--with",
            "description=",
            "--with",
            "targets=",
            "--build",
            "example",
        ],
    ] {
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_buildc"));
        let out = sealed(&mut command, &flow.bin("malformed-edit", None))
            .args(args)
            .current_dir(&malformed)
            .output()
            .unwrap();
        assert!(!out.status.success(), "malformed KDL was accepted");
        let error = String::from_utf8_lossy(&out.stderr);
        assert!(error.contains("is not valid KDL"), "{error}");
        assert_eq!(contents(&malformed), before, "a refused edit wrote files");
    }

    for (name, args, declaration) in [
        (
            "flow-add-component-lists",
            vec![
                "--no-tui",
                "--root",
                ".",
                "add",
                "component",
                "browser",
                "--library",
                "upstream",
            ],
            "browser",
        ),
        (
            "flow-create-lists",
            vec![
                "--no-tui",
                "--root",
                ".",
                "create",
                "component",
                "local-tool",
                "--with",
                "description=",
                "--with",
                "targets=",
                "--build",
                "example",
            ],
            "local-tool",
        ),
    ] {
        let root = flow.repo_sourced(name);
        let image_file = root.join("example.kdl");
        flow.buildc(&root, &args);
        let image = std::fs::read_to_string(&image_file).unwrap();
        assert!(
            buildc::parse::image::listed(&image, declaration) == 1,
            "{image}"
        );
    }
}

fn setup_add_component_pin(flow: &Flow) {
    let root = flow.repo("flow-add-component-pin");
    seed_library(&root);
    let pin = crate::harness::LIBRARY_COMMIT;
    flow.buildc(
        &root,
        &[
            "--no-tui",
            "--root",
            ".",
            "add",
            "component",
            "flatpak",
            "--pin",
            pin,
        ],
    );
    let image = std::fs::read_to_string(root.join("example.kdl")).unwrap();
    assert!(
        image.contains(&format!("\nflatpak pin=\"{pin}\"\n")),
        "{image}"
    );
    flow.buildc(&root, &["--no-tui", "--root", ".", "check"]);
}

fn setup_add_component_several(flow: &Flow) {
    let several = flow.repo_sourced("flow-several");
    flow.run(
        "flow-add-component-several",
        &several,
        None,
        &["--root", ".", "add", "component"],
    );
    let image = std::fs::read_to_string(several.join("example.kdl")).unwrap();
    for component in ["flatpak", "browser", "custom-kernel"] {
        assert!(
            buildc::parse::image::listed(&image, component) == 1,
            "{image}"
        );
    }
    flow.buildc(&several, &["--no-tui", "--root", ".", "check"]);
}

fn setup_add_component_nested(flow: &Flow) {
    let nested = flow.repo_with("flow-nested", "grouped");
    flow.run(
        "flow-add-component-nested",
        &nested,
        None,
        &["--root", ".", "add", "component"],
    );
    let image = std::fs::read_to_string(nested.join("example.kdl")).unwrap();
    assert!(
        image.contains("\nhardening {\n    coredumps\n}\n"),
        "{image}"
    );
    flow.buildc(&nested, &["--no-tui", "--root", ".", "check"]);
    flow.buildc(&nested, &["--no-tui", "--root", ".", "generate"]);
    let plan = std::fs::read_to_string(nested.join("generated/plan.json")).unwrap();
    assert!(plan.contains("\"path\": \"hardening/coredumps\""), "{plan}");
}

fn setup_collides(flow: &Flow) {
    let target = flow.target;
    let collide = flow.repo_sourced("flow-collides");
    let remotes = "components/editor/files/usr/share/example";
    std::fs::create_dir_all(collide.join(remotes)).unwrap();
    std::fs::write(
        collide.join("components/editor/component.kdl"),
        "schema-version 1\n\ndescription \"Editor shipping its own flatpak remotes\"\n\ntargets \"fedora\"\n",
    )
    .unwrap();
    std::fs::write(collide.join(remotes).join("remotes.list"), "editor\n").unwrap();
    let image = collide.join("example.kdl");
    let listed = listing(&std::fs::read_to_string(&image).unwrap(), "editor\n");
    assert!(
        buildc::parse::image::listed(&listed, "editor") == 1,
        "{listed}"
    );
    std::fs::write(&image, listed).unwrap();
    flow.buildc(
        &collide,
        &[
            "--no-tui",
            "--root",
            ".",
            "add",
            "component",
            "flatpak",
            "--library",
            "upstream",
        ],
    );
    assert!(
        buildc::run(Command::Check, None, &collide)
            .problems
            .issues
            .plain()
            .contains("`flatpak` overwrites `/usr/share/example/remotes.list`"),
        "check reports the collision"
    );
    flow.run(target, &collide, None, &["--root", ".", "check"]);
}

fn setup_key_absent(flow: &Flow) {
    let target = flow.target;
    flow.run(
        target,
        &flow.repo_sourced("flow-key-none"),
        None,
        &["--root", ".", "create", "key", "cosign"],
    );
}

fn setup_key_kinds(flow: &Flow) {
    let target = flow.target;
    let root = flow.repo("flow-key");
    std::fs::create_dir_all(root.join("components/signed-kernel")).unwrap();
    std::fs::write(
        root.join("components/signed-kernel/component.kdl"),
        KEYHOLDER,
    )
    .unwrap();
    flow.run(target, &root, None, &["--root", ".", "create", "key"]);
}

fn setup_key_undeclared(flow: &Flow) {
    let target = flow.target;
    flow.run(
        target,
        &flow.repo_sourced("flow-key-undeclared"),
        None,
        &["--root", ".", "create", "key", "sbom"],
    );
}

fn setup_key_refused(flow: &Flow, args: &[&str]) {
    let target = flow.target;
    let root = flow.repo("flow-key");
    std::fs::create_dir_all(root.join("components/signed-kernel")).unwrap();
    std::fs::write(
        root.join("components/signed-kernel/component.kdl"),
        KEYHOLDER.replace("bits=4096", "bits=1024"),
    )
    .unwrap();
    flow.run(target, &root, None, args);
}

fn setup_key_no_kind(flow: &Flow) {
    let target = flow.target;
    flow.run(
        target,
        &flow.repo("flow-key-no-kind"),
        None,
        &["--root", ".", "create", "key"],
    );
}

macro_rules! flow_cases {
    ($($test:ident => $fixture:literal),+ $(,)?) => {
        const FLOW_CASES: &[&str] = &[$($fixture),+];

        $(
            #[test]
            fn $test() {
                let flow = Flow::new($fixture);
                flow_case(&flow);
                flow.done();
            }
        )+
    };
}

flow_cases! {
    flow_check_claims => "flow-check-claims",
    flow_check_collides => "flow-check-collides",
    flow_check_conforms => "flow-check-conforms",
    flow_check_conforms_stranger => "flow-check-conforms-stranger",
    flow_check_unfetched => "flow-check-unfetched",
    flow_check_unmet => "flow-check-unmet",
    flow_check_unpinned => "flow-check-unpinned",
    flow_coverage => "flow-coverage",
    flow_create_image => "flow-create-image",
    flow_create_component => "flow-create-component",
    flow_create_component_with_schema_issues => "flow-create-component-with-schema-issues",
    flow_add_package => "flow-add-package",
    flow_create_repo => "flow-create-repo",
    flow_create_repo_forgejo => "flow-create-repo-forgejo",
    flow_create_repo_no_gh => "flow-create-repo-no-gh",
    flow_create_repo_minimal => "flow-create-repo-minimal",
    flow_create_repo_offer => "flow-create-repo-offer",
    flow_create_repo_signed_in => "flow-create-repo-signed-in",
    flow_create_repo_signed_out => "flow-create-repo-signed-out",
    flow_image_default => "flow-image-default",
    flow_add_component_declined => "flow-add-component-declined",
    flow_add_component_dependencies => "flow-add-component-dependencies",
    flow_add_component_exclusive => "flow-add-component-exclusive",
    flow_add_component_exclusive_no_tui => "flow-add-component-exclusive-no-tui",
    flow_add_component_default => "flow-add-component-default",
    flow_add_component_family => "flow-add-component-family",
    flow_add_component => "flow-add-component",
    flow_add_component_nested => "flow-add-component-nested",
    flow_add_component_requires => "flow-add-component-requires",
    flow_add_component_requires_namesakes => "flow-add-component-requires-namesakes",
    flow_add_component_requires_no_tui => "flow-add-component-requires-no-tui",
    flow_add_component_requires_no_terminal => "flow-add-component-requires-no-terminal",
    flow_add_component_several => "flow-add-component-several",
    flow_add_component_skip => "flow-add-component-skip",
    flow_add_component_unavailable => "flow-add-component-unavailable",
    flow_key_absent => "flow-key-absent",
    flow_key_kinds => "flow-key-kinds",
    flow_key_no_kind => "flow-key-no-kind",
    flow_key_refused => "flow-key-refused",
    flow_key_refused_no_kind => "flow-key-refused-no-kind",
    flow_key_undeclared => "flow-key-undeclared",
    flow_component_taken => "flow-component-taken",
    flow_component_two_images => "flow-component-two-images",
    flow_set_claims => "flow-set-claims",
    flow_set_claims_again => "flow-set-claims-again",
    flow_set_claims_two => "flow-set-claims-two",
    flow_set_conforms => "flow-set-conforms",
    flow_set_conforms_again => "flow-set-conforms-again",
    flow_set_conforms_copy => "flow-set-conforms-copy",
    flow_set_conforms_copy_cancelled => "flow-set-conforms-copy-cancelled",
    flow_set_library_named => "flow-set-library-named",
    flow_set_conforms_pinned => "flow-set-conforms-pinned",
    flow_set_conforms_enforced => "flow-set-conforms-enforced",
    flow_set_workflows => "flow-set-workflows",
    flow_unanswered => "flow-unanswered",
}

#[test]
fn add_component_pin() {
    let flow = Flow::new("add-component-pin");
    flow_case(&flow);
    flow.done();
}

#[test]
fn import_edit_guards() {
    let flow = Flow::new("import-edit-guards");
    flow_case(&flow);
    flow.done();
}

#[test]
fn every_answers_directory_has_a_case() {
    let mut found: Vec<String> = std::fs::read_dir(crate_dir().join("tests/answers"))
        .expect("tests/answers exists")
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    found.sort();
    for name in &found {
        assert!(
            FLOW_CASES.contains(&name.as_str()),
            "tests/answers/{name} has no named test"
        );
    }
}

#[test]
fn buildc_terminal_smoke_waits_for_each_screen() {
    let root = crate_dir().join("tests/repos/enforced");
    let buildc = env!("CARGO_BIN_EXE_buildc");
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
    assert!(graph.contains("# `enforced` capability graph"), "{graph}");
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
        graph.contains("# `enforced` capability graph") && !graph.contains('\u{250c}'),
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

    let flow = Flow::new("buildc-terminal-smoke");
    let stream = datastream();
    let claims_root = flow.repo("buildc-terminal-smoke-claims");
    lists_sshd(&claims_root);
    std::fs::create_dir_all(claims_root.join("components/sshd")).unwrap();
    std::fs::write(claims_root.join("components/sshd/component.kdl"), CLAIMANT).unwrap();
    let claims_command = format!(
        "'{}' --root . set claims sshd --datastream '{stream}'",
        env!("CARGO_BIN_EXE_buildc")
    );
    let claim_steps = [
        (b"SCAP profile".as_slice(), b"\r".as_slice()),
        (
            b"rules `sshd` claims".as_slice(),
            b"aide \x1b[B\r".as_slice(),
        ),
    ];
    let (status, errors, raw) = drawn_run(&claims_root, &claims_command, &claim_steps);
    assert!(
        status.success(),
        "{errors}{}",
        String::from_utf8_lossy(&raw)
    );
    let picked =
        std::fs::read_to_string(claims_root.join("components/sshd/component.kdl")).unwrap();
    assert!(picked.contains("    standard \"1.1.1.1\"\n"), "{picked}");

    let made = flow.repo_sourced("flow-create-image-cancel-in");
    let root = made.parent().unwrap().join("cancel-default");
    std::fs::rename(&made, &root).unwrap();
    let binary = env!("CARGO_BIN_EXE_buildc");
    for (command, steps, absent) in [
        (
            format!("'{binary}' --root . create image"),
            vec![(b"image name".as_slice(), b"\x1b".as_slice())],
            "cancel-default.kdl",
        ),
        (
            format!("'{binary}' --root . create image"),
            vec![
                (b"image name".as_slice(), b"Beta\r".as_slice()),
                (b"base image".as_slice(), b"\x1b".as_slice()),
            ],
            "beta.kdl",
        ),
    ] {
        let before = std::fs::read_to_string(root.join("repo.kdl")).unwrap();
        let (status, errors, raw) = drawn_run(&root, &command, &steps);
        assert!(
            status.success(),
            "{errors}{}",
            String::from_utf8_lossy(&raw)
        );
        assert!(!root.join(absent).exists(), "Escape wrote {absent}");
        assert_eq!(
            std::fs::read_to_string(root.join("repo.kdl")).unwrap(),
            before,
            "Escape edited repo.kdl"
        );
    }

    let (status, errors, raw) = drawn_run(
        &root,
        &format!("'{binary}' --root . create image Gamma --base example.invalid/own:1"),
        &[],
    );
    assert!(
        status.success(),
        "{errors}{}",
        String::from_utf8_lossy(&raw)
    );
    let image = std::fs::read_to_string(root.join("gamma.kdl")).unwrap();
    assert!(!image.contains("family"), "{image}");
    let base =
        std::fs::read_to_string(root.join("components/base-image/own/component.kdl")).unwrap();
    assert!(!base.contains("family"), "{base}");
}
