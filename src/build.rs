use crate::layout;
use crate::model::image::Backend;
use crate::planned::Planned;
use crate::provenance::build as record;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt as _;
use std::path::Path;
use std::process::Command;

#[derive(Default)]
pub struct Options {
    pub target: Option<String>,
    pub kernel: Option<String>,
    pub tags: Vec<String>,
    pub secrets: Vec<String>,
    pub backend: Option<String>,
    pub oci_output: Option<String>,
    pub no_cache_from: bool,
    pub cache_to: bool,
}

pub enum Stopped {
    Repository,
    Built,
}

struct Prepared {
    backend: Backend,
    containerfile: std::path::PathBuf,
    build_args: Vec<String>,
    validate: Vec<String>,
    family: String,
    target: String,
    version: String,
    tags: Vec<String>,
    labels: Vec<String>,
    import: Vec<String>,
    export: Option<String>,
    uki: bool,
}

pub fn run(root: &Path, opts: &Options) -> Result<Stopped, String> {
    let Some(prepared) = prepare(root, opts)? else {
        return Ok(Stopped::Repository);
    };
    let secrets = secrets(opts)?;
    let Prepared {
        backend,
        containerfile,
        build_args,
        validate,
        family,
        target,
        version,
        tags,
        labels,
        import,
        export,
        uki,
    } = prepared;

    install(root)?;

    eprintln!(
        "buildc: {} target={target} version={version}{}",
        backend.name(),
        match &opts.kernel {
            Some(kernel) => format!(" kernel={kernel}"),
            None => String::new(),
        }
    );
    eprintln!("buildc: tags {}", tags.join(" "));
    if !import.is_empty() {
        eprintln!("buildc: importing cache from {}", import.join(" "));
    }
    if let Some(export) = &export {
        eprintln!("buildc: exporting cache to {export}");
    }

    let program = match backend {
        Backend::Buildx => "docker",
        Backend::Buildah => "podman",
    };
    let prefix = build_prefix(backend, &containerfile, &build_args, &secrets);

    if backend == Backend::Buildah {
        if export.is_some() {
            return Err("buildah cannot export a BuildKit layer cache".into());
        }
        if opts.oci_output.is_some() {
            return Err("the buildah backend cannot write an OCI archive".into());
        }
        if !import.is_empty() {
            eprintln!("buildc: buildah ignores the registry layer cache");
        }
    }

    if uki {
        if backend == Backend::Buildx {
            return Err(
                "`--backend buildx` cannot build a UKI target: the composefs digest is read \
                 from the local buildah store, and buildx has none"
                    .into(),
            );
        }
        build_uki(
            root,
            &Invocation {
                program,
                containerfile: &containerfile,
                build_args: &build_args,
                validate: &validate,
                family: &family,
                target: &target,
                tags: &tags,
                labels: &labels,
            },
            &secrets,
        )?;
        return Ok(Stopped::Built);
    }

    let mut args = prefix;
    for tag in &tags {
        args.extend(["--tag".to_string(), tag.clone()]);
    }
    for label in &labels {
        args.extend(["--label".to_string(), label.clone()]);
    }
    if backend == Backend::Buildx {
        for reference in &import {
            args.extend([
                "--cache-from".to_string(),
                format!("type=registry,ref={reference}"),
            ]);
        }
        if let Some(export) = &export {
            args.extend([
                "--cache-to".to_string(),
                format!("type=registry,ref={export}"),
            ]);
        }
        args.push("--provenance=false".into());
        match &opts.oci_output {
            Some(path) => args.extend(["--output".to_string(), format!("type=oci,dest={path}")]),
            None => args.push("--load".into()),
        }
    } else {
        args.push("--pull=newer".into());
    }
    args.push(".".into());

    Err(format!(
        "{program}: {}",
        Command::new(program).args(&args).current_dir(root).exec()
    ))
}

