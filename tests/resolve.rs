use buildc::Command;
use std::path::PathBuf;

fn unprovided(dir: &str, line: &str) -> PathBuf {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(dir);
    let _ = std::fs::remove_dir_all(&root);
    for component in ["signed", "plain"] {
        std::fs::create_dir_all(root.join("components").join(component))
            .expect("the test target directory is writable");
    }
    std::fs::write(
        root.join("repo.kdl"),
        "schema-version 1\nname \"After\"\nbuilds { ordered }\n",
    )
    .expect("the test target directory is writable");
    write_base(&root, "fedora", "");
    std::fs::write(
        root.join("components/ordered.kdl"),
        rooted(
            r#"schema-version 1

base-image {
    example
}
signed
plain
"#,
        ),
    )
    .expect("the test target directory is writable");
    std::fs::write(
        root.join("components/signed/manifest.kdl"),
        format!(
            "schema-version 1\n\ndescription \"Signs when a key exists\"\ntargets \"fedora\"\n{line} \"secureboot-key\"\n"
        ),
    )
    .expect("the test target directory is writable");
    std::fs::write(
        root.join("components/plain/manifest.kdl"),
        "schema-version 1\n\ndescription \"Orders nothing\"\ntargets \"fedora\"\n",
    )
    .expect("the test target directory is writable");
    root
}

fn capability_row(root: &std::path::Path, name: &str) -> (Vec<String>, bool) {
    buildc::run(Command::Graph, None, root)
        .report
        .parts
        .into_iter()
        .find_map(|part| match part {
            buildc::emit::Part::Table(table) if table.title == "Capabilities" => {
                table.rows.into_iter().find(|(cells, _)| cells[0] == name)
            }
            _ => None,
        })
        .expect("the capability table holds the row")
}

fn key_row_is_a_defect(root: &std::path::Path) -> bool {
    capability_row(root, "secureboot-key").1
}

