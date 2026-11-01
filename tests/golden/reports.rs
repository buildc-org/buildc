use crate::harness::{assert_golden, copy, crate_dir, tmp};

use std::path::{Path, PathBuf};

fn generated(fixture: &Path, name: &str) -> PathBuf {
    let root = tmp().join(name);
    if let Err(err) = std::fs::remove_dir_all(&root) {
        assert_eq!(
            err.kind(),
            std::io::ErrorKind::NotFound,
            "{}: {err}",
            root.display()
        );
    }
    copy(fixture, &root);
    std::env::set_var("BUILDC_ASSETS", crate_dir().join("assets"));
    let (_, said, code) = scap_run(&root, &["--root", ".", "generate"]);
    assert_eq!(code, 0, "generate refused {}: {said}", fixture.display());
    root
}

fn scap_run(root: &Path, args: &[&str]) -> (String, String, i32) {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_buildc"))
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

    assert_golden("coverage", "report.txt", &out);
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

    for (heading, at, named) in [
        (
            "the content it is measured with",
            generated(
                &crate_dir().join("tests/repos/multi-image"),
                "scap-content-measured",
            ),
            &["--target", "workstation"][..],
        ),
        (
            "and for an image declaring claims but no profile",
            generated(&claim_only, "scap-content-claims"),
            &[][..],
        ),
        (
            "and for an image declaring neither a claim nor a profile",
            generated(
                &crate_dir().join("tests/repos/minimal"),
                "scap-content-minimal",
            ),
            &[][..],
        ),
    ] {
        let mut args = vec!["--root", ".", "scap", "content"];
        args.extend_from_slice(named);
        let (path, _, code) = scap_run(&at, &args);
        out.push_str(&format!("==== {heading}\n{path}==== exit {code}\n"));
    }

    assert_golden("scap", "report.txt", &out);
}
