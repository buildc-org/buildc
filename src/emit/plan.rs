use crate::model::asset::Asset;
use crate::model::component::Component;
use crate::model::image::{Entry, Image, Inline, List, Target};
use crate::resolve::overlay;
use crate::resolve::workflow::Declared;
use crate::resolve::Resolved;
use common::json::Json;
use std::path::Path;

pub(crate) fn of_target<'a>(list: &'a List, name: &str) -> Option<(&'a Image, Vec<&'a Entry>)> {
    let image = list.images.iter().find(|i| i.id == name)?;
    Some((image, image.entries.iter().collect()))
}

pub const SCHEMA_VERSION: u32 = 1;

pub fn build(root: &Path, list: &List, resolved: &[Resolved], workflows: &[Declared]) -> Json {
    Json::object([
        ("schema_version", Json::number(SCHEMA_VERSION)),
        (
            "default_image",
            Json::optional(list.default_image().map(|i| i.id.clone())),
        ),
        (
            "pr_image",
            Json::optional(list.pr_image().map(|i| i.id.clone())),
        ),
        (
            "default_target",
            Json::optional(list.default_target().map(|t| t.to_string())),
        ),
        (
            "default_published",
            Json::optional(list.default_target().map(|t| t.published())),
        ),
        (
            "pr_target",
            Json::optional(list.pr_target().map(|t| t.to_string())),
        ),
        (
            "pr_published",
            Json::optional(list.pr_target().map(|t| t.published())),
        ),
        ("cache_image", Json::optional(list.cache_image())),
        ("backend", Json::string(list.backend.name())),
        ("manifest_label", Json::Bool(list.manifest_label)),
        ("workflows", workflow_plan(list, workflows)),
        (
            "audit",
            Json::object([("enforce", Json::Bool(list.audit_enforce))]),
        ),
        ("assets", every_asset(list)),
        (
            "images",
            Json::array(
                list.images
                    .iter()
                    .zip(resolved)
                    .map(|(image, resolved)| self::image(root, list, image, resolved)),
            ),
        ),
    ])
}

fn image(root: &Path, list: &List, image: &Image, resolved: &Resolved) -> Json {
    Json::object([
        ("id", Json::string(&image.id)),
        ("file", Json::string(&image.file)),
        ("description", Json::string(&image.description)),
        ("keywords", Json::strings(image.keywords.iter().cloned())),
        ("logo_url", Json::string(&image.logo_url)),
        ("conforms", Json::string(&image.conforms)),
        ("uki", Json::Bool(image.seals_uki())),
        ("allows", refusals(&image.allows)),
        (
            "layout",
            match &image.layout {
                None => Json::Null,
                Some(layout) => Json::object([
                    ("filesystem", Json::string(&layout.filesystem)),
                    ("bootloader", Json::string(&layout.bootloader)),
                ]),
            },
        ),
        (
            "base",
            match &image.base {
                None => Json::Null,
                Some(base) => Json::object([
                    ("path", Json::string(&base.path)),
                    (
                        "defined",
                        match image
                            .base_entry
                            .as_ref()
                            .and_then(|held| held.entry.inline.as_ref())
                        {
                            Some(inline) => defined(root, inline),
                            None => Json::Null,
                        },
                    ),
                    ("image", Json::string(&base.image)),
                    ("family", Json::string(&base.family)),
                    ("scap_content", Json::string(&base.scap_content)),
                    (
                        "scap_pin",
                        base.scap_pin.as_ref().map_or(Json::Null, |pin| pin.json()),
                    ),
                    ("signed", Json::Bool(base.signed)),
                    (
                        "provides",
                        Json::strings(base.shipped().map(|decl| decl.name.clone())),
                    ),
                    ("witnesses", witnesses(image)),
                    (
                        "requires",
                        Json::strings(base.requires.iter().map(|d| d.name.clone())),
                    ),
                    (
                        "satisfies",
                        Json::array(base.satisfies.iter().map(|coverage| {
                            Json::object([
                                ("benchmark", Json::string(&coverage.benchmark)),
                                ("rules", Json::strings(&coverage.rules)),
                            ])
                        })),
                    ),
                ]),
            },
        ),
        (
            "targets",
            Json::array(
                list.targets()
                    .iter()
                    .filter(|t| t.image == image.id)
                    .map(|t| target(root, list, image, resolved, t)),
            ),
        ),
    ])
}

