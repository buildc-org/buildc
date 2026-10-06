use crate::harness::{assert_golden, contents, crate_dir, tmp};

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
            "flow-create-flavour" => 2,
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
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_tect"));
        let status = sealed(&mut command, &self.bin(name, gh))
            .env("BUILDC_ASSETS", assets)
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
        if name == self.target {
            self.note_assertion();
            assert_golden(name, "transcript.txt", &transcript);
        }
    }
    fn offering(&self, name: &str, dir: &Path) {
        use std::os::unix::fs::PermissionsExt;
        let work = self.at(&format!("{name}-src"));
        let _ = std::fs::remove_dir_all(&work);
        std::fs::create_dir_all(&work).unwrap();

        let tarball = work.join("upstream.tar.gz");
        let status = std::process::Command::new("tar")
            .args(["czf", &tarball.display().to_string(), "upstream"])
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
                "sources {{\n    upstream {{\n        pin {{\n            version \"1\"\n\
                 \x20           url \"file://{}\"\n            sha256 \"{sha256}\"\n\
                 \x20       }}\n    }}\n}}\n",
                tarball.display()
            ),
        )
        .unwrap();

        let path = self.bin(name, None);
        for tool in ["curl", "tar", "gzip"] {
            let at = std::env::var("PATH")
                .unwrap_or_default()
                .split(':')
                .map(|at| Path::new(at).join(tool))
                .find(|at| at.is_file())
                .unwrap_or_else(|| panic!("{tool} on PATH"));
            std::os::unix::fs::symlink(at, path.join(tool)).unwrap();
        }

        let fixture = crate_dir().join("tests/answers").join(name);
        let log = self.at(&format!("{name}.log"));
        let scratch = work.join("tmp");
        std::fs::create_dir_all(&scratch).unwrap();
        let file = std::fs::File::create(&log).unwrap();
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_tect"));
        let status = command
            .env("PATH", &path)
            .env("HOME", tmp())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("TMPDIR", &scratch)
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
        if name == self.target {
            self.note_assertion();
            assert_golden(name, "transcript.txt", &transcript);
        }

        let root = dir.join("example");
        assert!(!root.join("out").exists(), "the fetch cached into the repo");
        let strays: Vec<PathBuf> = std::fs::read_dir(&scratch)
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
            image.contains("source \"upstream\" {\n            module \"fedora-family\"\n"),
            "{image}"
        );
    }
    fn drawn(&self, name: &str, dir: &Path, command: &str, after: &str, steps: &[&[u8]]) {
        let (status, errors, raw) = drawn_run(dir, command, steps);
        assert!(
            status.success(),
            "{errors}{}",
            String::from_utf8_lossy(&raw)
        );

        let text = String::from_utf8_lossy(&raw);
        let stable = text.rsplit_once(after).unwrap().1;
        if name == self.target {
            self.note_assertion();
            assert_golden(
                name,
                "transcript.txt",
                &format!(
                    "{after}{stable}==== exit 0
    "
                ),
            );
        }
    }
    fn buildc(&self, dir: &Path, args: &[&str]) {
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_tect"));
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
        self.buildc(
            &dir,
            &[
                "--no-tui", "create", "repo", "Example", "--owner", "someone", "--image", "Example",
            ],
        );
        dir.join("example")
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
            .replace(&buildc::init::sources(&crate_dir().join("assets")), "");
        repo.push_str(&format!(
            "sources {{\n    upstream {:?}\n    community {:?}\n}}\n",
            collections.join("upstream").display(),
            collections.join("community").display()
        ));
        std::fs::write(root.join("repo.kdl"), repo).unwrap();
        root
    }

    fn repo_with(&self, name: &str, collection: &str) -> PathBuf {
        let root = self.repo(name);
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

    fn repo_claiming_two(&self, name: &str) -> PathBuf {
        let root = self.repo_claiming(name);
        self.buildc(
            &root,
            &["--no-tui", "--root", ".", "create", "image", "Server"],
        );
        root
    }
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

fn drawn_run(
    dir: &Path,
    command: &str,
    steps: &[&[u8]],
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
                // The test answers the cursor-position request an inline viewport sends, because a pseudo-terminal has no terminal to answer it.
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
    (status, errors, raw.clone())
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

fn scaffolded(area: buildc::emit::schema_md::Area, file: &Path) {
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
    let text =
        std::fs::read_to_string(file).unwrap_or_else(|err| panic!("{}: {err}", file.display()));
    let doc: kdl::KdlDocument = text.parse().expect("the scaffold is KDL");
    let mut written = Vec::new();
    walk(doc.nodes(), "", &mut written);
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

fn flow_case(flow: &Flow) {
    match flow.target {
        "flow-create-repo"
        | "flow-create-repo-no-gh"
        | "flow-create-repo-signed-out"
        | "flow-create-repo-signed-in"
        | "flow-create-repo-forgejo"
        | "flow-image-default" => setup_create_repo(flow),
        "flow-create-repo-offer" => setup_create_repo_offer(flow),
        "flow-create-image" => setup_create_image(flow),
        "flow-check-shadow" => setup_check_shadow(flow),
        "flow-check-unpinned" => setup_check_unpinned(flow),
        "flow-check-conforms" | "flow-check-claims" | "flow-coverage" => setup_conforms(flow),
        "flow-check-conforms-stranger" => setup_check_conforms_stranger(flow),
        "flow-create-module" | "flow-module-taken" | "flow-unanswered" => setup_module(flow),
        "flow-module-two-images" => setup_module_two_images(flow),
        "flow-create-flavour" => setup_create_flavour(flow),
        "flow-module-in-flavour" => setup_module_in_flavour(flow),
        "flow-set-conforms" | "flow-set-conforms-again" => setup_set_conforms(flow),
        "flow-import-conforms" => setup_import_conforms(flow),
        "flow-import-conforms-declined" => setup_import_conforms_declined(flow),
        "flow-set-conforms-enforced" => setup_set_conforms_enforced(flow),
        "flow-import-conforms-enforced" => setup_import_conforms_enforced(flow),
        "flow-import-conforms-two" => setup_import_conforms_two(flow),
        "flow-import-datastream" => setup_import_datastream(flow),
        "flow-import-family" => setup_import_family(flow),
        "flow-copy-conforms" => setup_copy_conforms(flow),
        "flow-set-claims" | "flow-set-claims-again" => setup_set_claims(flow),
        "flow-set-claims-two" => setup_set_claims_two(flow),
        "flow-set-claims-fetched" => setup_set_claims_fetched(flow),
        "flow-set-claims-drawn" => setup_set_claims_drawn(flow),
        "flow-set-workflows" | "flow-set-workflows-drawn" => setup_workflows(flow),
        "flow-import-requires" => setup_import_requires(flow),
        "flow-check-unmet" => setup_check_unmet(flow),
        "flow-import-skip" => setup_import_skip(flow),
        "flow-check-unfetched" => setup_check_unfetched(flow),
        "flow-why-picker" => setup_why_picker(flow),
        "flow-import-kernel" => setup_import_kernel(flow),
        "flow-import-module" => setup_import_module(flow),
        "flow-import-default"
        | "flow-copy-with-schema-issues"
        | "flow-create-module-with-schema-issues" => setup_import_default(flow),
        "import-edit-guards" => setup_import_edit_guards(flow),
        "flow-import-several" => setup_import_several(flow),
        "flow-import-nested" => setup_import_nested(flow),
        "flow-import-suffix" => setup_import_suffix(flow),
        "flow-import-suffix-ambiguous" => setup_import_suffix_ambiguous(flow),
        "flow-import-collides" | "flow-check-collides" => setup_collides(flow),
        "flow-copy-nested" => setup_copy_nested(flow),
        "flow-copy-collides" => setup_copy_collides(flow),
        "flow-copy-module" => setup_copy_module(flow),
        "flow-key-absent" => setup_key_absent(flow),
        "flow-key-kinds" => setup_key_kinds(flow),
        "flow-key-undeclared" => setup_key_undeclared(flow),
        "flow-key-no-kind" => setup_key_no_kind(flow),
        target => panic!("no flow case for {target}"),
    }
}

fn datastream() -> String {
    crate_dir()
        .join("tests/scap/datastream.xml")
        .display()
        .to_string()
}

fn import_sshd(stream: &str) -> [&str; 7] {
    [
        "--root",
        ".",
        "import",
        "module",
        "claims/sshd",
        "--datastream",
        stream,
    ]
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
    flow.run(
        target,
        &flow.empty(&format!("{target}-in")),
        gh,
        &["create", "repo"],
    );
    if target == "flow-create-repo" {
        let created = flow.at("flow-create-repo-in").join("example");
        scaffolded(buildc::emit::schema_md::Area::Repo, &created.join("repo.kdl"));
        scaffolded(
            buildc::emit::schema_md::Area::Image,
            &created.join("desktop.image.kdl"),
        );
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
    let image = std::fs::read_to_string(root.join("beta.image.kdl")).unwrap();
    assert!(
            image.contains(
                "    modules {\n        source \"upstream\" {\n            module \"fedora-family\"\n        }\n    }"
            ),
            "{image}"
        );
    flow.buildc(&root, &["--no-tui", "--root", ".", "fetch", "modules"]);
    flow.buildc(&root, &["--no-tui", "--root", ".", "check"]);
}

fn setup_check_shadow(flow: &Flow) {
    let target = flow.target;
    flow.run(
        target,
        &flow.repo_sourced("flow-shadow"),
        None,
        &["--root", ".", "check"],
    );
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
    let image = stranger.join("example.image.kdl");
    let declared = std::fs::read_to_string(&image).unwrap().replace(
        "    modules {",
        "    conforms \"cusp_fedora\"\n\n    modules {",
    );
    std::fs::write(&image, declared).unwrap();
    flow.run(
        target,
        &stranger,
        None,
        &["--root", ".", "check", "--datastream", &stream],
    );
}

fn setup_module(flow: &Flow) {
    let target = flow.target;
    let root = flow.repo("flow-module");
    let module = ["--root", ".", "create", "module"];
    flow.run("flow-create-module", &root, None, &module);
    scaffolded(
        buildc::emit::schema_md::Area::Module,
        &root.join("modules/my-editor/module.kdl"),
    );
    if target == "flow-create-module" {
        return;
    }
    if target == "flow-module-taken" {
        flow.run(target, &root, None, &[&module[..], &["My Editor"]].concat());
        return;
    }
    flow.run(target, &root, None, &module);
}

fn setup_module_two_images(flow: &Flow) {
    let target = flow.target;
    flow.run(
        target,
        &flow.repo_two("flow-module-both"),
        None,
        &["--root", ".", "create", "module"],
    );
}

fn setup_create_flavour(flow: &Flow) {
    let target = flow.target;
    let flavoured = flow.repo("flow-flavour");
    flow.run(
        target,
        &flavoured,
        None,
        &["--root", ".", "create", "flavour"],
    );
    let image = std::fs::read_to_string(flavoured.join("example.image.kdl")).unwrap();
    flow.note_assertion();
    assert_golden(target, "example.image.kdl", &image);
}

fn setup_module_in_flavour(flow: &Flow) {
    let target = flow.target;
    let root = flow.repo("flow-gated");
    flow.buildc(
        &root,
        &[
            "--no-tui", "--root", ".", "create", "flavour", "dx", "--image", "example",
        ],
    );
    flow.run(target, &root, None, &["--root", ".", "create", "module"]);
    let image = std::fs::read_to_string(root.join("example.image.kdl")).unwrap();
    assert!(
            image.contains(
                "    modules {\n        flavour \"dx\" {\n            module \"dev-tools\"\n        }\n    }"
            ),
            "{image}"
        );

    flow.buildc(
        &root,
        &[
            "--no-tui", "--root", ".", "create", "flavour", "gaming", "--image", "example",
        ],
    );
    let listed = |at: &str| {
        let (list, _, _) = buildc::declarations(&root);
        buildc::create::Listing::collect(&root, vec![at.into()], &common::prompt::Prompt::silent())
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
    let declared = std::fs::read_to_string(measured.join("example.image.kdl")).unwrap();
    assert_eq!(declared.matches("conforms ").count(), 1, "{declared}");
    assert!(
        declared.contains("    conforms \"ospp\"\n")
            && declared.contains("source \"claims\" {\n            module \"sshd\""),
        "{declared}"
    );
}

fn setup_import_conforms(flow: &Flow) {
    let target = flow.target;
    let stream = datastream();
    let claiming = flow.repo_claiming("flow-import-conforms-in");
    flow.run(target, &claiming, None, &import_sshd(&stream));
    let taken = std::fs::read_to_string(claiming.join("example.image.kdl")).unwrap();
    assert!(
        taken.contains("    conforms \"standard\"\n")
            && taken.contains("source \"claims\" {\n            module \"sshd\""),
        "{taken}"
    );
}

fn setup_import_conforms_declined(flow: &Flow) {
    let target = flow.target;
    let stream = datastream();
    let unmeasured = flow.repo_claiming("flow-import-conforms-none");
    flow.run(target, &unmeasured, None, &import_sshd(&stream));
    let left = std::fs::read_to_string(unmeasured.join("example.image.kdl")).unwrap();
    assert!(
        !left.contains("conforms") && left.contains("module \"sshd\""),
        "{left}"
    );
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

fn setup_import_conforms_enforced(flow: &Flow) {
    let target = flow.target;
    let stream = datastream();
    let importing = flow.repo_enforcing("flow-import-conforms-enforced-in");
    flow.run(target, &importing, None, &import_sshd(&stream));
}

fn setup_import_conforms_two(flow: &Flow) {
    let target = flow.target;
    let stream = datastream();
    let both = flow.repo_claiming_two("flow-import-conforms-two-in");
    flow.run(target, &both, None, &import_sshd(&stream));
    for named in ["example", "server"] {
        let written = std::fs::read_to_string(both.join(format!("{named}.image.kdl"))).unwrap();
        assert!(
            written.contains("    conforms \"standard\"\n") && written.contains("module \"sshd\""),
            "{named}: {written}"
        );
    }
}

fn setup_import_datastream(flow: &Flow) {
    let target = flow.target;
    let typo = flow.repo_claiming("flow-import-datastream-in");
    flow.run(
        target,
        &typo,
        None,
        &[
            "--root",
            ".",
            "import",
            "module",
            "claims/sshd",
            "--datastream",
            "no-such-datastream.xml",
        ],
    );
    let untouched = std::fs::read_to_string(typo.join("example.image.kdl")).unwrap();
    assert!(
        !untouched.contains("module \"sshd\"") && !typo.join("modules/sshd").exists(),
        "the refusal leaves the repository as it was: {untouched}"
    );
}

fn setup_import_family(flow: &Flow) {
    let target = flow.target;
    let family = flow.repo_with("flow-import-family-in", "upstream");
    flow.run(
        target,
        &family,
        None,
        &["--root", ".", "import", "module", "upstream/needs-family"],
    );
    let listed = std::fs::read_to_string(family.join("example.image.kdl")).unwrap();
    assert!(
        listed.contains("module \"fedora-family\"") && !listed.contains("debian-family"),
        "the adapter brought has to support the base's family: {listed}"
    );
}

fn setup_copy_conforms(flow: &Flow) {
    let target = flow.target;
    let stream = datastream();
    let vendored = flow.repo_claiming("flow-copy-conforms-in");
    flow.run(
        target,
        &vendored,
        None,
        &[
            "--root",
            ".",
            "copy",
            "module",
            "claims/sshd",
            "--datastream",
            stream.as_str(),
        ],
    );
    let copied = std::fs::read_to_string(vendored.join("example.image.kdl")).unwrap();
    assert!(
        copied.contains("conforms \"standard\"") && copied.contains("module \"sshd\""),
        "{copied}"
    );
}

fn setup_set_claims(flow: &Flow) {
    let target = flow.target;
    let stream = datastream();
    let claimed = flow.repo("flow-set-claims-in");
    lists_sshd(&claimed);
    std::fs::create_dir_all(claimed.join("modules/sshd")).unwrap();
    std::fs::write(
        claimed.join("modules/sshd/module.kdl"),
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
    std::fs::create_dir_all(two.join("modules/sshd")).unwrap();
    std::fs::write(
        two.join("modules/sshd/module.kdl"),
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
    let merged = std::fs::read_to_string(two.join("modules/sshd/module.kdl")).unwrap();
    assert_eq!(merged.matches("satisfies ").count(), 1, "{merged}");
    assert!(
        !merged.contains("cis-fedora") && !merged.contains("stig-fedora"),
        "both benchmark names collapse into the derived one: {merged}"
    );
    flow.buildc(&two, &["--no-tui", "--root", ".", "check"]);
}

fn setup_set_claims_fetched(flow: &Flow) {
    let target = flow.target;
    let stream = datastream();
    let claimed = flow.repo("flow-set-claims-in");
    lists_sshd(&claimed);
    std::fs::create_dir_all(claimed.join("modules/sshd")).unwrap();
    std::fs::write(
        claimed.join("modules/sshd/module.kdl"),
        format!("{CLAIMANT}\nsatisfies {{\n    cis-fedora \"5.5.2\"\n}}\n"),
    )
    .unwrap();
    for name in ["flow-set-claims", "flow-set-claims-again"] {
        flow.run(name, &claimed, None, &claims(&stream));
    }
    std::fs::create_dir_all(claimed.join("modules/.remote/upstream/sshd")).unwrap();
    std::fs::write(
        claimed.join("modules/.remote/upstream/sshd/module.kdl"),
        CLAIMANT,
    )
    .unwrap();
    flow.run(
        target,
        &claimed,
        None,
        &[
            "--root",
            ".",
            "set",
            "claims",
            ".remote/upstream/sshd",
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
    flow.buildc(&claimed, &["--no-tui", "--root", ".", "check"]);
}

fn setup_set_claims_drawn(flow: &Flow) {
    let target = flow.target;
    let stream = datastream();
    let drawn = flow.repo("flow-set-claims-drawn-in");
    lists_sshd(&drawn);
    std::fs::create_dir_all(drawn.join("modules/sshd")).unwrap();
    std::fs::write(drawn.join("modules/sshd/module.kdl"), CLAIMANT).unwrap();
    flow.drawn(
        target,
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
}

fn setup_workflows(flow: &Flow) {
    let prompted = flow.repo("flow-set");
    flow.run(
        "flow-set-workflows",
        &prompted,
        None,
        &["--root", ".", "set", "workflows"],
    );
    let declaration =
        "workflows at=\"05:45\" scan=\"scheduled\" {\n    build\n    base-sig-probe\n}";
    let prompted_repo = std::fs::read_to_string(prompted.join("repo.kdl")).unwrap();
    assert!(prompted_repo.contains(declaration), "{prompted_repo}");

    let drawn = flow.repo("flow-set-workflows-drawn");
    let repo_path = drawn.join("repo.kdl");
    let repo = std::fs::read_to_string(&repo_path).unwrap().replace(
        "workflows {",
        "workflows publish=\"scheduled\" scan=\"scheduled\" {",
    );
    std::fs::write(&repo_path, repo).unwrap();
    flow.drawn(
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

    let direct = flow.repo("flow-cadence-direct");
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

    let cadence = flow.repo("flow-publish-cadence");
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
}

fn setup_import_requires(flow: &Flow) {
    let target = flow.target;
    let requires = flow.repo_sourced("flow-requires");
    flow.run(
        target,
        &requires,
        None,
        &["--root", ".", "import", "module", "community/browser"],
    );
    let image = std::fs::read_to_string(requires.join("example.image.kdl")).unwrap();
    assert!(
        image.contains("source \"upstream\" {\n            module \"flatpak\"")
            && image.contains("source \"community\" {\n            module \"browser\""),
        "{image}"
    );
    flow.buildc(
        &requires,
        &[
            "--no-tui",
            "--root",
            ".",
            "import",
            "module",
            "upstream/fedora-family",
            "--image",
            "example",
        ],
    );
    flow.buildc(&requires, &["--no-tui", "--root", ".", "check"]);
}

fn setup_check_unmet(flow: &Flow) {
    let target = flow.target;
    let declined = flow.repo_sourced("flow-declined");
    flow.buildc(
        &declined,
        &[
            "--no-tui",
            "--root",
            ".",
            "import",
            "module",
            "community/browser",
            "--image",
            "example",
        ],
    );
    flow.buildc(
        &declined,
        &[
            "--no-tui",
            "--root",
            ".",
            "import",
            "module",
            "upstream/fedora-family",
            "--image",
            "example",
        ],
    );
    flow.run(target, &declined, None, &["--root", ".", "check"]);
}

fn setup_import_skip(flow: &Flow) {
    let skip = flow.repo_sourced("flow-skip");
    flow.buildc(
        &skip,
        &["--no-tui", "--root", ".", "create", "image", "Server"],
    );
    flow.buildc(
        &skip,
        &[
            "--no-tui",
            "--root",
            ".",
            "import",
            "module",
            "upstream/flatpak",
            "--image",
            "example",
        ],
    );
    flow.run(
        "flow-import-skip",
        &skip,
        None,
        &[
            "--root",
            ".",
            "import",
            "module",
            "community/browser",
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
    flow.buildc(
        &skip,
        &[
            "--no-tui",
            "--root",
            ".",
            "import",
            "module",
            "upstream/fedora-family",
            "--image",
            "example",
        ],
    );
    flow.buildc(&skip, &["--no-tui", "--root", ".", "fetch", "modules"]);
    flow.buildc(&skip, &["--no-tui", "--root", ".", "check"]);
}

fn setup_check_unfetched(flow: &Flow) {
    let target = flow.target;
    let unfetched = flow.repo("flow-unfetched");
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
    flow.run(target, &unfetched, None, &["--root", ".", "check"]);
}

fn setup_why_picker(flow: &Flow) {
    let target = flow.target;
    let why = flow.repo("flow-why-picker");
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
    flow.drawn(
        target,
        &why,
        &format!(
            "stty cols 80; '{}' --root . why",
            env!("CARGO_BIN_EXE_tect")
        ),
        "Which module?:",
        &[b"\r"],
    );
}

fn setup_import_kernel(flow: &Flow) {
    let target = flow.target;
    let kernel = flow.repo_sourced("flow-kernel");
    flow.run(
        target,
        &kernel,
        None,
        &["--root", ".", "import", "module", "upstream/custom-kernel"],
    );
    assert!(std::fs::read_to_string(kernel.join("repo.kdl"))
        .unwrap()
        .contains("    kernel-freshness\n"));
}

fn setup_import_module(flow: &Flow) {
    let target = flow.target;
    let root = flow.repo_sourced("flow-import");
    flow.run(target, &root, None, &["--root", ".", "import", "module"]);
    let image = std::fs::read_to_string(root.join("example.image.kdl")).unwrap();
    assert!(image.contains("source \"upstream\" {\n            module \"browser\"\n        }"));
    assert!(root
        .join("modules/.remote/upstream/browser/module.kdl")
        .is_file());
    assert!(!root.join("modules/browser").exists());
}

fn setup_import_default(flow: &Flow) {
    let target = flow.target;
    let defaulted = flow.repo("flow-import-default-in");
    let repo = defaulted.join("repo.kdl");
    let text = std::fs::read_to_string(&repo)
        .unwrap()
        .replace(&buildc::init::sources(&crate_dir().join("assets")), "")
        + "unfinished-repo-property \"kept for buildc check\"\n";
    std::fs::write(&repo, text).unwrap();
    let image_file = defaulted.join("example.image.kdl");
    let image = std::fs::read_to_string(&image_file).unwrap().replace(
        "    modules {",
        "    unfinished-image-property \"kept for buildc check\"\n    modules {",
    );
    std::fs::write(&image_file, image).unwrap();
    let (_, issues, _) = buildc::declarations(&defaulted);
    assert!(
        !issues.is_empty(),
        "the fixture must prove edits do not depend on a clean check"
    );
    let assets = flow.at("flow-import-default-assets");
    let _ = std::fs::remove_dir_all(&assets);
    std::fs::create_dir_all(&assets).unwrap();
    std::fs::write(
        assets.join(buildc::init::SOURCES_FILE),
        format!(
            "sources {{\n    upstream {:?}\n}}\n",
            crate_dir().join("tests/collections/upstream").display()
        ),
    )
    .unwrap();
    flow.run_with_assets(
        "flow-import-default",
        &defaulted,
        None,
        &[
            "--no-tui",
            "--root",
            ".",
            "import",
            "module",
            "upstream/browser",
            "--image",
            "example",
        ],
        &assets,
    );
    let declared = std::fs::read_to_string(&repo).unwrap();
    let image = std::fs::read_to_string(defaulted.join("example.image.kdl")).unwrap();
    assert!(
        declared.contains("sources {\n    upstream ")
            && image.contains("source \"upstream\" {\n            module \"browser\""),
        "{declared}\n{image}"
    );
    if target == "flow-import-default" {
        return;
    }
    flow.run(
        "flow-copy-with-schema-issues",
        &defaulted,
        None,
        &[
            "--no-tui",
            "--root",
            ".",
            "copy",
            "module",
            "upstream/flatpak",
            "--image",
            "example",
        ],
    );
    let image = std::fs::read_to_string(&image_file).unwrap();
    assert!(image.contains("module \"flatpak\""), "{image}");
    if target == "flow-copy-with-schema-issues" {
        return;
    }
    flow.run(
        "flow-create-module-with-schema-issues",
        &defaulted,
        None,
        &[
            "--no-tui",
            "--root",
            ".",
            "create",
            "module",
            "local-tool",
            "--pkg",
            "hello",
            "--with",
            "description=",
            "--with",
            "supports=",
            "--image",
            "example",
        ],
    );
    let image = std::fs::read_to_string(&image_file).unwrap();
    assert!(
        image.contains("module \"flatpak\"") && image.contains("module \"local-tool\""),
        "{image}"
    );
}

fn setup_import_edit_guards(flow: &Flow) {
    let malformed = flow.repo_sourced("flow-malformed-edit-in");
    let image_file = malformed.join("example.image.kdl");
    let mut image = std::fs::read_to_string(&image_file).unwrap();
    image.push_str("image {\n");
    std::fs::write(&image_file, image).unwrap();
    let before = contents(&malformed);
    for args in [
        vec![
            "--no-tui",
            "--root",
            ".",
            "import",
            "module",
            "upstream/browser",
            "--image",
            "example",
        ],
        vec![
            "--no-tui",
            "--root",
            ".",
            "copy",
            "module",
            "upstream/flatpak",
            "--image",
            "example",
        ],
        vec![
            "--no-tui",
            "--root",
            ".",
            "create",
            "module",
            "local-tool",
            "--with",
            "description=",
            "--with",
            "supports=",
            "--image",
            "example",
        ],
    ] {
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_tect"));
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
            "flow-import-without-modules",
            vec![
                "--no-tui",
                "--root",
                ".",
                "import",
                "module",
                "upstream/browser",
                "--image",
                "example",
            ],
            "source \"upstream\" {\n            module \"browser\"",
        ),
        (
            "flow-copy-without-modules",
            vec![
                "--no-tui",
                "--root",
                ".",
                "copy",
                "module",
                "upstream/flatpak",
                "--image",
                "example",
            ],
            "modules {\n        module \"flatpak\"",
        ),
        (
            "flow-create-without-modules",
            vec![
                "--no-tui",
                "--root",
                ".",
                "create",
                "module",
                "local-tool",
                "--with",
                "description=",
                "--with",
                "supports=",
                "--image",
                "example",
            ],
            "modules {\n        module \"local-tool\"",
        ),
    ] {
        let root = flow.repo_sourced(name);
        let image_file = root.join("example.image.kdl");
        let image = std::fs::read_to_string(&image_file)
            .unwrap()
            .replace("    modules {\n    }\n", "");
        assert!(!image.contains("modules {"), "fixture still has modules");
        std::fs::write(&image_file, image).unwrap();
        flow.buildc(&root, &args);
        let image = std::fs::read_to_string(&image_file).unwrap();
        assert!(image.contains(declaration), "{image}");
    }

    let root = flow.repo_sourced("flow-import-guards");
    flow.buildc(
        &root,
        &[
            "--no-tui",
            "--root",
            ".",
            "import",
            "module",
            "upstream/browser",
            "--image",
            "example",
        ],
    );
    let (list, issues, _) = buildc::declarations(&root);
    assert!(issues.is_empty(), "{}", issues.plain());
    let declined = buildc::import::Module::collect(
        Some("upstream/flatpak".into()),
        &root,
        &list.sources,
        false,
        Vec::new(),
        None,
        buildc::import::Place::Reference,
        &common::prompt::Prompt::silent(),
    )
    .unwrap_or_else(|err| panic!("{}", err.message()))
    .apply(&root, &list.sources)
    .unwrap_err();
    assert!(declined.contains("--image"), "{declined}");

    buildc::import::Module::collect(
        Some("upstream/flatpak".into()),
        &root,
        &list.sources,
        false,
        vec!["example".into()],
        None,
        buildc::import::Place::Reference,
        &common::prompt::Prompt::silent(),
    )
    .unwrap_or_else(|err| panic!("{}", err.message()))
    .apply(&root, &list.sources)
    .unwrap();
    let image = std::fs::read_to_string(root.join("example.image.kdl")).unwrap();
    assert_eq!(image.matches("source \"upstream\"").count(), 1);
    assert!(image.contains("module \"browser\"\n            module \"flatpak\""));

    let twice = buildc::import::Module::collect(
        Some("upstream/flatpak".into()),
        &root,
        &list.sources,
        false,
        vec!["example".into()],
        None,
        buildc::import::Place::Reference,
        &common::prompt::Prompt::silent(),
    )
    .err()
    .map(|err| err.message().to_string())
    .unwrap_or_default();
    assert_eq!(twice, "`example` already lists `flatpak`");
}

fn setup_import_several(flow: &Flow) {
    let several = flow.repo_sourced("flow-several");
    flow.run(
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
    flow.buildc(
        &several,
        &[
            "--no-tui",
            "--root",
            ".",
            "import",
            "module",
            "upstream/fedora-family",
            "--image",
            "example",
        ],
    );
    flow.buildc(&several, &["--no-tui", "--root", ".", "check"]);
}

fn setup_import_nested(flow: &Flow) {
    let nested = flow.repo_with("flow-nested", "grouped");
    flow.run(
        "flow-import-nested",
        &nested,
        None,
        &["--root", ".", "import", "module"],
    );
    let image = std::fs::read_to_string(nested.join("example.image.kdl")).unwrap();
    assert!(
        image.contains(
            "source \"grouped\" {\n            module \"hardening/coredumps\"\n        }"
        ),
        "{image}"
    );
    assert!(nested
        .join("modules/.remote/grouped/hardening/coredumps/module.kdl")
        .is_file());
    flow.buildc(&nested, &["--no-tui", "--root", ".", "fetch", "modules"]);
    flow.buildc(&nested, &["--no-tui", "--root", ".", "check"]);
    flow.buildc(&nested, &["--no-tui", "--root", ".", "generate"]);
    assert!(nested
        .join("generated/example/modules/grouped/hardening/coredumps.sh")
        .is_file());
    let (list, _, _) = buildc::declarations(&nested);
    let refused = buildc::import::find(
        &nested,
        &list.sources,
        "grouped/hardening//coredumps",
        false,
    )
    .err()
    .expect("an empty part of a path is refused");
    assert_eq!(
        refused,
        "`grouped/hardening//coredumps` is not a module: a module is named by a path of names, \
             as `<path>`, or `<owner>/<path>` to name one collection, and no part of it may be \
             empty or start with a dot"
    );
}

fn setup_import_suffix(flow: &Flow) {
    let suffix = flow.repo_with("flow-suffix", "grouped");
    flow.run(
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
        image.contains(
            "source \"grouped\" {\n            module \"hardening/coredumps\"\n        }"
        ),
        "{image}"
    );
    assert!(!image.contains("module \"coredumps\""), "{image}");
    flow.buildc(&suffix, &["--no-tui", "--root", ".", "fetch", "modules"]);
    flow.buildc(&suffix, &["--no-tui", "--root", ".", "check"]);
    flow.buildc(&suffix, &["--no-tui", "--root", ".", "generate"]);
    assert!(suffix
        .join("generated/example/modules/grouped/hardening/coredumps.sh")
        .is_file());
}

fn setup_import_suffix_ambiguous(flow: &Flow) {
    let both = flow.repo("flow-suffix-ambiguous");
    let collections = crate_dir().join("tests/collections");
    let mut repo = std::fs::read_to_string(both.join("repo.kdl"))
        .unwrap()
        .replace(&buildc::init::sources(&crate_dir().join("assets")), "");
    repo.push_str(&format!(
        "sources {{\n    grouped {:?}\n    namesake {:?}\n}}\n",
        collections.join("grouped").display(),
        collections.join("namesake").display()
    ));
    std::fs::write(both.join("repo.kdl"), repo).unwrap();
    flow.run(
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
        image.contains(
            "source \"grouped\" {\n            module \"hardening/coredumps\"\n        }"
        ),
        "{image}"
    );
    assert!(!image.contains("namesake"), "{image}");
    assert!(!image.contains("sandbox"), "{image}");
}

fn setup_collides(flow: &Flow) {
    let target = flow.target;
    let collide = flow.repo_sourced("flow-collides");
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
    flow.buildc(
        &collide,
        &[
            "--no-tui",
            "--root",
            ".",
            "import",
            "module",
            "upstream/fedora-family",
            "--image",
            "example",
        ],
    );
    flow.run(
        "flow-import-collides",
        &collide,
        None,
        &[
            "--root",
            ".",
            "import",
            "module",
            "upstream/flatpak",
            "--image",
            "example",
        ],
    );
    assert!(
        buildc::run(Command::Check, None, &collide)
            .issues
            .plain()
            .contains("`upstream/flatpak` overwrites `/usr/share/example/remotes.list`"),
        "check reports the collision the import said"
    );
    if target == "flow-import-collides" {
        return;
    }
    flow.run(target, &collide, None, &["--root", ".", "check"]);
}

fn setup_copy_nested(flow: &Flow) {
    let copied_nested = flow.repo_with("flow-copy-nested", "grouped");
    flow.run(
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
    flow.buildc(&copied_nested, &["--no-tui", "--root", ".", "check"]);
    flow.buildc(&copied_nested, &["--no-tui", "--root", ".", "generate"]);
    assert!(copied_nested
        .join("generated/example/modules/hardening/coredumps.sh")
        .is_file());
}

fn setup_copy_collides(flow: &Flow) {
    let remotes = "modules/editor/files/usr/share/example";
    let copied = flow.repo_sourced("flow-copy-collides");
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
    flow.run(
        "flow-copy-collides",
        &copied,
        None,
        &[
            "--root",
            ".",
            "copy",
            "module",
            "upstream/flatpak",
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
}

fn setup_copy_module(flow: &Flow) {
    let target = flow.target;
    let root = flow.repo_sourced("flow-copy");
    flow.run(target, &root, None, &["--root", ".", "copy", "module"]);
    assert!(root.join("modules/browser/provenance.kdl").is_file());
    assert!(!root.join("modules/upstream").exists());
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
    std::fs::create_dir_all(root.join("modules/signed-kernel")).unwrap();
    std::fs::write(root.join("modules/signed-kernel/module.kdl"), KEYHOLDER).unwrap();
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
    flow_check_shadow => "flow-check-shadow",
    flow_check_unfetched => "flow-check-unfetched",
    flow_check_unmet => "flow-check-unmet",
    flow_check_unpinned => "flow-check-unpinned",
    flow_copy_collides => "flow-copy-collides",
    flow_copy_conforms => "flow-copy-conforms",
    flow_copy_module => "flow-copy-module",
    flow_copy_nested => "flow-copy-nested",
    flow_copy_with_schema_issues => "flow-copy-with-schema-issues",
    flow_coverage => "flow-coverage",
    flow_create_flavour => "flow-create-flavour",
    flow_create_image => "flow-create-image",
    flow_create_module => "flow-create-module",
    flow_create_module_with_schema_issues => "flow-create-module-with-schema-issues",
    flow_create_repo => "flow-create-repo",
    flow_create_repo_forgejo => "flow-create-repo-forgejo",
    flow_create_repo_no_gh => "flow-create-repo-no-gh",
    flow_create_repo_offer => "flow-create-repo-offer",
    flow_create_repo_signed_in => "flow-create-repo-signed-in",
    flow_create_repo_signed_out => "flow-create-repo-signed-out",
    flow_image_default => "flow-image-default",
    flow_import_collides => "flow-import-collides",
    flow_import_conforms => "flow-import-conforms",
    flow_import_conforms_declined => "flow-import-conforms-declined",
    flow_import_conforms_enforced => "flow-import-conforms-enforced",
    flow_import_conforms_two => "flow-import-conforms-two",
    flow_import_datastream => "flow-import-datastream",
    flow_import_default => "flow-import-default",
    flow_import_family => "flow-import-family",
    flow_import_kernel => "flow-import-kernel",
    flow_import_module => "flow-import-module",
    flow_import_nested => "flow-import-nested",
    flow_import_requires => "flow-import-requires",
    flow_import_several => "flow-import-several",
    flow_import_skip => "flow-import-skip",
    flow_import_suffix => "flow-import-suffix",
    flow_import_suffix_ambiguous => "flow-import-suffix-ambiguous",
    flow_key_absent => "flow-key-absent",
    flow_key_kinds => "flow-key-kinds",
    flow_key_no_kind => "flow-key-no-kind",
    flow_key_undeclared => "flow-key-undeclared",
    flow_module_in_flavour => "flow-module-in-flavour",
    flow_module_taken => "flow-module-taken",
    flow_module_two_images => "flow-module-two-images",
    flow_set_claims => "flow-set-claims",
    flow_set_claims_again => "flow-set-claims-again",
    flow_set_claims_drawn => "flow-set-claims-drawn",
    flow_set_claims_fetched => "flow-set-claims-fetched",
    flow_set_claims_two => "flow-set-claims-two",
    flow_set_conforms => "flow-set-conforms",
    flow_set_conforms_again => "flow-set-conforms-again",
    flow_set_conforms_enforced => "flow-set-conforms-enforced",
    flow_set_workflows => "flow-set-workflows",
    flow_set_workflows_drawn => "flow-set-workflows-drawn",
    flow_unanswered => "flow-unanswered",
    flow_why_picker => "flow-why-picker",
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

#[test]
fn flow_create_repo_drawn() {
    let flow = Flow::new("flow-create-repo-drawn");
    let dir = flow.empty("flow-create-repo-drawn-in");
    flow.drawn(
        "flow-create-repo-drawn",
        &dir,
        &format!(
            "env PATH='{}' HOME='{}' GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null \
             '{}' create repo",
            flow.bin("flow-create-repo-drawn", None).display(),
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
            b"\r",
            b"\x1b[B\x1b[B\x1b[B\x1b[B\x1b[B\x1b[B\x1b[B\x1b[B\x1b[B\r",
        ],
    );
    flow.done();
    assert!(dir.join("example/repo.kdl").is_file());
}

#[test]
fn escape_cancels_create_image_at_each_prompt() {
    let flow = Flow::new("escape-create-image");
    let made = flow.repo_sourced("flow-create-image-cancel-in");
    let root = made.parent().unwrap().join("cancel-default");
    std::fs::rename(&made, &root).unwrap();
    let binary = env!("CARGO_BIN_EXE_tect");
    for (command, steps, absent) in [
        (
            format!("'{binary}' --root . create image"),
            vec![b"\x1b".as_slice()],
            "cancel-default.image.kdl",
        ),
        (
            format!("'{binary}' --root . create image"),
            vec![b"Beta\r".as_slice(), b"\x1b".as_slice()],
            "beta.image.kdl",
        ),
        (
            format!("'{binary}' --root . create image Gamma --base example.invalid/collected:1"),
            vec![b"\x1b".as_slice()],
            "gamma.image.kdl",
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
}
