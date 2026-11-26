use crate::diag::{Issue, Issues};
use crate::model::component::{Capability, Component};
use crate::model::image::{Entry, Image};
use crate::model::version::Version;
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
    let [provider, ..] = providers else {
        return false;
    };
    let Some(first) = provider.keys.iter().find(|key| key.kind == kind) else {
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
        true => "`buildc add component` lists what the declared libraries provide",
        false => "repo.kdl declares no `libraries`, so there is no library to add one from",
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
    let family = image.family();
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
                || image
                    .components()
                    .any(|other| !std::ptr::eq(other, component) && other.answers(&decl.name));
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
    match providers.len() > 1 && providers.iter().any(declares) {
        true => providers.to_vec(),
        false => Vec::new(),
    }
}

/// The user reads the issues in the order that these checks run.
pub fn check_graph(image: &Image, index: &Index, issues: &mut Issues) {
    check_helpers(image, issues);
    check_base_overlap(image, issues);
    check_family(image, issues);
    check_exclusive(image, issues);
    check_base_requires(image, index, issues);
    check_removes(image, issues);
    check_requires(image, index, issues);
    check_versions(image, issues);
}

fn check_helpers(image: &Image, issues: &mut Issues) {
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
}

fn check_base_overlap(image: &Image, issues: &mut Issues) {
    let base_caps: BTreeMap<&str, &Capability> = image
        .base
        .iter()
        .flat_map(|b| b.provides.iter())
        .map(|decl| (decl.name.as_str(), decl))
        .collect();
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
}