fn target(root: &Path, list: &List, image: &Image, resolved: &Resolved, target: &Target) -> Json {
    let family = image.family();
    let entries: Vec<&Entry> = image.entries.iter().collect();
    let components: Vec<&Component> = entries
        .iter()
        .filter_map(|e| e.component.as_ref())
        .collect();

    Json::object([
        ("name", Json::string(target.to_string())),
        ("image", Json::string(&target.image)),
        ("published", Json::string(target.published())),
        ("reference", Json::optional(target.reference())),
        ("conforms", Json::string(&image.conforms)),
        ("allows", refusals(&image.allows)),
        (
            "default",
            Json::Bool(
                list.default_target()
                    .is_some_and(|d| d.to_string() == target.to_string()),
            ),
        ),
        (
            "pr",
            Json::Bool(
                list.pr_target()
                    .is_some_and(|p| p.to_string() == target.to_string()),
            ),
        ),
        (
            "components",
            Json::array(
                entries
                    .iter()
                    .map(|entry| self::component(root, entry, family, image, &entries, resolved)),
            ),
        ),
        (
            "suppressed",
            Json::array(
                image
                    .suppressed
                    .iter()
                    .map(|entry| self::component(root, entry, family, image, &entries, resolved)),
            ),
        ),
        (
            "secrets",
            unique(&components, |m| {
                m.secrets.iter().map(|d| d.name.clone()).collect()
            }),
        ),
        (
            "contract_files",
            Json::strings(contract_files(image, &components)),
        ),
        (
            "verify_exceptions",
            Json::array(
                verify_exceptions(&components)
                    .into_iter()
                    .map(|(class, unit)| {
                        Json::object([("class", Json::string(class)), ("unit", Json::string(unit))])
                    }),
            ),
        ),
        ("assets", assets(&components)),
        ("overlay_files", overlay_files(image, &resolved.shipped)),
        ("overlay_overridden", overridden(image, &resolved.shipped)),
        (
            "collected_files",
            Json::map(entries.iter().filter_map(|entry| {
                resolved
                    .collected
                    .by_component
                    .get(&entry.path)
                    .map(|staged| {
                        (
                            entry.path.clone(),
                            Json::array(staged.iter().map(|c| {
                                Json::object([
                                    ("file", Json::string(&c.file)),
                                    ("staged", Json::string(&c.staged)),
                                ])
                            })),
                        )
                    })
            })),
        ),
        (
            "collect_destinations",
            Json::strings(resolved.collected.destinations.clone()),
        ),
        (
            "datastream",
            match (image.conforms.is_empty(), &resolved.scap) {
                (false, Some(measured)) => Json::string(
                    crate::scap::pinned::dir(image)
                        .join(&measured.file)
                        .display()
                        .to_string(),
                ),
                (true, _) | (false, None) => Json::Null,
            },
        ),
        (
            "build",
            build_facts(list, image, resolved, target, &entries),
        ),
    ])
}

