use crate::diag::{Issue, Issues};
use crate::model::component::{Capability, Component, Position};
use crate::model::image::{Entry, Image};
use crate::provider::Index;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub fn suppress(image: &mut Image) {
    let Some(base) = &image.base else {
        return;
    };
    let required: BTreeSet<String> = image
        .components()
        .flat_map(|component| component.requires.iter())
        .map(|decl| decl.name.clone())
        .collect();
    let (suppressed, kept): (Vec<Entry>, Vec<Entry>) = image.entries.drain(..).partition(|entry| {
        !required.contains(&entry.path)
            && entry.component.as_ref().is_some_and(|component| {
                base.covers(component.provides.iter().map(|decl| decl.name.as_str()))
            })
    });
    image.entries = kept;
    image.suppressed = suppressed;
}

fn names<'a>(decls: impl IntoIterator<Item = &'a str>) -> String {
    decls
        .into_iter()
        .map(|name| format!("`{name}`"))
        .collect::<Vec<String>>()
        .join(", ")
}

fn same_key(capability: &str, providers: &[&Component]) -> bool {
    let Some(kind) = capability.strip_suffix("-key") else {
        return false;
    };
    let Some(first) = providers[0].keys.iter().find(|key| key.kind == kind) else {
        return false;
    };
    providers.iter().all(|component| {
        component.keys.iter().any(|key| {
            key.kind == first.kind
                && key.generator == first.generator
                && key.profile == first.profile
                && key.bits == first.bits
                && key.public == first.public
                && key.format == first.format
                && key.private == first.private
        })
    })
}

fn nowhere(index: &Index, capability: &str, image: &Image) -> String {
    let next = match index.sourced() {
        true => "`buildc import component` lists what the declared libraries provide",
        false => "repo.kdl declares no `sources`, so there is no collection to import one from",
    };
    let base = match &image.base {
        Some(base) => format!("the base `{}`", base.path),
        None => "a base".to_string(),
    };
    format!(
        "no component in the repository declares `provides {capability:?}`, and neither does {base}. {next}"
    )
}

fn satisfied_by(index: &Index, image: &Image, name: &str) -> String {
    let family = image.base.as_ref().map_or("", |base| base.family.as_str());
    let fits = index.fitting(name, family);
    let named = |candidates: &[&crate::provider::Provider]| -> Vec<String> {
        candidates
            .iter()
            .map(|p| format!("`{}`", p.qualified()))
            .collect()
    };
    if fits.len() > 1 {
        return format!(
            "{} each provide it; list one of them in this image",
            crate::said(&named(&fits))
        );
    }
    if let Some(first) = fits.first() {
        return format!(
            "{} would satisfy it; add it to this image",
            named(&[first])[0]
        );
    }
    let unfit = index.of(name);
    match unfit.is_empty() {
        true => nowhere(index, name, image),
        false => format!(
            "{} would satisfy it on a family other than `{family}`",
            crate::said(&named(&unfit))
        ),
    }
}

fn check_removes(image: &Image, issues: &mut Issues) {
    for component in image.components() {
        for decl in &component.removes {
            let supplied = image.base_answers(&decl.name)
                || image.components().any(|other| {
                    !std::ptr::eq(other, component)
                        && super::meet(other.flavour.as_deref(), component.flavour.as_deref())
                        && other.answers(&decl.name)
                });
            if supplied {
                continue;
            }
            issues.push(
                Issue::new(
                    format!(
                        "`{}` removes `{}`, which nothing in the image provides",
                        component.path, decl.name
                    ),
                    &component.src,
                )
                .at(decl.span, "nothing to remove")
                .help("a `removes` withdraws what the base or another component provides; drop it, so it cannot outlive its reason"),
            );
        }
    }
}

fn importer<'a>(image: &'a Image, path: &str) -> Option<&'a str> {
    image
        .entries
        .iter()
        .find(|entry| entry.path == path)
        .and_then(|entry| entry.imported_by.as_deref())
}

