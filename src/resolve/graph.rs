use crate::diag::{Issue, Issues};
use crate::model::image::{Entry, Image};
use crate::model::module::{Decl, Module, Position};
use crate::provider::Index;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub fn suppress(image: &mut Image) {
    let base: BTreeSet<String> = image
        .base
        .iter()
        .flat_map(|b| b.provides.iter())
        .map(|decl| decl.name.clone())
        .collect();
    if base.is_empty() {
        return;
    }
    let (suppressed, kept): (Vec<Entry>, Vec<Entry>) = image
        .entries
        .drain(..)
        .partition(|entry| covered(entry, &base));
    image.entries = kept;
    image.suppressed = suppressed;
}

fn covered(entry: &Entry, base: &BTreeSet<String>) -> bool {
    let Some(module) = &entry.module else {
        return false;
    };
    !module.provides.is_empty() && module.provides.iter().all(|decl| base.contains(&decl.name))
}

fn names(decls: &[&Decl]) -> String {
    decls
        .iter()
        .map(|decl| format!("`{}`", decl.name))
        .collect::<Vec<String>>()
        .join(", ")
}

fn same_key(capability: &str, providers: &[&Module]) -> bool {
    let Some(kind) = capability.strip_suffix("-key") else {
        return false;
    };
    let Some(first) = providers[0].keys.iter().find(|key| key.kind == kind) else {
        return false;
    };
    providers.iter().all(|module| {
        module.keys.iter().any(|key| {
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

fn nowhere(index: &Index, capability: &str, src: &str) -> String {
    let (searched, unsearched) = match (index.unread().is_empty(), index.sourced()) {
        (true, true) => ("the repository or its collections", String::new()),
        (true, false) => (
            "the repository",
            ". repo.kdl declares no `sources`, so there is no collection to import one from"
                .to_string(),
        ),
        (false, _) => ("the repository", format!(". {}", index.unsearched())),
    };
    format!(
        "no module in {searched} declares `provides {capability:?}`, and neither does the `base` node in {src}{unsearched}"
    )
}

fn satisfied_by(index: &Index, image: &Image, name: &str) -> String {
    let family = image.base.as_ref().map_or("", |base| base.family.as_str());
    let fits = index.fitting(name, family);
    let candidates = match fits.is_empty() {
        true => index.of(name),
        false => fits,
    };
    let named: Vec<String> = candidates.iter().map(|p| p.qualified()).collect();
    match candidates.iter().find(|p| p.here) {
        Some(_) => format!(
            "{} would satisfy it; add it to this image. Nothing is included automatically, so the list stays the complete statement of what is in the image",
            named.join(" or ")
        ),
        None => match candidates.first() {
            Some(first) => format!(
                "{} would satisfy it; `buildc import module {}` brings it in and lists it",
                named.join(" or "),
                first.qualified()
            ),
            None => nowhere(index, name, image.src.name()),
        },
    }
}

fn elsewhere(module: &Module, providers: &[&Module]) -> Option<String> {
    let reaches = |provider: &&Module| {
        provider.flavour.is_none() || provider.flavour.as_ref() == module.flavour.as_ref()
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

#[expect(
    clippy::too_many_lines,
    reason = "every graph check reads the provider maps the function builds first"
)]
pub fn check_graph(image: &Image, index: &Index, issues: &mut Issues) {
    let mut offered: BTreeMap<&str, Vec<&Module>> = BTreeMap::new();
    for module in image.modules() {
        for decl in &module.provides {
            offered.entry(decl.name.as_str()).or_default().push(module);
        }
    }

    let base_caps: BTreeMap<&str, &crate::model::module::Decl> = image
        .base
        .iter()
        .flat_map(|b| b.provides.iter())
        .map(|decl| (decl.name.as_str(), decl))
        .collect();

    let mut helpers: BTreeMap<&str, &Module> = BTreeMap::new();
    for module in image.modules() {
        for helper in &module.helpers {
            let name = Path::new(&helper.name)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            if let Some(first) = helpers.insert(name, module) {
                issues.push(
                    Issue::new(
                        format!("two enabled modules declare helper `{name}`"),
                        &module.src,
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

    for module in image.modules() {
        let (covered, rest): (Vec<&Decl>, Vec<&Decl>) = module
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
                    module.path,
                    names(&covered)
                ),
                &module.src,
            )
            .at(first.span, "already provided by the base")
            .help(format!(
                "a module the base covers entirely is suppressed instead. This one also provides {}, so its layer still builds and would provision {} a second time: split it, or drop the declaration",
                names(&rest),
                names(&covered)
            )),
        );
    }

    let base_family = image
        .base
        .as_ref()
        .map(|b| b.family.as_str())
        .filter(|f| !f.is_empty());
    if let Some(base_family) = base_family {
        let needs_adapter = image.modules().any(|module| {
            (base_family == "fedora" && !module.coprs.is_empty())
                || module
                    .packages
                    .iter()
                    .chain(&module.groups)
                    .any(|group| group.family == base_family)
        });
        let has_adapter = offered.get("build-environment").is_some_and(|providers| {
            providers
                .iter()
                .any(|module| super::supports(&module.supports, base_family))
        });
        if needs_adapter && !has_adapter {
            issues.push(
                Issue::new(
                    format!(
                        "image `{}` installs packages for `{base_family}`, but no enabled module provides `build-environment` for that family",
                        image.id
                    ),
                    &image.src,
                )
                .at(image.span, "missing family adapter"),
            );
        }
    }
    for module in image.modules() {
        let Some(base_family) = base_family else {
            break;
        };
        if !super::supports(&module.supports, base_family) {
            issues.push(
                Issue::new(
                    format!(
                        "`{}` does not support the `{base_family}` base family",
                        module.path
                    ),
                    &module.src,
                )
                .help(format!(
                    "it declares support for: {}",
                    module.supports.join(", ")
                )),
            );
        }
    }

    for (capability, providers) in &offered {
        let keyed = capability.strip_suffix("-key").is_some_and(|kind| {
            providers
                .iter()
                .any(|module| module.keys.iter().any(|key| key.kind == kind))
        });
        let declares = |module: &&Module| module.exclusive.iter().any(|name| name == capability);
        let why = match keyed {
            true => (providers.len() > 1 && !same_key(capability, providers)).then(|| {
                "Enable one provider, so that the image holds one key of this kind".to_string()
            }),
            false => super::exclusive(&providers.iter().map(declares).collect::<Vec<bool>>()).then(
                || {
                    let holder = providers
                        .iter()
                        .find(|module| declares(module))
                        .map_or("", |m| m.path.as_str());
                    format!("`{holder}` declares it `exclusive`, so enable one provider")
                },
            ),
        };
        if let Some(why) = why {
            let names: Vec<&str> = providers.iter().map(|m| m.path.as_str()).collect();
            let first = providers[0];
            issues.push(
                Issue::new(
                    format!("`{capability}` is provided by more than one enabled module"),
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
            if offered.contains_key(decl.name.as_str()) {
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

    if image.boot.is_empty() && offered.contains_key("boot-chain-uki") {
        issues.push(
            Issue::new(
                format!("`{}` enables a UKI chain but declares no `boot`", image.id),
                &image.src,
            )
            .at(image.span, "no UKI trust route")
            .help("declare `boot \"uki-shim\"` or `boot \"uki-db\"`"),
        );
    }
    for capability in ["boot-chain-uki", "secureboot-enrolment"] {
        let present = offered
            .get(capability)
            .is_some_and(|providers| providers.iter().any(|module| module.flavour.is_none()));
        if !image.boot.is_empty() && !present {
            issues.push(
                Issue::new(
                    format!(
                        "`{}` declares `boot \"{}\"` but no ungated module provides `{capability}`",
                        image.id, image.boot
                    ),
                    &image.src,
                )
                .at(image.span, "incomplete UKI chain")
                .help(satisfied_by(index, image, capability)),
            );
        }
    }

    for module in image.modules() {
        for decl in &module.requires {
            if base_caps.contains_key(decl.name.as_str()) {
                continue;
            }
            if !offered.contains_key(decl.name.as_str())
                && image
                    .modules()
                    .any(|declaring| declaring.refused_keys.contains(&decl.name))
            {
                continue;
            }

            let Some(providers) = offered.get(decl.name.as_str()) else {
                issues.push(
                    Issue::new(
                        format!(
                            "`{}` requires `{}`, which nothing enabled provides",
                            module.path, decl.name
                        ),
                        &module.src,
                    )
                    .at(decl.span, "unsatisfied")
                    .help(satisfied_by(index, image, &decl.name)),
                );
                continue;
            };

            if let Some(gated) = elsewhere(module, providers) {
                issues.push(
                    Issue::new(
                        format!(
                            "`{}` requires `{}`, which only {gated} provides",
                            module.path, decl.name
                        ),
                        &module.src,
                    )
                    .at(decl.span, "unsatisfied on every other target")
                    .help("either gate this module to the same flavour, or move the provider out of the flavour block"),
                );
            }
        }
    }

    for module in image.modules() {
        for decl in &module.after {
            if base_caps.contains_key(decl.name.as_str()) {
                continue;
            }
            let Some(providers) = offered.get(decl.name.as_str()) else {
                continue;
            };
            if let Some(gated) = elsewhere(module, providers) {
                issues.push(
                    Issue::new(
                        format!(
                            "`{}` builds after `{}`, which only {gated} provides",
                            module.path, decl.name
                        ),
                        &module.src,
                    )
                    .at(decl.span, "ineffective on every other target")
                    .help("either gate this module to the same flavour, or move the provider out of the flavour block"),
                );
            }
        }
    }
}

pub fn check_fragments(image: &Image, issues: &mut Issues) {
    let mut gated = false;
    for entry in &image.entries {
        let Some(module) = &entry.module else {
            continue;
        };
        gated |= entry.flavour.is_some();
        let Some(body) = &module.fragment else {
            continue;
        };
        let path = &entry.path;

        let above_gate = !gated && module.fragment_position != Position::Tail;
        if above_gate && (body.contains("${FLAVOUR}") || body.contains("$FLAVOUR")) {
            issues.push(
                Issue::new(
                    format!("`{path}` expands FLAVOUR above the flavour gate"),
                    &image.src,
                )
                .at(entry.span, "listed above the first flavour-gated module")
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