fn prepare(root: &Path, opts: &Options) -> Result<Option<Prepared>, String> {
    let Some(plan) = Planned::checked(root)? else {
        return Ok(None);
    };
    eprintln!(
        "buildc: {} generated files match the snapshot",
        plan.generated
    );
    let backend = chosen_backend(opts.backend.clone(), env("BUILD_BACKEND"), plan.backend()?)?;
    let target = plan.target(opts.target.as_deref())?;
    let published = target.published();

    let version = match env("IMAGE_VERSION") {
        Some(named) => named,
        None => today()?,
    };
    let declared_base = target.base();
    let resolved_base = match (env("BASE"), &declared_base) {
        (Some(given), _) => Some(given),
        (None, Some(declared)) => record::base(declared),
        (None, None) => None,
    };
    let source_commit = record::source_commit(root);
    confirm_clones(&target.cloned()?, plan.audit_enforce())?;
    let unresolved = target.unresolved();
    crate::provenance::enforce_build(
        plan.audit_enforce(),
        resolved_base.as_deref(),
        source_commit.as_deref(),
        &unresolved,
    )?;
    for why in &unresolved {
        eprintln!("buildc: {why}, so the build record leaves it out");
    }
    if let Some(declared) = &declared_base {
        match &resolved_base {
            Some(resolved) => eprintln!("buildc: base {declared} -> {resolved}"),
            None => {
                eprintln!(
                    "buildc: {declared} did not resolve to a digest; the build record says so"
                )
            }
        }
    }

    let namespace = crate::registry::namespace(root);
    if let Err(err) = &namespace {
        eprintln!("buildc: {err}; the image carries no IMAGE_REGISTRY");
    }

    let mut build_args = vec![
        format!("IMAGE_VERSION={version}"),
        format!(
            "IMAGE_REGISTRY={}",
            namespace.as_deref().unwrap_or_default()
        ),
        format!("SOURCE_COMMIT={}", source_commit.unwrap_or_default()),
        format!("BUILD_BACKEND={}", backend.name()),
        format!("BUILDC_VERSION={}", env!("CARGO_PKG_VERSION")),
    ];
    build_args.extend(target.args());
    if let Some(declared) = &declared_base {
        build_args.push(format!(
            "BASE={}",
            resolved_base.as_deref().unwrap_or(declared)
        ));
    }
    if let Some(kernel) = &opts.kernel {
        build_args.push(format!("KERNEL={kernel}"));
    }

    let mut tags = opts.tags.clone();
    tags.extend(lines("TAGS"));
    if tags.is_empty() {
        let default_tag = env("DEFAULT_TAG").unwrap_or_else(|| "latest".to_string());
        tags.push(match env("IMAGE_NAME") {
            Some(name) => format!("{name}:{default_tag}"),
            None => format!("{published}:{default_tag}"),
        });
    }
    let mut labels: Vec<String> = lines("LABELS");
    if plan.manifest_label() {
        labels.push(format!("org.buildc.manifest={}", record::MANIFEST));
    }

    let (import, export) = cache(&plan, &target, opts, namespace.ok())?;

    Ok(Some(Prepared {
        backend,
        containerfile: target.containerfile(),
        build_args,
        validate: target.validate(),
        family: target.family(),
        target: target.name.clone(),
        version,
        tags,
        labels,
        import,
        export,
        uki: target.uki(),
    }))
}

fn confirm_clones(cloned: &[crate::planned::Cloned], enforce: bool) -> Result<(), String> {
    for one in cloned {
        match record::clone_commit(&one.url, &one.version) {
            Some(now) if now != one.commit => {
                return Err(format!(
                    "{} named {} when `buildc generate` ran and names {now} now; run `buildc generate` to record the commit the build clones",
                    one.named, one.commit
                ))
            }
            Some(_) => {}
            None => {
                let why = format!(
                    "{} did not resolve {} at {}, so the build cannot confirm the commit `buildc generate` recorded",
                    one.named, one.version, one.url
                );
                match enforce {
                    true => {
                        return Err(format!(
                            "{why}; an enforced audit builds nothing it cannot confirm"
                        ))
                    }
                    false => eprintln!("buildc: {why}"),
                }
            }
        }
    }
    Ok(())
}

fn chosen_backend(
    flag: Option<String>,
    variable: Option<String>,
    declared: Backend,
) -> Result<Backend, String> {
    match flag.or(variable) {
        None => Ok(declared),
        Some(name) => Backend::parse(&name)
            .ok_or_else(|| format!("unknown backend `{name}` (buildx or buildah)")),
    }
}

fn build_prefix(
    backend: Backend,
    containerfile: &Path,
    build_args: &[String],
    secrets: &[Secret],
) -> Vec<String> {
    let mut args = match backend {
        Backend::Buildx => vec!["buildx".to_string(), "build".to_string()],
        Backend::Buildah => vec!["build".to_string()],
    };
    args.push("--file".to_string());
    args.push(containerfile.display().to_string());
    for arg in build_args {
        args.extend(["--build-arg".to_string(), arg.clone()]);
    }
    for Secret { id, path } in secrets {
        args.extend(["--secret".to_string(), format!("id={id},src={path}")]);
    }
    args
}