fn elsewhere(component: &Component, providers: &[&Component]) -> Option<String> {
    let reaches = |provider: &&Component| {
        provider.flavour.is_none() || provider.flavour.as_ref() == component.flavour.as_ref()
    };
    if providers.is_empty() || providers.iter().any(reaches) {
        return None;
    }
    let named: Vec<String> = providers
        .iter()
        .map(|provider| {
            format!(
                "`{}` on the `{}` flavour",
                provider.path,
                provider.flavour.as_deref().unwrap_or_default()
            )
        })
        .collect();
    Some(crate::said(&named))
}

fn answering<'m>(image: &'m Image, required: &str) -> Vec<&'m Component> {
    image
        .components()
        .filter(|component| component.answers(required))
        .collect()
}

fn clashing<'m>(
    providers: &[&'m Component],
    declares: &impl Fn(&&Component) -> bool,
) -> Vec<&'m Component> {
    let side = |component: &'m Component| (component.flavour.as_deref(), declares(&component));
    providers
        .iter()
        .enumerate()
        .filter(|&(at, &one)| {
            providers
                .iter()
                .enumerate()
                .any(|(other_at, &other)| at != other_at && super::conflict(side(one), side(other)))
        })
        .map(|(_, &component)| component)
        .collect()
}

#[expect(
    clippy::too_many_lines,
    reason = "every graph check reads the provider maps the function builds first"
)]
pub fn check_graph(image: &Image, index: &Index, issues: &mut Issues) {
    let mut offered: BTreeMap<&str, Vec<&Component>> = BTreeMap::new();
    for component in image.components() {
        for decl in &component.provides {
            offered
                .entry(decl.name.as_str())
                .or_default()
                .push(component);
        }
    }

    let base_caps: BTreeMap<&str, &Capability> = image
        .base
        .iter()
        .flat_map(|b| b.provides.iter())
        .map(|decl| (decl.name.as_str(), decl))
        .collect();

    let mut helpers: BTreeMap<&str, &Component> = BTreeMap::new();
    for component in image.components() {
        for helper in &component.helpers {
            let name = Path::new(&helper.name)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            if let Some(first) = helpers.insert(name, component) {
                issues.push(
                    Issue::new(
                        format!("two enabled components declare helper `{name}`"),
                        &component.src,
                    )
                    .at(helper.span, "mounted to the same path")
                    .help(format!(
                        "also declared by `{}`; helper basenames must be unique across an image",
                        first.path
                    )),
                );
            }
        }
    }

    for component in image.components() {
        let (covered, rest): (Vec<&Capability>, Vec<&Capability>) = component
            .provides
            .iter()
            .partition(|decl| base_caps.contains_key(decl.name.as_str()));
        let (Some(first), Some(_)) = (covered.first(), rest.first()) else {
            continue;
        };
        issues.push(
            Issue::new(
                format!(
                    "`{}` provides {}, which the base image already provides",
                    component.path,
                    names(covered.iter().map(|decl| decl.name.as_str()))
                ),
                &component.src,
            )
            .at(first.span, "already provided by the base")
            .help(format!(
                "a component the base covers entirely is suppressed instead. This one also provides {}, so its layer still builds and would provision {} a second time: split it, or drop the declaration",
                names(rest.iter().map(|decl| decl.name.as_str())),
                names(covered.iter().map(|decl| decl.name.as_str()))
            )),
        );
    }

    let base_family = image
        .base
        .as_ref()
        .map(|b| b.family.as_str())
        .filter(|f| !f.is_empty());
    if let Some(base_family) = base_family {
        let needs_adapter = image.components().any(|component| {
            (base_family == "fedora" && !component.coprs.is_empty())
                || component
                    .packages
                    .iter()
                    .chain(&component.groups)
                    .any(|group| group.family == base_family)
        });
        let has_adapter = answering(image, "build-environment")
            .iter()
            .any(|component| super::targets(&component.targets, base_family));
        if needs_adapter && !has_adapter {
            issues.push(
                Issue::new(
                    format!(
                        "image `{}` installs packages for `{base_family}`, but no enabled component provides `build-environment` for that family",
                        image.id
                    ),
                    &image.src,
                )
                .at(image.span, "missing family adapter"),
            );
        }
    }
    for component in image.components() {
        let Some(base_family) = base_family else {
            break;
        };
        if !super::targets(&component.targets, base_family) {
            issues.push(
                Issue::new(
                    format!(
                        "`{}` does not support the `{base_family}` base family",
                        component.path
                    ),
                    &component.src,
                )
                .help(format!(
                    "it declares support for: {}{}",
                    component.targets.join(", "),
                    importer(image, &component.path)
                        .map(|by| format!("; `{by}` imports it"))
                        .unwrap_or_default()
                )),
            );
        }
    }

    for (capability, providers) in &offered {
        let keyed = capability.strip_suffix("-key").is_some_and(|kind| {
            providers
                .iter()
                .any(|component| component.keys.iter().any(|key| key.kind == kind))
        });
        let declares =
            |component: &&Component| component.exclusive.iter().any(|name| name == capability);
        let found = match keyed {
            true => (providers.len() > 1 && !same_key(capability, providers)).then(|| {
                (
                    "Enable one provider, so that the image holds one key of this kind".to_string(),
                    providers.clone(),
                )
            }),
            false => {
                let clashing = clashing(providers, &declares);
                (!clashing.is_empty()).then(|| {
                    let holder = clashing
                        .iter()
                        .find(|component| declares(component))
                        .map_or("", |m| m.path.as_str());
                    (
                        format!("`{holder}` declares it `exclusive`, so enable one provider"),
                        clashing,
                    )
                })
            }
        };
        if let Some((why, clashing)) = found {
            let names: Vec<&str> = clashing.iter().map(|m| m.path.as_str()).collect();
            let first = clashing[0];
            issues.push(
                Issue::new(
                    format!("`{capability}` is provided by more than one enabled component"),
                    &first.src,
                )
                .at(
                    first
                        .provides
                        .iter()
                        .find(|d| d.name == **capability)
                        .map(|d| d.span)
                        .unwrap_or_default(),
                    "also provided elsewhere",
                )
                .help(format!("provided by: {}. {why}", names.join(", "))),
            );
        }
    }

    if let Some(base) = &image.base {
        for decl in &base.requires {
            if !answering(image, &decl.name).is_empty() {
                continue;
            }
            issues.push(
                Issue::new(
                    format!(
                        "`{}` requires `{}`, which nothing enabled provides",
                        base.image, decl.name
                    ),
                    &image.src,
                )
                .at(decl.span, "the base is not usable without it")
                .help(satisfied_by(index, image, &decl.name)),
            );
        }
    }

    if image.boot.is_empty() && !answering(image, "boot-chain-uki").is_empty() {
        issues.push(
            Issue::new(
                format!(
                    "`{}` enables a UKI chain but declares no `boot-chain`",
                    image.id
                ),
                &image.src,
            )
            .at(image.span, "no UKI trust route")
            .help("declare `boot-chain \"uki-shim\"` or `boot-chain \"uki-db\"`"),
        );
    }
    for capability in ["boot-chain-uki", "secureboot-enrolment"] {
        let present = answering(image, capability)
            .iter()
            .any(|component| component.flavour.is_none());
        if !image.boot.is_empty() && !present {
            issues.push(
                Issue::new(
                    format!(
                        "`{}` declares `boot-chain \"{}\"` but no ungated component provides `{capability}`",
                        image.id, image.boot
                    ),
                    &image.src,
                )
                .at(image.span, "incomplete UKI chain")
                .help(satisfied_by(index, image, capability)),
            );
        }
    }

    check_removes(image, issues);

    for component in image.components() {
        let flavour = component.flavour.as_deref();
        for decl in &component.requires {
            if let Some(remover) = image.remover(&decl.name, flavour) {
                issues.push(
                    Issue::new(
                        format!(
                            "`{}` requires `{}`, which `{}` removes",
                            component.path, decl.name, remover.path
                        ),
                        &component.src,
                    )
                    .at(decl.span, "removed from this image")
                    .help(format!(
                        "an image that lists `{}` no longer has `{}`; drop one of the two components from this image",
                        remover.path, decl.name
                    )),
                );
                continue;
            }
            if image.base_meets(&decl.name, flavour) {
                continue;
            }
            let providers: Vec<&Component> = answering(image, &decl.name)
                .into_iter()
                .filter(|provider| image.component_meets(provider, &decl.name, flavour))
                .collect();
            if providers.is_empty()
                && image
                    .components()
                    .any(|declaring| declaring.refused_keys.contains(&decl.name))
            {
                continue;
            }

            if providers.is_empty() {
                issues.push(
                    Issue::new(
                        format!(
                            "`{}` requires `{}`, which nothing enabled provides",
                            component.path, decl.name
                        ),
                        &component.src,
                    )
                    .at(decl.span, "unsatisfied")
                    .help(satisfied_by(index, image, &decl.name)),
                );
                continue;
            }

            if let Some(gated) = elsewhere(component, &providers) {
                issues.push(
                    Issue::new(
                        format!(
                            "`{}` requires `{}`, which only {gated} provides",
                            component.path, decl.name
                        ),
                        &component.src,
                    )
                    .at(decl.span, "unsatisfied on every other target")
                    .help("either gate this component to the same flavour, or move the provider out of the flavour block"),
                );
            }
        }
    }

    for component in image.components() {
        for decl in &component.after {
            if image.base_answers(&decl.name) {
                continue;
            }
            if let Some(gated) = elsewhere(component, &answering(image, &decl.name)) {
                issues.push(
                    Issue::new(
                        format!(
                            "`{}` builds after `{}`, which only {gated} provides",
                            component.path, decl.name
                        ),
                        &component.src,
                    )
                    .at(decl.span, "ineffective on every other target")
                    .help("either gate this component to the same flavour, or move the provider out of the flavour block"),
                );
            }
        }
    }

}

