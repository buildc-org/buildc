use crate::emit::{component_build, finalize};
use crate::layout;
use crate::model::component::{Component, Position};
use crate::model::image::{Entry, Image};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

pub const SKELETON: &str = "scripts/Containerfile.skeleton";
pub const BEGIN: &str = "# ---- BEGIN GENERATED (component layers) ----";
pub const END: &str = "# ---- END GENERATED ----";
pub const TAIL_BEGIN: &str = "# ---- BEGIN GENERATED (tail fragments) ----";
pub const TAIL_END: &str = "# ---- END GENERATED (tail fragments) ----";

pub fn path(image: &Image) -> PathBuf {
    layout::generated_image(&image.id).join(layout::CONTAINERFILE)
}

pub fn file(skeleton: &str, image: &Image, section: &str, tails: &str) -> String {
    let mut out = String::new();
    let mut lines = skeleton.lines().peekable();
    if lines.peek().is_some_and(|l| l.starts_with("# syntax=")) {
        let _ = writeln!(out, "{}", lines.next().unwrap_or_default());
    }
    out.push_str(&preamble(&image.id, &image.boot));

    let mut generated = false;
    for line in lines {
        if line == END || line == TAIL_END {
            generated = false;
        }
        if !generated {
            let _ = writeln!(out, "{line}");
        }
        if line == BEGIN {
            let _ = write!(out, "\n{section}");
            generated = true;
        } else if line == TAIL_BEGIN {
            let _ = write!(out, "\n{tails}");
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

const FAMILY_ARG: &str = "\
# ---- base family ----
ARG FAMILY";

const COMPONENT_DIR: &str = "@COMPONENT@";

const LINEAGE: &str = "rootfs";

pub const OS_RELEASE: &str = "os-release";

const BUILDC_MOUNT: &str = "--mount=type=bind,from=buildc,source=/buildc,target=/ctx/buildc \\\n    ";

const CONFORMS_ARG: &str = "\
# ---- declared conformance ----
ARG CONFORMS=";

const SCAP_ARGS: &str = "\
# ---- claimed and refused rules ----
ARG SCAP_CONTENT=
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

pub fn sections(image: &Image, root: &Path) -> (String, String) {
    let mut out = String::new();
    let mut tails: Vec<String> = Vec::new();
    let mut flavour_arg_emitted = false;

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
    let mut identity_declared = false;

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
            blocks.push(fragment(entry, body));
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
                ));
            }
            let script = component_build::path(image, entry);
            blocks.push(standard(
                entry,
                component,
                image,
                &script,
                component.access.scripts,
            ));
        }
        if let Some(body) = at(Position::After) {
            blocks.push(fragment(entry, body));
        }
        if let Some(body) = at(Position::Tail) {
            tails.push(fragment(entry, body));
        }

        if let Some(flavour) = &entry.flavour {
            let _ = writeln!(out, "# ---- [{flavour}] ----");
        }
        let _ = writeln!(out, "# ---- {} ----", entry.path);
        let _ = write!(out, "{}\n\n", blocks.join("\n\n"));

        if renders_os_release(image, component) {
            if !identity_declared {
                out.push_str(&identity_args);
                identity_declared = true;
            }
            let render = format!("{identity_env}/ctx/buildc os-release");
            let _ = write!(
                out,
                "# ---- os-release ----\nRUN {BUILDC_MOUNT}{}\n\n",
                flavoured(entry.flavour.as_deref(), &render)
            );
        }
    }

    if !flavour_arg_emitted {
        let _ = write!(out, "{FLAVOUR_ARG}\n\n");
    }

    let _ = write!(out, "{IMAGE_VERSION_ARG}\n\n");
    let _ = write!(out, "{FAMILY_ARG}\n\n");
    let _ = write!(out, "{CONFORMS_ARG}\n\n");
    let _ = write!(out, "{SCAP_ARGS}\n\n");

    if !identity_declared {
        out.push_str(&identity_args);
    }

    let _ = write!(out, "{}\n\n", finalize_layer(image, &identity_env, root));

    let mut tail_out = String::new();
    for tail in &tails {
        let _ = write!(tail_out, "{tail}\n\n");
    }

    (out, tail_out)
}