#[test]
fn after_with_no_provider_orders_nothing() {
    let root = unprovided("after-no-provider", "after");
    assert_eq!(
        buildc::run(Command::Check, None, &root)
            .problems
            .issues
            .plain(),
        ""
    );
    let plan = buildc::run(Command::Plan, None, &root).report.stdout;
    let at = |name: &str| plan.find(&format!("\"path\": \"{name}\"")).unwrap();
    assert!(at("signed") < at("plain"), "{plan}");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn graph_does_not_flag_an_after_with_no_provider() {
    let root = unprovided("after-no-provider-graph", "after");
    assert!(!key_row_is_a_defect(&root));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn graph_flags_a_requires_with_no_provider() {
    let root = unprovided("requires-no-provider-graph", "requires");
    assert!(key_row_is_a_defect(&root));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn shipped_policy_orders_a_component_after_the_mac_that_installs_it() {
    std::env::set_var(
        "BUILDC_ASSETS",
        concat!(env!("CARGO_MANIFEST_DIR"), "/assets"),
    );
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("policy-ordering");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("components/apparmor")).unwrap();
    std::fs::create_dir_all(root.join("components/yubikey/apparmor")).unwrap();
    std::fs::write(
        root.join("repo.kdl"),
        "schema-version 1\nname \"Mac\"\nbuilds { mac }\n",
    )
    .unwrap();
    write_base(&root, "debian", "");
    std::fs::write(
        root.join("components/mac.kdl"),
        rooted(
            r#"schema-version 1

base-image {
    example
}
yubikey
apparmor
"#,
        ),
    )
    .unwrap();
    std::fs::write(
        root.join("components/apparmor/manifest.kdl"),
        "schema-version 1\n\ndescription \"The MAC\"\ntargets \"debian\"\nprovides \"apparmor-policy\"\n",
    )
    .unwrap();
    std::fs::write(
        root.join("components/yubikey/manifest.kdl"),
        "schema-version 1\n\ndescription \"Ships a profile and requires nothing\"\ntargets \"debian\"\n",
    )
    .unwrap();
    std::fs::write(root.join("components/yubikey/apparmor/usr.sbin.pcscd"), "").unwrap();

    let plan = buildc::run(Command::Plan, None, &root).report.stdout;
    let at = |name: &str| plan.find(&format!("\"path\": \"{name}\"")).unwrap();
    assert!(at("apparmor") < at("yubikey"), "{plan}");
    assert_eq!(
        buildc::run(Command::Check, None, &root)
            .problems
            .issues
            .plain(),
        ""
    );

    std::fs::write(
        root.join("components/mac.kdl"),
        std::fs::read_to_string(root.join("components/mac.kdl"))
            .unwrap()
            .replace("apparmor\n", ""),
    )
    .unwrap();
    assert_eq!(
        buildc::run(Command::Check, None, &root)
            .problems
            .issues
            .plain(),
        ""
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_capability_conflicts_only_where_a_provider_declares_it_exclusive() {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("exclusive-provider");
    let _ = std::fs::remove_dir_all(&root);
    for component in ["one", "two"] {
        std::fs::create_dir_all(root.join("components").join(component)).unwrap();
    }
    std::fs::write(
        root.join("repo.kdl"),
        "schema-version 1\nname \"Two\"\nbuilds { image }\n",
    )
    .unwrap();
    write_base(&root, "fedora", "");
    write_image(
        &root,
        "image",
        "schema-version 1\n\nbase-image {\n    example\n}\none\ntwo\n",
    );
    let provider = |name: &str, provides: &str| {
        std::fs::write(
            root.join("components").join(name).join("manifest.kdl"),
            format!(
                "schema-version 1\n\ndescription \"Provides\"\ntargets \"fedora\"\n{provides}\n"
            ),
        )
        .unwrap();
    };
    provider("one", "provides \"tool\" witness=\"/usr/share/tool\"");
    provider("two", "provides \"tool\" witness=\"/usr/share/tool\"");
    let issues = buildc::run(Command::Check, None, &root)
        .problems
        .issues
        .plain();
    assert!(
        !issues.contains("more than one enabled component"),
        "{issues}"
    );

    provider(
        "two",
        "provides \"tool\" witness=\"/usr/share/tool\" exclusive=#true",
    );
    let issues = buildc::run(Command::Check, None, &root)
        .problems
        .issues
        .plain();
    assert!(
        issues.contains("`tool` is provided by more than one enabled component"),
        "{issues}"
    );
    assert!(issues.contains("`two` declares it `exclusive`"), "{issues}");
    let _ = std::fs::remove_dir_all(root);
}

const CHECKED_REPO: &str = "schema-version 1\nname \"Checked\"\nbuilds { checked }\n";

fn rooted(manifest: &str) -> String {
    let Ok(doc) = manifest.parse::<kdl::KdlDocument>() else {
        return manifest.to_string();
    };
    let schema = &buildc::parse::component::COMPONENT;
    let mut kept = manifest.to_string();
    let mut tree: Vec<String> = Vec::new();
    for node in doc.nodes().iter().rev() {
        let name = node.name().value();
        if schema
            .children
            .iter()
            .any(|child| !child.name.is_empty() && child.name == name)
        {
            continue;
        }
        let span = node.span();
        let (from, to) = (span.offset(), span.offset() + span.len());
        tree.insert(0, manifest[from..to].to_string());
        kept.replace_range(from..to, "");
    }
    match tree.is_empty() {
        true => kept,
        false => format!("{kept}\ncomponent \"/\" {{\n{}\n}}\n", tree.join("\n")),
    }
}

fn write_image(root: &std::path::Path, id: &str, text: &str) {
    std::fs::create_dir_all(root.join("components")).expect("the test repository is writable");
    std::fs::write(root.join(format!("components/{id}.kdl")), rooted(text))
        .expect("the test repository is writable");
}

fn written(name: &str, components: &str, manifests: &[(&str, &str)]) -> PathBuf {
    written_on(name, "", components, manifests)
}

fn written_on(name: &str, base: &str, components: &str, manifests: &[(&str, &str)]) -> PathBuf {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("the test repository is writable");
    std::fs::write(root.join("repo.kdl"), CHECKED_REPO).expect("the test repository is writable");
    write_image(
        &root,
        "checked",
        &format!(
            "schema-version 1\n\ncomponent \"/\" {{\nbase-image {{ example }}\n{components}\n}}\n"
        ),
    );
    write_base(&root, "fedora", &base.replace("; ", "\n"));
    for (component, manifest) in manifests {
        std::fs::create_dir_all(root.join("components").join(component))
            .expect("the test repository is writable");
        std::fs::write(
            root.join("components").join(component).join("manifest.kdl"),
            rooted(&format!(
                "schema-version 1\n\ndescription \"Checked\"\ntargets \"fedora\"\n{manifest}\n"
            )),
        )
        .expect("the test repository is writable");
    }
    root
}

fn checked(name: &str, components: &str, manifests: &[(&str, &str)]) -> String {
    let root = written(name, components, manifests);
    let issues = buildc::run(Command::Check, None, &root)
        .problems
        .issues
        .plain();
    let _ = std::fs::remove_dir_all(root);
    issues
}

fn checked_on(name: &str, base: &str, components: &str, manifests: &[(&str, &str)]) -> String {
    let root = written_on(name, base, components, manifests);
    let issues = buildc::run(Command::Check, None, &root)
        .problems
        .issues
        .plain();
    let _ = std::fs::remove_dir_all(root);
    issues
}

#[test]
fn a_moving_base_defers_its_version_floor_to_the_build() {
    let issues = checked_on(
        "moving-version",
        "provides \"libostree\" version=\"2025.2\" package=\"libostree-1-1\"",
        "        needs",
        &[("needs", "requires \"libostree\" min=\"2025.3\"")],
    );
    assert!(
        !issues.contains("requires `libostree` at or above"),
        "{issues}"
    );
}

#[test]
fn a_component_builds_after_every_provider_of_what_it_requires() {
    let provider = "provides \"tool\" witness=\"/usr/share/tool\"";
    let root = written(
        "order-every-provider",
        "        consumer\n        one\n        two",
        &[
            ("consumer", "requires \"tool\""),
            ("one", provider),
            ("two", provider),
        ],
    );
    let plan = buildc::run(Command::Plan, None, &root).report.stdout;
    let at = |name: &str| plan.find(&format!("\"path\": \"{name}\"")).unwrap();
    assert!(at("one") < at("consumer"), "{plan}");
    assert!(at("two") < at("consumer"), "{plan}");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_lone_provider_that_declares_exclusive_is_no_conflict() {
    let issues = checked(
        "exclusive-alone",
        "        solo",
        &[(
            "solo",
            "provides \"tool\" witness=\"/usr/share/tool\" exclusive=#true",
        )],
    );
    assert!(
        !issues.contains("more than one enabled component"),
        "{issues}"
    );
}

#[test]
fn two_keys_of_one_kind_conflict_unless_they_are_the_same_key() {
    let key = |bits: u32| {
        format!(
            "key \"secureboot\" {{\n    generator \"openssl\" profile=\"module-signing\" bits={bits}\n    public \"/usr/share/secureboot/sb_cert.der\" format=\"der\"\n    private \"MOK.priv\"\n}}"
        )
    };
    let components = "        one\n        two";
    let same = checked(
        "same-key",
        components,
        &[("one", &key(4096)), ("two", &key(4096))],
    );
    assert!(!same.contains("more than one enabled component"), "{same}");
    let differ = checked(
        "different-key",
        components,
        &[("one", &key(4096)), ("two", &key(2048))],
    );
    assert!(
        differ.contains("`secureboot-key` is provided by more than one enabled component"),
        "{differ}"
    );
}

#[test]
fn a_requirement_is_met_by_any_provider_that_reaches_it() {
    let provider = "provides \"tool\" witness=\"/usr/share/tool\"";
    let met = checked(
        "requires-any",
        "        consumer\n        one\n        two",
        &[
            ("consumer", "requires \"tool\""),
            ("one", provider),
            ("two", provider),
        ],
    );
    assert!(!met.contains("requires `tool`"), "{met}");
}

fn planned(name: &str, base: &str, components: &str, manifests: &[(&str, &str)]) -> String {
    let root = written_on(name, base, components, manifests);
    let plan = buildc::run(Command::Plan, None, &root).report.stdout;
    let _ = std::fs::remove_dir_all(root);
    plan
}

#[test]
fn an_imported_component_builds_and_records_its_importer() {
    let root = written(
        "imports-builds",
        "        consumer",
        &[
            ("consumer", "requires \"tool\"\nprovider"),
            ("provider", "provides \"tool\" witness=\"/usr/share/tool\""),
        ],
    );
    let plan = buildc::run(Command::Plan, None, &root).report.stdout;
    let issues = buildc::run(Command::Check, None, &root)
        .problems
        .issues
        .plain();
    let _ = std::fs::remove_dir_all(root);
    let at = |name: &str| plan.find(&format!("\"path\": \"{name}\"")).unwrap();
    assert!(at("provider") < at("consumer"), "{plan}");
    let flat = squashed(&plan);
    assert!(
        flat.contains(
            r#""met_by":{"tool":{"how":"imported","provider":"provider","by":"consumer"}}"#
        ),
        "{plan}"
    );
    assert!(
        flat.contains(r#""origin":{"how":"imported","by":"consumer"}"#),
        "{plan}"
    );
    assert!(issues.is_empty(), "{issues}");
}

#[test]
fn an_unmet_requirement_fails_while_its_provider_sits_unlisted() {
    let root = written(
        "imports-unlisted",
        "        consumer",
        &[
            ("consumer", "requires \"tool\""),
            ("provider", "provides \"tool\" witness=\"/usr/share/tool\""),
        ],
    );
    let plan = buildc::run(Command::Plan, None, &root).report.stdout;
    let issues = buildc::run(Command::Check, None, &root)
        .problems
        .issues
        .plain();
    let _ = std::fs::remove_dir_all(root);
    assert!(!plan.contains("\"path\": \"provider\""), "{plan}");
    assert!(
        issues.contains("`consumer` requires `tool`, which nothing enabled provides"),
        "{issues}"
    );
    assert!(
        issues.contains("`provider` would satisfy it; add it to this image"),
        "{issues}"
    );
}

#[test]
fn a_fact_the_base_provides_meets_a_requirement() {
    let issues = checked_on(
        "imports-base-fact",
        "; provides \"tool\"",
        "        consumer",
        &[("consumer", "requires \"tool\"")],
    );
    assert!(!issues.contains("requires `tool`"), "{issues}");
}

#[test]
fn a_base_with_no_family_imports_a_component_that_names_one() {
    let root = written_on(
        "imports-no-family",
        "; provider",
        "        consumer",
        &[
            ("consumer", "requires \"tool\""),
            ("provider", "provides \"tool\" witness=\"/usr/share/tool\""),
        ],
    );
    let base = root.join("components/base-image/example/manifest.kdl");
    let text = std::fs::read_to_string(&base).expect("the test repository is readable");
    assert!(text.contains(" { family \"fedora\"; }"), "{text}");
    std::fs::write(&base, text.replace(" { family \"fedora\"; }", ""))
        .expect("the test repository is writable");
    let plan = buildc::run(Command::Plan, None, &root).report.stdout;
    let issues = buildc::run(Command::Check, None, &root)
        .problems
        .issues
        .plain();
    let _ = std::fs::remove_dir_all(root);
    assert!(plan.contains("\"path\": \"provider\""), "{plan}");
    assert!(!issues.contains("requires `tool`"), "{issues}");
}

#[test]
fn a_component_required_by_its_path_is_not_suppressed() {
    let root = written_on(
        "suppress-by-path",
        "; provides \"flatpak\"",
        "        consumer\n        helper",
        &[
            ("consumer", "requires \"helper\""),
            ("helper", "provides \"flatpak\""),
        ],
    );
    let issues = buildc::run(Command::Check, None, &root)
        .problems
        .issues
        .plain();
    let _ = std::fs::remove_dir_all(root);
    assert!(!issues.contains("requires `helper`"), "{issues}");
}

#[test]
fn a_component_meets_a_requirement_for_its_own_path() {
    let components = "        consumer\n        helper";
    let manifests = [("consumer", "requires \"helper\""), ("helper", "")];
    let issues = checked("closure-self-checked", components, &manifests);
    assert!(!issues.contains("requires `helper`"), "{issues}");
    let plan = planned("closure-self", "", components, &manifests);
    assert!(
        squashed(&plan).contains(r#""met_by":{"helper":{"how":"listed","provider":"helper"}}"#),
        "{plan}"
    );
}

#[test]
fn a_declared_name_meets_its_ancestor_and_a_component_path_does_not() {
    let declared = checked(
        "hierarchy-declared",
        "        consumer\n        manager",
        &[
            ("consumer", "requires \"rpm\""),
            ("manager", "provides \"rpm/dnf5\""),
        ],
    );
    assert!(!declared.contains("requires `rpm`"), "{declared}");

    let pathed = checked(
        "hierarchy-path",
        "        consumer\n        rpm {\n            dnf5\n        }",
        &[("consumer", "requires \"rpm\""), ("rpm/dnf5", "")],
    );
    assert!(
        pathed.contains("`consumer` requires `rpm`, which nothing enabled provides"),
        "{pathed}"
    );
}

#[test]
fn two_unlisted_candidates_fail_naming_both() {
    let provider = "provides \"tool\" witness=\"/usr/share/tool\"";
    let root = written(
        "closure-ambiguous",
        "        consumer",
        &[
            ("consumer", "requires \"tool\""),
            ("one", provider),
            ("two", provider),
        ],
    );
    let run = buildc::run(Command::Generate, None, &root);
    let _ = std::fs::remove_dir_all(root);
    let issues = run.problems.issues.plain();
    assert!(
        issues.contains("`consumer` requires `tool`, which nothing enabled provides"),
        "{issues}"
    );
    assert!(
        issues.contains("`one` and `two` each provide it"),
        "{issues}"
    );
}

#[test]
fn a_base_import_builds_in_the_image() {
    let plan = planned(
        "imports-base",
        "; provider",
        "        consumer",
        &[
            ("consumer", "requires \"tool\""),
            ("provider", "provides \"tool\" witness=\"/usr/share/tool\""),
        ],
    );
    assert!(
        squashed(&plan).contains(r#""origin":{"how":"imported","by":"base-image/example"}"#),
        "{plan}"
    );
}

fn merged(name: &str, components: &str, manifests: &[(&str, &str)]) -> String {
    let root = written(name, components, manifests);
    let run = buildc::run(Command::Plan, None, &root);
    let _ = std::fs::remove_dir_all(root);
    let issues = run.problems.issues.plain();
    assert!(issues.is_empty(), "{issues}");
    run.report.stdout
}

#[test]
fn the_image_value_replaces_the_value_of_a_component_it_lists() {
    let plan = merged(
        "merge-image-nearest",
        "        one\n        runtime { driver \"btrfs\"; }",
        &[("one", "runtime { driver \"vfs\"; }"), ("runtime", RUNTIME)],
    );
    assert!(
        squashed(&plan).contains(r#""options":{"OPT_DRIVER":"btrfs"}"#),
        "{plan}"
    );
}

#[test]
fn a_nearer_value_wins_whichever_importer_is_read_first() {
    let plan = merged(
        "merge-read-order",
        "        inner\n        outer",
        &[
            ("inner", "runtime { driver \"vfs\"; }"),
            ("outer", "inner\nruntime { driver \"btrfs\"; }"),
            ("runtime", RUNTIME),
        ],
    );
    assert!(
        squashed(&plan).contains(r#""options":{"OPT_DRIVER":"btrfs"}"#),
        "{plan}"
    );
}

const APPS: &str = "option \"apps\" type=\"list\" { default \"a\"; }";

#[test]
fn a_list_appends_to_its_default_and_a_remove_takes_a_value_out() {
    let plan = merged(
        "merge-list",
        "        one\n        flatpak { apps \"c\" { remove \"a\"; }; }",
        &[("one", "flatpak { apps \"b\"; }"), ("flatpak", APPS)],
    );
    assert!(
        squashed(&plan).contains(r#""options":{"OPT_APPS":"bc"}"#),
        "{plan}"
    );
}

#[test]
fn a_block_of_removes_alone_sets_a_list() {
    let plan = merged(
        "merge-remove-only",
        "        flatpak { apps { remove \"a\"; }; }",
        &[("flatpak", APPS)],
    );
    assert!(
        squashed(&plan).contains(r#""options":{"OPT_APPS":""}"#),
        "{plan}"
    );
}

#[test]
fn a_remove_of_a_value_the_list_does_not_hold_is_refused() {
    let issues = checked(
        "merge-remove-absent",
        "        flatpak { apps { remove \"z\"; }; }",
        &[("flatpak", APPS)],
    );
    assert!(
        issues.contains("`checked` removes \"z\" from `apps` on `flatpak`, which does not hold it"),
        "{issues}"
    );
}

#[test]
fn a_remove_of_a_string_value_is_refused() {
    let issues = checked(
        "merge-remove-string",
        "        runtime { driver { remove \"overlay\"; }; }",
        &[("runtime", RUNTIME)],
    );
    assert!(issues.contains("which is no list"), "{issues}");
}

#[test]
fn a_value_that_lists_both_siblings_settles_their_clash() {
    let plan = merged(
        "merge-settled-clash",
        "        one\n        two\n        runtime { driver \"zfs\"; }",
        &[
            ("one", "runtime { driver \"vfs\"; }"),
            ("two", "runtime { driver \"btrfs\"; }"),
            ("runtime", RUNTIME),
        ],
    );
    assert!(
        squashed(&plan).contains(r#""options":{"OPT_DRIVER":"zfs"}"#),
        "{plan}"
    );
}

#[test]
fn two_siblings_appending_different_values_fail_naming_both() {
    let issues = checked(
        "merge-list-siblings",
        "        one\n        two\n        flatpak { apps \"d\"; }",
        &[
            ("one", "flatpak { apps \"b\"; }"),
            ("two", "flatpak { apps \"c\"; }"),
            ("flatpak", APPS),
        ],
    );
    assert!(
        issues.contains("`one` and `two` set different values for option `apps` of `flatpak`"),
        "{issues}"
    );
}

#[test]
fn two_siblings_appending_one_value_append_it_once() {
    let plan = merged(
        "merge-list-agree",
        "        one\n        two",
        &[
            ("one", "flatpak { apps \"b\"; }"),
            ("two", "flatpak { apps \"b\"; }"),
            ("flatpak", APPS),
        ],
    );
    assert!(
        squashed(&plan).contains(r#""options":{"OPT_APPS":"ab"}"#),
        "{plan}"
    );
}

#[test]
fn two_siblings_setting_different_options_both_apply() {
    let plan = merged(
        "merge-two-options",
        "        one\n        two",
        &[
            ("one", "runtime { driver \"vfs\"; }"),
            ("two", "runtime { privilege \"rootless\"; }"),
            (
                "runtime",
                &format!(
                    "{RUNTIME}\noption \"privilege\" type=\"string\" {{ default \"rootful\"; }}"
                ),
            ),
        ],
    );
    assert!(
        squashed(&plan).contains(r#""options":{"OPT_DRIVER":"vfs","OPT_PRIVILEGE":"rootless"}"#),
        "{plan}"
    );
}

#[test]
fn the_image_variant_replaces_the_variant_of_a_component_it_lists() {
    let plan = merged(
        "merge-variant",
        "        one\n        runtime variant=\"large\"",
        &[
            ("one", "runtime variant=\"small\""),
            (
                "runtime",
                &format!(
                    "{RUNTIME}\nvariant \"small\" {{ set \"driver\" \"vfs\"; }}\nvariant \"large\" {{ set \"driver\" \"btrfs\"; }}"
                ),
            ),
        ],
    );
    assert!(
        squashed(&plan).contains(r#""options":{"OPT_DRIVER":"btrfs"}"#),
        "{plan}"
    );
}

#[test]
fn why_reports_each_value_a_setter_applies() {
    let root = written(
        "merge-why",
        "        one\n        flatpak { apps \"c\" { remove \"a\"; }; }",
        &[("one", "flatpak { apps \"b\"; }"), ("flatpak", APPS)],
    );
    let why = buildc::run(Command::Why, Some("flatpak"), &root)
        .report
        .stdout;
    let _ = std::fs::remove_dir_all(root);
    for row in [
        "| `checked` | `apps` | `one` | appends \"b\" |",
        "| `checked` | `apps` | `checked` | removes \"a\" |",
        "| `checked` | `apps` | `checked` | appends \"c\" |",
    ] {
        assert!(why.contains(row), "{row}\n{why}");
    }
}

#[test]
fn why_reports_a_nearer_value_that_restates_a_farther_one() {
    let root = written(
        "merge-why-restated",
        "        one\n        runtime { driver \"vfs\"; }",
        &[("one", "runtime { driver \"vfs\"; }"), ("runtime", RUNTIME)],
    );
    let why = buildc::run(Command::Why, Some("runtime"), &root)
        .report
        .stdout;
    let _ = std::fs::remove_dir_all(root);
    for row in [
        "| `checked` | `driver` | `one` | sets \"vfs\" |",
        "| `checked` | `driver` | `checked` | replaces \"vfs\" |",
    ] {
        assert!(why.contains(row), "{row}\n{why}");
    }
}

#[test]
fn a_merged_value_selects_the_part_that_builds() {
    let parts = "option \"modules\" type=\"string\" { default \"open\"; }\n\"{modules}\"";
    let root = with_parts("merge-part", "        inner\n        outer", parts);
    for (component, manifest) in [
        ("inner", "nvidia { modules \"open\"; }"),
        ("outer", "inner\nnvidia { modules \"proprietary\"; }"),
    ] {
        std::fs::create_dir_all(root.join("components").join(component))
            .expect("the test repository is writable");
        std::fs::write(
            root.join("components").join(component).join("manifest.kdl"),
            rooted(&format!(
                "schema-version 1\n\ntargets \"fedora\"\n{manifest}\n"
            )),
        )
        .expect("the test repository is writable");
    }
    let plan = buildc::run(Command::Plan, None, &root).report.stdout;
    let _ = std::fs::remove_dir_all(root);
    assert!(
        plan.contains("\"path\": \"nvidia/proprietary\"") && !plan.contains("nvidia/open"),
        "{plan}"
    );
}

#[test]
fn an_imported_path_no_library_holds_fails_naming_the_importer() {
    let issues = checked(
        "imports-missing",
        "        consumer",
        &[("consumer", "absent")],
    );
    assert!(
        issues.contains("`consumer` lists `absent`, which no library holds"),
        "{issues}"
    );
}

#[test]
fn an_import_cycle_is_refused() {
    let issues = checked(
        "imports-cycle",
        "        one",
        &[("one", "two"), ("two", "one")],
    );
    assert!(
        issues.contains("`one` and `two` import each other in a cycle"),
        "{issues}"
    );
    assert!(!issues.contains("is listed in the image"), "{issues}");
}

#[test]
fn a_self_import_is_named_as_one() {
    let issues = checked("imports-self", "        one", &[("one", "one")]);
    assert!(issues.contains("`one` imports itself"), "{issues}");
}

#[test]
fn an_import_builds_before_the_component_that_imports_it() {
    let plan = planned(
        "imports-order-entry",
        "",
        "        consumer",
        &[("consumer", "provider"), ("provider", "")],
    );
    let at = |name: &str| plan.find(&format!("\"path\": \"{name}\"")).unwrap();
    assert!(at("provider") < at("consumer"), "{plan}");
}

#[test]
fn an_import_cycle_reached_through_a_second_importer_is_refused() {
    let issues = checked(
        "imports-cycle-second",
        "        a\n        b",
        &[("a", "c"), ("b", "d"), ("c", "d"), ("d", "c")],
    );
    assert!(issues.contains("import each other in a cycle"), "{issues}");
}

#[test]
fn a_setting_from_a_component_that_the_import_reaches_is_dropped() {
    let issues = checked(
        "imports-cycle-reaches",
        "        a\n        b",
        &[
            ("a", "c"),
            ("b", "d"),
            (
                "c",
                "option \"driver\" type=\"string\" { default \"overlay\"; }\nd\n\"{driver}\"",
            ),
            ("d", "c { driver \"vfs\"; }"),
            // overlay exists and vfs does not. If the setting from d applied, then c would list vfs and check would report that no library holds it.
            ("overlay", ""),
        ],
    );
    assert!(issues.contains("import each other in a cycle"), "{issues}");
    assert!(
        !issues.contains("which no library holds"),
        "a setting from an importer that the entry reaches applies: {issues}"
    );
}

#[test]
fn an_import_cycle_names_every_member() {
    let issues = checked(
        "imports-cycle-three",
        "        one",
        &[("one", "two"), ("two", "three"), ("three", "one")],
    );
    assert!(
        issues.contains("`one`, `two` and `three` import each other in a cycle"),
        "{issues}"
    );
}

#[test]
fn a_base_import_builds_before_the_listed_components() {
    let plan = planned(
        "imports-order",
        "; provider",
        "        consumer",
        &[("consumer", ""), ("provider", "")],
    );
    let at = |name: &str| plan.find(&format!("\"path\": \"{name}\"")).unwrap();
    assert!(at("provider") < at("consumer"), "{plan}");
}

#[test]
fn an_option_import_left_empty_is_refused() {
    let issues = checked(
        "imports-empty",
        "        consumer",
        &[(
            "consumer",
            "\"{runtime}\"\noption \"runtime\" type=\"string\"",
        )],
    );
    assert!(
        issues.contains("`consumer` imports a path that its option leaves empty"),
        "{issues}"
    );
}

#[test]
fn the_graph_draws_every_importer_and_met_by_names_one() {
    let root = written(
        "imports-graph",
        "        one\n        two",
        &[
            ("one", "requires \"driver\"\nruntime"),
            ("two", "runtime"),
            ("runtime", "provides \"driver\""),
        ],
    );
    let graph = squashed(&buildc::run(Command::GraphJson, None, &root).report.stdout);
    let plan = squashed(&buildc::run(Command::Plan, None, &root).report.stdout);
    let _ = std::fs::remove_dir_all(root);
    assert!(
        graph.contains(r#"{"component":"runtime","by":"one","how":"imports"}"#)
            && graph.contains(r#"{"component":"runtime","by":"two","how":"imports"}"#)
            && graph.contains(r#"{"component":"one","by":"checked","how":"lists"}"#),
        "{graph}"
    );
    assert!(
        plan.contains(r#""met_by":{"driver":{"how":"imported","provider":"runtime","by":"one"}}"#),
        "{plan}"
    );
}

#[test]
fn a_listed_provider_clashes_with_an_exclusive_one_the_base_imports() {
    let exclusive = "provides \"build-environment\" exclusive=#true";
    let issues = checked_on(
        "imports-exclusive",
        "; fedora-env",
        "        deb-env",
        &[("fedora-env", exclusive), ("deb-env", exclusive)],
    );
    assert!(
        issues.contains("more than one enabled component"),
        "{issues}"
    );
}

const RUNTIME: &str = "option \"driver\" type=\"string\" { default \"overlay\"; }";

#[test]
fn an_option_an_importer_passes_reaches_the_imported_component() {
    let plan = planned(
        "imports-passes",
        "",
        "        consumer",
        &[
            ("consumer", "runtime { driver \"vfs\"; }"),
            ("runtime", RUNTIME),
        ],
    );
    assert!(
        squashed(&plan).contains(r#""options":{"OPT_DRIVER":"vfs"}"#),
        "{plan}"
    );
}

#[test]
fn two_importers_passing_one_value_agree() {
    let issues = checked(
        "imports-agree",
        "        one\n        two",
        &[
            ("one", "runtime { driver \"vfs\"; }"),
            ("two", "runtime { driver \"vfs\"; }"),
            ("runtime", RUNTIME),
        ],
    );
    assert!(!issues.contains("set different values"), "{issues}");
}

fn with_parts(name: &str, components: &str, parts: &str) -> PathBuf {
    let root = written(
        name,
        components,
        &[("nvidia/open", ""), ("nvidia/proprietary", "")],
    );
    std::fs::create_dir_all(root.join("components/nvidia"))
        .expect("the test repository is writable");
    std::fs::write(
        root.join("components/nvidia/manifest.kdl"),
        format!("schema-version 1\n\ntargets \"fedora\"\n{parts}\n"),
    )
    .expect("the test repository is writable");
    root
}

#[test]
fn a_part_builds_from_the_option_that_selects_it() {
    let parts = "option \"modules\" type=\"string\" { default \"open\"; }\n\"{modules}\"";
    let plan = |name: &str, components: &str| {
        let root = with_parts(name, components, parts);
        let plan = buildc::run(Command::Plan, None, &root).report.stdout;
        let _ = std::fs::remove_dir_all(root);
        plan
    };
    let default = plan("part-default", "nvidia");
    let chosen = plan("part-chosen", "nvidia { modules \"proprietary\"; }");
    assert!(
        default.contains("\"path\": \"nvidia/open\"") && !default.contains("nvidia/proprietary"),
        "{default}"
    );
    assert!(
        chosen.contains("\"path\": \"nvidia/proprietary\"") && !chosen.contains("nvidia/open"),
        "{chosen}"
    );
}

#[test]
fn a_part_is_no_cycle_with_the_component_that_lists_it() {
    let root = with_parts("part-order", "        nvidia", "open");
    let issues = buildc::run(Command::Check, None, &root)
        .problems
        .issues
        .plain();
    let _ = std::fs::remove_dir_all(root);
    assert!(issues.is_empty(), "{issues}");
}

#[test]
fn a_standalone_file_is_a_component_and_a_second_manifest_is_refused() {
    let root = written("standalone-file", "        tools", &[]);
    std::fs::write(
        root.join("components/tools.kdl"),
        "schema-version 1\n\ntargets \"fedora\"\npackages \"htop\"\n",
    )
    .expect("the test repository is writable");
    let plan = buildc::run(Command::Plan, None, &root).report.stdout;
    std::fs::create_dir_all(root.join("components/tools"))
        .expect("the test repository is writable");
    std::fs::write(
        root.join("components/tools/manifest.kdl"),
        "schema-version 1\n",
    )
    .expect("the test repository is writable");
    let issues = buildc::run(Command::Check, None, &root)
        .problems
        .issues
        .plain();
    let _ = std::fs::remove_dir_all(root);
    assert!(plan.contains("\"path\": \"tools\""), "{plan}");
    assert!(plan.contains("htop"), "{plan}");
    assert!(issues.contains("`tools` has two manifests"), "{issues}");
}

#[test]
fn an_image_field_below_a_build_is_refused() {
    let issues = checked(
        "field-below-build",
        "        tools",
        &[("tools", "conforms \"standard\"")],
    );
    assert!(
        issues.contains("`tools` sets `conforms`, which only an image takes"),
        "{issues}"
    );
}

#[test]
fn a_bare_child_of_the_image_reads_below_the_image_path() {
    let root = written("image-relative", "", &[("checked/extras", "")]);
    let image = root.join("components/checked.kdl");
    let text = std::fs::read_to_string(&image).expect("the image file is written");
    std::fs::write(&image, format!("{text}extras\n")).expect("the test repository is writable");
    let plan = buildc::run(Command::Plan, None, &root).report.stdout;
    let _ = std::fs::remove_dir_all(root);
    assert!(plan.contains("\"path\": \"checked/extras\""), "{plan}");
}

#[test]
fn the_image_files_own_content_builds_as_its_component() {
    let root = written(
        "image-content",
        "        adapter",
        &[("adapter", "provides \"build-environment\"")],
    );
    let image = root.join("components/checked.kdl");
    let text = std::fs::read_to_string(&image).expect("the image file is written");
    std::fs::write(&image, format!("{text}packages \"htop\"\n"))
        .expect("the test repository is writable");
    let plan = buildc::run(Command::Plan, None, &root).report.stdout;
    let issues = buildc::run(Command::Check, None, &root)
        .problems
        .issues
        .plain();
    let _ = std::fs::remove_dir_all(root);
    assert!(issues.is_empty(), "{issues}");
    assert!(plan.contains("\"path\": \"checked\""), "{plan}");
    assert!(plan.contains("htop"), "{plan}");
}

#[test]
fn an_option_valued_child_resolves_under_its_organising_segment() {
    let root = written(
        "option-child",
        "        consumer",
        &[
            (
                "consumer",
                "option \"repo\" type=\"string\" { default \"fast\"; }\nrepos { \"{repo}\" }",
            ),
            ("repos/fast", ""),
        ],
    );
    let plan = buildc::run(Command::Plan, None, &root).report.stdout;
    let issues = buildc::run(Command::Check, None, &root)
        .problems
        .issues
        .plain();
    let _ = std::fs::remove_dir_all(root);
    assert!(issues.is_empty(), "{issues}");
    assert!(plan.contains("\"path\": \"repos/fast\""), "{plan}");
}

#[test]
fn a_gated_child_builds_only_on_its_family() {
    let root = written(
        "gated-child",
        "        consumer",
        &[
            (
                "consumer",
                "on \"fedora\" { component \"/\" { fedora-only; }; }\non \"debian\" { component \"/\" { deb-only; }; }",
            ),
            ("fedora-only", ""),
            ("deb-only", ""),
        ],
    );
    for (component, targets) in [
        ("consumer", "\"fedora\" \"debian\""),
        ("deb-only", "\"debian\""),
    ] {
        let manifest = root.join(format!("components/{component}/manifest.kdl"));
        let text = std::fs::read_to_string(&manifest).expect("the test repository is readable");
        std::fs::write(
            &manifest,
            text.replace("targets \"fedora\"", &format!("targets {targets}")),
        )
        .expect("the test repository is writable");
    }
    let plan = buildc::run(Command::Plan, None, &root).report.stdout;
    let _ = std::fs::remove_dir_all(root);
    assert!(
        plan.contains("\"path\": \"fedora-only\"") && !plan.contains("deb-only"),
        "{plan}"
    );
}

#[test]
fn a_digest_off_the_base_is_refused() {
    let issues = checked(
        "digest-off-base",
        "        consumer digest=\"sha256:ab\"",
        &[("consumer", "")],
    );
    assert!(
        issues.contains("`consumer` is not the base, so it takes no `digest=`"),
        "{issues}"
    );
}

#[test]
fn an_imported_component_off_the_family_names_its_importer() {
    let root = written(
        "imports-off-family",
        "        consumer",
        &[("consumer", "deb-only"), ("deb-only", "")],
    );
    let manifest = root.join("components/deb-only/manifest.kdl");
    let text = std::fs::read_to_string(&manifest).expect("the test repository is readable");
    std::fs::write(
        &manifest,
        text.replace("targets \"fedora\"", "targets \"debian\""),
    )
    .expect("the test repository is writable");
    let issues = buildc::run(Command::Check, None, &root)
        .problems
        .issues
        .plain();
    let _ = std::fs::remove_dir_all(root);
    assert!(
        issues.contains("`deb-only` does not support the `fedora` base family"),
        "{issues}"
    );
    assert!(issues.contains("`consumer` imports it"), "{issues}");
}

#[test]
fn two_importers_listing_different_variants_fail_naming_both() {
    let issues = checked(
        "imports-variants",
        "        one\n        two",
        &[
            ("one", "runtime variant=\"small\""),
            ("two", "runtime variant=\"large\""),
            ("runtime", "variant \"small\"\nvariant \"large\""),
        ],
    );
    assert!(
        issues.contains("`one` and `two` list different variants of `runtime`"),
        "{issues}"
    );
}

#[test]
fn two_importers_passing_different_values_fail_naming_both() {
    let issues = checked(
        "imports-disagree",
        "        one\n        two",
        &[
            ("one", "runtime { driver \"vfs\"; }"),
            ("two", "runtime { driver \"btrfs\"; }"),
            ("runtime", RUNTIME),
        ],
    );
    assert!(
        issues.contains("`one` and `two` set different values for option `driver` of `runtime`"),
        "{issues}"
    );
}

#[test]
fn the_graph_names_the_path_or_descendant_that_meets_a_requirement() {
    let root = written(
        "graph-meets",
        "        consumer\n        helper\n        manager",
        &[
            ("consumer", "requires \"helper\"\nrequires \"rpm\""),
            ("helper", ""),
            ("manager", "provides \"rpm/dnf5\""),
        ],
    );
    let helper = capability_row(&root, "helper").0;
    let rpm = capability_row(&root, "rpm").0;
    let _ = std::fs::remove_dir_all(root);
    assert_eq!(helper[2], "helper");
    assert_eq!(rpm[2], "manager");
}

#[test]
fn a_requirement_cycle_is_refused() {
    let issues = checked(
        "requires-cycle",
        "        one\n        two",
        &[("one", "requires \"two\""), ("two", "requires \"one\"")],
    );
    assert!(
        issues.contains("the component graph has a cycle"),
        "{issues}"
    );
}

fn squashed(json: &str) -> String {
    json.split_whitespace().collect()
}

const CHOOSER: &str =
    "requires \"tool\"\n\"{runtime}\"\noption \"runtime\" type=\"string\" { default \"one\"; }";

#[test]
fn an_option_import_takes_the_path_the_image_sets() {
    let provider = "provides \"tool\" witness=\"/usr/share/tool\"";
    let plan = planned(
        "imports-chosen",
        "",
        "        consumer { runtime \"two\"; }",
        &[("consumer", CHOOSER), ("one", provider), ("two", provider)],
    );
    assert!(!plan.contains("\"path\": \"one\""), "{plan}");
    assert!(
        squashed(&plan)
            .contains(r#""met_by":{"tool":{"how":"imported","provider":"two","by":"consumer"}}"#),
        "{plan}"
    );
}

#[test]
fn an_option_import_takes_its_default_where_the_image_sets_none() {
    let provider = "provides \"tool\" witness=\"/usr/share/tool\"";
    let plan = planned(
        "imports-chosen-default",
        "",
        "        consumer",
        &[("consumer", CHOOSER), ("one", provider), ("two", provider)],
    );
    assert!(plan.contains("\"path\": \"one\""), "{plan}");
    assert!(!plan.contains("\"path\": \"two\""), "{plan}");
}

#[test]
fn an_option_import_that_names_no_component_fails_naming_the_path() {
    let provider = "provides \"tool\" witness=\"/usr/share/tool\"";
    let issues = checked(
        "imports-chosen-missing",
        "        consumer { runtime \"three\"; }",
        &[("consumer", CHOOSER), ("one", provider), ("two", provider)],
    );
    assert!(
        issues.contains("`consumer` lists `three`, which no library holds"),
        "{issues}"
    );
}

#[test]
fn a_reference_to_an_option_that_is_not_a_string_fails_at_its_node() {
    let issues = checked(
        "reference-not-string",
        "        consumer",
        &[(
            "consumer",
            "option \"fast\" type=\"bool\" { default #false; }\npackages \"tool={fast}\" \"other={absent}\"",
        )],
    );
    assert!(issues.contains("`{fast}` reads a bool option"), "{issues}");
    assert!(
        issues.contains("`{absent}` names no option of this component"),
        "{issues}"
    );
}

#[test]
fn an_image_entry_version_reaches_the_package_line() {
    let root = written(
        "version-reaches-packages",
        "        browser { version \"130.0\"; }\n        adapter",
        &[
            (
                "browser",
                "option \"version\" type=\"string\" { default \"1\"; }\npackages \"firefox={version}\"",
            ),
            ("adapter", "provides \"build-environment\""),
        ],
    );
    let run = buildc::run(Command::Generate, None, &root);
    let _ = std::fs::remove_dir_all(root);
    let script = run
        .files
        .iter()
        .find(|(path, _)| path.ends_with("components/browser.sh"))
        .map(|(_, text)| text.as_str())
        .unwrap_or_else(|| panic!("{}", run.problems.issues.plain()));
    assert!(
        script.contains("dnf install -y 'firefox-130.0'"),
        "{script}"
    );
}

#[test]
fn an_option_value_that_spoils_a_package_name_fails_at_the_entry() {
    let issues = checked(
        "version-spoils-packages",
        "        browser { version \"1 2\"; }",
        &[(
            "browser",
            "option \"version\" type=\"string\" { default \"1\"; }\npackages \"firefox={version}\"",
        )],
    );
    assert!(
        issues.contains("`browser` installs `firefox={version}` as `firefox=1 2`"),
        "{issues}"
    );
}

const PROVIDER: (&str, &str) = ("provider", "provides \"tool\" witness=\"/usr/share/tool\"");
const REMOVER: (&str, &str) = ("remover", "removes \"tool\"");

#[test]
fn a_component_that_requires_a_removed_name_fails_naming_the_remover() {
    for (name, components) in [
        (
            "removes-before",
            "        remover\n        provider\n        consumer",
        ),
        (
            "removes-after",
            "        provider\n        consumer\n        remover",
        ),
    ] {
        let issues = checked(
            name,
            components,
            &[PROVIDER, REMOVER, ("consumer", "requires \"tool\"")],
        );
        assert!(
            issues.contains("`consumer` requires `tool`, which `remover` removes"),
            "{issues}"
        );
    }
}

#[test]
fn a_removes_that_nothing_provides_fails() {
    let issues = checked("removes-nothing", "        remover", &[REMOVER]);
    assert!(
        issues.contains("`remover` removes `tool`, which nothing in the image provides"),
        "{issues}"
    );
}

#[test]
fn a_remover_builds_after_the_provider_and_its_witness_must_be_absent() {
    let plan = planned(
        "removes-order",
        "",
        "        remover\n        provider",
        &[PROVIDER, REMOVER],
    );
    let at = |name: &str| plan.find(&format!("\"path\": \"{name}\"")).unwrap();
    assert!(at("provider") < at("remover"), "{plan}");
    assert!(
        squashed(&plan).contains(r#""contract_files":["!/usr/share/tool"]"#),
        "{plan}"
    );
}

#[test]
fn a_removed_base_name_is_no_longer_claimed() {
    let plan = planned(
        "removes-base",
        "; provides \"bootupctl\" witness=\"/usr/bin/bootupctl\"",
        "        remover",
        &[("remover", "removes \"bootupctl\"")],
    );
    let root = written_on(
        "removes-base-check",
        "; provides \"bootupctl\" witness=\"/usr/bin/bootupctl\"",
        "        remover",
        &[("remover", "removes \"bootupctl\"")],
    );
    let issues = buildc::run(Command::Check, None, &root)
        .problems
        .issues
        .plain();
    let _ = std::fs::remove_dir_all(root);
    assert!(
        !issues.contains("nothing in the image provides"),
        "{issues}"
    );
    assert!(
        squashed(&plan).contains(r#""contract_files":["!/usr/bin/bootupctl"]"#),
        "{plan}"
    );
}

fn write_base(root: &std::path::Path, family: &str, extra: &str) {
    let dir = root.join("components/base-image/example");
    std::fs::create_dir_all(&dir).expect("the test repository is writable");
    std::fs::write(
        dir.join("manifest.kdl"),
        rooted(&format!(
            "schema-version 1\n\nbase \"example\" {{ family \"{family}\"; }}\n{extra}\n"
        )),
    )
    .expect("the test repository is writable");
}

fn bare_component(name: &str, files: &[&str]) -> PathBuf {
    let root = written(
        name,
        "        adapter\n        bare",
        &[("adapter", "provides \"build-environment\"")],
    );
    for file in files {
        let at = root.join("components/bare").join(file);
        let made = match file.strip_suffix('/') {
            Some(_) => std::fs::create_dir_all(at.join("etc")),
            None => at
                .parent()
                .map_or(Ok(()), std::fs::create_dir_all)
                .and_then(|()| std::fs::write(&at, "true\n")),
        };
        made.expect("the test repository is writable");
    }
    root
}

fn generated(root: &std::path::Path) -> String {
    let run = buildc::run(Command::Generate, None, root);
    assert!(
        run.problems.issues.is_empty(),
        "{}",
        run.problems.issues.plain()
    );
    run.files.into_iter().map(|(_, text)| text).collect()
}

#[test]
fn the_generated_build_job_follows_the_declared_backend() {
    let root = written("backend-buildx", "", &[]);
    std::fs::write(
        root.join("repo.kdl"),
        "schema-version 1\nname \"Checked\"\nbuilds default=\"checked\" { checked }\nbackend \"buildx\"\ngithub {\n    workflows {\n        image-build\n    }\n}\n",
    )
    .expect("the test repository is writable");
    let run = buildc::run(Command::Generate, None, &root);
    let _ = std::fs::remove_dir_all(&root);
    assert!(
        run.problems.issues.is_empty(),
        "{}",
        run.problems.issues.plain()
    );
    let (_, workflow) = run
        .files
        .iter()
        .find(|(path, _)| path.ends_with("image-build.yml"))
        .expect("the build workflow is generated");
    assert!(workflow.contains("setup-buildx-action"), "{workflow}");
    assert!(!workflow.contains("sudo env \\"), "{workflow}");
}

#[test]
fn a_uki_seal_under_a_buildx_backend_is_refused() {
    let root = written("buildx-uki", "seal", &[("seal", "")]);
    std::fs::write(
        root.join("components/seal/Containerfile.inc"),
        // A stage from SPLIT_BASE marks the image as sealing a UKI.
        "FROM ${SPLIT_BASE} AS split\n",
    )
    .expect("the test repository is writable");
    std::fs::write(
        root.join("repo.kdl"),
        "schema-version 1\nname \"Checked\"\nbuilds default=\"checked\" { checked }\nbackend \"buildx\"\n",
    )
    .expect("the test repository is writable");
    let run = buildc::run(Command::Check, None, &root);
    let _ = std::fs::remove_dir_all(&root);
    let plain = run.problems.issues.plain();
    assert!(
        plain.contains("`backend \"buildx\"` cannot build `checked`, which seals a UKI"),
        "{plain}"
    );
}

#[test]
fn a_component_with_one_script_and_no_manifest_builds() {
    for (file, line) in [
        (
            "files/",
            "COPY --from=ctx /generated/checked/components/bare/files/ /",
        ),
        ("repo.sh", "source /ctx/components/bare/repo.sh"),
        (
            "packages.sh",
            "source /ctx/lib/family.sh\n\nsource /ctx/components/bare/packages.sh",
        ),
        ("component.sh", "source /ctx/components/bare/component.sh"),
        ("finalize.sh", "source /ctx/components/bare/finalize.sh"),
    ] {
        let root = bare_component(&format!("bare-{}", file.trim_end_matches('/')), &[file]);
        let out = generated(&root);
        let _ = std::fs::remove_dir_all(&root);
        assert!(out.contains(line), "{file}: {out}");
        if file == "packages.sh" {
            assert!(
                out.contains(
                    "source=/generated/checked/components/bare,target=/ctx/components/bare"
                ),
                "{out}"
            );
        }
    }
}

fn script_if_any(run: &buildc::run::Run, ending: &str) -> String {
    run.files
        .iter()
        .find(|(path, _)| path.to_string_lossy().ends_with(ending))
        .map(|(_, text)| text.clone())
        .unwrap_or_default()
}

fn files_named(run: &buildc::run::Run, ending: &str) -> String {
    run.files
        .iter()
        .find(|(path, _)| path.to_string_lossy().ends_with(ending))
        .map(|(_, text)| text.clone())
        .unwrap_or_else(|| panic!("no file ending {ending}: {}", run.problems.issues.plain()))
}

#[test]
fn a_component_witness_is_checked_in_its_own_layer() {
    let root = written(
        "witness-layer",
        "        checker",
        &[("checker", "provides \"tool\" witness=\"/usr/share/tool\"")],
    );
    let run = buildc::run(Command::Generate, None, &root);
    let _ = std::fs::remove_dir_all(&root);
    assert!(
        run.problems.issues.is_empty(),
        "{}",
        run.problems.issues.plain()
    );
    let script = files_named(&run, "components/checker.sh");
    assert!(
        script.contains(
            "component checker provides tool, and its witness /usr/share/tool is missing"
        ),
        "{script}"
    );
}

#[test]
fn an_overlay_witness_is_checked_after_the_copy() {
    let root = written(
        "witness-overlay",
        "        overlay-check",
        &[(
            "overlay-check",
            "provides \"tool\" witness=\"/usr/share/tool\"",
        )],
    );
    let at = root.join("components/overlay-check/files/usr/share/tool");
    std::fs::create_dir_all(at.parent().unwrap()).unwrap();
    std::fs::write(at, "true\n").unwrap();
    let run = buildc::run(Command::Generate, None, &root);
    let _ = std::fs::remove_dir_all(&root);
    assert!(
        run.problems.issues.is_empty(),
        "{}",
        run.problems.issues.plain()
    );
    let script = script_if_any(&run, "components/overlay-check.sh");
    assert!(!script.contains("witness for tool"), "{script}");
    let tail = files_named(&run, "components/overlay-check.sh.tail.sh");
    assert!(
        tail.contains("component overlay-check provides tool"),
        "{tail}"
    );
}

#[test]
fn a_witness_created_by_an_after_fragment_is_checked_after_it() {
    let root = written(
        "witness-after",
        "        after-check",
        &[(
            "after-check",
            "provides \"tool\" witness=\"/usr/share/tool\"\nfragment position=\"after\"",
        )],
    );
    std::fs::write(
        root.join("components/after-check/Containerfile.inc"),
        "RUN true\n",
    )
    .unwrap();
    let run = buildc::run(Command::Generate, None, &root);
    let _ = std::fs::remove_dir_all(&root);
    assert!(
        run.problems.issues.is_empty(),
        "{}",
        run.problems.issues.plain()
    );
    let script = script_if_any(&run, "components/after-check.sh");
    assert!(!script.contains("witness for tool"), "{script}");
    let containerfile = files_named(&run, "Containerfile");
    let after = containerfile
        .find("verbatim from components/after-check/Containerfile.inc")
        .expect("the after fragment is emitted");
    let witness = containerfile
        .find("components/after-check.sh.witness.sh")
        .expect("the witness step is emitted");
    assert!(after < witness, "{containerfile}");
}

#[test]
fn every_generated_path_the_containerfile_reads_is_written() {
    let fragments = [
        ("after-check", "\nfragment position=\"after\""),
        ("tail-check", "\nfragment position=\"tail\""),
        ("whole-check", "\nfragment standard-layer=#false"),
        ("plain-check", ""),
    ];
    let manifests: Vec<(&str, String)> = fragments
        .iter()
        .map(|(name, fragment)| {
            (
                *name,
                format!("provides \"{name}\" witness=\"/usr/share/{name}\"{fragment}"),
            )
        })
        .collect();
    let borrowed: Vec<(&str, &str)> = manifests
        .iter()
        .map(|(name, manifest)| (*name, manifest.as_str()))
        .collect();
    let root = written(
        "mounts-written",
        "        after-check\n        tail-check\n        whole-check\n        plain-check",
        &borrowed,
    );
    for (name, _) in fragments
        .iter()
        .filter(|(_, fragment)| !fragment.is_empty())
    {
        std::fs::write(
            root.join("components").join(name).join("Containerfile.inc"),
            "RUN true\n",
        )
        .unwrap();
    }
    let run = buildc::run(Command::Generate, None, &root);
    let _ = std::fs::remove_dir_all(&root);
    assert!(
        run.problems.issues.is_empty(),
        "{}",
        run.problems.issues.plain()
    );
    let containerfile = files_named(&run, "Containerfile");
    let written: Vec<String> = run
        .files
        .iter()
        .map(|(path, _)| path.display().to_string())
        .chain(
            run.copies
                .iter()
                .map(|copy| copy.path.display().to_string()),
        )
        .collect();
    let read: Vec<&str> = ["from=ctx,source=/", "COPY --from=ctx /"]
        .iter()
        .flat_map(|marker| containerfile.split(marker).skip(1))
        .filter_map(|rest| rest.split([',', ' ', '\n']).next())
        .filter(|path| path.starts_with("generated/"))
        .map(|path| path.trim_end_matches('/'))
        .collect();
    // The loop below passes on an empty parse. The count proves both witness steps are read.
    let witness_steps = read
        .iter()
        .filter(|path| path.ends_with(".witness.sh"))
        .count();
    assert_eq!(witness_steps, 2, "{containerfile}");
    for path in read {
        let inside = format!("{path}/");
        assert!(
            written
                .iter()
                .any(|file| file == path || file.starts_with(&inside)),
            "the Containerfile reads {path}, which generate does not write:\n{containerfile}"
        );
    }
}

#[test]
fn a_finalize_hook_witness_is_checked_after_the_hooks_and_before_the_tail() {
    let root = written(
        "witness-finalize",
        "        hook-check",
        &[(
            "hook-check",
            "provides \"tool\" witness=\"/usr/share/tool\"\nfragment position=\"after\"",
        )],
    );
    std::fs::write(
        root.join("components/hook-check/Containerfile.inc"),
        "RUN true\n",
    )
    .unwrap();
    std::fs::write(
        root.join("components/hook-check/finalize.sh"),
        "touch /usr/share/tool\n",
    )
    .unwrap();
    let run = buildc::run(Command::Generate, None, &root);
    let _ = std::fs::remove_dir_all(&root);
    assert!(
        run.problems.issues.is_empty(),
        "{}",
        run.problems.issues.plain()
    );
    let script = script_if_any(&run, "components/hook-check.sh");
    assert!(!script.contains("witness for tool"), "{script}");
    let containerfile = files_named(&run, "Containerfile");
    assert!(!containerfile.contains("witness.sh"), "{containerfile}");
    let finalize = files_named(&run, "finalize.sh");
    let hook = finalize
        .find("source /ctx/components/hook-check/finalize.sh")
        .expect("the finalize layer runs the hook");
    let witness = finalize
        .find("component hook-check provides tool")
        .expect("the finalize layer checks the witness");
    let tail = finalize
        .find("zz-opt-symlinks.conf")
        .expect("the finalize tail is emitted");
    assert!(hook < witness && witness < tail, "{finalize}");
}

#[test]
fn a_tail_fragment_witness_is_checked_in_its_own_layer() {
    let root = written(
        "witness-tail",
        "        tail-check",
        &[(
            "tail-check",
            "provides \"tool\" witness=\"/usr/share/tool\"\nfragment position=\"tail\"",
        )],
    );
    std::fs::write(
        root.join("components/tail-check/Containerfile.inc"),
        "FROM scratch AS other\n",
    )
    .unwrap();
    let run = buildc::run(Command::Generate, None, &root);
    let _ = std::fs::remove_dir_all(&root);
    assert!(
        run.problems.issues.is_empty(),
        "{}",
        run.problems.issues.plain()
    );
    let script = files_named(&run, "components/tail-check.sh");
    assert!(
        script.contains("component tail-check provides tool"),
        "{script}"
    );
    let containerfile = files_named(&run, "Containerfile");
    assert!(!containerfile.contains("witness.sh"), "{containerfile}");
}

#[test]
fn a_fragment_component_carries_only_its_family() {
    let root = written(
        "fragment-family",
        "        inc",
        &[("inc", "fragment standard-layer=#false")],
    );
    std::fs::write(
        root.join("components/inc/manifest.kdl"),
        "schema-version 1\n\ntargets \"fedora\" \"debian\"\nfragment standard-layer=#false\n",
    )
    .unwrap();
    std::fs::write(root.join("components/inc/Containerfile.inc"), "RUN true\n").unwrap();
    for (path, body) in [
        ("components/inc/plain", "plain\n"),
        ("components/inc/fedora/own", "fedora\n"),
        ("components/inc/debian/other", "debian\n"),
    ] {
        let at = root.join(path);
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::write(at, body).unwrap();
    }
    let run = buildc::run(Command::Generate, None, &root);
    let _ = std::fs::remove_dir_all(&root);
    assert!(
        run.problems.issues.is_empty(),
        "{}",
        run.problems.issues.plain()
    );
    let names: Vec<String> = run
        .copies
        .iter()
        .map(|copy| copy.path.display().to_string())
        .collect();
    assert!(
        names.iter().any(|path| path.ends_with("/inc/plain")),
        "{names:?}"
    );
    assert!(
        names.iter().any(|path| path.ends_with("/inc/fedora/own")),
        "{names:?}"
    );
    assert!(
        !names.iter().any(|path| path.ends_with("/inc/debian/other")),
        "{names:?}"
    );
}

#[test]
fn an_overlay_symlink_is_kept_as_a_link() {
    let root = written("overlay-link", "        linker", &[("linker", "")]);
    let at = root.join("components/linker/files/usr/bin/linked");
    std::fs::create_dir_all(at.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink("/usr/bin/true", &at).unwrap();
    let run = buildc::run(Command::Generate, None, &root);
    let _ = std::fs::remove_dir_all(&root);
    assert!(
        run.problems.issues.is_empty(),
        "{}",
        run.problems.issues.plain()
    );
    let copy = run
        .copies
        .iter()
        .find(|copy| copy.path.ends_with("files/usr/bin/linked"))
        .expect("the link is resolved");
    assert_eq!(
        copy.link.as_deref(),
        Some(std::path::Path::new("/usr/bin/true"))
    );
    assert!(copy.bytes.is_empty());
}

#[test]
fn a_fragment_witness_gets_its_own_step() {
    let root = written(
        "witness-fragment",
        "        inc-check",
        &[(
            "inc-check",
            "provides \"tool\" witness=\"/usr/share/tool\"\nfragment standard-layer=#false",
        )],
    );
    std::fs::write(
        root.join("components/inc-check/Containerfile.inc"),
        "RUN true\n",
    )
    .unwrap();
    let run = buildc::run(Command::Generate, None, &root);
    let _ = std::fs::remove_dir_all(&root);
    assert!(
        run.problems.issues.is_empty(),
        "{}",
        run.problems.issues.plain()
    );
    let containerfile = files_named(&run, "Containerfile");
    assert!(
        containerfile.contains("components/inc-check.sh.witness.sh"),
        "{containerfile}"
    );
    let script = files_named(&run, "components/inc-check.sh.witness.sh");
    assert!(
        script.contains("component inc-check provides tool"),
        "{script}"
    );
}

#[test]
fn packages_sh_runs_after_the_declared_packages_from_broad_to_specific() {
    let root = bare_component(
        "packages-sh-order",
        &[
            "packages.sh",
            "rpm/packages.sh",
            "fedora/packages.sh",
            "debian/packages.sh",
        ],
    );
    std::fs::write(
        root.join("components/bare/manifest.kdl"),
        "schema-version 1

packages \"tool\"\n",
    )
    .unwrap();
    let out = generated(&root);
    let _ = std::fs::remove_dir_all(&root);
    let at = |line: &str| out.find(line).unwrap_or_else(|| panic!("{line}: {out}"));
    assert!(at("dnf install -y 'tool'") < at("source /ctx/components/bare/packages.sh"));
    assert!(
        at("source /ctx/components/bare/packages.sh")
            < at("source /ctx/components/bare/rpm/packages.sh")
    );
    assert!(
        at("source /ctx/components/bare/rpm/packages.sh")
            < at("source /ctx/components/bare/fedora/packages.sh")
    );
    assert!(!out.contains("debian/packages.sh"), "{out}");
}

#[test]
fn why_names_the_component_that_imported_this_one() {
    let root = written(
        "why-imported",
        "        consumer",
        &[("consumer", "provider"), ("provider", "")],
    );
    let imported = buildc::run(Command::Why, Some("provider"), &root)
        .report
        .stdout;
    let listed = buildc::run(Command::Why, Some("consumer"), &root)
        .report
        .stdout;
    let _ = std::fs::remove_dir_all(root);
    assert!(
        imported.contains("`consumer` imports it into `checked`."),
        "{imported}"
    );
    assert!(!listed.contains("imports it into"), "{listed}");
}

#[test]
fn listing_a_child_includes_the_component_that_holds_it() {
    let root = written(
        "nesting",
        "        desktop { kde }\n        group {\n            tool\n        }",
        &[("desktop", ""), ("desktop/kde", ""), ("group/tool", "")],
    );
    let plan = buildc::run(Command::Plan, None, &root).report.stdout;
    let issues = buildc::run(Command::Check, None, &root)
        .problems
        .issues
        .plain();
    std::fs::write(
        root.join("components/checked.kdl"),
        rooted("schema-version 1\n\nbase-image {\n    example\n}\ndesktop\n"),
    )
    .expect("the test repository is writable");
    let parent = buildc::run(Command::Plan, None, &root).report.stdout;
    let _ = std::fs::remove_dir_all(&root);
    assert!(issues.is_empty(), "{issues}");
    let at = |name: &str| plan.find(&format!("\"path\": \"{name}\"")).unwrap();
    assert!(at("desktop") < at("desktop/kde"), "{plan}");
    assert!(!plan.contains("\"path\": \"group\""), "{plan}");
    assert!(!parent.contains("\"path\": \"desktop/kde\""), "{parent}");
}

#[test]
fn a_base_component_resolves_its_reference_from_the_entry() {
    let root = written("base-component", "", &[]);
    std::fs::write(
        root.join("components/base-image/example/manifest.kdl"),
        "schema-version 1\n\nprovides \"bootc\"\nbase \"quay.io/fedora/fedora-bootc:{version}\" { family \"fedora\"; }\noption \"version\" type=\"string\" { default \"44\"; }\n",
    )
    .expect("the test repository is writable");
    std::fs::write(
        root.join("components/checked.kdl"),
        rooted("schema-version 1\n\nbase-image {\n    example digest=\"sha256:abc\" signed=#true {\n        version \"45\"\n    }\n}\n"),
    )
    .expect("the test repository is writable");
    let plan = buildc::run(Command::Plan, None, &root).report.stdout;
    let issues = buildc::run(Command::Check, None, &root)
        .problems
        .issues
        .plain();
    let _ = std::fs::remove_dir_all(&root);
    assert!(issues.is_empty(), "{issues}");
    let flat = squashed(&plan);
    assert!(
        flat.contains(r#""image":"quay.io/fedora/fedora-bootc:45@sha256:abc""#),
        "{plan}"
    );
    assert!(flat.contains(r#""signed":true"#), "{plan}");
}

#[test]
fn an_image_that_lists_no_component_declaring_base_fails() {
    let root = written("base-not-a-base", "", &[("plain", "")]);
    std::fs::write(
        root.join("components/checked.kdl"),
        rooted("schema-version 1\n\nplain\n"),
    )
    .expect("the test repository is writable");
    let issues = buildc::run(Command::Check, None, &root)
        .problems
        .issues
        .plain();
    let _ = std::fs::remove_dir_all(&root);
    assert!(issues.contains("`checked` lists no base"), "{issues}");
}

#[test]
fn a_family_outside_a_base_block_points_at_the_base() {
    let issues = checked(
        "family-not-a-base",
        "        plain",
        &[("plain", "family \"fedora\"")],
    );
    assert!(
        issues.contains("`family` belongs in the `base` block"),
        "{issues}"
    );
}

#[test]
fn the_finalize_layer_carries_the_image_id_and_no_os_release_field() {
    let root = written("identity-env", "        later", &[("later", "")]);
    let out = generated(&root);
    let _ = std::fs::remove_dir_all(&root);
    let at = |line: &str| out.find(line).unwrap_or_else(|| panic!("{line}: {out}"));
    let finalize = at("# ---- finalize ----");
    assert!(at("ARG IMAGE_ID=\"checked\"") < finalize, "{out}");
    for name in ["IMAGE_ID", "IMAGE_VERSION"] {
        assert!(
            out[finalize..].contains(&format!("{name}=\"${{{name}}}\"")),
            "{name}: {out}"
        );
    }
    for gone in [
        "IMAGE_NAME",
        "IMAGE_PRETTY_NAME",
        "IMAGE_URL",
        "IMAGE_ISSUES_URL",
    ] {
        assert!(!out.contains(gone), "{gone}: {out}");
    }
    // The os-release component owns that file. The finalize layer must not write it.
    assert!(!out[finalize..].contains("os-release"), "{out}");
}

fn reference_repo(name: &str, repo: &str, entry: &str, manifest: &str) -> std::path::PathBuf {
    let root = written(name, entry, &[("stamp", manifest)]);
    std::fs::write(
        root.join("repo.kdl"),
        format!("schema-version 1\nname \"Checked\"\nbuilds {{ checked }}\n{repo}"),
    )
    .expect("the test repository is writable");
    std::fs::write(
        root.join("components/stamp/finalize.sh"),
        "echo \"${OPT_HOME}\"\n",
    )
    .expect("the test repository is writable");
    root
}

fn referenced(name: &str, repo: &str, entry: &str, manifest: &str) -> buildc::Run {
    let root = reference_repo(name, repo, entry, manifest);
    let run = buildc::run(Command::Generate, None, &root);
    let _ = std::fs::remove_dir_all(&root);
    run
}

fn finalize_of(run: &buildc::Run) -> &str {
    run.files
        .iter()
        .find(|(path, _)| path.ends_with("finalize.sh") && path.starts_with("generated/checked"))
        .map(|(_, text)| text.as_str())
        .unwrap_or_else(|| panic!("{}", run.problems.issues.plain()))
}

const STAMP: &str = "option \"home\" type=\"string\" {\n    default \"\"\n}";

#[test]
fn an_option_value_reads_the_repository_url_and_the_image_id() {
    let run = referenced(
        "references-set",
        "base-url \"https://example.org/checked\"\n",
        "        stamp {\n            home \"{base-url}/tree/{image-id}\"\n        }",
        STAMP,
    );
    assert!(
        run.problems.issues.is_empty(),
        "{}",
        run.problems.issues.plain()
    );
    let script = finalize_of(&run);
    assert!(
        script.contains("export OPT_HOME=\"https://example.org/checked/tree/checked\""),
        "{script}"
    );
}

#[test]
fn a_default_reads_the_image_id() {
    let run = referenced(
        "references-default",
        "",
        "        stamp",
        "option \"home\" type=\"string\" {\n    default \"{image-id}-host\"\n}",
    );
    assert!(
        run.problems.issues.is_empty(),
        "{}",
        run.problems.issues.plain()
    );
    assert!(finalize_of(&run).contains("export OPT_HOME=\"checked-host\""));
}

#[test]
fn a_value_that_reads_an_undeclared_base_url_is_refused() {
    let run = referenced(
        "references-undeclared",
        "",
        "        stamp {\n            home \"{base-url}\"\n        }",
        STAMP,
    );
    let issues = run.problems.issues.plain();
    assert!(
        issues.contains("reads `{base-url}`, which nothing declares"),
        "{issues}"
    );
    assert!(issues.contains("declare `base-url"), "{issues}");
}

#[test]
fn a_default_that_reads_the_repository_url_is_refused() {
    let run = referenced(
        "references-default-url",
        "base-url \"https://example.org/checked\"\n",
        "        stamp",
        "option \"home\" type=\"string\" {\n    default \"{base-url}\"\n}",
    );
    let issues = run.problems.issues.plain();
    assert!(
        issues.contains("reads `{base-url}`, which a default cannot read"),
        "{issues}"
    );
}

#[test]
fn a_base_url_that_is_not_https_is_refused() {
    let run = referenced(
        "references-http",
        "base-url \"http://example.org\"\n",
        "        stamp",
        STAMP,
    );
    let issues = run.problems.issues.plain();
    assert!(issues.contains("is not a usable `base-url`"), "{issues}");
}

#[test]
fn a_variant_that_reads_the_repository_url_is_refused() {
    let root = reference_repo(
        "references-variant-url",
        "base-url \"https://example.org/checked\"\n",
        "        stamp variant=\"remote\"",
        "option \"home\" type=\"string\" {\n    default \"\"\n}\nvariant \"remote\" {\n    set \"home\" \"{base-url}\"\n}",
    );
    let issues = buildc::run(Command::Check, None, &root)
        .problems
        .issues
        .plain();
    let _ = std::fs::remove_dir_all(&root);
    assert!(
        issues.contains("variant `remote` sets `home` to a value that reads `{base-url}`"),
        "{issues}"
    );
}

#[test]
fn a_base_entry_option_reads_the_repository_url() {
    let root = written("base-entry-reference", "", &[]);
    std::fs::write(
        root.join("repo.kdl"),
        "schema-version 1\nname \"Checked\"\nbuilds { checked }\nbase-url \"https://example.org/checked\"\n",
    )
    .expect("the test repository is writable");
    std::fs::write(
        root.join("components/base-image/example/manifest.kdl"),
        "schema-version 1\n\nbase \"example\" {\n    family \"fedora\"\n}\noption \"tag\" type=\"string\" {\n    default \"latest\"\n}\n",
    )
    .expect("the test repository is writable");
    std::fs::write(
        root.join("components/checked.kdl"),
        rooted("schema-version 1\n\nbase-image {\n    example {\n        tag \"{base-url}/tree/{image-id}\"\n    }\n}\n"),
    )
    .expect("the test repository is writable");
    let issues = buildc::run(Command::Check, None, &root)
        .problems
        .issues
        .plain();
    let _ = std::fs::remove_dir_all(&root);
    assert!(issues.is_empty(), "{issues}");
}

#[test]
fn a_finalize_hook_reads_its_own_options_and_the_next_hook_does_not() {
    let root = written(
        "hook-options",
        "        stamp {\n            label \"Checked\"\n        }\n        later",
        &[
            (
                "stamp",
                "option \"label\" type=\"string\" {\n    default \"\"\n}",
            ),
            ("later", ""),
        ],
    );
    std::fs::write(
        root.join("components/stamp/finalize.sh"),
        "echo \"${OPT_LABEL}\"\n",
    )
    .expect("the test repository is writable");
    std::fs::write(root.join("components/later/finalize.sh"), "echo later\n")
        .expect("the test repository is writable");
    let run = buildc::run(Command::Generate, None, &root);
    let _ = std::fs::remove_dir_all(&root);
    assert!(
        run.problems.issues.is_empty(),
        "{}",
        run.problems.issues.plain()
    );
    let (_, script) = run
        .files
        .iter()
        .find(|(path, _)| path.ends_with("finalize.sh") && path.starts_with("generated/checked"))
        .expect("the image has a finalize script");
    let stamp = script.find("# ---- stamp ----").expect("the stamp hook");
    let later = script.find("# ---- later ----").expect("the later hook");
    let hook = &script[stamp..later];
    let exported = hook
        .find("export OPT_LABEL=\"Checked\"")
        .expect("the option is exported");
    let sourced = hook
        .find("source /ctx/components/stamp/finalize.sh")
        .expect("the hook is sourced");
    let unset = hook.find("unset OPT_LABEL").expect("the option is unset");
    assert!(exported < sourced && sourced < unset, "{hook}");
}

#[test]
fn a_base_that_ships_a_script_is_refused() {
    let root = written("base-ships", "", &[]);
    std::fs::write(
        root.join("components/base-image/example/component.sh"),
        "true\n",
    )
    .expect("the test repository is writable");
    let issues = buildc::run(Command::Check, None, &root)
        .problems
        .issues
        .plain();
    let _ = std::fs::remove_dir_all(&root);
    assert!(
        issues.contains("`base-image/example` is a base, and a base builds no layer"),
        "{issues}"
    );
}

#[test]
fn a_removes_of_a_name_only_the_remover_provides_fails() {
    let issues = checked(
        "removes-self",
        "        remover",
        &[("remover", "provides \"tool\"\nremoves \"tool\"")],
    );
    assert!(
        issues.contains("`remover` removes `tool`, which nothing in the image provides"),
        "{issues}"
    );
}

#[test]
fn a_base_witness_of_its_own_reaches_the_contract() {
    let plan = planned(
        "base-own-witness",
        "; provides \"bootc\"; provides \"tool\" witness=\"/opt/tool\"",
        "",
        &[],
    );
    assert!(
        squashed(&plan).contains(r#""contract_files":["/opt/tool"]"#),
        "{plan}"
    );
}

#[test]
fn a_reference_is_checked_in_a_gate_an_image_and_an_import() {
    let issues = checked(
        "reference-places",
        "        consumer",
        &[(
            "consumer",
            "on \"fedora\" { packages \"a={gated}\"; }\n\"{chosen}\"\ntool { driver \"{passed}\"; }",
        )],
    );
    assert!(issues.contains("`{gated}` names no option"), "{issues}");
    assert!(issues.contains("`{chosen}` names no option"), "{issues}");
    assert!(issues.contains("`{passed}` names no option"), "{issues}");
    let root = written("reference-image", "", &[]);
    write_base(&root, "fedora", "base \"x:{tag}\"");
    let issues = buildc::run(Command::Check, None, &root)
        .problems
        .issues
        .plain();
    let _ = std::fs::remove_dir_all(&root);
    assert!(issues.contains("`{tag}` names no option"), "{issues}");
}

#[test]
fn removing_the_only_lower_name_leaves_the_broader_requirement_unmet() {
    let root = written_on(
        "removes-hierarchy",
        "; provides \"rpm/dnf5\"",
        "        remover\n        consumer",
        &[
            ("remover", "removes \"rpm/dnf5\""),
            ("consumer", "requires \"rpm\""),
        ],
    );
    let issues = buildc::run(Command::Check, None, &root)
        .problems
        .issues
        .plain();
    let _ = std::fs::remove_dir_all(root);
    assert!(
        issues.contains("`consumer` requires `rpm`, which nothing enabled provides"),
        "{issues}"
    );
}

#[test]
fn a_base_listed_twice_is_refused() {
    let issues = checked("base-listed", "    base-image { example }", &[]);
    assert!(
        issues.contains("`base-image/example` is listed twice"),
        "{issues}"
    );
}

#[test]
fn a_requirement_the_base_meets_is_recorded_as_the_base_s() {
    let plan = planned(
        "met-by-base",
        "; provides \"tool\"",
        "        consumer",
        &[("consumer", "requires \"tool\"")],
    );
    assert!(
        squashed(&plan).contains(r#""met_by":{"tool":{"how":"base"}}"#),
        "{plan}"
    );
}

#[test]
fn a_component_the_base_requires_leaves_no_base_witness() {
    for (case, requires) in [
        ("capability", "os-release"),
        ("path", "identity"),
        ("broader", "os"),
    ] {
        let plan = planned(
            &format!("base-requires-witness-{case}"),
            &format!("; requires \"{requires}\""),
            "        identity\n        tool",
            &[
                (
                    "identity",
                    "provides \"os/release\" witness=\"/usr/lib/os-release\"\nprovides \"os-release\"",
                ),
                ("tool", "provides \"tool\" witness=\"/usr/bin/tool\""),
            ],
        );
        let squashed = squashed(&plan);
        assert!(
            squashed.contains(r#""witnesses":{"tool":["/usr/bin/tool"]}"#),
            "{case}: {plan}"
        );
        assert!(
            squashed.contains(r#""/usr/lib/os-release""#),
            "{case}: {plan}"
        );
    }
}

#[test]
fn a_component_the_base_imports_leaves_no_base_witness() {
    let plan = planned(
        "base-imports-witness",
        "; identity",
        "        tool",
        &[
            (
                "identity",
                "provides \"os-release\" witness=\"/usr/lib/os-release\"\nhelper",
            ),
            ("helper", "provides \"helper\" witness=\"/usr/bin/helper\""),
            ("tool", "provides \"tool\" witness=\"/usr/bin/tool\""),
        ],
    );
    assert!(
        squashed(&plan).contains(r#""witnesses":{"tool":["/usr/bin/tool"]}"#),
        "{plan}"
    );
}

#[test]
fn an_import_of_a_suppressed_base_import_leaves_no_base_witness() {
    let plan = planned(
        "base-imports-suppressed-witness",
        "; provides \"os-release\"; identity",
        "        tool",
        &[
            ("identity", "provides \"os-release\"\nhelper"),
            ("helper", "provides \"helper\" witness=\"/usr/bin/helper\""),
            ("tool", "provides \"tool\" witness=\"/usr/bin/tool\""),
        ],
    );
    assert!(
        squashed(&plan).contains(r#""witnesses":{"tool":["/usr/bin/tool"]}"#),
        "{plan}"
    );
}

#[test]
fn a_component_whose_lower_name_is_removed_no_longer_meets_the_broader_one() {
    let issues = checked(
        "component-lower-removed",
        "        consumer\n        manager\n        remover",
        &[
            ("consumer", "requires \"pm\""),
            ("manager", "provides \"pm/dnf5\""),
            ("remover", "removes \"pm/dnf5\""),
        ],
    );
    assert!(
        issues.contains("`consumer` requires `pm`, which nothing enabled provides"),
        "{issues}"
    );
}

#[test]
fn a_base_that_ships_files_or_packages_is_refused() {
    for (case, extra) in [
        ("files", ""),
        ("packages", "packages \"vim\""),
        ("groups", "targets \"fedora\"\npackage-groups \"core\""),
    ] {
        let root = written(&format!("base-ships-{case}"), "", &[]);
        write_base(&root, "fedora", extra);
        if case == "files" {
            std::fs::create_dir_all(root.join("components/base-image/example/files/etc"))
                .expect("the test repository is writable");
        }
        let issues = buildc::run(Command::Check, None, &root)
            .problems
            .issues
            .plain();
        let _ = std::fs::remove_dir_all(&root);
        assert!(
            issues.contains("`base-image/example` is a base, and a base builds no layer"),
            "{case}: {issues}"
        );
    }
}

#[test]
fn a_base_version_tracker_of_its_own_reaches_renovate() {
    let root = written("base-own-tracker", "", &[]);
    std::fs::write(
        root.join("components/base-image/example/manifest.kdl"),
        "schema-version 1\n\nbase \"example.invalid/base:{version}\" { family \"fedora\"; }\n\
         option \"arch\" type=\"string\" { default \"x\"; }\n\
         option \"version\" type=\"string\" { default \"1\"; renovate datasource=\"github-releases\" depName=\"owner/base\"; }\n",
    )
    .expect("the test repository is writable");
    std::fs::write(
        root.join("repo.kdl"),
        "schema-version 1\nname \"Checked\"\nbuilds { checked }\nrenovate {\n    timezone \"UTC\"\n}\n",
    )
    .expect("the test repository is writable");
    let image = std::fs::read_to_string(root.join("components/checked.kdl"))
        .expect("the image file is written");
    std::fs::write(
        root.join("components/checked.kdl"),
        image.replace(
            "base-image { example }\n",
            "base-image { example { version \"1\"; } }\n",
        ),
    )
    .expect("the test repository is writable");
    let out = generated(&root);
    let _ = std::fs::remove_dir_all(&root);
    assert!(out.contains("owner/base"), "{out}");
    assert!(!out.contains("example.invalid/base\""), "{out}");
}

#[test]
fn an_inline_definition_records_the_image_file_as_its_origin() {
    let root = written(
        "inline-origin",
        "    tools {\n        packages \"vim\"\n    }\n    adapter",
        &[("adapter", "provides \"build-environment\"")],
    );
    let out = generated(&root);
    let _ = std::fs::remove_dir_all(&root);
    assert!(
        out.contains(r#""defined": "./components/checked.kdl""#),
        "{out}"
    );
}

#[test]
fn an_inline_definition_reads_its_own_option() {
    let root = written(
        "inline-option",
        "    tools {\n        option \"pkg\" type=\"string\" { default \"htop\"; }\n        packages \"{pkg}\"\n    }\n    adapter",
        &[("adapter", "provides \"build-environment\"")],
    );
    let found = problems(&root);
    let out = generated(&root);
    let _ = std::fs::remove_dir_all(&root);
    assert_eq!(found, "");
    assert!(out.contains("htop"), "{out}");
}

#[test]
fn a_component_that_installs_nothing_builds_no_layer() {
    let root = written("idle-layer", "        idle", &[("idle", "")]);
    let run = buildc::run(Command::Generate, None, &root);
    let _ = std::fs::remove_dir_all(&root);
    assert!(
        run.problems.issues.is_empty(),
        "{}",
        run.problems.issues.plain()
    );
    let containerfile = files_named(&run, "Containerfile");
    assert!(
        !containerfile.contains("# ---- idle ----"),
        "{containerfile}"
    );
    assert!(
        !containerfile.contains("# ---- checked ----"),
        "{containerfile}"
    );
    assert!(script_if_any(&run, "components/idle.sh").is_empty());
}

fn based_as(name: &str, listing: &str) -> PathBuf {
    let root = written(name, "", &[]);
    let image = root.join("components/checked.kdl");
    let text = std::fs::read_to_string(&image).expect("the image file is written");
    std::fs::write(
        &image,
        text.replace(
            "base-image { example }",
            &format!("base-image {{ {listing} }}"),
        ),
    )
    .expect("the test repository is writable");
    std::fs::write(
        root.join("components/base-image/example/manifest.kdl"),
        "schema-version 1\noption \"tag\" type=\"string\" { default \"a\"; }\nvariant \"x\" { set \"tag\" \"b\"; }\nbase \"example:{tag}\" { family \"fedora\"; }\n",
    )
    .expect("the test repository is writable");
    root
}

#[test]
fn the_variant_the_build_lists_on_its_base_selects_the_base_values() {
    let root = based_as("base-variant", "example variant=\"x\"");
    let plan = buildc::run(Command::Plan, None, &root).report.stdout;
    let _ = std::fs::remove_dir_all(root);
    assert!(plan.contains("example:b"), "{plan}");
}

#[test]
fn a_pin_the_build_lists_on_a_local_base_is_refused() {
    let root = based_as("base-pin", "example pin=\"v1\"");
    let found = problems(&root);
    let _ = std::fs::remove_dir_all(root);
    assert!(found.contains("which the user edits in place"), "{found}");
}

#[test]
fn a_component_that_lists_a_path_twice_is_refused() {
    let root = with_libraries(
        "listed-twice-in-component",
        &tools_repo(),
        &format!("component \"/\" {{\n{BASE}    desktop\n}}\n"),
        &[
            (
                "desktop",
                "description \"Desktop\"\ntargets \"fedora\"\ntools\ntools\n",
            ),
            (
                "desktop/tools",
                "description \"Tools\"\ntargets \"fedora\"\n",
            ),
        ],
    );
    let found = problems(&root);
    let _ = std::fs::remove_dir_all(root);
    assert!(found.contains("`desktop/tools` is listed twice"), "{found}");
}

#[test]
fn a_value_a_component_sets_reads_the_image_id() {
    let root = reference_repo("references-relay", "", "        relay", STAMP);
    std::fs::create_dir_all(root.join("components/relay"))
        .expect("the test repository is writable");
    std::fs::write(
        root.join("components/relay/manifest.kdl"),
        "schema-version 1\ndescription \"Relay\"\ntargets \"fedora\"\ncomponent \"/\" {\n    stamp {\n        home \"{image-id}-host\"\n    }\n}\n",
    )
    .expect("the test repository is writable");
    let run = buildc::run(Command::Generate, None, &root);
    let _ = std::fs::remove_dir_all(&root);
    assert!(
        run.problems.issues.is_empty(),
        "{}",
        run.problems.issues.plain()
    );
    let script = finalize_of(&run);
    assert!(
        script.contains("export OPT_HOME=\"checked-host\""),
        "{script}"
    );
}

#[test]
fn an_inline_definition_in_a_component_builds_and_records_its_manifest() {
    let root = with_libraries(
        "inline-in-component",
        &tools_repo(),
        &format!("component \"/\" {{\n{BASE}    desktop\n    adapter\n}}\n"),
        &[
            (
                "adapter",
                "targets \"fedora\"\nprovides \"build-environment\"\n",
            ),
            (
                "desktop",
                "description \"Desktop\"\ntargets \"fedora\"\ntools {\n    packages \"vim\"\n}\n",
            ),
        ],
    );
    let out = generated(&root);
    let why = why_of(&root, "desktop/tools");
    let _ = std::fs::remove_dir_all(root);
    assert!(out.contains("vim"), "{out}");
    assert!(
        out.contains(r#""defined": "./components/desktop/manifest.kdl""#),
        "{out}"
    );
    assert!(why.contains("`desktop` imports it"), "{why}");
}

#[test]
fn an_inline_definition_in_a_library_component_reads_from_that_library() {
    let root = with_libraries(
        "inline-in-library",
        &tools_repo(),
        &format!("component \"/\" {{\n{BASE}    podman\n    adapter\n}}\n"),
        &[
            ("adapter", "targets \"fedora\"\nprovides \"build-environment\"\n"),
            (
            "tools/podman",
            "description \"Podman\"\ntargets \"fedora\"\nextras {\n    packages \"podman-compose\"\n}\n",
            ),
        ],
    );
    let out = generated(&root);
    let why = why_of(&root, "podman/extras");
    let _ = std::fs::remove_dir_all(root);
    assert!(out.contains("podman-compose"), "{out}");
    assert!(why.contains("tools"), "{why}");
}

#[test]
fn a_pin_on_an_inline_definition_in_a_component_is_refused() {
    let root = with_libraries(
        "inline-pinned",
        &tools_repo(),
        &format!("component \"/\" {{\n{BASE}    desktop\n}}\n"),
        &[(
            "desktop",
            "description \"Desktop\"\ntargets \"fedora\"\ntools pin=\"v1\" {\n    packages \"vim\"\n}\n",
        )],
    );
    let found = problems(&root);
    let _ = std::fs::remove_dir_all(root);
    assert!(
        found.contains("`desktop` defines `desktop/tools` inline at `pin=\"v1\"`"),
        "{found}"
    );
}

#[test]
fn an_inline_provider_answers_only_its_own_image() {
    let root = written("inline-scope", "", &[("needs", "requires \"tool\"")]);
    std::fs::write(
        root.join("repo.kdl"),
        "schema-version 1\nbuilds {\n    a\n    b\n}\n",
    )
    .expect("the test repository is writable");
    write_image(
        &root,
        "a",
        "schema-version 1\n\nbase-image { example }\ntools {\n    provides \"tool\"\n}\n",
    );
    write_image(
        &root,
        "b",
        "schema-version 1\n\nbase-image { example }\nneeds\n",
    );
    let issues = buildc::run(Command::Check, None, &root)
        .problems
        .issues
        .plain();
    let _ = std::fs::remove_dir_all(&root);
    assert!(
        issues.contains("no component in the repository declares `provides \"tool\"`"),
        "{issues}"
    );
    assert_eq!(issues.matches("provides \"tool\"").count(), 1, "{issues}");
}

const PINNED_BASE: &str = "components/base-image/fedora-bootc-44/manifest.kdl";

fn conforming(name: &str) -> PathBuf {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    if let Err(err) = std::fs::remove_dir_all(&root) {
        assert_eq!(
            err.kind(),
            std::io::ErrorKind::NotFound,
            "{}: {err}",
            root.display()
        );
    }
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/repos/multi-image");
    let copied = std::process::Command::new("cp")
        .arg("-r")
        .arg(&fixture)
        .arg(&root)
        .status()
        .expect("cp runs");
    assert!(copied.success());
    root
}

fn edit(path: &std::path::Path, from: &str, to: &str) {
    let text = std::fs::read_to_string(path).expect("the fixture file reads");
    assert!(text.contains(from), "{} holds no {from}", path.display());
    std::fs::write(path, text.replace(from, to)).expect("the fixture file writes");
}

fn zipped(root: &std::path::Path) {
    let archive = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/scap/datastream.zip");
    std::fs::copy(archive, root.join("scap.zip")).expect("the archive copies");
}

fn written_datastream(run: &buildc::Run) -> Option<&[u8]> {
    run.copies
        .iter()
        .find(|copy| copy.path.ends_with("workstation/scap/ssg-fedora-ds.xml"))
        .map(|copy| copy.bytes.as_slice())
}

#[test]
fn a_datastream_that_is_not_the_pinned_one_refuses_the_generate() {
    let root = conforming("scap-tampered");
    edit(
        &root.join(PINNED_BASE),
        "402fe92ace1c30bd5a9c0f2806f96b412fec6b781023459bd30c99c6c5163c7b",
        &"0".repeat(64),
    );
    let run = buildc::run(Command::Generate, None, &root);
    std::fs::remove_dir_all(&root).expect("the scratch repository goes");
    let said = run.problems.issues.plain();
    assert!(said.contains("is not the datastream"), "{said}");
    assert!(said.contains("402fe92ace1c30bd5a9c0f2806f96b41"), "{said}");
    assert!(written_datastream(&run).is_none());
}

#[test]
fn a_committed_datastream_that_matches_its_pin_needs_no_fetch() {
    let root = conforming("scap-committed");
    let run = buildc::run(Command::Generate, None, &root);
    buildc::write_generated(&root, &run.files, &run.copies).unwrap();
    std::fs::remove_file(root.join("components/base-image/fedora-bootc-44/ssg-fedora-ds.xml"))
        .unwrap();
    let again = buildc::run(Command::Generate, None, &root);
    std::fs::remove_dir_all(&root).expect("the scratch repository goes");
    assert_eq!(again.problems.issues.plain(), "");
    assert_eq!(written_datastream(&again), written_datastream(&run));
}

#[test]
fn a_zipped_datastream_is_read_from_its_member() {
    let root = conforming("scap-zipped");
    zipped(&root);
    edit(
        &root.join(PINNED_BASE),
        "url \"components/base-image/fedora-bootc-44/ssg-fedora-ds.xml\"",
        "url \"scap.zip\"",
    );
    let run = buildc::run(Command::Generate, None, &root);
    std::fs::remove_dir_all(&root).expect("the scratch repository goes");
    assert_eq!(run.problems.issues.plain(), "");
    let expected =
        std::fs::read(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/scap/datastream.xml"))
            .unwrap();
    assert_eq!(written_datastream(&run), Some(expected.as_slice()));
}

#[test]
fn a_claim_the_pinned_datastream_does_not_reach_refuses_the_generate() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/repos/enforced");
    let said = buildc::run(Command::Generate, None, &root)
        .problems
        .issues
        .plain();
    assert!(
        said.contains("9.9.9.9 reaches no rule in ssg-fedora-ds.xml"),
        "{said}"
    );
}

#[test]
fn a_zip_without_the_named_datastream_refuses_the_generate() {
    let root = conforming("scap-zip-missing");
    zipped(&root);
    let base = root.join(PINNED_BASE);
    edit(
        &base,
        "url \"components/base-image/fedora-bootc-44/ssg-fedora-ds.xml\"",
        "url \"scap.zip\"",
    );
    edit(
        &base,
        "scap-content \"ssg-fedora-ds.xml\"",
        "scap-content \"ssg-cs10-ds.xml\"",
    );
    let said = buildc::run(Command::Generate, None, &root)
        .problems
        .issues
        .plain();
    std::fs::remove_dir_all(&root).expect("the scratch repository goes");
    let flat = said
        .replace('|', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert!(flat.contains("holds no ssg-cs10-ds.xml"), "{said}");
}

#[test]
fn a_scan_defaults_to_the_generated_datastream() {
    let root = conforming("scap-default");
    let (list, _, _) = buildc::declarations(&root);
    let measured = buildc::scap::datastream(&list, Some("workstation"));
    let unmeasured = buildc::scap::datastream(&list, Some("server"));
    std::fs::remove_dir_all(&root).expect("the scratch repository goes");
    assert_eq!(
        measured.as_deref(),
        Ok("generated/workstation/scap/ssg-fedora-ds.xml")
    );
    assert_eq!(unmeasured.as_deref(), Ok(""));
}

#[test]
fn a_cloned_asset_resolves_into_the_plan_of_the_target_that_lists_it() {
    let root = conforming("clone-resolution");
    let commit = "0123456789abcdef0123456789abcdef01234567";
    edit(
        &root.join("components/server.kdl"),
        "    shared\n",
        "    shared\n    dev-tools\n",
    );
    let manifest = root.join("components/core/dev-tools/manifest.kdl");
    let text = std::fs::read_to_string(&manifest).expect("the fixture file reads");
    std::fs::write(
        &manifest,
        format!(
            "{text}\nasset \"src\" {{\n    pin {{\n        manual \"a test\"\n        version \"{commit}\"\n        url \"https://example.com/src.git\"\n    }}\n}}\n"
        ),
    )
    .expect("the fixture file writes");
    let run = buildc::run(Command::Plan, None, &root);
    std::fs::remove_dir_all(&root).expect("the scratch repository goes");
    let plan = run.report.stdout;
    let resolved = format!("ASSET_RESOLUTIONS=core/dev-tools|src|{commit}|{commit}");
    assert_eq!(
        plan.matches(&resolved).count(),
        1,
        "{}",
        run.problems.issues.plain()
    );
}

#[test]
fn a_datastream_pin_that_is_neither_https_nor_a_repository_path_is_refused() {
    let root = conforming("scap-file-scheme");
    edit(
        &root.join(PINNED_BASE),
        "url \"components/base-image/fedora-bootc-44/ssg-fedora-ds.xml\"",
        "url \"file:///usr/share/xml/scap/ssg/content/ssg-fedora-ds.xml\"",
    );
    let said = buildc::run(Command::Generate, None, &root)
        .problems
        .issues
        .plain();
    std::fs::remove_dir_all(&root).expect("the scratch repository goes");
    assert!(said.contains("datastream downloads over"), "{said}");
}

#[test]
fn a_changed_pin_url_does_not_reuse_the_committed_datastream() {
    let root = conforming("scap-moved-url");
    let run = buildc::run(Command::Generate, None, &root);
    buildc::write_generated(&root, &run.files, &run.copies).expect("generate writes");
    edit(
        &root.join(PINNED_BASE),
        "url \"components/base-image/fedora-bootc-44/ssg-fedora-ds.xml\"",
        "url \"moved/ssg-fedora-ds.xml\"",
    );
    let said = buildc::run(Command::Generate, None, &root)
        .problems
        .issues
        .plain();
    std::fs::remove_dir_all(&root).expect("the scratch repository goes");
    assert!(said.contains("moved/ssg-fedora-ds.xml"), "{said}");
}

#[test]
fn an_enforced_audit_refuses_a_clone_that_names_no_commit_at_generate() {
    let root = conforming("clone-enforced");
    let repo = root.join("repo.kdl");
    let text = std::fs::read_to_string(&repo).expect("repo.kdl reads");
    std::fs::write(&repo, format!("{text}\naudit {{\n    enforce #true\n}}\n"))
        .expect("repo.kdl writes");
    let manifest = root.join("components/core/shared/manifest.kdl");
    let text = std::fs::read_to_string(&manifest).expect("the fixture file reads");
    std::fs::write(
        &manifest,
        format!(
            "{text}\nasset \"src\" {{\n    pin {{\n        manual \"a test\"\n        url \"https://example.com/src.git\"\n    }}\n}}\n"
        ),
    )
    .expect("the fixture file writes");
    let said = buildc::run(Command::Generate, None, &root)
        .problems
        .issues
        .plain();
    std::fs::remove_dir_all(&root).expect("the scratch repository goes");
    assert!(said.contains("names no `version` to resolve"), "{said}");
    assert_eq!(
        said.matches("names no `version` to resolve").count(),
        1,
        "{said}"
    );
}

#[test]
fn a_copr_on_a_fedora_base_needs_a_family_adapter() {
    let issues = checked(
        "copr-adapter",
        "        tools",
        &[("tools", "copr \"owner/project\"")],
    );
    assert!(issues.contains("missing family adapter"), "{issues}");
}

#[test]
fn a_named_image_or_target_must_be_declared() {
    let root = written("unknown-names", "", &[]);
    let issues = |command, arg| {
        buildc::run(command, Some(arg), &root)
            .problems
            .issues
            .plain()
    };
    let image = issues(Command::Section, "nosuch");
    let target = issues(Command::Summary, "nosuch");
    let known = issues(Command::Section, "checked");
    let _ = std::fs::remove_dir_all(&root);
    assert!(
        image.contains("`nosuch` is not a declared image"),
        "{image}"
    );
    assert!(
        target.contains("`nosuch` is not a build target"),
        "{target}"
    );
    assert!(!known.contains("is not a declared image"), "{known}");
}

fn with_libraries(name: &str, repo: &str, image: &str, files: &[(&str, &str)]) -> PathBuf {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("components")).expect("the test repository is writable");
    std::fs::write(root.join("repo.kdl"), repo).expect("the test repository is writable");
    std::fs::write(
        root.join("components/checked.kdl"),
        format!("schema-version 1\n{image}"),
    )
    .expect("the test repository is writable");
    write_base(&root, "fedora", "");
    for (path, manifest) in files {
        let dir = root.join("components").join(path);
        std::fs::create_dir_all(&dir).expect("the test repository is writable");
        std::fs::write(
            dir.join("manifest.kdl"),
            format!("schema-version 1\n{manifest}"),
        )
        .expect("the test repository is writable");
    }
    root
}

fn problems(root: &std::path::Path) -> String {
    buildc::run(Command::Check, None, root)
        .problems
        .issues
        .plain()
}

fn why_of(root: &std::path::Path, component: &str) -> String {
    buildc::run(Command::Why, Some(component), root)
        .report
        .stdout
}

const BASE: &str = "    base-image { example }\n";

fn tools_repo() -> String {
    "schema-version 1\nname \"Checked\"\nbuilds { checked }\nlibraries default=\"checked-library\" {\n    local {\n        checked-library { source \".\" }\n        tools { source \"components/tools\" }\n    }\n}\n".to_string()
}

fn two_libraries(default: &str) -> String {
    format!(
        "schema-version 1\nname \"Checked\"\nbuilds {{ checked }}\nlibraries{default} {{\n    local {{\n        checked-library {{ source \".\" }}\n        tools {{ source \"components/tools\" }}\n        stable {{ source \"components/stable\" }}\n    }}\n}}\n",
        default = match default {
            "" => " default=\"checked-library\"",
            given => given,
        }
    )
}

#[test]
fn a_rooted_path_resolves_to_the_only_library_that_holds_it() {
    let root = with_libraries(
        "library-only-holder",
        &tools_repo(),
        &format!("component \"/\" {{\n{BASE}    podman\n}}\n"),
        &[(
            "tools/podman",
            "description \"Podman from tools\"\ntargets \"fedora\"\n",
        )],
    );
    let found = problems(&root);
    let why = why_of(&root, "podman");
    let _ = std::fs::remove_dir_all(root);
    assert_eq!(found, "");
    assert!(why.contains("Podman from tools"), "{why}");
    assert!(why.contains("read from library `tools`"), "{why}");
}

#[test]
fn a_build_reads_its_component_when_the_root_is_no_library() {
    let repo = "schema-version 1\nname \"Checked\"\nbuilds { checked }\nlibraries {\n    local {\n        tools { source \"components/tools\" }\n    }\n}\n";
    let tool = (
        "tools/fedora",
        "base \"quay.io/fedora/fedora-bootc:44\" { family \"fedora\"; }\n",
    );
    let root = with_libraries(
        "library-no-root",
        repo,
        "component \"/\" { fedora }\n",
        &[tool],
    );
    let found = problems(&root);
    let _ = std::fs::remove_dir_all(root);
    assert_eq!(found, "");

    let root = with_libraries(
        "library-no-root-path",
        repo,
        "component \"/\" {\n    fedora\n    podman\n}\n",
        &[
            tool,
            ("podman", "description \"Podman\"\ntargets \"fedora\"\n"),
        ],
    );
    let found = problems(&root);
    let _ = std::fs::remove_dir_all(root);
    assert!(
        found.contains("lists `podman`, which no library holds"),
        "{found}"
    );
}

#[test]
fn the_root_library_name_reads_this_repository() {
    let root = with_libraries(
        "library-root-name",
        &two_libraries(" default=\"tools\""),
        &format!("component \"/\" {{\n{BASE}    podman library=\"checked-library\"\n}}\n"),
        &[
            (
                "podman",
                "description \"Podman from here\"\ntargets \"fedora\"\n",
            ),
            (
                "tools/podman",
                "description \"Podman from tools\"\ntargets \"fedora\"\n",
            ),
        ],
    );
    let found = problems(&root);
    let why = why_of(&root, "podman");
    let _ = std::fs::remove_dir_all(root);
    assert_eq!(found, "");
    assert!(why.contains("Podman from here"), "{why}");
    assert!(!why.contains("read from library"), "{why}");
}

#[test]
fn a_library_property_chooses_the_library_that_holds_the_path() {
    let root = with_libraries(
        "library-property",
        &two_libraries(""),
        &format!("component \"/\" {{\n{BASE}    podman library=\"tools\"\n}}\n"),
        &[
            (
                "tools/podman",
                "description \"Podman from tools\"\ntargets \"fedora\"\n",
            ),
            (
                "stable/podman",
                "description \"Podman from stable\"\ntargets \"fedora\"\n",
            ),
        ],
    );
    let found = problems(&root);
    let why = why_of(&root, "podman");
    let _ = std::fs::remove_dir_all(root);
    assert_eq!(found, "");
    assert!(why.contains("Podman from tools"), "{why}");
    assert!(!why.contains("Podman from stable"), "{why}");
}

#[test]
fn the_default_library_settles_an_ambiguous_rooted_path() {
    let root = with_libraries(
        "library-default",
        &two_libraries(" default=\"stable\""),
        &format!("component \"/\" {{\n{BASE}    podman\n}}\n"),
        &[
            (
                "tools/podman",
                "description \"Podman from tools\"\ntargets \"fedora\"\n",
            ),
            (
                "stable/podman",
                "description \"Podman from stable\"\ntargets \"fedora\"\n",
            ),
        ],
    );
    let found = problems(&root);
    let why = why_of(&root, "podman");
    let _ = std::fs::remove_dir_all(root);
    assert_eq!(found, "");
    assert!(why.contains("Podman from stable"), "{why}");
}

#[test]
fn the_default_library_settles_a_path_that_this_repository_also_holds() {
    let repo = "schema-version 1\nname \"Checked\"\nbuilds { checked }\nlibraries default=\"stable\" {\n    local {\n        checked-library { source \".\" }\n        stable { source \"components/stable\" }\n    }\n}\n";
    // stable is a folder of this repository's components and a library of its own. The same directory answers two paths.
    let files = [
        (
            "podman",
            "description \"Podman from here\"\ntargets \"fedora\"\n",
        ),
        (
            "stable/podman",
            "description \"Podman from stable\"\ntargets \"fedora\"\n",
        ),
    ];
    let root = with_libraries(
        "library-default-over-root",
        repo,
        &format!("component \"/\" {{\n{BASE}    podman\n}}\n"),
        &files,
    );
    let found = problems(&root);
    let short = why_of(&root, "podman");
    let _ = std::fs::remove_dir_all(root);
    let root = with_libraries(
        "library-root-by-path",
        repo,
        &format!("component \"/\" {{\n{BASE}    stable {{\n        podman\n    }}\n}}\n"),
        &files,
    );
    let long = why_of(&root, "stable/podman");
    let _ = std::fs::remove_dir_all(root);
    assert_eq!(found, "");
    assert!(short.contains("read from library `stable`"), "{short}");
    assert!(!long.contains("read from library"), "{long}");
}

#[test]
fn an_ambiguous_rooted_path_is_refused_with_the_library_fix() {
    let root = with_libraries(
        "library-ambiguous",
        &two_libraries(""),
        &format!("component \"/\" {{\n{BASE}    podman\n}}\n"),
        &[
            (
                "tools/podman",
                "description \"Podman from tools\"\ntargets \"fedora\"\n",
            ),
            (
                "stable/podman",
                "description \"Podman from stable\"\ntargets \"fedora\"\n",
            ),
        ],
    );
    let found = problems(&root);
    let _ = std::fs::remove_dir_all(root);
    assert!(
        found.contains("which `tools` and `stable` each hold"),
        "{found}"
    );
    assert!(found.contains("`library=\"tools\"`"), "{found}");
}

fn two_kits() -> [(&'static str, &'static str); 4] {
    [
        (
            "tools/kit",
            "description \"Kit from tools\"\ntargets \"fedora\"\n",
        ),
        (
            "tools/kit/podman",
            "description \"Podman from tools\"\ntargets \"fedora\"\n",
        ),
        (
            "stable/kit",
            "description \"Kit from stable\"\ntargets \"fedora\"\n",
        ),
        (
            "stable/kit/podman",
            "description \"Podman from stable\"\ntargets \"fedora\"\n",
        ),
    ]
}

#[test]
fn a_child_library_leaves_its_ambiguous_parent_segment_refused() {
    let root = with_libraries(
        "library-ambiguous-parent",
        &two_libraries(""),
        &format!(
            "component \"/\" {{\n{BASE}    kit {{\n        podman library=\"tools\"\n    }}\n}}\n"
        ),
        &two_kits(),
    );
    let found = problems(&root);
    let _ = std::fs::remove_dir_all(root);
    assert!(
        found.contains("lists `kit`, which `tools` and `stable` each hold"),
        "{found}"
    );
}

#[test]
fn an_undeclared_library_on_a_parent_segment_is_refused() {
    let root = with_libraries(
        "library-undeclared-parent",
        &two_libraries(" default=\"stable\""),
        &format!("component \"/\" {{\n{BASE}    kit library=\"typo\" {{\n        podman library=\"tools\"\n    }}\n}}\n"),
        &two_kits(),
    );
    let found = problems(&root);
    let _ = std::fs::remove_dir_all(root);
    assert!(
        found.contains("reads `kit` from `typo`, which `libraries` does not declare"),
        "{found}"
    );
}

#[test]
fn the_default_library_settles_a_parent_segment_while_its_child_names_another() {
    let root = with_libraries(
        "library-default-parent",
        &two_libraries(" default=\"stable\""),
        &format!(
            "component \"/\" {{\n{BASE}    kit {{\n        podman library=\"tools\"\n    }}\n}}\n"
        ),
        &two_kits(),
    );
    let found = problems(&root);
    let kit = why_of(&root, "kit");
    let podman = why_of(&root, "kit/podman");
    let _ = std::fs::remove_dir_all(root);
    assert_eq!(found, "");
    assert!(kit.contains("Kit from stable"), "{kit}");
    assert!(podman.contains("Podman from tools"), "{podman}");
}

#[test]
fn a_relative_path_stays_in_the_library_that_lists_it() {
    let root = with_libraries(
        "library-relative",
        &tools_repo(),
        &format!("component \"/\" {{\n{BASE}    podman\n}}\n"),
        &[
            (
                "tools/podman",
                "description \"Podman\"\ntargets \"fedora\"\ncomponent \"..\" { cuda }\n",
            ),
            // stable/cuda is a component of the default library. The refusal names tools and proves the climb stayed there.
            ("stable/cuda", "description \"CUDA\"\ntargets \"fedora\"\n"),
        ],
    );
    let found = problems(&root);
    let _ = std::fs::remove_dir_all(root);
    assert!(
        found.contains("reads `cuda` from library `tools`, which does not hold it"),
        "{found}"
    );
}

#[test]
fn a_rooted_import_from_a_library_component_resolves_across_libraries() {
    let root = with_libraries(
        "library-rooted-import",
        &two_libraries(""),
        &format!("component \"/\" {{\n{BASE}    podman\n}}\n"),
        &[
            (
                "tools/podman",
                "description \"Podman\"\ntargets \"fedora\"\ncomponent \"/\" { cuda }\n",
            ),
            ("stable/cuda", "description \"CUDA\"\ntargets \"fedora\"\n"),
        ],
    );
    let found = problems(&root);
    let why = why_of(&root, "cuda");
    let _ = std::fs::remove_dir_all(root);
    assert_eq!(found, "");
    assert!(why.contains("CUDA"), "{why}");
    assert!(why.contains("`podman` imports it into `checked`."), "{why}");
}

fn default_choosing(name: &str, default: &str) -> PathBuf {
    with_libraries(
        name,
        &format!(
            "schema-version 1\nname \"Checked\"\nbuilds {{ checked }}\nlibraries{default} {{\n    local {{\n        checked-library {{ source \".\" }}\n        tools {{ source \"components/tools\" }}\n        stable {{ source \"components/stable\" }}\n    }}\n}}\n"
        ),
        &format!("component \"/\" {{\n{BASE}    podman\n}}\n"),
        &[
            (
                "tools/podman",
                "description \"Podman\"\ntargets \"fedora\"\ncomponent \"/\" { cuda }\n",
            ),
            ("cuda", "description \"CUDA from here\"\ntargets \"fedora\"\n"),
            ("stable/cuda", "description \"CUDA from stable\"\ntargets \"fedora\"\n"),
        ],
    )
}

#[test]
fn a_default_naming_the_root_library_reads_this_repository() {
    let root = default_choosing("library-default-root", " default=\"checked-library\"");
    let found = problems(&root);
    let why = why_of(&root, "cuda");
    let _ = std::fs::remove_dir_all(root);
    assert_eq!(found, "");
    assert!(why.contains("CUDA from here"), "{why}");
}

#[test]
fn several_local_libraries_without_a_default_leave_a_rooted_path_ambiguous() {
    let root = default_choosing("library-no-default", "");
    let found = problems(&root);
    let _ = std::fs::remove_dir_all(root);
    assert!(
        found.contains("which this repository and `stable` each hold"),
        "{found}"
    );
}

#[test]
fn two_paths_to_one_component_directory_are_refused() {
    let root = with_libraries(
        "library-one-directory",
        &tools_repo(),
        &format!(
            "component \"/\" {{\n{BASE}    tools {{ podman }}\n    podman library=\"tools\"\n}}\n"
        ),
        &[(
            "tools/podman",
            "description \"Podman\"\ntargets \"fedora\"\n",
        )],
    );
    let found = problems(&root);
    let _ = std::fs::remove_dir_all(root);
    assert!(found.contains("are one component directory"), "{found}");
    assert!(found.contains("`tools/podman` and `podman`"), "{found}");
}

#[test]
fn an_undeclared_library_is_refused() {
    let root = with_libraries(
        "library-undeclared",
        &tools_repo(),
        &format!("component \"/\" {{\n{BASE}    podman library=\"nope\"\n}}\n"),
        &[(
            "tools/podman",
            "description \"Podman\"\ntargets \"fedora\"\n",
        )],
    );
    let found = problems(&root);
    let _ = std::fs::remove_dir_all(root);
    assert!(
        found.contains("from `nope`, which `libraries` does not declare"),
        "{found}"
    );
}

#[test]
fn a_remote_library_without_a_cache_cannot_hold_a_path() {
    let root = with_libraries(
        "library-no-cache",
        "schema-version 1\nname \"Checked\"\nbuilds { checked }\nlibraries {\n    remote {\n        stable { source \"https://example.invalid/library\" }\n    }\n}\n",
        &format!("component \"/\" {{\n{BASE}    podman\n}}\n"),
        &[],
    );
    let found = problems(&root);
    let _ = std::fs::remove_dir_all(root);
    assert!(found.contains("which no library holds"), "{found}");
    assert!(
        found.contains("buildc cannot read the components folder of `stable`"),
        "{found}"
    );
}

#[test]
fn a_written_library_without_a_cache_is_named() {
    let root = with_libraries(
        "library-written-no-cache",
        "schema-version 1\nname \"Checked\"\nbuilds { checked }\nlibraries {\n    remote {\n        stable { source \"https://example.invalid/library\" }\n    }\n}\n",
        &format!("component \"/\" {{\n{BASE}    podman library=\"stable\"\n}}\n"),
        &[],
    );
    let found = problems(&root);
    let _ = std::fs::remove_dir_all(root);
    assert!(
        found.contains("from library `stable`, which has no components"),
        "{found}"
    );
}

#[test]
fn a_base_component_loads_from_its_library() {
    let root = with_libraries(
        "library-base",
        "schema-version 1\nname \"Checked\"\nbuilds { checked }\nlibraries default=\"buildc-org\" {\n    local {\n        buildc-org { source \".\" }\n        upstream { source \"components/upstream\" }\n    }\n}\n",
        "component \"/\" { fedora }\n",
        &[(
            "upstream/fedora",
            "base \"quay.io/fedora/fedora-bootc:44\" { family \"fedora\"; }\n",
        )],
    );
    let found = problems(&root);
    let _ = std::fs::remove_dir_all(root);
    assert_eq!(found, "");
}

#[test]
fn a_library_on_a_context_holds_for_every_path_below_it() {
    let root = with_libraries(
        "library-context",
        &two_libraries(""),
        &format!(
            "component \"/\" {{\n{BASE}}}\ncomponent \"/\" library=\"tools\" {{\n    podman\n}}\n"
        ),
        &[
            (
                "tools/podman",
                "description \"Podman from tools\"\ntargets \"fedora\"\n",
            ),
            (
                "stable/podman",
                "description \"Podman from stable\"\ntargets \"fedora\"\n",
            ),
        ],
    );
    let found = problems(&root);
    let why = why_of(&root, "podman");
    let _ = std::fs::remove_dir_all(root);
    assert_eq!(found, "");
    assert!(why.contains("Podman from tools"), "{why}");
}

#[test]
fn a_library_component_ships_its_files_into_the_generated_output() {
    let root = with_libraries(
        "library-overlay",
        &tools_repo(),
        &format!("component \"/\" {{\n{BASE}    podman\n}}\n"),
        &[(
            "tools/podman",
            "description \"Podman\"\ntargets \"fedora\"\n",
        )],
    );
    let config = root.join("components/tools/podman/files/etc/podman.conf");
    std::fs::create_dir_all(config.parent().expect("the file has a parent"))
        .expect("the test repository is writable");
    std::fs::write(&config, "hello\n").expect("the test repository is writable");
    let run = buildc::run(Command::Generate, None, &root);
    let containerfile = run
        .files
        .iter()
        .find(|(path, _)| path.ends_with("Containerfile"))
        .map(|(_, text)| text.clone())
        .unwrap_or_default();
    let shipped = run
        .copies
        .iter()
        .find(|copy| {
            copy.path
                .ends_with("components/podman/files/etc/podman.conf")
        })
        .expect("the overlay reaches the generated output");
    let _ = std::fs::remove_dir_all(root);
    assert!(containerfile.contains("podman/files/ /"), "{containerfile}");
    assert_eq!(shipped.bytes, b"hello\n");
}

#[test]
fn a_written_library_that_lacks_the_path_is_refused() {
    let root = with_libraries(
        "library-written-lacks",
        &tools_repo(),
        &format!("component \"/\" {{\n{BASE}    podman library=\"tools\"\n}}\n"),
        &[
            (
                "podman",
                "description \"Local podman\"\ntargets \"fedora\"\n",
            ),
            ("tools/other", "description \"Other\"\ntargets \"fedora\"\n"),
        ],
    );
    let found = problems(&root);
    let _ = std::fs::remove_dir_all(root);
    assert!(
        found.contains("reads `podman` from library `tools`, which does not hold it"),
        "{found}"
    );
}

#[test]
fn the_outer_path_builds_a_parent_that_is_a_component() {
    let root = with_libraries(
        "library-outer-path",
        &tools_repo(),
        &format!("component \"/\" {{\n{BASE}    tools {{ podman }}\n}}\n"),
        &[
            ("tools", "description \"Tools\"\ntargets \"fedora\"\n"),
            (
                "tools/podman",
                "description \"Podman\"\ntargets \"fedora\"\n",
            ),
        ],
    );
    let plan = buildc::run(Command::Plan, None, &root).report.stdout;
    let _ = std::fs::remove_dir_all(root);
    assert!(plan.contains("\"path\": \"tools\""), "{plan}");
    assert!(plan.contains("\"path\": \"tools/podman\""), "{plan}");
}

#[test]
fn a_local_component_that_only_ships_a_library_collected_file_is_read() {
    let root = with_libraries(
        "library-collector",
        "schema-version 1\nname \"Checked\"\nbuilds { checked }\nlibraries default=\"checked-library\" {\n    local {\n        checked-library { source \".\" }\n        collectors { source \"upstream\" }\n    }\n}\n",
        &format!(
            "component \"/\" {{\n{BASE}    menu library=\"collectors\"\n    noise\n}}\n"
        ),
        &[],
    );
    let menu = root.join("upstream/menu");
    std::fs::create_dir_all(&menu).expect("the test repository is writable");
    std::fs::write(
        menu.join("manifest.kdl"),
        "schema-version 1\ndescription \"Menu\"\ntargets \"fedora\"\ncollects \"justfile.inc\" into=\"/usr/share/menu/justfile.apps\" priority=500\n",
    )
    .expect("the test repository is writable");
    let noise = root.join("components/noise");
    std::fs::create_dir_all(&noise).expect("the test repository is writable");
    std::fs::write(noise.join("justfile.inc"), "x\n").expect("the test repository is writable");
    let found = problems(&root);
    let _ = std::fs::remove_dir_all(root);
    assert_eq!(found, "");
}

#[test]
fn a_fragment_component_ships_its_own_files() {
    let root = written(
        "fragment-files",
        "        spliced",
        &[("spliced", "fragment position=\"after\"")],
    );
    std::fs::write(
        root.join("components/spliced/Containerfile.inc"),
        "RUN /ctx/components/spliced/setup.sh\n",
    )
    .expect("the test repository is writable");
    std::fs::write(root.join("components/spliced/setup.sh"), "true\n")
        .expect("the test repository is writable");
    let run = buildc::run(Command::Generate, None, &root);
    let copied = run
        .copies
        .iter()
        .any(|copy| copy.path.ends_with("components/spliced/setup.sh"));
    let _ = std::fs::remove_dir_all(root);
    assert!(
        run.problems.issues.is_empty(),
        "{}",
        run.problems.issues.plain()
    );
    assert!(copied, "the fragment's own file is copied with it");
}

#[test]
fn a_library_component_without_a_manifest_loads_bare() {
    let root = with_libraries(
        "library-bare",
        &tools_repo(),
        &format!("component \"/\" {{\n{BASE}    bare\n}}\n"),
        &[],
    );
    let bare = root.join("components/tools/bare");
    std::fs::create_dir_all(&bare).expect("the test repository is writable");
    std::fs::write(bare.join("component.sh"), "echo bare\n")
        .expect("the test repository is writable");
    let run = buildc::run(Command::Generate, None, &root);
    let copied = run
        .copies
        .iter()
        .any(|copy| copy.path.ends_with("components/bare/component.sh"));
    let _ = std::fs::remove_dir_all(root);
    assert!(
        run.problems.issues.is_empty(),
        "{}",
        run.problems.issues.plain()
    );
    assert!(copied, "the bare component's script is copied");
}

#[test]
fn two_references_to_one_path_from_two_libraries_are_refused() {
    let root = with_libraries(
        "library-two-references",
        &two_libraries(""),
        &format!(
            "component \"/\" {{\n{BASE}    podman library=\"tools\"\n    consumer\n}}\n"
        ),
        &[
            (
                "tools/podman",
                "description \"Podman from tools\"\ntargets \"fedora\"\n",
            ),
            (
                "stable/podman",
                "description \"Podman from stable\"\ntargets \"fedora\"\n",
            ),
            (
                "consumer",
                "description \"Consumer\"\ntargets \"fedora\"\ncomponent \"/\" { podman library=\"stable\" }\n",
            ),
        ],
    );
    let found = problems(&root);
    let _ = std::fs::remove_dir_all(root);
    assert!(found.contains("reads `podman` from `stable`"), "{found}");
    assert!(found.contains("`checked` reads it from"), "{found}");
}

#[test]
fn a_remote_library_component_loads_from_its_cache() {
    let root = with_libraries(
        "library-cache",
        "schema-version 1\nname \"Checked\"\nbuilds { checked }\nlibraries {\n    remote {\n        stable { source \"https://example.invalid/library\"; version \"v1\" }\n    }\n}\n",
        &format!("component \"/\" {{\n{BASE}    podman\n}}\n"),
        &[],
    );
    let cache = buildc::collection::seed(
        &root,
        "https://example.invalid/library",
        "v1",
        &"1".repeat(40),
    )
    .expect("the source key is computed");
    std::fs::create_dir_all(cache.join("components/podman"))
        .expect("the test repository is writable");
    std::fs::write(
        cache.join("repo.kdl"),
        "schema-version 1\nname \"Upstream\"\n",
    )
    .expect("the test repository is writable");
    std::fs::write(
        cache.join("components/podman/manifest.kdl"),
        "schema-version 1\ndescription \"Podman from cache\"\ntargets \"fedora\"\n",
    )
    .expect("the test repository is writable");
    let found = problems(&root);
    let why = why_of(&root, "podman");
    let _ = std::fs::remove_dir_all(root);
    assert_eq!(found, "");
    assert!(why.contains("Podman from cache"), "{why}");
    assert!(why.contains("read from library `stable`"), "{why}");
}

const REMOTE: &str = "https://example.invalid/library";

fn remote_repo(name: &str, tree: &str, files: &[(&str, &str)]) -> PathBuf {
    with_libraries(
        name,
        &format!("schema-version 1\nname \"Checked\"\nbuilds {{ checked }}\nlibraries {{\n    remote {{\n        stable {{ source \"{REMOTE}\"; version \"v1\" }}\n    }}\n}}\n"),
        &format!("component \"/\" {{\n{BASE}{tree}}}\n"),
        files,
    )
}

fn remote_at(root: &std::path::Path, selector: &str, mark: char) -> String {
    let commit = mark.to_string().repeat(40);
    let cache = buildc::collection::seed(root, REMOTE, selector, &commit)
        .expect("the source key is computed");
    std::fs::write(
        cache.join("repo.kdl"),
        "schema-version 1\nname \"Upstream\"\n",
    )
    .expect("the test repository is writable");
    std::fs::create_dir_all(cache.join("components/podman"))
        .expect("the test repository is writable");
    std::fs::write(
        cache.join("components/podman/manifest.kdl"),
        // Each commit gives podman its own description. `why` shows which commit the build read.
        format!("schema-version 1\ndescription \"Podman {mark}\"\ntargets \"fedora\"\n"),
    )
    .expect("the test repository is writable");
    commit
}

#[test]
fn with_no_local_block_the_repository_is_the_default_for_a_remote_referrer() {
    let root = with_libraries(
        "library-remote-default",
        "schema-version 1\nname \"Checked\"\nbuilds { checked }\nlibraries {\n    remote {\n        stable { source \"https://example.invalid/stable\" }\n        other { source \"https://example.invalid/other\" }\n    }\n}\n",
        &format!("component \"/\" {{\n{BASE}    podman\n}}\n"),
        &[("cuda", "description \"CUDA from here\"\ntargets \"fedora\"\n")],
    );
    for (url, path, manifest) in [
        (
            "https://example.invalid/stable",
            "podman",
            "description \"Podman\"\ntargets \"fedora\"\ncomponent \"/\" { cuda }\n",
        ),
        (
            "https://example.invalid/other",
            "cuda",
            "description \"CUDA from other\"\ntargets \"fedora\"\n",
        ),
    ] {
        let cache = buildc::collection::seed(&root, url, "HEAD", &"a".repeat(40))
            .expect("the source key is computed");
        std::fs::write(cache.join("repo.kdl"), "schema-version 1\n")
            .expect("the test repository is writable");
        std::fs::create_dir_all(cache.join("components").join(path))
            .expect("the test repository is writable");
        std::fs::write(
            cache.join("components").join(path).join("manifest.kdl"),
            format!("schema-version 1\n{manifest}"),
        )
        .expect("the test repository is writable");
    }
    let found = problems(&root);
    let why = why_of(&root, "cuda");
    let _ = std::fs::remove_dir_all(root);
    assert_eq!(found, "");
    assert!(why.contains("CUDA from here"), "{why}");
}

fn lock(root: &std::path::Path) -> bool {
    let run = buildc::run(Command::Generate, None, root);
    assert!(
        run.problems.issues.is_empty(),
        "{}",
        run.problems.issues.plain()
    );
    buildc::provenance::repo_lock::of(&run.reading.list.images)
        .write(root)
        .expect("the lock is writable")
}

fn generate(root: &std::path::Path) {
    buildc::dispatch::dispatch(
        buildc::command::Verb::Generate,
        "generate",
        &[],
        buildc::dispatch::Flags::default(),
        &common::prompt::Prompt::silent(),
        &buildc::command::Context::Repo(root.to_path_buf()),
    )
    .unwrap_or_else(|err| panic!("{}", err.message()));
}

fn locked_commit(root: &std::path::Path) -> Option<String> {
    buildc::provenance::repo_lock::Lock::read(root).builds["checked"]
        .iter()
        .find(|held| held.library == "stable" && held.path == "podman")
        .map(|held| held.commit.clone())
}

#[test]
fn a_locked_component_keeps_its_commit_until_generate_moves_it() {
    let root = remote_repo("lock-moves", "    podman\n", &[]);
    let first = remote_at(&root, "v1", 'a');
    assert!(lock(&root));
    assert!(!lock(&root));
    assert_eq!(locked_commit(&root), Some(first));

    let second = remote_at(&root, "v1", 'b');
    assert!(why_of(&root, "podman").contains("Podman a"));
    assert_eq!(problems(&root), "");

    generate(&root);
    let found = why_of(&root, "podman");
    let held = locked_commit(&root);
    let _ = std::fs::remove_dir_all(root);
    assert!(found.contains("Podman b"), "{found}");
    assert_eq!(held, Some(second));
}

#[test]
fn generate_keeps_a_pinned_component_at_its_pin() {
    let pin = "c".repeat(40);
    let root = remote_repo(
        "lock-fetch-pinned",
        &format!("    podman pin=\"{pin}\"\n"),
        &[],
    );
    remote_at(&root, &pin, 'c');
    remote_at(&root, "v1", 'a');
    assert!(lock(&root));
    remote_at(&root, "v1", 'b');
    let before = std::fs::read_to_string(root.join(buildc::provenance::repo_lock::FILE)).unwrap();
    generate(&root);
    let after = std::fs::read_to_string(root.join(buildc::provenance::repo_lock::FILE)).unwrap();
    let held = locked_commit(&root);
    let found = why_of(&root, "podman");
    let _ = std::fs::remove_dir_all(root);
    assert_eq!(
        before, after,
        "a pinned component leaves the lock as it was"
    );
    assert_eq!(held, Some(pin));
    assert!(found.contains("Podman c"), "{found}");
}

#[test]
fn a_cached_component_that_differs_from_its_lock_is_refused() {
    let root = remote_repo("lock-mismatch", "    podman\n", &[]);
    let commit = remote_at(&root, "v1", 'a');
    lock(&root);
    let cache = buildc::collection::cache_path(&root, REMOTE, &commit).expect("the key hashes");
    std::fs::write(cache.join("components/podman/extra.sh"), "true\n")
        .expect("the test repository is writable");
    let found = problems(&root);
    let _ = std::fs::remove_dir_all(root);
    assert!(
        found.contains("`checked` locks `podman` from `stable`"),
        "{found}"
    );
}

#[test]
fn a_pin_reads_its_own_commit_and_a_tag_reads_the_tag() {
    let pin = "c".repeat(40);
    let root = remote_repo("lock-pins", &format!("    podman pin=\"{pin}\"\n"), &[]);
    remote_at(&root, "v1", 'a');
    remote_at(&root, &pin, 'c');
    lock(&root);
    let by_commit = why_of(&root, "podman");
    let held = locked_commit(&root);
    std::fs::write(
        root.join("components/checked.kdl"),
        format!("schema-version 1\ncomponent \"/\" {{\n{BASE}    podman pin=\"v2\"\n}}\n"),
    )
    .expect("the test repository is writable");
    remote_at(&root, "refs/tags/v2", 'd');
    let by_tag = why_of(&root, "podman");
    let _ = std::fs::remove_dir_all(root);
    assert!(by_commit.contains("Podman c"), "{by_commit}");
    assert_eq!(held, Some(pin));
    assert!(by_tag.contains("Podman d"), "{by_tag}");
}

fn git(dir: &std::path::Path, args: &[&str]) {
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
        .expect("git runs");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn podman_commit(upstream: &std::path::Path, mark: &str) {
    std::fs::write(
        upstream.join("components/podman/manifest.kdl"),
        format!("schema-version 1\ndescription \"Podman {mark}\"\ntargets \"fedora\"\n"),
    )
    .expect("the test repository is writable");
    git(upstream, &["add", "."]);
    git(upstream, &["commit", "--quiet", "-m", mark]);
}

#[test]
fn check_names_a_tag_pin_that_moved_and_leaves_it_out_of_the_version() {
    let root = with_libraries(
        "lock-tag-remote",
        "",
        &format!("component \"/\" {{\n{BASE}    podman pin=\"v2\"\n}}\n"),
        &[],
    );
    let upstream = root.join("upstream");
    std::fs::create_dir_all(upstream.join("components/podman"))
        .expect("the test repository is writable");
    std::fs::write(
        upstream.join("repo.kdl"),
        "schema-version 1\nname \"Upstream\"\n",
    )
    .expect("the test repository is writable");
    // The upstream is a real Git repository. `moved` asks it where the branch and the tag point.
    git(&upstream, &["init", "--quiet", "--initial-branch=main"]);
    podman_commit(&upstream, "a");
    git(&upstream, &["tag", "v2"]);
    std::fs::write(
        root.join("repo.kdl"),
        "schema-version 1\nname \"Checked\"\nbuilds { checked }\nlibraries {\n    remote {\n        stable { source \"upstream\"; version \"main\" }\n    }\n}\n",
    )
    .expect("the test repository is writable");
    generate(&root);
    podman_commit(&upstream, "b");
    let version_moved = buildc::provenance::repo_lock::moved(
        &root,
        &buildc::run(Command::Check, None, &root).reading.list,
    );
    git(&upstream, &["tag", "-f", "v2"]);
    let check = buildc::run(Command::Check, None, &root);
    let tag_moved = buildc::provenance::repo_lock::moved(&root, &check.reading.list);
    let read = why_of(&root, "podman");
    for marker in
        std::fs::read_dir(root.join(buildc::layout::SOURCES_CACHE)).expect("the cache exists")
    {
        let marker = marker.expect("the cache is readable").path();
        // A library refresh runs once in each process. Removing the markers makes the next generate read the remote again.
        if marker.extension().is_some_and(|ext| ext == "run") {
            std::fs::remove_file(marker).expect("the cache is writable");
        }
    }
    generate(&root);
    let moved_on = why_of(&root, "podman");
    let _ = std::fs::remove_dir_all(root);
    assert!(version_moved.is_empty(), "{version_moved:?}");
    assert_eq!(tag_moved.len(), 1, "{tag_moved:?}");
    assert!(
        tag_moved[0].contains("for `pin=\"v2\"`, and the tag is at"),
        "{tag_moved:?}"
    );
    assert!(read.contains("Podman a"), "{read}");
    assert!(moved_on.contains("Podman b"), "{moved_on}");
}

#[test]
fn a_locked_tag_pin_keeps_its_commit_until_generate_moves_it() {
    let root = remote_repo("lock-tag-moves", "    podman pin=\"v2\"\n", &[]);
    remote_at(&root, "v1", 'a');
    remote_at(&root, "refs/tags/v2", 'd');
    assert!(lock(&root));
    let second = remote_at(&root, "refs/tags/v2", 'e');
    let before = why_of(&root, "podman");
    let clean = problems(&root);
    generate(&root);
    let after = why_of(&root, "podman");
    let held = locked_commit(&root);
    let _ = std::fs::remove_dir_all(root);
    assert!(before.contains("Podman d"), "{before}");
    assert_eq!(clean, "");
    assert!(after.contains("Podman e"), "{after}");
    assert_eq!(held, Some(second));
}

#[test]
fn a_pin_on_a_local_component_is_refused() {
    let root = with_libraries(
        "lock-local-pin",
        "schema-version 1\nname \"Checked\"\nbuilds { checked }\n",
        &format!("component \"/\" {{\n{BASE}    podman pin=\"v1\"\n}}\n"),
        &[("podman", "description \"Podman\"\ntargets \"fedora\"\n")],
    );
    let found = problems(&root);
    std::fs::remove_dir_all(root.join("components/podman"))
        .expect("the test repository is writable");
    std::fs::write(
        root.join("components/checked.kdl"),
        format!("schema-version 1\ncomponent \"/\" {{\n{BASE}    podman pin=\"v1\" {{\n        packages \"podman\"\n    }}\n}}\n"),
    )
    .expect("the test repository is writable");
    let inline = problems(&root);
    let _ = std::fs::remove_dir_all(root);
    assert!(found.contains("which the user edits in place"), "{found}");
    assert!(
        inline.contains("`checked` defines `podman` inline at `pin=\"v1\"`"),
        "{inline}"
    );
}

#[test]
fn the_pin_nearer_the_build_wins_and_sibling_pins_are_refused() {
    let near = "c".repeat(40);
    let far = "d".repeat(40);
    let apps = format!(
        "description \"Apps\"\ntargets \"fedora\"\ncomponent \"/\" {{ podman pin=\"{far}\" }}\n"
    );
    let root = remote_repo(
        "lock-nearer-pin",
        &format!("    apps\n    podman pin=\"{near}\"\n"),
        &[("apps", &apps)],
    );
    remote_at(&root, "v1", 'a');
    remote_at(&root, &near, 'c');
    remote_at(&root, &far, 'd');
    let nearer = why_of(&root, "podman");
    let clean = problems(&root);
    std::fs::write(
        root.join("components/checked.kdl"),
        format!("schema-version 1\ncomponent \"/\" {{\n{BASE}    apps\n    tools\n}}\n"),
    )
    .expect("the test repository is writable");
    let tools = format!(
        "description \"Tools\"\ntargets \"fedora\"\ncomponent \"/\" {{ podman pin=\"{near}\" }}\n"
    );
    std::fs::create_dir_all(root.join("components/tools"))
        .expect("the test repository is writable");
    std::fs::write(root.join("components/tools/manifest.kdl"), tools)
        .expect("the test repository is writable");
    let siblings = problems(&root);
    let _ = std::fs::remove_dir_all(root);
    assert!(nearer.contains("Podman c"), "{nearer}");
    assert_eq!(clean, "");
    assert!(
        siblings.contains("pin `podman` at different revisions"),
        "{siblings}"
    );
}

#[test]
fn check_names_a_remote_component_the_lock_does_not_record() {
    let root = remote_repo("lock-unlocked", "    podman\n", &[]);
    remote_at(&root, "v1", 'a');
    let before = buildc::run(Command::Check, None, &root).problems.unlocked;
    lock(&root);
    let after = buildc::run(Command::Check, None, &root).problems.unlocked;
    let _ = std::fs::remove_dir_all(root);
    assert_eq!(before.len(), 1, "{before:?}");
    assert!(
        before[0].contains("`checked` reads `podman` from `stable`"),
        "{before:?}"
    );
    assert!(after.is_empty(), "{after:?}");
}

#[test]
fn check_names_a_remote_component_whose_content_cannot_be_hashed() {
    let root = remote_repo("lock-unhashed", "    podman\n", &[]);
    let commit = remote_at(&root, "v1", 'a');
    let cache = buildc::collection::cache_path(&root, REMOTE, &commit).expect("the key hashes");
    std::os::unix::fs::symlink("/nonexistent", cache.join("components/podman/dangling"))
        .expect("the test repository is writable");
    let unlocked = buildc::run(Command::Check, None, &root).problems.unlocked;
    let _ = std::fs::remove_dir_all(root);
    assert!(
        unlocked
            .iter()
            .any(|line| line.contains("`podman`") && line.contains("cannot be hashed")),
        "{unlocked:?}"
    );
}

#[test]
fn a_rooted_reference_inside_a_library_prefers_that_library() {
    let root = with_libraries(
        "library-referrer",
        &two_libraries(" default=\"stable\""),
        &format!("component \"/\" {{\n{BASE}    podman library=\"tools\"\n}}\n"),
        &[
            (
                "tools/podman",
                "description \"Podman\"\ntargets \"fedora\"\ncomponent \"/\" { cuda }\n",
            ),
            (
                "tools/cuda",
                "description \"CUDA from tools\"\ntargets \"fedora\"\n",
            ),
            (
                "stable/cuda",
                "description \"CUDA from stable\"\ntargets \"fedora\"\n",
            ),
        ],
    );
    let found = problems(&root);
    let why = why_of(&root, "cuda");
    let _ = std::fs::remove_dir_all(root);
    assert_eq!(found, "");
    assert!(why.contains("CUDA from tools"), "{why}");
    assert!(!why.contains("CUDA from stable"), "{why}");
}

#[test]
fn a_seed_names_a_base_from_its_library() {
    let root = with_libraries(
        "library-seed-base",
        "schema-version 1\nname \"Checked\"\nbuilds { checked }\nseed \"checked\" collection=\"buildc-org\"\nlibraries default=\"buildc-org\" {\n    local {\n        buildc-org { source \".\" }\n        upstream { source \"components/upstream\" }\n    }\n}\n",
        "component \"/\" { fedora; tool }\n",
        &[
            (
                "upstream/fedora",
                "base \"quay.io/fedora/fedora-bootc:44\" { family \"fedora\"; }\n",
            ),
            (
                "upstream/tool",
                "description \"Tool\"\ntargets \"fedora\"\n",
            ),
        ],
    );
    let run = buildc::run(Command::Generate, None, &root);
    let seed = run
        .files
        .iter()
        .find(|(path, _)| path == std::path::Path::new("generated/seed.kdl"))
        .map(|(_, text)| text.clone())
        .unwrap_or_default();
    let _ = std::fs::remove_dir_all(root);
    assert!(
        run.problems.issues.is_empty(),
        "{}",
        run.problems.issues.plain()
    );
    assert!(seed.contains("base \"upstream/fedora\""), "{seed}");
    assert!(seed.contains("component \"upstream/tool\""), "{seed}");
    assert!(
        seed.contains("upstream { source \"components/upstream\" }"),
        "{seed}"
    );
}

#[test]
fn a_library_component_runs_its_finalize_hook() {
    let root = with_libraries(
        "library-finalize",
        &tools_repo(),
        &format!("component \"/\" {{\n{BASE}    podman\n}}\n"),
        &[(
            "tools/podman",
            "description \"Podman\"\ntargets \"fedora\"\n",
        )],
    );
    std::fs::write(
        root.join("components/tools/podman/finalize.sh"),
        "echo hook\n",
    )
    .expect("the test repository is writable");
    let run = buildc::run(Command::Generate, None, &root);
    let finalize = run
        .files
        .iter()
        .find(|(path, _)| path == std::path::Path::new("generated/checked/finalize.sh"))
        .map(|(_, text)| text.clone())
        .unwrap_or_default();
    let copied = run
        .copies
        .iter()
        .any(|copy| copy.path.ends_with("components/podman/finalize.sh"));
    let _ = std::fs::remove_dir_all(root);
    assert!(
        run.problems.issues.is_empty(),
        "{}",
        run.problems.issues.plain()
    );
    assert!(
        finalize.contains("source /ctx/components/podman/finalize.sh"),
        "{finalize}"
    );
    assert!(copied, "the hook reaches the generated output");
}

#[test]
fn a_root_image_file_reads_its_bare_children_from_the_library_root() {
    let repo = tools_repo().replace("builds { checked }", "builds { checked \"./checked\" }");
    let root = with_libraries(
        "root-image-file",
        &repo,
        &format!("{BASE}podman\n"),
        &[(
            "tools/podman",
            "description \"Podman from tools\"\ntargets \"fedora\"\n",
        )],
    );
    std::fs::rename(
        root.join("components/checked.kdl"),
        root.join("checked.kdl"),
    )
    .expect("the test repository is writable");
    std::fs::create_dir_all(root.join("checked/files/etc"))
        .expect("the test repository is writable");
    std::fs::write(root.join("checked/files/etc/motd"), "hello\n")
        .expect("the test repository is writable");
    let why = why_of(&root, "podman");
    let run = buildc::run(Command::Generate, None, &root);
    let _ = std::fs::remove_dir_all(&root);
    assert!(
        run.problems.issues.is_empty(),
        "{}",
        run.problems.issues.plain()
    );
    assert!(why.contains("tools"), "{why}");
    let containerfile = files_named(&run, "checked/Containerfile");
    assert!(
        containerfile.contains("COPY --from=ctx /generated/checked/components/checked/files/ /"),
        "{containerfile}"
    );
}

#[test]
fn a_root_image_file_that_git_ignores_is_refused() {
    let repo = tools_repo().replace("builds { checked }", "builds { checked \"./checked\" }");
    let root = with_libraries("root-image-ignored", &repo, BASE, &[]);
    std::fs::rename(
        root.join("components/checked.kdl"),
        root.join("checked.kdl"),
    )
    .expect("the test repository is writable");
    std::fs::write(root.join(".gitignore"), "checked.kdl\n")
        .expect("the test repository is writable");
    let initialised = std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(&root)
        .status()
        .is_ok_and(|status| status.success());
    let found = problems(&root);
    let _ = std::fs::remove_dir_all(root);
    assert!(initialised);
    assert!(
        found.contains("names `./checked`, which git ignores"),
        "{found}"
    );
}

fn copied(root: &std::path::Path, paths: &[&str], from: Option<&str>) -> Result<String, String> {
    copied_for(root, paths, from, None)
}

fn copied_for(
    root: &std::path::Path,
    paths: &[&str],
    from: Option<&str>,
    build: Option<&str>,
) -> Result<String, String> {
    let request = buildc::copy_component::Request {
        paths: paths.iter().map(ToString::to_string).collect(),
        from: from.map(str::to_string),
        build: build.map(str::to_string),
    };
    let mut shown = String::new();
    let copy = buildc::copy_component::Copy::collect(
        &request,
        root,
        &common::prompt::Prompt::silent(),
        &mut |text: &str| shown.push_str(text),
    )
    .map_err(|err| err.message().to_string())?
    .expect("a silent prompt confirms");
    copy.apply(root)?;
    Ok(shown)
}

#[test]
fn a_copied_component_records_its_commit_and_the_build_reads_the_copy() {
    let root = remote_repo("copy-plain", "    podman\n", &[]);
    let commit = remote_at(&root, "v1", 'a');
    let shown = copied(&root, &["stable/podman"], None).expect("the copy is made");
    let again = copied(&root, &["stable/podman"], None).expect_err("the path is taken");
    let found = problems(&root);
    let why = why_of(&root, "podman");
    let recorded = buildc::provenance::origins::read(&root);
    let _ = std::fs::remove_dir_all(root);
    assert!(
        shown.contains("stable/podman > ./components/podman\n"),
        "{shown}"
    );
    assert!(!shown.contains("references to update"), "{shown}");
    assert!(
        again.starts_with("`components/podman` already exists"),
        "{again}"
    );
    assert_eq!(found, "");
    assert!(why.contains("Podman a"), "{why}");
    assert!(!why.contains("read from library"), "{why}");
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].path, "podman");
    assert_eq!(recorded[0].pin.as_deref(), Some(commit.as_str()));
}

#[test]
fn a_copy_into_a_local_library_records_an_origin_that_why_and_check_read() {
    let root = with_libraries(
        "copy-into-mine",
        &format!("schema-version 1\nname \"Checked\"\nbuilds {{ checked }}\nlibraries default=\"mine\" {{\n    local {{\n        mine {{ source \"mine\" }}\n    }}\n    remote {{\n        stable {{ source \"{REMOTE}\"; version \"v1\" }}\n    }}\n}}\n"),
        &format!("component \"/\" {{\n{BASE}    podman\n}}\n"),
        &[],
    );
    std::fs::create_dir_all(root.join("mine")).expect("the test repository is writable");
    // The base moves into mine because a local block that leaves out the root leaves the repository root unread.
    std::fs::rename(
        root.join("components/base-image"),
        root.join("mine/base-image"),
    )
    .expect("the test repository is writable");
    remote_at(&root, "v1", 'a');
    copied(&root, &["stable/podman"], None).expect("the copy is made");
    let why = why_of(&root, "podman");
    let clean = problems(&root);
    std::fs::write(root.join("mine/origins.kdl"), "origin {\n")
        .expect("the test repository is writable");
    let broken = problems(&root);
    let _ = std::fs::remove_dir_all(root);
    assert!(
        why.contains("It is the copy in library `mine` of `podman`"),
        "{why}"
    );
    assert_eq!(clean, "");
    assert!(broken.contains("mine/origins.kdl"), "{broken}");
}

#[test]
fn a_copy_through_a_linked_components_folder_is_refused() {
    let root = remote_repo("copy-linked", "", &[]);
    let commit = remote_at(&root, "v1", 'a');
    let cache = buildc::collection::cache_path(&root, REMOTE, &commit).expect("the key hashes");
    let host = root.join("host");
    std::fs::create_dir_all(host.join("podman")).expect("the test repository is writable");
    std::fs::write(host.join("podman/manifest.kdl"), "schema-version 1\n")
        .expect("the test repository is writable");
    std::fs::remove_dir_all(cache.join("components")).expect("the cache is writable");
    std::os::unix::fs::symlink(&host, cache.join("components")).expect("the cache is writable");
    let refused = copied(&root, &["stable/podman"], None).expect_err("the copy is refused");
    let absent = !root.join("components/podman").exists();
    let _ = std::fs::remove_dir_all(root);
    assert!(refused.contains("outside its library"), "{refused}");
    assert!(absent);
}

#[test]
fn a_pinned_reference_is_copied_at_its_pin_and_loses_its_library_and_pin() {
    let root = remote_repo(
        "copy-pinned",
        "    podman library=\"stable\" pin=\"v2\"\n",
        &[],
    );
    remote_at(&root, "v1", 'a');
    // The library version reads commit a and the pin reads commit b. The copy then shows which one it took.
    remote_at(&root, "refs/tags/v2", 'b');
    let shown = copied(&root, &["podman"], Some("stable")).expect("the copy is made");
    let image = std::fs::read_to_string(root.join("components/checked.kdl")).unwrap();
    let found = problems(&root);
    let why = why_of(&root, "podman");
    let _ = std::fs::remove_dir_all(root);
    assert!(
        shown.ends_with("Component references to update:\ncomponents/checked.kdl\n\n"),
        "{shown}"
    );
    assert!(image.contains("    podman\n"), "{image}");
    assert_eq!(found, "");
    assert!(why.contains("Podman b"), "{why}");
}

#[test]
fn a_library_from_an_enclosing_node_is_refused_and_nothing_is_copied() {
    let root = remote_repo(
        "copy-enclosed",
        "    component \"/\" library=\"stable\" {\n        podman\n    }\n",
        &[],
    );
    remote_at(&root, "v1", 'a');
    let refused = copied(&root, &["stable/podman"], None).expect_err("the copy is refused");
    let absent = !root.join("components/podman").exists();
    let _ = std::fs::remove_dir_all(root);
    assert!(
        refused
            .starts_with("components/checked.kdl:4 sets `library=\"stable\"` on `component \"/\"`"),
        "{refused}"
    );
    assert!(absent);
}

#[test]
fn a_copy_brings_its_nested_components_and_refuses_an_inline_one_alone() {
    let root = remote_repo("copy-nested", "    podman\n", &[]);
    let commit = remote_at(&root, "v1", 'a');
    let cache = buildc::collection::seed(&root, REMOTE, "v1", &commit).unwrap();
    std::fs::write(
        cache.join("components/podman/manifest.kdl"),
        "schema-version 1\ndescription \"Podman a\"\ntargets \"fedora\"\ntools { description \"Tools\" }\n",
    )
    .unwrap();
    std::fs::create_dir_all(cache.join("components/podman/extra")).unwrap();
    std::fs::write(
        cache.join("components/podman/extra/manifest.kdl"),
        "schema-version 1\ndescription \"Extra\"\ntargets \"fedora\"\n",
    )
    .unwrap();
    let refused =
        copied(&root, &["stable/podman/tools"], None).expect_err("an inline copy is refused");
    let shown =
        copied(&root, &["stable/podman/extra", "stable/podman"], None).expect("the copy is made");
    let found = problems(&root);
    let recorded: Vec<String> = buildc::provenance::origins::read(&root)
        .into_iter()
        .map(|copy| copy.path)
        .collect();
    let _ = std::fs::remove_dir_all(root);
    assert!(refused.contains("defined inline in `podman`"), "{refused}");
    assert_eq!(shown.matches("stable/podman/extra >").count(), 1, "{shown}");
    assert_eq!(found, "");
    assert_eq!(recorded, ["podman", "podman/extra"]);
}

#[test]
fn a_path_that_a_remote_component_also_lists_is_refused() {
    let root = remote_repo(
        "copy-two-libraries",
        "    podman library=\"stable\"\n    runner\n",
        &[],
    );
    let commit = remote_at(&root, "v1", 'a');
    let cache = buildc::collection::seed(&root, REMOTE, "v1", &commit).unwrap();
    std::fs::create_dir_all(cache.join("components/runner")).unwrap();
    std::fs::write(
        cache.join("components/runner/manifest.kdl"),
        "schema-version 1\ndescription \"Runner\"\ntargets \"fedora\"\ncomponent \"/\" { podman }\n",
    )
    .unwrap();
    let refused = copied(&root, &["stable/podman"], None).expect_err("the copy is refused");
    let shown = copied(&root, &["stable/podman", "stable/runner"], None).expect("both are copied");
    let image = std::fs::read_to_string(root.join("components/checked.kdl")).unwrap();
    let found = problems(&root);
    let _ = std::fs::remove_dir_all(root);
    assert!(
        refused.contains("`runner` lists `podman` and stays in `stable`"),
        "{refused}"
    );
    assert!(
        shown.contains("stable/runner > ./components/runner\n"),
        "{shown}"
    );
    assert!(image.contains("    podman\n"), "{image}");
    assert_eq!(found, "");
}

#[test]
fn a_build_copies_every_remote_component_it_reads_and_no_path_needs_a_choice() {
    let root = remote_repo("copy-build", "    podman\n", &[]);
    let commit = remote_at(&root, "v1", 'a');
    let cache = buildc::collection::seed(&root, REMOTE, "v1", &commit).unwrap();
    std::fs::write(
        cache.join("components/podman/manifest.kdl"),
        "schema-version 1\ndescription \"Podman a\"\ntargets \"fedora\"\ntools { description \"Tools\" }\n",
    )
    .unwrap();
    let unchosen = copied(&root, &[], None).expect_err("a silent prompt chooses nothing");
    let unnamed = copied_for(&root, &[], None, Some("other")).expect_err("no such build");
    let shown = copied_for(&root, &[], None, Some("checked")).expect("the copy is made");
    let found = problems(&root);
    let _ = std::fs::remove_dir_all(root);
    assert!(unchosen.starts_with("no component chosen"), "{unchosen}");
    assert!(unnamed.contains("no build `other`"), "{unnamed}");
    assert_eq!(
        shown,
        "Components to be copied:\nstable/podman > ./components/podman\n\n"
    );
    assert_eq!(found, "");
}

#[test]
fn two_builds_at_different_commits_need_the_build_named() {
    let root = remote_repo("copy-two-builds", "    podman\n", &[]);
    std::fs::write(
        root.join("repo.kdl"),
        format!("schema-version 1\nname \"Checked\"\nbuilds {{ checked; other }}\nlibraries {{\n    remote {{\n        stable {{ source \"{REMOTE}\"; version \"v1\" }}\n    }}\n}}\n"),
    )
    .unwrap();
    std::fs::write(
        root.join("components/other.kdl"),
        format!("schema-version 1\ncomponent \"/\" {{\n{BASE}    podman pin=\"v2\"\n}}\n"),
    )
    .unwrap();
    remote_at(&root, "v1", 'a');
    remote_at(&root, "refs/tags/v2", 'b');
    let refused = copied(&root, &["stable/podman"], None).expect_err("two commits are refused");
    copied_for(&root, &["stable/podman"], None, Some("other")).expect("the copy is made");
    let found = problems(&root);
    let why = why_of(&root, "podman");
    let _ = std::fs::remove_dir_all(root);
    assert!(
        refused.contains("read `podman` at different commits"),
        "{refused}"
    );
    assert!(refused.contains("`--build <name>`"), "{refused}");
    assert_eq!(found, "");
    assert!(why.contains("Podman b"), "{why}");
}

#[test]
fn a_copy_that_leaves_a_build_unloadable_is_undone() {
    let root = remote_repo(
        "copy-undone",
        "    podman library=\"stable\"\n    runner\n",
        &[("cuda", "description \"Local CUDA\"\ntargets \"fedora\"\n")],
    );
    let commit = remote_at(&root, "v1", 'a');
    let cache = buildc::collection::seed(&root, REMOTE, "v1", &commit).unwrap();
    // The copy of podman reads /cuda from this repository. Runner still reads it from stable.
    for (path, manifest) in [
        (
            "podman",
            "description \"Podman a\"\ntargets \"fedora\"\ncomponent \"/\" { cuda }\n",
        ),
        (
            "runner",
            "description \"Runner\"\ntargets \"fedora\"\ncomponent \"/\" { cuda }\n",
        ),
        ("cuda", "description \"CUDA\"\ntargets \"fedora\"\n"),
    ] {
        std::fs::create_dir_all(cache.join("components").join(path)).unwrap();
        std::fs::write(
            cache.join("components").join(path).join("manifest.kdl"),
            format!("schema-version 1\n{manifest}"),
        )
        .unwrap();
    }
    let image = std::fs::read_to_string(root.join("components/checked.kdl")).unwrap();
    assert_eq!(problems(&root), "");
    let undone = copied(&root, &["stable/podman"], None).expect_err("the copy is undone");
    let kept = std::fs::read_to_string(root.join("components/checked.kdl")).unwrap();
    let gone =
        !root.join("components/podman").exists() && !root.join("components/origins.kdl").exists();
    let _ = std::fs::remove_dir_all(root);
    assert!(undone.starts_with("the copy is undone"), "{undone}");
    assert_eq!(kept, image);
    assert!(gone);
}

#[test]
fn a_component_that_links_outside_its_library_is_not_copied() {
    let root = remote_repo("copy-linked-out", "    podman\n", &[]);
    let commit = remote_at(&root, "v1", 'a');
    let cache = buildc::collection::seed(&root, REMOTE, "v1", &commit).unwrap();
    let outside = root.join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(
        outside.join("manifest.kdl"),
        "schema-version 1\ndescription \"Host\"\n",
    )
    .unwrap();
    std::os::unix::fs::symlink(&outside, cache.join("components/host")).unwrap();
    let refused = copied(&root, &["stable/host"], None).expect_err("a link is refused");
    let absent = !root.join("components/host").exists();
    let _ = std::fs::remove_dir_all(root);
    assert!(refused.contains("outside its library"), "{refused}");
    assert!(absent);
}

// The build reads a component's overlay and family folders by name. A link at either one would bring host files into the image.
fn linked_check(name: &str, link: &str) -> String {
    let root = remote_repo(name, "    podman\n", &[]);
    remote_at(&root, "v1", 'a');
    let cache = buildc::collection::seed(&root, REMOTE, "v1", &"a".repeat(40))
        .expect("the source key is computed");
    let outside = root.join("outside");
    std::fs::create_dir_all(&outside).expect("the test repository is writable");
    std::os::unix::fs::symlink(&outside, cache.join("components/podman").join(link))
        .expect("the test repository is writable");
    let found = problems(&root);
    let _ = std::fs::remove_dir_all(root);
    found
}

#[test]
fn a_component_whose_overlay_links_outside_its_library_is_refused() {
    let found = linked_check("check-linked-overlay", "files");
    assert!(
        found.contains("`podman` in `stable` holds a link to"),
        "{found}"
    );
}

#[test]
fn a_component_whose_family_folder_links_outside_its_library_is_refused() {
    let found = linked_check("check-linked-family", "fedora");
    assert!(found.contains("holds a link to"), "{found}");
}

#[test]
fn a_component_that_links_through_its_own_library_is_refused() {
    let root = remote_repo("check-linked-within", "    podman\n", &[]);
    remote_at(&root, "v1", 'a');
    let cache = buildc::collection::seed(&root, REMOTE, "v1", &"a".repeat(40)).unwrap();
    let shared = cache.join("shared");
    std::fs::create_dir_all(&shared).unwrap();
    std::fs::create_dir_all(root.join("outside")).unwrap();
    std::os::unix::fs::symlink(root.join("outside"), shared.join("lib")).unwrap();
    std::os::unix::fs::symlink(&shared, cache.join("components/podman/fedora")).unwrap();
    let found = problems(&root);
    let _ = std::fs::remove_dir_all(root);
    assert!(found.contains("holds a link to"), "{found}");
}

#[test]
fn a_link_inside_an_overlay_reaches_the_image_unrefused() {
    let root = remote_repo("check-overlay-link", "    podman\n", &[]);
    remote_at(&root, "v1", 'a');
    let cache = buildc::collection::seed(&root, REMOTE, "v1", &"a".repeat(40)).unwrap();
    let systemd = cache.join("components/podman/files/etc/systemd/system");
    std::fs::create_dir_all(&systemd).unwrap();
    // systemd masks a unit through a link to /dev/null and an overlay ships such a link on purpose.
    std::os::unix::fs::symlink("/dev/null", systemd.join("example.service")).unwrap();
    let found = problems(&root);
    let _ = std::fs::remove_dir_all(root);
    assert_eq!(found, "");
}

#[test]
fn a_link_inside_a_manifestless_child_overlay_is_unrefused() {
    let root = remote_repo("check-child-overlay-link", "    podman\n", &[]);
    remote_at(&root, "v1", 'a');
    let cache = buildc::collection::seed(&root, REMOTE, "v1", &"a".repeat(40)).unwrap();
    let systemd = cache.join("components/podman/extra/files/etc/systemd/system");
    std::fs::create_dir_all(&systemd).unwrap();
    std::os::unix::fs::symlink("/dev/null", systemd.join("example.service")).unwrap();
    let found = problems(&root);
    let _ = std::fs::remove_dir_all(root);
    assert_eq!(found, "");
}

// The build resolves each collected name below the component folder. A climb leaves the component.
fn podman_declares(name: &str, line: &str, link: Option<&str>) -> String {
    let root = remote_repo(name, "    podman\n", &[]);
    remote_at(&root, "v1", 'a');
    let cache = buildc::collection::seed(&root, REMOTE, "v1", &"a".repeat(40))
        .expect("the source key is computed");
    let podman = cache.join("components/podman");
    std::fs::write(
        podman.join("manifest.kdl"),
        format!("schema-version 1\ndescription \"Podman\"\ntargets \"fedora\"\n{line}\n"),
    )
    .expect("the test repository is writable");
    if let Some(link) = link {
        let outside = root.join("outside");
        std::fs::create_dir_all(&outside).expect("the test repository is writable");
        std::fs::write(outside.join("x.sh"), "true\n").expect("the test repository is writable");
        let at = podman.join(link);
        std::fs::create_dir_all(at.parent().expect("the link has a folder"))
            .expect("the test repository is writable");
        std::os::unix::fs::symlink(&outside, at).expect("the test repository is writable");
    }
    let found = problems(&root);
    let _ = std::fs::remove_dir_all(root);
    found
}

#[test]
fn a_collected_name_that_climbs_out_of_its_component_is_refused() {
    let found = podman_declares(
        "check-collects-climbs",
        "collects \"../../../../outside/x.sh\" into=\"/usr/share/x\" priority=500",
        None,
    );
    assert!(found.contains("is not a filename"), "{found}");
}

#[test]
fn a_collected_name_that_names_its_component_folder_is_refused() {
    let found = podman_declares(
        "check-collects-dot",
        "collects \".\" into=\"/usr/share/x\" priority=500",
        None,
    );
    assert!(found.contains("`.` is not a filename"), "{found}");
}

#[test]
fn a_helper_reached_through_a_link_is_refused() {
    // The walk skips an overlay and a helper names a path that can run through one.
    let found = podman_declares(
        "check-helper-link",
        "helpers \"lib/files/sub/x.sh\"",
        Some("lib/files/sub"),
    );
    assert!(
        found.contains("`podman` in `stable` holds a link to"),
        "{found}"
    );
}

#[test]
fn a_file_component_that_links_outside_its_library_is_refused() {
    let root = remote_repo("check-linked-file", "    podman\n", &[]);
    remote_at(&root, "v1", 'a');
    let cache = buildc::collection::seed(&root, REMOTE, "v1", &"a".repeat(40)).unwrap();
    let outside = root.join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::rename(
        cache.join("components/podman/manifest.kdl"),
        outside.join("podman.kdl"),
    )
    .unwrap();
    std::fs::remove_dir(cache.join("components/podman")).unwrap();
    std::os::unix::fs::symlink(
        outside.join("podman.kdl"),
        cache.join("components/podman.kdl"),
    )
    .unwrap();
    let found = problems(&root);
    let _ = std::fs::remove_dir_all(root);
    assert!(
        found.contains("`podman` in `stable` resolves to"),
        "{found}"
    );
    assert!(found.contains("outside its library"), "{found}");
}

#[test]
fn a_link_to_a_shared_folder_of_its_library_is_refused() {
    let root = remote_repo("check-shared-link", "    podman\n", &[]);
    remote_at(&root, "v1", 'a');
    let cache = buildc::collection::seed(&root, REMOTE, "v1", &"a".repeat(40)).unwrap();
    // A component holds its own files. A shared folder outside it cannot change them.
    std::fs::create_dir_all(cache.join("shared")).unwrap();
    std::fs::write(cache.join("shared/x.sh"), "true\n").unwrap();
    std::fs::write(
        cache.join("components/podman/manifest.kdl"),
        "schema-version 1\ndescription \"Podman\"\ntargets \"fedora\"\nhelpers \"lib/x.sh\"\n",
    )
    .unwrap();
    std::os::unix::fs::symlink(cache.join("shared"), cache.join("components/podman/lib")).unwrap();
    let found = problems(&root);
    let _ = std::fs::remove_dir_all(root);
    assert!(found.contains("holds a link to"), "{found}");
}

#[test]
fn a_link_that_leads_back_into_its_component_is_refused() {
    let root = remote_repo("check-looping-link", "    podman\n", &[]);
    remote_at(&root, "v1", 'a');
    let cache = buildc::collection::seed(&root, REMOTE, "v1", &"a".repeat(40)).unwrap();
    std::os::unix::fs::symlink(".", cache.join("components/podman/again")).unwrap();
    let found = problems(&root);
    let _ = std::fs::remove_dir_all(root);
    assert!(
        found.contains("holds a link that leads back into itself"),
        "{found}"
    );
}

#[test]
fn a_link_inside_its_own_component_is_followed_and_unrefused() {
    let root = remote_repo("check-own-link", "    podman\n", &[]);
    remote_at(&root, "v1", 'a');
    let cache = buildc::collection::seed(&root, REMOTE, "v1", &"a".repeat(40)).unwrap();
    let podman = cache.join("components/podman");
    std::fs::create_dir_all(podman.join("lib")).unwrap();
    std::fs::write(podman.join("lib/x.sh"), "true\n").unwrap();
    std::fs::write(
        podman.join("manifest.kdl"),
        "schema-version 1\ndescription \"Podman\"\ntargets \"fedora\"\nhelpers \"tools/x.sh\"\n",
    )
    .unwrap();
    std::os::unix::fs::symlink("lib", podman.join("tools")).unwrap();
    let found = problems(&root);
    let _ = std::fs::remove_dir_all(root);
    assert_eq!(found, "");
}

#[test]
fn a_link_into_a_component_it_holds_is_refused() {
    let root = remote_repo("check-child-link", "    podman\n", &[]);
    remote_at(&root, "v1", 'a');
    let cache = buildc::collection::seed(&root, REMOTE, "v1", &"a".repeat(40)).unwrap();
    let podman = cache.join("components/podman");
    std::fs::create_dir_all(podman.join("compose")).unwrap();
    std::fs::write(
        podman.join("compose/manifest.kdl"),
        "schema-version 1\ndescription \"Compose\"\n",
    )
    .unwrap();
    std::os::unix::fs::symlink("compose", podman.join("lib")).unwrap();
    let found = problems(&root);
    let _ = std::fs::remove_dir_all(root);
    assert!(found.contains("holds a link to"), "{found}");
}

#[test]
fn a_component_folder_that_aliases_another_component_is_refused() {
    let root = remote_repo("check-aliased", "    podman\n", &[]);
    remote_at(&root, "v1", 'a');
    let cache = buildc::collection::seed(&root, REMOTE, "v1", &"a".repeat(40)).unwrap();
    let components = cache.join("components");
    std::fs::rename(components.join("podman"), components.join("docker")).unwrap();
    std::os::unix::fs::symlink("docker", components.join("podman")).unwrap();
    let found = problems(&root);
    let _ = std::fs::remove_dir_all(root);
    assert!(found.contains("the path of another component"), "{found}");
}