fn check_family(image: &Image, issues: &mut Issues) {
    let Some(base_family) = image
        .base
        .as_ref()
        .map(|b| b.family.as_str())
        .filter(|f| !f.is_empty())
    else {
        return;
    };
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
    for component in image.components() {
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
}

fn check_exclusive(image: &Image, issues: &mut Issues) {
    let mut offered: BTreeMap<&str, Vec<&Component>> = BTreeMap::new();
    for component in image.components() {
        for decl in &component.provides {
            offered
                .entry(decl.name.as_str())
                .or_default()
                .push(component);
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
}

fn check_base_requires(image: &Image, index: &Index, issues: &mut Issues) {
    let Some(base) = &image.base else {
        return;
    };
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

fn check_requires(image: &Image, index: &Index, issues: &mut Issues) {
    for component in image.components() {
        for decl in &component.requires {
            if let Some(prefer) = &decl.prefer {
                let held = index.matching(prefer);
                match held.as_slice() {
                    [] => issues.push(
                        Issue::new(
                            format!(
                                "`{}` prefers `{prefer}` for `{}`, which no declared library that buildc read holds",
                                component.path, decl.name
                            ),
                            &component.src,
                        )
                        .at(decl.span, "not held")
                        .help(match index.unsearched() {
                            why if why.is_empty() => "a preference names a component path with an optional library prefix; `buildc add component` lists what the libraries hold".to_string(),
                            why => format!("{why}; a preference names a component path with an optional library prefix"),
                        }),
                    ),
                    [preferred] => {
                        if !preferred
                            .declares
                            .provides
                            .iter()
                            .any(|offered| crate::resolve::meets(offered, &decl.name))
                        {
                            let providers: Vec<String> = index
                                .of(&decl.name)
                                .iter()
                                .map(|provider| format!("`{}`", provider.qualified()))
                                .collect();
                            issues.push(
                                Issue::new(
                                    format!(
                                        "`{}` prefers `{prefer}` for `{}`, and `{prefer}` does not provide it",
                                        component.path, decl.name
                                    ),
                                    &component.src,
                                )
                                .at(decl.span, "does not provide it")
                                .help(match providers.is_empty() {
                                    true => format!(
                                        "nothing in the declared libraries provides `{}`",
                                        decl.name
                                    ),
                                    false => format!("providers: {}", crate::said(&providers)),
                                }),
                            );
                        }
                    }
                    many => {
                        let names: Vec<String> = many
                            .iter()
                            .map(|provider| format!("`{}`", provider.qualified()))
                            .collect();
                        issues.push(
                            Issue::new(
                                format!(
                                    "`{}` prefers `{prefer}` for `{}`, and more than one library holds that path",
                                    component.path, decl.name
                                ),
                                &component.src,
                            )
                            .at(decl.span, "ambiguous")
                            .help(format!("write the library prefix: {}", crate::said(&names))),
                        );
                    }
                }
            }
            if let Some(remover) = image.remover(&decl.name) {
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
            if image.base_meets(&decl.name) {
                continue;
            }
            let providers: Vec<&Component> = answering(image, &decl.name)
                .into_iter()
                .filter(|provider| image.component_meets(provider, &decl.name))
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
        }
    }
}

fn check_versions(image: &Image, issues: &mut Issues) {
    let Some(base) = &image.base else {
        return;
    };
    // A base pinned by digest is checked against the versions that its probe recorded.
    // An unpinned base is measured during the image build instead.
    let pinned = base.image.contains('@');
    for (component, requirement) in base.image_requirements(image.components()) {
        let (owner, src) = component.map_or((&base.image, &base.src), |component| {
            (&component.path, &component.src)
        });
        let Some(min) = &requirement.min else {
            continue;
        };
        let base_offers = base.answers(&requirement.name);
        let providers: Vec<&Component> = image
            .components()
            .filter(|component| component.answers(&requirement.name))
            .collect();
        let mut known: Vec<(&str, &Version)> = Vec::new();
        let mut measurable = false;
        if base_offers {
            for decl in base
                .provides
                .iter()
                .filter(|decl| crate::resolve::meets(&decl.name, &requirement.name))
            {
                if pinned {
                    if let Some(version) = &decl.version {
                        known.push((&base.path, version));
                    }
                } else if decl.package.is_some() {
                    measurable = true;
                }
            }
        }
        for component in &providers {
            for decl in component
                .provides
                .iter()
                .filter(|decl| crate::resolve::meets(&decl.name, &requirement.name))
            {
                if !component.builds_only(&decl.name) && decl.package.is_some() {
                    measurable = true;
                }
            }
        }
        let all_build_only = !providers.is_empty()
            && providers.iter().all(|component| {
                component
                    .provides
                    .iter()
                    .filter(|decl| crate::resolve::meets(&decl.name, &requirement.name))
                    .all(|decl| component.builds_only(&decl.name))
            });
        if measurable || known.iter().any(|(_, version)| *version >= min) {
            continue;
        }
        if let Some((provider, version)) = known.iter().max_by(|one, other| one.1.cmp(other.1)) {
            issues.push(
                Issue::new(
                    format!(
                        "`{owner}` requires `{}` at or above `{min}`. `{provider}` carries `{version}`",
                        requirement.name
                    ),
                    src,
                )
                .at(requirement.span, "below the minimum")
                .help(
                    "pin a newer base that carries the minimum",
                ),
            );
        } else if pinned && base_offers {
            issues.push(
                Issue::new(
                    format!(
                        "`{owner}` requires `{}` at or above `{min}`. The pinned base records no version",
                        requirement.name
                    ),
                    src,
                )
                .at(requirement.span, "no measured version")
                .help(
                    "The base probe records the version of a capability beside its digest. A \
                     pinned base without one cannot be checked.",
                ),
            );
        } else if !base_offers && all_build_only {
            issues.push(
                Issue::new(
                    format!(
                        "`{owner}` requires `{}` at or above `{min}`. Every provider declares it `build-only`",
                        requirement.name
                    ),
                    src,
                )
                .at(requirement.span, "gone from the final image")
                .help(
                    "The capability exists only while the build runs. Its version is not in \
                     the finished image. Drop the floor. Otherwise, provide the capability \
                     from a package that the final image keeps.",
                ),
            );
        } else if base_offers || !providers.is_empty() {
            issues.push(
                Issue::new(
                    format!(
                        "`{owner}` requires `{}` at or above `{min}`. No provider that keeps it in the final image names a package to measure",
                        requirement.name
                    ),
                    src,
                )
                .at(requirement.span, "no package named")
                .help(
                    "A versioned requirement needs a provider that keeps the capability in the \
                     finished image and names the package whose version measures it. A \
                     build-only provider cannot be measured.",
                ),
            );
        }
    }
}
