use crate::emit::{component_build, finalize};
use crate::layout;
use crate::model::component::{Component, Position};
use crate::model::image::{Entry, Image};
use crate::resolve::collect::Collection;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

pub const SKELETON: &str = "scripts/Containerfile.skeleton";
pub const BEGIN: &str = "# ---- BEGIN GENERATED (component layers) ----";
pub const END: &str = "# ---- END GENERATED ----";
pub const RECORD_BEGIN: &str = "# ---- BEGIN GENERATED (build record) ----";
pub const RECORD_END: &str = "# ---- END GENERATED (build record) ----";
pub const TAIL_BEGIN: &str = "# ---- BEGIN GENERATED (tail fragments) ----";
pub const TAIL_END: &str = "# ---- END GENERATED (tail fragments) ----";
pub const VALIDATE_BEGIN: &str = "# ---- BEGIN GENERATED (validate) ----";
pub const VALIDATE_END: &str = "# ---- END GENERATED (validate) ----";

pub struct Regions {
    pub layers: String,
    pub record: String,
    pub tails: String,
    pub validate: String,
}

const ENDS: [&str; 4] = [END, RECORD_END, TAIL_END, VALIDATE_END];

pub fn path(image: &Image) -> PathBuf {
    layout::generated_image(&image.id).join(layout::CONTAINERFILE)
}

pub fn file(skeleton: &str, image: &Image, regions: &Regions) -> String {
    let mut out = String::new();
    let mut lines = skeleton.lines().peekable();
    if lines.peek().is_some_and(|l| l.starts_with("# syntax=")) {
        let _ = writeln!(out, "{}", lines.next().unwrap_or_default());
    }
    out.push_str(&preamble(&image.id, &image.boot));

    let mut generated = false;
    for line in lines {
        if ENDS.contains(&line) {
            generated = false;
        }
        if !generated {
            let _ = writeln!(out, "{line}");
        }
        let body = match line {
            BEGIN => Some(&regions.layers),
            RECORD_BEGIN => Some(&regions.record),
            TAIL_BEGIN => Some(&regions.tails),
            VALIDATE_BEGIN => Some(&regions.validate),
            _ => None,
        };
        if let Some(body) = body {
            let _ = write!(out, "\n{body}");
            generated = true;
        }
    }
    out
}

fn preamble(id: &str, boot: &str) -> String {
    let mut out = format!(
        "# GENERATED FILE, do not edit. Produced by `buildc generate` from\n\
         # {SKELETON} and the {id} image definition.\n\n"
    );
    if !boot.is_empty() {
        out.push_str("ARG SPLIT_BASE=kernel-split\n\n");
    }
    out
}

const FLAVOUR_ARG: &str = "\
# ---- flavour gate ----
ARG FLAVOUR";

fn family_arg(image: &Image) -> String {
    let family = image.base.as_ref().map_or("", |base| base.family.as_str());
    format!("# ---- base family ----\nARG FAMILY={family}")
}

const COMPONENT_DIR: &str = "@COMPONENT@";

const LINEAGE: &str = "rootfs";

const CONFORMS_ARG: &str = "\
# ---- declared conformance ----
ARG CONFORMS=";

const SCAP_CONTENT_ARG: &str = "\
# ---- measured content ----
ARG SCAP_CONTENT=";

const SCAP_RULES_ARGS: &str = "\
# ---- claimed and refused rules ----
ARG SCAP_CLAIMED=
ARG SCAP_REFUSED=";

const IMAGE_VERSION_ARG: &str = "\
# ---- image version ----
ARG IMAGE_VERSION=dev";