pub fn renders_os_release(image: &Image, component: &Component) -> bool {
    component.answers(OS_RELEASE)
        && image
            .remover(OS_RELEASE, component.flavour.as_deref())
            .is_none()
}

pub fn flavoured(flavour: Option<&str>, line: &str) -> String {
    match flavour {
        None => line.to_string(),
        Some(flavour) => format!("if [ \"${{FLAVOUR:-}}\" = \"{flavour}\" ]; then {line}; fi"),
    }
}

pub fn section(image: &Image, root: &Path) -> String {
    let (mut out, tail) = sections(image, root);
    out.push_str(&tail);
    out
}

fn finalize_layer(image: &Image, identity_env: &str, root: &Path) -> String {
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
         RUN {network}--mount=type=bind,from=ctx,source=/{script},target=/ctx/finalize.sh \\\n    \
         --mount=type=bind,from=ctx,source=/generated/lib,target=/ctx/lib \\\n    "
    );

    let mut mounted: Vec<&str> = Vec::new();
    for (entry, _) in hooks {
        let dir = entry.path.as_str();
        if mounted.contains(&dir) {
            continue;
        }
        let _ = write!(
            out,
            "--mount=type=bind,from=ctx,source=/components/{dir},target=/ctx/components/{dir} \\\n    "
        );
        mounted.push(dir);
    }

    out.push_str(BUILDC_MOUNT);
    out.push_str(
        "--mount=type=cache,target=/var/cache \\\n    \
         --mount=type=cache,target=/var/log \\\n    \
         --mount=type=tmpfs,target=/tmp \\\n    \
         FLAVOUR=${FLAVOUR} IMAGE_VERSION=\"${IMAGE_VERSION}\" \\\n    \
         CONFORMS=\"${CONFORMS}\" FAMILY=\"${FAMILY}\" \\\n    \
         SCAP_CONTENT=\"${SCAP_CONTENT}\" SCAP_CLAIMED=\"${SCAP_CLAIMED}\" \\\n    \
         SCAP_REFUSED=\"${SCAP_REFUSED}\" \\\n    ",
    );
    out.push_str(identity_env);
    out.push_str("bash /ctx/finalize.sh");
    out
}

fn standard(
    entry: &Entry,
    component: &Component,
    image: &Image,
    script: &Path,
    open: bool,
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
                "--mount=type=bind,from=ctx,source=/components/{}/{},target=/ctx/lib/{name} \\\n    ",
                owner.dir, helper.name
            )
        })
        .collect();
    let rw = if helpers.is_empty() { "" } else { ",rw" };
    let network = match open {
        true => "",
        false => "--network=none ",
    };
    let _ = write!(
        out,
        "RUN {network}--mount=type=bind,from=ctx,source=/components/{path},target=/ctx/components/{path} \\\n    \
         --mount=type=bind,from=ctx,source=/generated/lib,target=/ctx/lib{rw} \\\n    \
         {helpers}\
         --mount=type=bind,from=ctx,source=/{script},target=/ctx/component.sh \\\n    \
         {keys}{BUILDC_MOUNT}\
         --mount=type=cache,target=/var/cache \\\n    \
         --mount=type=cache,target=/var/log \\\n    \
         --mount=type=tmpfs,target=/tmp \\\n    \
         {secrets}{env}bash /ctx/component.sh"
    );
    out
}

fn fragment(entry: &Entry, body: &str) -> String {
    let dir = &entry.path;
    let mut out = String::new();
    let _ = write!(
        out,
        "# verbatim from components/{dir}/Containerfile.inc:\n{}",
        body.trim_end_matches('\n')
            .replace(COMPONENT_DIR, &format!("/components/{dir}"))
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