fn build_facts(
    list: &List,
    image: &Image,
    resolved: &Resolved,
    target: &Target,
    entries: &[&Entry],
) -> Json {
    let components: Vec<&Component> = entries
        .iter()
        .filter_map(|e| e.component.as_ref())
        .collect();
    let listed = |path: &str| components.iter().any(|component| component.path == path);
    let clones: Vec<&crate::provenance::build::Cloned> = resolved
        .clones
        .iter()
        .filter(|cloned| listed(&cloned.component))
        .collect();

    let mut args = vec![
        format!("TARGET={target}"),
        format!(
            "COMPONENT_HASHES={}",
            components
                .iter()
                .filter_map(|m| m.content.as_ref().map(|c| format!("{}|{c}", m.path)))
                .collect::<Vec<_>>()
                .join(" ")
        ),
        format!(
            "ASSET_RESOLUTIONS={}",
            clones
                .iter()
                .filter_map(|cloned| cloned.resolved.as_deref().ok())
                .collect::<Vec<_>>()
                .join(" ")
        ),
    ];
    if list.audit_enforce {
        args.push("AUDIT_ENFORCE=true".to_string());
    }
    let conforms = crate::scap::profile_name(&image.conforms);
    if !image.conforms.is_empty() {
        let content = match (conforms.is_empty(), &resolved.scap) {
            (false, Some(measured)) => format!("{}/{}", crate::scap::pinned::MOUNT, measured.file),
            (true, _) | (false, None) => String::new(),
        };
        args.push(format!("CONFORMS={conforms}"));
        args.push(format!("SCAP_CONTENT={content}"));
    }
    let (claimed, refused) = crate::scap::exclusions(image);
    if !claimed.is_empty() || !refused.is_empty() {
        let claimed = resolved
            .scap
            .as_ref()
            .map(|measured| measured.claimed.join(" "))
            .unwrap_or_default();
        args.push(format!("SCAP_CLAIMED={claimed}"));
        args.push(format!("SCAP_REFUSED={}", refused.join(" ")));
    }
    if let Some(base) = &image.base {
        args.push(format!("BASE_DECLARED={}", base.image));
    }

    let mut validate = Vec::new();
    let contracts = contract_files(image, &components);
    if !contracts.is_empty() {
        validate.push(format!("CONTRACT_FILES={}", contracts.join(" ")));
    }
    let exceptions: Vec<String> = verify_exceptions(&components)
        .iter()
        .map(|(class, unit)| format!("{class}|{unit}"))
        .collect();
    if !exceptions.is_empty() {
        validate.push(format!("VERIFY_EXCEPTIONS={}", exceptions.join(" ")));
    }
    let presets = preset_files(&resolved.shipped);
    if !presets.is_empty() {
        validate.push(format!("COMPONENT_PRESETS={}", presets.join(" ")));
    }
    if provides(image, entries, crate::emit::recipe::LUKS_INITRAMFS) {
        validate.push("LUKS_INITRAMFS=true".to_string());
    }
    if !image.is_bootc() {
        validate.push("BOOTC=false".to_string());
    }
    let versioned = crate::emit::measure::versioned(&components);
    if !versioned.is_empty() {
        validate.push(format!("VERSIONED={}", versioned.join(" ")));
    }
    let offered = crate::emit::measure::provided(&components);
    let floors = crate::emit::measure::required(image, &components, &offered);
    if !floors.is_empty() {
        validate.push(format!("FLOORS={}", floors.join(" ")));
    }

    let facts = vec![
        (
            "containerfile",
            Json::string(
                crate::emit::containerfile::path(image)
                    .display()
                    .to_string(),
            ),
        ),
        ("uki", Json::Bool(image.seals_uki())),
        ("backend", Json::string(list.backend.name())),
        ("family", Json::string(image.family())),
        (
            "base",
            Json::optional(image.base.as_ref().map(|base| base.image.clone())),
        ),
        ("args", Json::strings(args)),
        (
            "unresolved",
            Json::strings(
                clones
                    .iter()
                    .filter_map(|cloned| cloned.resolved.as_ref().err().cloned()),
            ),
        ),
        ("validate", Json::strings(validate)),
    ];
    Json::object(facts)
}

fn verify_exceptions(components: &[&Component]) -> Vec<(String, String)> {
    unique_pairs(components, |m| {
        m.verify_exceptions
            .iter()
            .map(|e| (e.class.clone(), e.unit.clone()))
            .collect()
    })
}

fn met_by(entry: &Entry, image: &Image, entries: &[&Entry]) -> Json {
    let Some(component) = &entry.component else {
        return Json::map(std::iter::empty::<(String, Json)>());
    };
    Json::map(component.requires.iter().filter_map(|decl| {
        if image.base_answers(&decl.name) {
            return Some((
                decl.name.clone(),
                Json::object([("how", Json::string("base"))]),
            ));
        }
        let provider = entries.iter().find(|other| {
            other
                .component
                .as_ref()
                .is_some_and(|component| component.answers(&decl.name))
        })?;
        Some((
            decl.name.clone(),
            match &provider.imported_by {
                None => Json::object([
                    ("how", Json::string("listed")),
                    ("provider", Json::string(&provider.path)),
                ]),
                Some(by) => Json::object([
                    ("how", Json::string("imported")),
                    ("provider", Json::string(&provider.path)),
                    ("by", Json::string(by)),
                ]),
            },
        ))
    }))
}

