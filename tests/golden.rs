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
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|err| {
        panic!("{}: {err}\nrun UPDATE_GOLDEN=1 cargo test", path.display())
    });
    assert!(
        expected == actual,
        "{} changed. Rerun with UPDATE_GOLDEN=1 and read the diff",
        path.display()
    );
}

fn capture(name: &str, root: &Path) {
    std::env::set_current_dir(root).expect("fixture root exists");
    let here = Path::new(".");
    for (command, file) in [("plan", "plan.json"), ("section", "section.txt")] {
        compare(name, file, &buildc::run(command, None, here).stdout);
    }
    compare(name, "issues.txt", &buildc::run("check", None, here).issues.plain());
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

    for name in names {
        capture(&name, &dir.join(&name));
    }
}