fn build_argv(
    prefix: &[String],
    extra: &[(&str, &str)],
    target: Option<&str>,
    tags: &[String],
    labels: &[String],
    pull: bool,
) -> Vec<String> {
    let mut args = prefix.to_vec();
    for (name, value) in extra {
        args.extend(["--build-arg".to_string(), format!("{name}={value}")]);
    }
    if let Some(target) = target {
        args.extend(["--target".to_string(), target.to_string()]);
    }
    for tag in tags {
        args.extend(["--tag".to_string(), tag.clone()]);
    }
    for label in labels {
        args.extend(["--label".to_string(), label.clone()]);
    }
    if pull {
        args.push("--pull=newer".to_string());
    }
    args.push(".".to_string());
    args
}

fn scratch_tag(kind: &str, target: &str) -> String {
    let tag: String = target
        .chars()
        .map(
            |c| match c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                true => c,
                false => '-',
            },
        )
        .collect();
    format!("localhost/buildc-uki-{kind}:{tag}-{}", std::process::id())
}

struct Invocation<'a> {
    program: &'a str,
    containerfile: &'a Path,
    build_args: &'a [String],
    validate: &'a [String],
    family: &'a str,
    target: &'a str,
    tags: &'a [String],
    labels: &'a [String],
}

fn build_uki(root: &Path, invocation: &Invocation, secrets: &[Secret]) -> Result<(), String> {
    let &Invocation {
        program,
        containerfile,
        build_args,
        validate,
        family,
        target,
        tags,
        labels,
    } = invocation;
    let root = fs::canonicalize(root).map_err(|err| format!("{}: {err}", root.display()))?;
    let containerfile = root.join(containerfile);
    let root = root.as_path();
    if tags.is_empty() {
        return Err("a UKI build has no tag to publish".into());
    }
    let text = fs::read_to_string(&containerfile).map_err(|err| {
        format!(
            "{}: {err}; run `buildc generate` to write the boot chain's tail",
            containerfile.display()
        )
    })?;
    if !text.contains("${SPLIT_BASE}") || !text.contains("ARG COMPOSEFS_DIGEST") {
        return Err(format!(
            "{} carries no two-pass seal (`FROM ${{SPLIT_BASE}}` and `ARG COMPOSEFS_DIGEST`); \
             the component collection providing its boot chain predates this tool, so update it, \
             then run `buildc generate`",
            containerfile.display()
        ));
    }
    let seal = seal_containerfile(&text).ok_or_else(|| {
        format!(
            "{} has no tail section to seal; run `buildc generate` to write it",
            containerfile.display()
        )
    })?;
    let seal_file = root
        .join(layout::OUT)
        .join(format!("uki-seal-{}.Containerfile", std::process::id()));
    if let Some(dir) = seal_file.parent() {
        fs::create_dir_all(dir).map_err(|err| format!("{}: {err}", dir.display()))?;
    }
    fs::write(&seal_file, seal).map_err(|err| format!("{}: {err}", seal_file.display()))?;

    let split_tag = scratch_tag("split", target);
    let sealed_tag = scratch_tag("sealed", target);
    let sealed = (|| -> Result<String, String> {
        let prefix = build_prefix(Backend::Buildah, &containerfile, build_args, secrets);
        let split = build_argv(
            &prefix,
            &[],
            Some("split"),
            std::slice::from_ref(&split_tag),
            &[],
            true,
        );
        run_build(
            program,
            root,
            &split,
            &format!("building the split image {split_tag}"),
        )?;
        let store = graph_root()?;
        let digest = composefs_digest(&store, &split_tag)?;
        let prefix = build_prefix(Backend::Buildah, &seal_file, build_args, secrets);
        let argv = build_argv(
            &prefix,
            &[
                ("SPLIT_BASE", split_tag.as_str()),
                ("COMPOSEFS_DIGEST", digest.as_str()),
            ],
            None,
            std::slice::from_ref(&sealed_tag),
            labels,
            true,
        );
        run_build(
            program,
            root,
            &argv,
            &format!("sealing with composefs digest {digest}"),
        )?;
        Ok(digest)
    })();
    crate::init::tidy(&seal_file);
    let digest = match sealed {
        Ok(digest) => {
            remove_image(program, &split_tag);
            digest
        }
        Err(err) => {
            remove_image(program, &split_tag);
            remove_image(program, &sealed_tag);
            return Err(err);
        }
    };

    let checked = (|| -> Result<(), String> {
        let store = graph_root()?;
        let published = composefs_digest(&store, &sealed_tag)?;
        if published != digest {
            return Err(format!(
                "the sealed image digests {published}, not {digest} the UKI embeds; a step \
                 after the seal changed the image, so `bootc install` would refuse it"
            ));
        }
        let out = Command::new(program)
            .args(verify_argv(&sealed_tag))
            .output()
            .map_err(|err| format!("{program}: {err}"))?;
        let cmdline = String::from_utf8_lossy(&out.stdout);
        if !(out.status.success() && carries_digest(&cmdline, &digest)) {
            return Err(format!(
                "{sealed_tag} does not carry composefs={digest} on its UKI cmdline"
            ));
        }
        validate_sealed(root, &sealed_tag, validate, family)
    })();
    if let Err(err) = checked {
        remove_image(program, &sealed_tag);
        return Err(err);
    }

    for tag in tags {
        let status = match Command::new(program)
            .args(["tag", &sealed_tag, tag])
            .status()
        {
            Ok(status) => status,
            Err(err) => {
                remove_image(program, &sealed_tag);
                return Err(format!("{program}: {err}"));
            }
        };
        if !status.success() {
            remove_image(program, &sealed_tag);
            return Err(format!("tagging {tag}: {program} exited {status}"));
        }
    }
    remove_image(program, &sealed_tag);
    Ok(())
}