fn identity(image: &Image) -> Vec<(&'static str, String)> {
    let mut vars: Vec<(&'static str, String)> = Vec::new();
    for (name, value) in [
        ("IMAGE_ID", &image.id),
        ("IMAGE_NAME", &image.name),
        ("IMAGE_PRETTY_NAME", &image.pretty_name),
        ("IMAGE_URL", &image.url),
        ("IMAGE_ISSUES_URL", &image.issues_url),
    ] {
        if !value.is_empty() {
            vars.push((name, value.clone()));
        }
    }
    vars.push(("IMAGE_REGISTRY", String::new()));
    vars
}

pub fn sections(
    image: &Image,
    root: &Path,
    audit_enforce: bool,
    collection: &Collection,
) -> Regions {
    let mut out = String::new();
    let mut tails: Vec<String> = Vec::new();
    let mut flavour_arg_emitted = false;
    let family = image.base.as_ref().map_or("", |base| base.family.as_str());

    if image
        .entries
        .iter()
        .filter_map(|entry| entry.component.as_ref())
        .any(|component| !component.keys.is_empty())
    {
        let _ = write!(
            out,
            "### Public keys\nFROM ctx AS public-keys\nCOPY keys/public/ /public/\n\n"
        );
    }

    if image.base.is_some() {
        let _ = write!(out, "### Base Image\nFROM ${{BASE}} AS {LINEAGE}\n\n");
    }

    // The base measurement runs before the first component layer because a component can change the packages the base ships.
    let early = super::measure::early(image);
    if !early.is_empty() {
        if !image.flavours.is_empty() {
            let _ = write!(out, "{FLAVOUR_ARG}\n\n");
            flavour_arg_emitted = true;
        }
        out.push_str(&early);
    }

    let identity = identity(image);
    let mut identity_args = String::from("# ---- image identity ----\n");
    for (name, value) in &identity {
        let _ = writeln!(identity_args, "ARG {name}=\"{value}\"");
    }
    identity_args.push('\n');
    let identity_env: String = identity
        .iter()
        .map(|(name, _)| format!("{name}=\"${{{name}}}\" "))
        .collect();

    let _ = write!(out, "## Component layers\n\n");
    let _ = write!(
        out,
        "# ---- /opt build root ----\nRUN mv /opt /opt.bak && mkdir /opt\n\n"
    );
    let _ = write!(
        out,
        "# ---- /usr/local build root ----\n\
         RUN if [ -L /usr/local ]; then mv /usr/local /usr/local.bak && \\\n    \
         mkdir -p /usr/local/bin /usr/local/etc /usr/local/games /usr/local/include \\\n    \
         /usr/local/lib /usr/local/lib64 /usr/local/libexec /usr/local/share /usr/local/src; fi\n\n"
    );

    for entry in &image.entries {
        let Some(component) = &entry.component else {
            continue;
        };

        if entry.flavour.is_some() && !flavour_arg_emitted {
            let _ = write!(out, "{FLAVOUR_ARG}\n\n");
            flavour_arg_emitted = true;
        }

        let at = |want| {
            component
                .fragment
                .as_ref()
                .filter(|_| component.fragment_position == want)
        };

        let mut blocks: Vec<String> = Vec::new();
        if let Some(body) = at(Position::Before) {
            blocks.push(fragment(image, entry, body));
        }
        if component.standard_layer {
            if component_build::splits(image, entry, component, root) {
                let packages = component_build::packages_path(image, entry);
                blocks.push(standard(
                    entry,
                    component,
                    image,
                    &packages,
                    component.access.packages,
                    component_build::package_reads_dir(root, entry, family),
                ));
            }
            let script = component_build::path(image, entry);
            blocks.push(standard(
                entry,
                component,
                image,
                &script,
                component.access.scripts,
                component_build::script_reads_dir(
                    root, entry, component, image, family, collection,
                ),
            ));
            if entry.flavour.is_none() {
                let overlays = copies(image, entry, family, root);
                if !overlays.is_empty() {
                    blocks.push(overlays);
                }
                if component_build::tail(entry, component, collection, family, root).is_some() {
                    let tail = component_build::tail_path(image, entry);
                    blocks.push(tail_layer(
                        entry,
                        image,
                        &tail,
                        component_build::tail_reads_dir(root, entry, family, collection),
                    ));
                }
            }
        }
        if let Some(body) = at(Position::After) {
            blocks.push(fragment(image, entry, body));
        }
        if component_build::witness_step(root, entry, family, component) {
            let script = component_build::witness_path(image, entry);
            blocks.push(witness_layer(entry, &script));
        }
        if let Some(body) = at(Position::Tail) {
            tails.push(fragment(image, entry, body));
        }

        if let Some(flavour) = &entry.flavour {
            let _ = writeln!(out, "# ---- [{flavour}] ----");
        }
        let _ = writeln!(out, "# ---- {} ----", entry.path);
        let _ = write!(out, "{}\n\n", blocks.join("\n\n"));
    }

    let flavours = !image.flavours.is_empty();
    if flavours && !flavour_arg_emitted {
        let _ = write!(out, "{FLAVOUR_ARG}\n\n");
    }

    let conforms = image.declares_conforms();
    let (claimed, refused) = crate::scap::exclusions(image);
    let rules = !claimed.is_empty() || !refused.is_empty();

    let _ = write!(out, "{IMAGE_VERSION_ARG}\n\n");
    let _ = write!(out, "{}\n\n", family_arg(image));
    if conforms {
        let _ = write!(out, "{CONFORMS_ARG}\n\n");
        let _ = write!(out, "{SCAP_CONTENT_ARG}\n\n");
    }
    if rules {
        let _ = write!(out, "{SCAP_RULES_ARGS}\n\n");
    }

    out.push_str(&identity_args);

    let _ = write!(
        out,
        "{}\n\n",
        finalize_layer(image, &identity_env, root, flavours, conforms, rules)
    );

    // A base image can carry a versions file from an earlier build. The record measurement resets the file unless the early measurement already wrote it.
    let early_measured = !early.is_empty();
    let record = record_layer(
        audit_enforce,
        image.is_bootc(),
        early_measured,
        &super::measure::record(image, !early_measured),
    );
    let validate = String::new();

    let mut tail_out = String::new();
    for tail in &tails {
        let _ = write!(tail_out, "{tail}\n\n");
    }

    Regions {
        layers: out,
        record,
        tails: tail_out,
        validate,
    }
}

pub fn section(image: &Image, root: &Path, audit_enforce: bool, collection: &Collection) -> String {
    let regions = sections(image, root, audit_enforce, collection);
    let parts = [
        regions.layers,
        regions.record,
        regions.tails,
        regions.validate,
    ];
    parts
        .iter()
        .filter(|part| !part.is_empty())
        .map(|part| part.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

fn record_layer(audit_enforce: bool, bootc: bool, early: bool, measure: &str) -> String {
    let mut out = String::from(
        "COPY --from=ctx /generated/plan.json /usr/share/buildc/manifest.json\n\n\
         ARG BASE\n\
         ARG BASE_DECLARED=\"\"\n\
         ARG TARGET=\"\"\n\
         ARG SOURCE_COMMIT=\"\"\n\
         ARG BUILD_BACKEND=\"\"\n\
         ARG COMPONENT_HASHES=\"\"\n\
         ARG ASSET_RESOLUTIONS=\"\"\n\
         ARG BUILDC_VERSION=\"\"\n",
    );
    if audit_enforce {
        out.push_str("ARG AUDIT_ENFORCE=\"\"\n");
    }
    if !measure.is_empty() {
        out.push('\n');
        out.push_str(measure);
    }
    let _ = write!(
        out,
        "RUN --mount=type=bind,from=ctx,source=/generated/lib,target=/ctx/lib \\\n    \
         BASE=\"${{BASE}}\" BASE_DECLARED=\"${{BASE_DECLARED}}\" TARGET=\"${{TARGET}}\" \\\n    \
         SOURCE_COMMIT=\"${{SOURCE_COMMIT}}\" BUILD_BACKEND=\"${{BUILD_BACKEND}}\" \\\n    \
         COMPONENT_HASHES=\"${{COMPONENT_HASHES}}\" \\\n    \
         ASSET_RESOLUTIONS=\"${{ASSET_RESOLUTIONS}}\" BUILDC_VERSION=\"${{BUILDC_VERSION}}\" "
    );
    if early || !measure.is_empty() {
        out.push_str("VERSIONS_FILE=/usr/share/buildc/versions ");
    }
    if audit_enforce {
        out.push_str("AUDIT_ENFORCE=\"${AUDIT_ENFORCE}\" ");
    }
    out.push_str(
        "\\\n    \
         IMAGE_ID=\"${IMAGE_ID}\" IMAGE_VERSION=\"${IMAGE_VERSION}\" bash /ctx/lib/build-record.sh\n",
    );
    if bootc {
        out.push_str("\nRUN --mount=type=tmpfs,target=/run --network=none bootc container lint\n");
    }
    out
}

fn finalize_layer(
    image: &Image,
    identity_env: &str,
    root: &Path,
    flavours: bool,
    conforms: bool,
    rules: bool,
) -> String {
    let hooks = finalize::hooks(image, root);
    let script = finalize::path(image).display().to_string();
    let closed = !hooks.is_empty()
        && hooks
            .iter()
            .all(|(entry, _)| entry.component.as_ref().is_some_and(|m| !m.access.scripts));
    let network = match closed {
        true => "--network=none ",
        false => "",
    };
    let mut out = format!(
        "# ---- finalize ----\n\
         RUN {network}--mount=type=bind,from=ctx,source=/{script},target=/{script} \\\n    \
         --mount=type=bind,from=ctx,source=/generated/lib,target=/ctx/lib \\\n    "
    );

    if conforms && image.base.is_some() {
        let _ = write!(
            out,
            "--mount=type=bind,from=ctx,source=/{},target={} \\\n    ",
            crate::scap::pinned::dir(image).display(),
            crate::scap::pinned::MOUNT
        );
    }
    let mut mounted: Vec<&str> = Vec::new();
    for (entry, _) in hooks {
        let dir = entry.path.as_str();
        if mounted.contains(&dir) {
            continue;
        }
        let tree = component_build::dir(image).join(dir).display().to_string();
        let _ = write!(
            out,
            "--mount=type=bind,from=ctx,source=/{tree},target=/ctx/components/{dir} \\\n    "
        );
        mounted.push(dir);
    }

    out.push_str(
        "--mount=type=cache,target=/var/cache \\\n    \
         --mount=type=cache,target=/var/log \\\n    \
         --mount=type=tmpfs,target=/tmp \\\n    ",
    );
    let mut env = String::new();
    if flavours {
        env.push_str("FLAVOUR=${FLAVOUR} ");
    }
    env.push_str("IMAGE_VERSION=\"${IMAGE_VERSION}\" ");
    if conforms {
        env.push_str("CONFORMS=\"${CONFORMS}\" SCAP_CONTENT=\"${SCAP_CONTENT}\" ");
    }
    env.push_str("FAMILY=\"${FAMILY}\" ");
    if rules {
        env.push_str("SCAP_CLAIMED=\"${SCAP_CLAIMED}\" SCAP_REFUSED=\"${SCAP_REFUSED}\" ");
    }
    out.push_str(&env);
    out.push_str(identity_env);
    let _ = write!(out, "bash /{script}");
    out
}

fn standard(
    entry: &Entry,
    component: &Component,
    image: &Image,
    script: &Path,
    open: bool,
    reads_dir: bool,
) -> String {
    let mut env = String::new();

    let mut secrets = String::new();
    for decl in &component.secrets {
        let id = &decl.name;
        let _ = write!(
            secrets,
            "--mount=type=secret,id={id},target=/run/secrets/{id},required=false \\\n    "
        );
    }
    for decl in component.args.iter().rev() {
        let name = &decl.name;
        env.insert_str(0, &format!("{name}=${{{name}}} "));
    }

    let path = &entry.path;
    let tree = component_build::dir(image).display().to_string();
    let script = script.display();
    let keys = match component.keys.is_empty() {
        true => String::new(),
        false => "--mount=type=bind,from=public-keys,source=/public,target=/ctx/keys \\\n    "
            .to_string(),
    };
    let mut out = String::new();
    let helpers: String = image
        .components()
        .flat_map(|owner| owner.helpers.iter().map(move |helper| (owner, helper)))
        .map(|(owner, helper)| {
            let name = Path::new(&helper.name)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy();
            format!(
                "--mount=type=bind,from=ctx,source=/{}/{}/{},target=/ctx/lib/{name} \\\n    ",
                component_build::dir(image).display(),
                owner.path,
                helper.name
            )
        })
        .collect();
    let rw = if helpers.is_empty() { "" } else { ",rw" };
    let network = match open {
        true => "",
        false => "--network=none ",
    };
    let dir = match reads_dir {
        true => format!(
            "--mount=type=bind,from=ctx,source=/{tree}/{path},target=/ctx/components/{path} \\\n    "
        ),
        false => String::new(),
    };
    let _ = write!(
        out,
        "RUN {network}{dir}\
         --mount=type=bind,from=ctx,source=/generated/lib,target=/ctx/lib{rw} \\\n    \
         {helpers}\
         --mount=type=bind,from=ctx,source=/{script},target=/{script} \\\n    \
         {keys}\
         --mount=type=cache,target=/var/cache \\\n    \
         --mount=type=cache,target=/var/log \\\n    \
         --mount=type=tmpfs,target=/tmp \\\n    \
         {secrets}{env}bash /{script}"
    );
    out
}

fn copies(image: &Image, entry: &Entry, family: &str, root: &Path) -> String {
    let tree = component_build::dir(image);
    let mut out = String::new();
    for gated in component_build::overlay_prefixes(root, entry, family) {
        let at = match gated.is_empty() {
            true => format!("/{}/{}/files/", tree.display(), entry.path),
            false => format!("/{}/{}/{gated}/files/", tree.display(), entry.path),
        };
        let _ = write!(
            out,
            "# ---- {} overlays ----\nCOPY --from=ctx {at} /\n",
            entry.path
        );
    }
    out.trim_end_matches('\n').to_string()
}

fn tail_layer(entry: &Entry, image: &Image, script: &Path, reads_dir: bool) -> String {
    let path = &entry.path;
    let tree = component_build::dir(image);
    let dir = match reads_dir {
        true => format!(
            "--mount=type=bind,from=ctx,source=/{}/{path},target=/ctx/components/{path} \\\n    ",
            tree.display()
        ),
        false => String::new(),
    };
    format!(
        "# ---- {path} after the copies ----\n\
         RUN --network=none {dir}\
         --mount=type=bind,from=ctx,source=/{},target=/{} \\\n    \
         --mount=type=tmpfs,target=/tmp \\\n    \
         bash /{}",
        script.display(),
        script.display(),
        script.display()
    )
}

fn witness_layer(entry: &Entry, script: &Path) -> String {
    let path = &entry.path;
    let at = format!("/{}", script.display());
    format!(
        "# ---- {path} witnesses ----\n\
         RUN --network=none --mount=type=bind,from=ctx,source={at},target={at} bash {at}"
    )
}

fn fragment(image: &Image, entry: &Entry, body: &str) -> String {
    let dir = &entry.path;
    let tree = component_build::dir(image).join(dir).display().to_string();
    let mut out = String::new();
    let _ = write!(
        out,
        "# verbatim from components/{dir}/Containerfile.inc:\n{}",
        body.trim_end_matches('\n')
            .replace(COMPONENT_DIR, &format!("/{tree}"))
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_uki_target_declares_the_split_base() {
        assert!(preamble("db", "uki-db").contains("ARG SPLIT_BASE=kernel-split"));
        assert!(!preamble("db", "").contains("SPLIT_BASE"));
    }
}
