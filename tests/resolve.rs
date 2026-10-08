use std::path::PathBuf;
use buildc::Command;

#[test]
fn after_rejects_a_provider_on_another_target() {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("after-gated-provider");
    let _ = std::fs::remove_dir_all(&root);
    for module in ["consumer", "provider", "coinstalled", "separate"] {
        std::fs::create_dir_all(root.join("modules").join(module)).unwrap();
    }
    std::fs::write(root.join("repo.kdl"), "schema-version 1\nname \"After\"\n").unwrap();
    std::fs::write(
        root.join("image.kdl"),
        r#"schema-version 1

image {
    name "After"
    base "example" { family "fedora" }
    flavours {
        dev
        separate
    }
    modules {
        module "consumer"
        flavour "dev" {
            module "provider"
            module "coinstalled"
        }
        flavour "separate" {
            module "separate"
        }
    }
}
"#,
    )
    .unwrap();
    std::fs::write(
        root.join("modules/consumer/module.kdl"),
        "schema-version 1\n\ndescription \"Consumes everywhere\"\nsupports \"fedora\"\nafter \"tool\"\n",
    )
    .unwrap();
    std::fs::write(
        root.join("modules/provider/module.kdl"),
        "schema-version 1\n\ndescription \"Provides tools\"\nsupports \"fedora\"\nprovides \"tool\"\n",
    )
    .unwrap();
    std::fs::write(
        root.join("modules/coinstalled/module.kdl"),
        "schema-version 1\n\ndescription \"Consumes with provider\"\nsupports \"fedora\"\nafter \"tool\"\n",
    )
    .unwrap();
    std::fs::write(
        root.join("modules/separate/module.kdl"),
        "schema-version 1\n\ndescription \"Consumes elsewhere\"\nsupports \"fedora\"\nafter \"tool\"\n",
    )
    .unwrap();

    let issues = buildc::run(Command::Check, None, &root).issues.plain();
    assert!(
        issues.contains("`consumer` builds after `tool`, which only `provider` provides"),
        "{issues}"
    );
    assert!(
        issues.contains("`separate` builds after `tool`, which only `provider` provides"),
        "{issues}"
    );
    assert!(!issues.contains("`coinstalled` builds after"), "{issues}");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn shipped_policy_orders_a_module_after_the_mac_that_installs_it() {
    std::env::set_var(
        "BUILDC_ASSETS",
        concat!(env!("CARGO_MANIFEST_DIR"), "/assets"),
    );
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("policy-ordering");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("modules/apparmor")).unwrap();
    std::fs::create_dir_all(root.join("modules/yubikey/apparmor")).unwrap();
    std::fs::write(root.join("repo.kdl"), "schema-version 1\nname \"Mac\"\n").unwrap();
    std::fs::write(
        root.join("image.kdl"),
        r#"schema-version 1

image {
    name "Mac"
    base "example" { family "debian" }
    modules {
        module "yubikey"
        module "apparmor"
    }
}
"#,
    )
    .unwrap();
    std::fs::write(
        root.join("modules/apparmor/module.kdl"),
        "schema-version 1\n\ndescription \"The MAC\"\nsupports \"debian\"\nprovides \"apparmor-policy\"\n",
    )
    .unwrap();
    std::fs::write(
        root.join("modules/yubikey/module.kdl"),
        "schema-version 1\n\ndescription \"Ships a profile and requires nothing\"\nsupports \"debian\"\n",
    )
    .unwrap();
    std::fs::write(root.join("modules/yubikey/apparmor/usr.sbin.pcscd"), "").unwrap();

    let plan = buildc::run(Command::Plan, None, &root).stdout;
    let at = |name: &str| plan.find(&format!("\"path\": \"{name}\"")).unwrap();
    assert!(at("apparmor") < at("yubikey"), "{plan}");
    assert_eq!(buildc::run(Command::Check, None, &root).issues.plain(), "");

    std::fs::write(
        root.join("image.kdl"),
        std::fs::read_to_string(root.join("image.kdl"))
            .unwrap()
            .replace("        module \"apparmor\"\n", ""),
    )
    .unwrap();
    assert_eq!(buildc::run(Command::Check, None, &root).issues.plain(), "");
    let _ = std::fs::remove_dir_all(root);
}