pub fn check_fragments(image: &Image, issues: &mut Issues) {
    let mut gated = false;
    for entry in &image.entries {
        let Some(component) = &entry.component else {
            continue;
        };
        gated |= entry.flavour.is_some();
        let Some(body) = &component.fragment else {
            continue;
        };
        let path = &entry.path;

        let above_gate = !gated && component.fragment_position != Position::Tail;
        if above_gate && (body.contains("${FLAVOUR}") || body.contains("$FLAVOUR")) {
            issues.push(
                Issue::new(
                    format!("`{path}` expands FLAVOUR above the flavour gate"),
                    &image.src,
                )
                .at(entry.span, "listed above the first flavour-gated component")
                .help("ARG FLAVOUR is declared directly above the first gated entry, so a fragment before it would expand to an empty string"),
            );
        }

        let runs = body.lines().any(|l| l.trim_start().starts_with("RUN "));
        let Some(flavour) = entry.flavour.as_ref().filter(|_| runs) else {
            continue;
        };
        let declared = body
            .split("FLAVOUR_GATE=")
            .nth(1)
            .map(|rest| rest.split_whitespace().next().unwrap_or_default());
        match declared {
            Some(d) if d == flavour => {}
            Some(d) => issues.push(
                Issue::new(
                    format!("`{path}` is listed under `{flavour}` but its fragment gates on `{d}`"),
                    &image.src,
                )
                .at(entry.span, "listed here"),
            ),
            None => issues.push(
                Issue::new(
                    format!(
                        "`{path}` is listed under `{flavour}` but its fragment sets no FLAVOUR_GATE"
                    ),
                    &image.src,
                )
                .at(entry.span, "the flavour gate would be silently ignored")
                .help(
                    "a fragment is emitted unconditionally, so anything it runs has to carry the gate itself",
                ),
            ),
        }
    }
}