fn component(
    root: &Path,
    entry: &Entry,
    family: &str,
    image: &Image,
    entries: &[&Entry],
    resolved: &Resolved,
) -> Json {
    let component = entry.component.as_ref();
    Json::object([
        ("path", Json::string(&entry.path)),
        ("variant", Json::optional(entry.variant.clone())),
        (
            "description",
            Json::string(
                component
                    .map(|m| m.description.as_str())
                    .unwrap_or_default(),
            ),
        ),
        (
            "options",
            Json::map(
                component
                    .map(|m| m.resolved.as_slice())
                    .unwrap_or_default()
                    .iter()
                    .map(|(name, value)| (name.clone(), Json::string(value))),
            ),
        ),
        (
            "provides",
            Json::strings(decls(component, |m| {
                m.provides.iter().map(|decl| decl.name.clone()).collect()
            })),
        ),
        (
            "requires",
            Json::strings(decls(component, |m| {
                m.requires.iter().map(|decl| decl.name.clone()).collect()
            })),
        ),
        ("met_by", met_by(entry, image, entries)),
        (
            "witnessed",
            Json::strings(
                component
                    .map(|m| m.files.as_slice())
                    .unwrap_or_default()
                    .iter()
                    .filter(|file| !file.build_only)
                    .map(|file| file.file.clone()),
            ),
        ),
        (
            "removes",
            Json::strings(decls(component, |m| {
                m.removes.iter().map(|decl| decl.name.clone()).collect()
            })),
        ),
        (
            "files",
            Json::map(
                component
                    .map(|m| m.files.as_slice())
                    .unwrap_or_default()
                    .iter()
                    .map(|f| (f.name.clone(), Json::string(&f.file))),
            ),
        ),
        (
            "packages",
            batches(component.map(|m| m.packages.as_slice()), family),
        ),
        (
            "groups",
            batches(component.map(|m| m.groups.as_slice()), family),
        ),
        ("coprs", coprs(component, family)),
        ("provenance", provenance(root, entry, resolved)),
        (
            "origin",
            match &entry.imported_by {
                None => Json::object([("how", Json::string("listed"))]),
                Some(by) => {
                    Json::object([("how", Json::string("imported")), ("by", Json::string(by))])
                }
            },
        ),
        (
            "satisfies",
            Json::array(
                component
                    .map(|m| m.satisfies.as_slice())
                    .unwrap_or_default()
                    .iter()
                    .map(|coverage| {
                        Json::object([
                            ("benchmark", Json::string(&coverage.benchmark)),
                            ("rules", Json::strings(&coverage.rules)),
                        ])
                    }),
            ),
        ),
        (
            "refuses",
            refusals(component.map(|m| m.refuses.as_slice()).unwrap_or_default()),
        ),
    ])
}

fn refusals(declared: &[crate::model::component::Refusal]) -> Json {
    Json::array(declared.iter().map(|refusal| {
        Json::object([
            ("rule", Json::string(&refusal.rule)),
            ("because", Json::string(&refusal.because)),
        ])
    }))
}

fn batches(declared: Option<&[crate::model::component::PackageGroup]>, family: &str) -> Json {
    Json::array(
        declared
            .unwrap_or_default()
            .iter()
            .filter(|batch| batch.family == family)
            .map(|batch| {
                Json::object([
                    ("family", Json::string(&batch.family)),
                    ("names", Json::strings(batch.packages.iter().cloned())),
                    ("enablerepo", Json::optional(batch.enablerepo.clone())),
                ])
            }),
    )
}

fn coprs(component: Option<&Component>, family: &str) -> Json {
    if family != "fedora" {
        return Json::array(std::iter::empty());
    }
    Json::array(
        component
            .map(|m| m.coprs.as_slice())
            .unwrap_or_default()
            .iter()
            .map(|copr| {
                Json::object([
                    ("id", Json::string(copr.name())),
                    ("url", Json::string(copr.url())),
                ])
            }),
    )
}

fn decls(component: Option<&Component>, of: impl Fn(&Component) -> Vec<String>) -> Vec<String> {
    component.map(of).unwrap_or_default()
}

fn defined(root: &Path, inline: &Inline) -> Json {
    Json::string(format!(
        "./{}",
        crate::layout::under(root, Path::new(inline.src.name())).display()
    ))
}