fn seal_containerfile(text: &str) -> Option<String> {
    let at = text.rfind(crate::emit::containerfile::TAIL_END)?;
    let end = at + crate::emit::containerfile::TAIL_END.len();
    Some(text[..end].to_string())
}

pub fn validate(root: &Path, reference: &str, target: Option<&str>) -> Result<Stopped, String> {
    let Some(plan) = Planned::checked(root)? else {
        return Ok(Stopped::Repository);
    };
    let target = plan.target(target)?;
    let tool = root.join(layout::MOUNTED);
    if !tool.is_file() {
        return Err(format!(
            "{}: not there; run `buildc build` first, which stages the tool",
            tool.display()
        ));
    }
    let argv = validate_argv(root, reference, &target.validate(), &target.family());
    let status = Command::new("podman")
        .args(&argv)
        .status()
        .map_err(|err| format!("podman: {err}"))?;
    if !status.success() {
        return Err(format!("validating {reference}: podman exited {status}"));
    }
    Ok(Stopped::Built)
}

fn validate_sealed(
    root: &Path,
    tag: &str,
    validate: &[String],
    family: &str,
) -> Result<(), String> {
    let argv = validate_argv(root, tag, validate, family);
    let status = Command::new("podman")
        .args(&argv)
        .status()
        .map_err(|err| format!("podman: {err}"))?;
    match status.success() {
        true => Ok(()),
        false => Err(format!("validating {tag}: podman exited {status}")),
    }
}

fn validate_argv(root: &Path, tag: &str, validate: &[String], family: &str) -> Vec<String> {
    let tool = root.join(layout::MOUNTED);
    let mut argv: Vec<String> = vec![
        "run".to_string(),
        "--rm".to_string(),
        "--tmpfs".to_string(),
        "/tmp".to_string(),
        "--tmpfs".to_string(),
        "/run".to_string(),
        "-v".to_string(),
        format!("{}:/ctx/buildc:ro,z", tool.display()),
        "--entrypoint".to_string(),
        "/ctx/buildc".to_string(),
    ];
    for pair in validate {
        argv.extend(["-e".to_string(), pair.clone()]);
    }
    argv.extend(["-e".to_string(), format!("FAMILY={family}")]);
    argv.push(tag.to_string());
    argv.push("validate-image".to_string());
    argv
}

fn run_build(program: &str, root: &Path, argv: &[String], step: &str) -> Result<(), String> {
    eprintln!("buildc: {step}");
    let status = Command::new(program)
        .args(argv)
        .current_dir(root)
        .status()
        .map_err(|err| format!("{program}: {err}"))?;
    match status.success() {
        true => Ok(()),
        false => Err(format!("{step}: {program} exited {status}")),
    }
}

fn graph_root() -> Result<String, String> {
    let out = Command::new("podman")
        .args(["info", "--format", "{{.Store.GraphRoot}}"])
        .output()
        .map_err(|err| format!("podman info: {err}"))?;
    let store = String::from_utf8_lossy(&out.stdout).trim().to_string();
    match out.status.success() && !store.is_empty() {
        true => Ok(store),
        false => Err("`podman info --format '{{.Store.GraphRoot}}'` named no store".into()),
    }
}