fn provenance(root: &Path, entry: &Entry, resolved: &Resolved) -> Json {
    let component = entry.component.as_ref();
    Json::object([
        (
            "defined",
            match (&entry.inline, component) {
                (Some(inline), Some(_)) => defined(root, inline),
                _ => Json::Null,
            },
        ),
        (
            "content",
            Json::optional(component.and_then(|m| m.content.clone())),
        ),
        (
            "resolved",
            Json::optional(component.and_then(|m| resolved.hashes.get(&m.path).cloned())),
        ),
        ("repo", Json::Bool(component.is_some_and(|m| m.repo))),
        (
            "imported",
            match component.and_then(|m| m.imported.as_ref()) {
                None => Json::Null,
                Some(copy) => Json::object([
                    ("origin", Json::string(&copy.origin)),
                    ("path", Json::string(&copy.path)),
                    ("content", Json::string(&copy.hash)),
                    ("pin", Json::optional(copy.pin.clone())),
                ]),
            },
        ),
    ])
}

pub(crate) fn contract_files(image: &Image, components: &[&Component]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut add = |paths: Vec<String>, removed: bool| {
        let token = match removed {
            true => format!("!{}", paths.join("|")),
            false => paths.join("|"),
        };
        if !out.contains(&token) {
            out.push(token);
        }
    };
    let removed = |name: &str| components.iter().any(|component| component.withdraws(name));
    let base = image.base.iter().flat_map(|base| &base.files);
    for file in base.chain(components.iter().flat_map(|component| &component.files)) {
        if !file.build_only {
            add(vec![file.file.clone()], removed(&file.name));
        }
    }
    out
}

// A withdrawal wins over the base and over every provider. A build-only witness is gone from the finished image and cannot answer there.
pub(crate) fn provides(image: &Image, entries: &[&Entry], name: &str) -> bool {
    let mut answered = image.base_answers(name);
    for component in entries.iter().filter_map(|entry| entry.component.as_ref()) {
        if component.withdraws(name) {
            return false;
        }
        answered |= component.answers(name);
    }
    answered
}

fn witnesses(image: &Image) -> Json {
    let brought = brought(image);
    let stands_in = |component: &&crate::model::component::Component| {
        brought.contains(component.path.as_str())
            || image.base.iter().any(|base| {
                base.requires.iter().any(|decl| {
                    crate::resolve::answers(
                        &component.path,
                        component.provides.iter().map(|has| has.name.as_str()),
                        &decl.name,
                    )
                })
            })
    };
    let components = image
        .entries
        .iter()
        .chain(&image.suppressed)
        .filter_map(|entry| entry.component.as_ref())
        .filter(|component| !stands_in(component));
    let files = image
        .base
        .iter()
        .flat_map(|base| &base.files)
        .chain(components.flat_map(|component| &component.files));
    let mut paths: std::collections::BTreeMap<&str, Vec<String>> = Default::default();
    for file in files {
        if file.build_only {
            continue;
        }
        let held = paths.entry(&file.name).or_default();
        if !held.contains(&file.file) {
            held.push(file.file.clone());
        }
    }
    Json::map(
        paths
            .into_iter()
            .map(|(name, paths)| (name.to_string(), Json::strings(paths))),
    )
}

fn brought(image: &Image) -> std::collections::BTreeSet<&str> {
    let mut held = std::collections::BTreeSet::new();
    let mut todo: Vec<&str> = image
        .base
        .iter()
        .flat_map(|base| &base.imports)
        .map(|import| import.path.as_str())
        .collect();
    while let Some(path) = todo.pop() {
        if !held.insert(path) {
            continue;
        }
        let component = image
            .entries
            .iter()
            .chain(&image.suppressed)
            .find(|entry| entry.path == path)
            .and_then(|entry| entry.component.as_ref());
        if let Some(component) = component {
            todo.extend(component.imports.iter().map(|import| import.path.as_str()));
        }
    }
    held
}

fn overlay_files(image: &Image, shipped: &overlay::Index) -> Json {
    Json::map(shipped.iter().filter_map(|(path, owners)| {
        owners
            .last()
            .map(|&owner| (path.clone(), Json::string(&image.entries[owner].path)))
    }))
}