fn digest_argv(store: &str, image: &str) -> Vec<String> {
    vec![
        "run".to_string(),
        "--rm".to_string(),
        "--privileged".to_string(),
        "--security-opt".to_string(),
        "label=disable".to_string(),
        "--tmpfs".to_string(),
        "/var/tmp:size=8g".to_string(),
        "-v".to_string(),
        format!("{store}:/var/lib/containers/storage"),
        image.to_string(),
        "bootc".to_string(),
        "container".to_string(),
        "compute-composefs-digest-from-storage".to_string(),
        image.to_string(),
    ]
}

fn composefs_digest(store: &str, image: &str) -> Result<String, String> {
    let argv = digest_argv(store, image);
    let out = Command::new("podman")
        .args(&argv)
        .output()
        .map_err(|err| format!("podman: {err}"))?;
    let digest = String::from_utf8_lossy(&out.stdout).trim().to_string();
    match out.status.success() && digest.len() == 128 && digest.bytes().all(lower_hex) {
        true => Ok(digest),
        false => Err(format!(
            "`podman {}` did not print a composefs digest: `{digest}`",
            argv.join(" ")
        )),
    }
}

fn lower_hex(byte: u8) -> bool {
    byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
}

fn remove_image(program: &str, tag: &str) {
    match Command::new(program).args(["rmi", "--force", tag]).output() {
        Ok(out) if out.status.success() || !image_exists(program, tag) => {}
        Ok(out) => eprintln!(
            "buildc: `{program} rmi --force {tag}` failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ),
        Err(err) => eprintln!("buildc: `{program} rmi --force {tag}`: {err}"),
    }
}

fn image_exists(program: &str, tag: &str) -> bool {
    Command::new(program)
        .args(["image", "inspect", tag])
        .output()
        .is_ok_and(|out| out.status.success())
}

fn carries_digest(cmdline: &str, digest: &str) -> bool {
    cmdline.contains(&format!("composefs={digest}"))
}

fn verify_argv(tag: &str) -> Vec<String> {
    vec![
        "run".to_string(),
        "--rm".to_string(),
        "--entrypoint".to_string(),
        "/bin/sh".to_string(),
        tag.to_string(),
        "-c".to_string(),
        "uki=$(find /boot/EFI/Linux -name \"*.efi\" -print -quit); \
         objcopy -O binary --only-section=.cmdline \"$uki\" /dev/stdout | tr -d \"\\0\""
            .to_string(),
    ]
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}

fn lines(name: &str) -> Vec<String> {
    env(name)
        .iter()
        .flat_map(|value| value.lines())
        .filter(|line| !line.trim().is_empty())
        .map(str::to_string)
        .collect()
}

struct Secret {
    id: String,
    path: String,
}

fn secrets(opts: &Options) -> Result<Vec<Secret>, String> {
    let mut out: Vec<Secret> = Vec::new();
    for pair in &opts.secrets {
        match pair.split_once('=') {
            Some((id, path)) if !id.is_empty() && !path.is_empty() => out.push(Secret {
                id: id.to_string(),
                path: path.to_string(),
            }),
            _ => return Err(format!("`--secret` takes `<id>=<path>`, not `{pair}`")),
        }
    }
    if let Some(path) = env("MOK_KEY_PATH") {
        if out.iter().any(|secret| secret.id == "mok_privkey") {
            return Err("MOK_KEY_PATH and `--secret mok_privkey=` both set; use one".into());
        }
        out.push(Secret {
            id: "mok_privkey".to_string(),
            path,
        });
    }
    for Secret { id, path } in &out {
        if !Path::new(path).is_file() {
            return Err(format!(
                "secret `{id}` points at `{path}`, which is not there"
            ));
        }
    }
    Ok(out)
}

fn cache(
    plan: &Planned,
    target: &crate::planned::Target,
    opts: &Options,
    namespace: Option<String>,
) -> Result<(Vec<String>, Option<String>), String> {
    if opts.no_cache_from && !opts.cache_to {
        return Ok((Vec::new(), None));
    }
    let (Some(namespace), Some(image)) = (namespace, plan.cache_image()) else {
        if opts.cache_to {
            return Err(
                "`--cache-to` needs a registry namespace and a `default-image` in repo.kdl".into(),
            );
        }
        eprintln!("buildc: skipping the registry layer cache");
        return Ok((Vec::new(), None));
    };
    let repo = format!("{namespace}/{image}");

    let published = target.published();
    let mut import = Vec::new();
    if !opts.no_cache_from {
        import.push(format!("{repo}:{published}"));
        for sibling in target.siblings() {
            import.push(format!("{repo}:{sibling}"));
        }
    }
    let export = opts
        .cache_to
        .then(|| format!("{repo}:{published},mode=max"));
    Ok((import, export))
}

fn install(root: &Path) -> Result<(), String> {
    let from = std::env::current_exe().map_err(|err| format!("this binary: {err}"))?;
    let to = root.join(layout::MOUNTED);
    if from.canonicalize().ok() == to.canonicalize().ok() {
        return Ok(());
    }
    let dir = to.parent().unwrap_or(Path::new(layout::OUT));
    fs::create_dir_all(dir).map_err(|err| format!("{}: {err}", dir.display()))?;
    fs::copy(&from, &to).map_err(|err| format!("{}: {err}", to.display()))?;
    fs::set_permissions(&to, fs::Permissions::from_mode(0o755))
        .map_err(|err| format!("{}: {err}", to.display()))
}

fn today() -> Result<String, String> {
    let out = Command::new("date")
        .args(["-u", "+%Y%m%d"])
        .output()
        .map_err(|err| format!("date: {err}"))?;
    let stamp = String::from_utf8_lossy(&out.stdout).trim().to_string();
    match out.status.success() && stamp.len() == 8 {
        true => Ok(stamp),
        false => Err(format!("date -u +%Y%m%d said `{stamp}`")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_by_a_build(root: &Path, target: &str) -> Vec<String> {
        let opts = Options {
            target: Some(target.to_string()),
            ..Options::default()
        };
        let prepared = prepare(root, &opts)
            .expect("the build prepares")
            .expect("the generated tree is the snapshot");
        let plan = Planned::checked(root)
            .expect("the plan reads")
            .expect("the generated tree is the snapshot");
        let target = plan.target(Some(target)).expect("the target is planned");
        let mut out = vec![
            prepared.backend.name().to_string(),
            prepared.containerfile.display().to_string(),
            prepared.family,
            prepared.uki.to_string(),
        ];
        out.extend(prepared.build_args);
        out.extend(prepared.validate);
        out.extend(prepared.tags);
        out.extend(prepared.import);
        for component in target.components() {
            out.push(component.path);
            out.extend(component.witnessed);
        }
        out.extend(
            target
                .overlay_owners()
                .into_iter()
                .map(|(path, owner)| format!("{path} {owner}")),
        );
        out.extend(target.datastream());
        out
    }

    fn generated_fixture(name: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("buildc-{name}-{}", std::process::id()));
        if let Err(err) = fs::remove_dir_all(&root) {
            assert_eq!(
                err.kind(),
                std::io::ErrorKind::NotFound,
                "{}: {err}",
                root.display()
            );
        }
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/repos/multi-image");
        let copied = Command::new("cp")
            .arg("-r")
            .arg(&fixture)
            .arg(&root)
            .status()
            .expect("cp runs");
        assert!(copied.success());
        std::env::set_var("BASE", "quay.io/fedora/fedora-bootc:44@sha256:0");
        std::env::set_var("IMAGE_VERSION", "1");
        std::env::set_var("IMAGE_REGISTRY", "ghcr.io/example");
        let run = crate::run::run(crate::Command::Generate, None, &root);
        assert_eq!(run.problems.issues.plain(), "");
        crate::write_generated(&root, &run.files, &run.copies).expect("generate writes");
        root
    }

    #[test]
    fn a_selector_that_moved_since_generate_refuses_the_build() {
        let cloned = |version: &str| crate::planned::Cloned {
            named: "`core/one` asset `src`".to_string(),
            url: "https://example.com/src.git".to_string(),
            version: version.to_string(),
            commit: "a".repeat(40),
        };
        assert_eq!(confirm_clones(&[cloned(&"a".repeat(40))], false), Ok(()));
        let moved = confirm_clones(&[cloned(&"b".repeat(40))], false)
            .err()
            .unwrap_or_default();
        assert!(moved.contains("run `buildc generate`"), "{moved}");
        let mut unreachable = cloned("v1");
        unreachable.url = "https://invalid.invalid/src.git".to_string();
        assert_eq!(
            confirm_clones(std::slice::from_ref(&unreachable), false),
            Ok(())
        );
        let enforced = confirm_clones(&[unreachable], true)
            .err()
            .unwrap_or_default();
        assert!(enforced.contains("an enforced audit"), "{enforced}");
    }

    #[test]
    fn a_build_adds_its_own_inputs_to_the_planned_arguments() {
        let root = generated_fixture("prepared");
        let opts = Options {
            target: Some("workstation".to_string()),
            ..Options::default()
        };
        let prepared = prepare(&root, &opts)
            .expect("the build prepares")
            .expect("the generated tree is the snapshot");
        fs::remove_dir_all(&root).expect("the scratch repository goes");
        assert_eq!(prepared.backend, Backend::Buildah);
        assert_eq!(prepared.target, "workstation");
        assert_eq!(prepared.version, "1");
        assert_eq!(prepared.tags, vec!["workstation:latest"]);
        assert_eq!(
            prepared.import[0],
            "ghcr.io/example/workstation-cache:workstation"
        );
        assert!(prepared.import.len() > 1, "{:?}", prepared.import);
        assert!(prepared.export.is_none());
        assert!(!prepared.uki);
        for arg in [
            "IMAGE_VERSION=1",
            "IMAGE_REGISTRY=ghcr.io/example",
            "TARGET=workstation",
            "CONFORMS=standard",
            "BASE=quay.io/fedora/fedora-bootc:44@sha256:0",
        ] {
            assert!(
                prepared.build_args.iter().any(|have| have == arg),
                "{arg}: {:?}",
                prepared.build_args
            );
        }
    }

    #[test]
    fn a_source_edit_after_generate_reaches_no_build_input() {
        let root = generated_fixture("inert");
        let before = read_by_a_build(&root, "workstation");
        assert!(
            before
                .iter()
                .any(|arg| arg == "SCAP_CONTENT=/ctx/scap/ssg-fedora-ds.xml"),
            "{before:?}"
        );

        fs::rename(root.join("components"), root.join("moved")).expect("the sources move");
        fs::create_dir(root.join("components")).expect("an empty components directory is made");
        fs::write(root.join("components/components.lock.kdl"), "not a lock")
            .expect("a broken lock is written");
        let image = root.join("workstation.image.kdl");
        let text = fs::read_to_string(&image).expect("the image file reads");
        fs::write(&image, text.replace("conforms \"standard\"", ""))
            .expect("the image file writes");
        let repo = root.join("repo.kdl");
        let text = fs::read_to_string(&repo).expect("repo.kdl reads");
        fs::write(&repo, format!("{text}backend \"buildx\"\n")).expect("repo.kdl writes");

        let after = read_by_a_build(&root, "workstation");
        assert_eq!(before, after);

        fs::remove_dir_all(root.join("components")).expect("the broken lock goes");
        fs::rename(root.join("moved"), root.join("components")).expect("the sources return");
        let run = crate::run::run(crate::Command::Generate, None, &root);
        assert_eq!(run.problems.issues.plain(), "");
        crate::write_generated(&root, &run.files, &run.copies).expect("generate writes");
        let regenerated = read_by_a_build(&root, "workstation");
        fs::remove_dir_all(&root).expect("the scratch repository goes");
        assert_ne!(before, regenerated);
        assert!(
            regenerated.iter().any(|fact| fact == "buildx"),
            "{regenerated:?}"
        );
        assert!(
            regenerated.iter().any(|arg| arg == "CONFORMS="),
            "{regenerated:?}"
        );
    }

    #[test]
    fn the_flag_wins_over_the_variable_and_the_variable_over_repo_kdl() {
        let named = |name: &str| Some(name.to_string());
        let chosen = |flag, variable| chosen_backend(flag, variable, Backend::Buildx);
        assert_eq!(chosen(None, None), Ok(Backend::Buildx));
        assert_eq!(chosen(None, named("buildah")), Ok(Backend::Buildah));
        assert_eq!(
            chosen(named("buildah"), named("buildx")),
            Ok(Backend::Buildah)
        );
        assert!(chosen(named("kaniko"), None).is_err());
    }

    fn prefix() -> Vec<String> {
        build_prefix(
            Backend::Buildah,
            Path::new("generated/next44/Containerfile"),
            &["FLAVOUR=".to_string(), "BASE=example:1".to_string()],
            &[Secret {
                id: "mok_privkey".to_string(),
                path: "/keys/mok.key".to_string(),
            }],
        )
    }

    #[test]
    fn a_secret_whose_file_is_absent_stops_the_build() {
        let opts = Options {
            secrets: vec!["signing=/nonexistent/buildc-test.key".to_string()],
            ..Options::default()
        };
        let refused = secrets(&opts).err().unwrap_or_default();
        assert!(
            refused.contains("/nonexistent/buildc-test.key"),
            "{refused}"
        );
    }

    #[test]
    fn the_scratch_tags_are_references_and_apart() {
        assert_eq!(
            scratch_tag("split", "next44"),
            format!("localhost/buildc-uki-split:next44-{}", std::process::id())
        );
        let flavoured = scratch_tag("split", "next44/dev");
        let (_, tag) = flavoured.rsplit_once(':').expect("a tag");
        assert!(!tag.is_empty() && !tag.contains('/'), "{flavoured}");
        assert_ne!(
            scratch_tag("split", "next44"),
            scratch_tag("sealed", "next44")
        );
    }

    #[test]
    fn pass_one_builds_the_split_stage_only() {
        let temp = scratch_tag("split", "next44");
        let args = build_argv(
            &prefix(),
            &[],
            Some("split"),
            std::slice::from_ref(&temp),
            &[],
            true,
        );
        assert!(args
            .windows(2)
            .any(|window| window[0] == "--target" && window[1] == "split"));
        assert!(args.contains(&temp));
        assert!(args.contains(&"--pull=newer".to_string()));
        assert!(!args.contains(&"example/next44:latest".to_string()));
    }

    #[test]
    fn the_digest_command_is_the_measured_shape() {
        assert_eq!(
            digest_argv(
                "/var/lib/containers/storage",
                "localhost/buildc-uki-split:next44"
            ),
            vec![
                "run",
                "--rm",
                "--privileged",
                "--security-opt",
                "label=disable",
                "--tmpfs",
                "/var/tmp:size=8g",
                "-v",
                "/var/lib/containers/storage:/var/lib/containers/storage",
                "localhost/buildc-uki-split:next44",
                "bootc",
                "container",
                "compute-composefs-digest-from-storage",
                "localhost/buildc-uki-split:next44",
            ]
        );
    }

    #[test]
    fn the_seal_file_stops_at_the_tail() {
        let text = format!(
            "ARG BASE\nFROM scratch AS rootfs\nRUN echo build\n{}\nARG FAMILY\nRUN /ctx/buildc validate-image\n",
            crate::emit::containerfile::TAIL_END
        );
        let seal = seal_containerfile(&text).expect("a tail section");
        assert!(seal.contains("RUN echo build"));
        assert!(seal.ends_with(crate::emit::containerfile::TAIL_END));
        assert!(!seal.contains("validate-image"));
    }

    #[test]
    fn validation_runs_the_sealed_image() {
        let root = Path::new("/repo");
        let args = vec![
            "FAMILY=fedora".to_string(),
            "CONTRACT_FILES=a b".to_string(),
        ];
        let argv = validate_argv(root, "example:latest", &args, "fedora");
        assert!(argv
            .windows(2)
            .any(|w| w[0] == "--entrypoint" && w[1] == "/ctx/buildc"));
        assert!(argv
            .windows(2)
            .any(|w| w[0] == "-e" && w[1] == "FAMILY=fedora"));
        assert!(argv
            .windows(2)
            .any(|w| w[0] == "-e" && w[1] == "CONTRACT_FILES=a b"));
        assert_eq!(argv[argv.len() - 2], "example:latest");
        assert_eq!(argv[argv.len() - 1], "validate-image");
        assert!(argv
            .iter()
            .any(|arg| arg == "/repo/out/buildc:/ctx/buildc:ro,z"));
        assert!(!argv.iter().any(|arg| arg == "label=disable"));
    }

    #[test]
    fn pass_two_carries_the_split_base_and_the_digest() {
        let split = scratch_tag("split", "next44");
        let sealed = scratch_tag("sealed", "next44");
        let digest = "a1".repeat(64);
        let labels = vec!["org.buildc.manifest=/usr/share/buildc/manifest.json".to_string()];
        let args = build_argv(
            &prefix(),
            &[
                ("SPLIT_BASE", split.as_str()),
                ("COMPOSEFS_DIGEST", digest.as_str()),
            ],
            None,
            std::slice::from_ref(&sealed),
            &labels,
            true,
        );
        assert!(args.contains(&format!("SPLIT_BASE={split}")));
        assert!(args.contains(&format!("COMPOSEFS_DIGEST={digest}")));
        assert!(args.contains(&sealed));
        assert!(!args.contains(&"example/next44:latest".to_string()));
        assert!(!args.iter().any(|arg| arg == "--target"));
        assert!(carries_digest(
            &format!("rw composefs={digest} quiet"),
            &digest
        ));
        assert!(!carries_digest("rw composefs=deadbeef quiet", &digest));
    }
}