fn overridden(image: &Image, shipped: &overlay::Index) -> Json {
    Json::array(
        overrides(image, shipped)
            .into_iter()
            .map(|(path, component, by)| {
                Json::object([
                    ("path", Json::string(path)),
                    ("component", Json::string(component)),
                    ("by", Json::string(by)),
                ])
            }),
    )
}

pub(crate) fn overrides(image: &Image, shipped: &overlay::Index) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    for (path, owners) in shipped {
        let Some((&winner, losers)) = owners.split_last() else {
            continue;
        };
        for &loser in losers {
            out.push((
                path.clone(),
                image.entries[loser].path.clone(),
                image.entries[winner].path.clone(),
            ));
        }
    }
    out
}

pub(crate) fn preset_files(shipped: &overlay::Index) -> Vec<String> {
    shipped
        .iter()
        .filter(|(path, owners)| is_preset(path) && !owners.is_empty())
        .map(|(path, _)| path.clone())
        .collect()
}

fn is_preset(path: &str) -> bool {
    let Some(name) = path
        .strip_prefix("/usr/lib/systemd/system-preset/")
        .or_else(|| path.strip_prefix("/usr/lib/systemd/user-preset/"))
    else {
        return false;
    };
    name.starts_with(crate::runtime::COMPONENT_PRESET) && name.ends_with(".preset")
}

pub(crate) fn pinned<'a>(components: &[&'a Component]) -> Vec<(&'a Component, &'a Asset)> {
    let mut seen: Vec<(&str, &str)> = Vec::new();
    let mut out = Vec::new();
    for component in components {
        for asset in &component.assets {
            if seen.contains(&(component.path.as_str(), asset.name.as_str())) {
                continue;
            }
            seen.push((component.path.as_str(), asset.name.as_str()));
            out.push((*component, asset));
        }
    }
    out
}

fn assets(components: &[&Component]) -> Json {
    Json::array(pinned(components).into_iter().map(|(component, asset)| {
        Json::object([
            ("component", Json::string(&component.path)),
            ("name", Json::string(&asset.name)),
            (
                "manifest",
                Json::string(format!("components/{}/manifest.kdl", component.dir)),
            ),
            ("pin", asset.pin.json()),
        ])
    }))
}

fn every_asset(list: &List) -> Json {
    let components: Vec<&Component> = list.images.iter().flat_map(Image::components).collect();
    assets(&components)
}

fn unique(components: &[&Component], of: impl Fn(&Component) -> Vec<String>) -> Json {
    Json::strings(distinct(
        components.iter().flat_map(|component| of(component)),
    ))
}

fn unique_pairs(
    components: &[&Component],
    of: impl Fn(&Component) -> Vec<(String, String)>,
) -> Vec<(String, String)> {
    distinct(components.iter().flat_map(|component| of(component)))
}

pub(crate) fn distinct<T: PartialEq>(values: impl IntoIterator<Item = T>) -> Vec<T> {
    let mut out: Vec<T> = Vec::new();
    for value in values {
        if !out.contains(&value) {
            out.push(value);
        }
    }
    out
}

fn workflow_plan(list: &List, workflows: &[Declared]) -> Json {
    use crate::model::image::On;
    use crate::resolve::workflow::{builds_on, dispatched_builds, Scope, SHIPPED};
    let triggers = [
        ("push", On::Push),
        ("pr", On::Pr),
        (
            "schedule",
            On::Schedule {
                weekly: None,
                time: None,
            },
        ),
        ("tag", On::Tag(String::new())),
    ];
    Json::object(SHIPPED.iter().filter_map(|shipped| {
        let declared = workflows
            .iter()
            .find(|declared| declared.file == shipped.file())?;
        let builds = match shipped.scope {
            Scope::Repository => Json::Null,
            Scope::Build => Json::object(
                triggers
                    .iter()
                    .map(|(name, on)| (*name, Json::strings(builds_on(list, shipped.stem, on))))
                    .chain([(
                        "dispatch",
                        Json::strings(dispatched_builds(list, shipped.stem)),
                    )]),
            ),
        };
        Some((
            shipped.stem,
            Json::object([
                ("file", Json::string(&declared.file)),
                ("schedule", Json::optional(declared.cron.clone())),
                ("builds", builds),
            ]),
        ))
    }))
}
