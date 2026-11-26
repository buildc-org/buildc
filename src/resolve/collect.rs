use crate::diag::{Issue, Issues};
use crate::layout;
use crate::model::component::Component;
use crate::model::image::Image;
use crate::resolve::library::Collector;
use std::collections::BTreeMap;

pub struct Collected {
    pub file: String,
    pub staged: String,
    pub into: String,
}

#[derive(Default)]
pub struct Collection {
    pub by_component: BTreeMap<String, Vec<Collected>>,
    pub destinations: Vec<String>,
    pub assembled: BTreeMap<String, Vec<String>>,
}

pub fn resolve_collects(
    image: &Image,
    collectors: &BTreeMap<String, Collector>,
    issues: &mut Issues,
) -> Collection {
    let mut by_file: BTreeMap<&str, &Component> = BTreeMap::new();
    for component in image.components() {
        for collect in &component.collects {
            if let Some(first) = by_file.get(collect.file.as_str()) {
                issues.push(
                    Issue::new(
                        format!("two enabled components collect `{}`", collect.file),
                        &component.src,
                    )
                    .at(collect.span, "collected again here")
                    .help(format!("already collected by `{}`", first.path)),
                );
            } else {
                by_file.insert(collect.file.as_str(), component);
            }
        }
    }

    let mut out = Collection {
        destinations: by_file
            .values()
            .flat_map(|collector| collector.collects.iter().map(|c| c.into.clone()))
            .collect(),
        ..Collection::default()
    };
    out.destinations.sort();
    out.destinations.dedup();

    let family = image.family();
    for component in image.components() {
        let mut dirs = Vec::new();
        if let Some(variant) = layout::variant_home(&component.home, component.variant.as_deref()) {
            dirs.push(variant);
        }
        dirs.push(component.home.clone());
        for (file, collector) in collectors {
            let Some((_, shipped)) = layout::shipped_across(&dirs, family, file) else {
                continue;
            };
            match by_file.get(file.as_str()) {
                Some(enabled) => {
                    let declared = enabled.collects.iter().find(|c| &c.file == file);
                    let into = declared.map(|c| c.into.clone()).unwrap_or_default();
                    let priority = component
                        .contributes
                        .iter()
                        .find(|c| &c.file == file)
                        .map(|c| c.priority)
                        .or_else(|| declared.map(|c| c.priority))
                        .unwrap_or_default();
                    if !component.standard_layer {
                        issues.push(
                            Issue::new(
                                format!(
                                    "`{}` ships a {file} with no standard layer to collect it from",
                                    component.path
                                ),
                                &component.src,
                            )
                            .help(format!(
                                "`standard-layer #false` makes the fragment the whole layer, so it has to append the file to {into} itself"
                            )),
                        );
                        continue;
                    }
                    out.by_component
                        .entry(component.path.clone())
                        .or_default()
                        .push(Collected {
                            file: shipped,
                            staged: staged(&into, priority, &component.path),
                            into,
                        });
                }
                None if image
                    .components()
                    .any(|enabled| enabled.home == collector.home) => {}
                None => issues.push(
                    Issue::new(
                        format!(
                            "`{}` ships a {file} but nothing enabled collects it",
                            component.path
                        ),
                        &component.src,
                    )
                    .help(format!(
                        "`{}` collects it; add it to this image, or drop the {file}",
                        collector.path
                    )),
                ),
            }
        }
    }

    let mut assembled: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for collected in out.by_component.values().flatten() {
        assembled
            .entry(collected.into.clone())
            .or_default()
            .push(collected.staged.clone());
    }
    for parts in assembled.values_mut() {
        parts.sort();
    }
    out.assembled = assembled;
    out
}

fn staged(into: &str, priority: u32, component: &str) -> String {
    format!(
        "{into}.d/{priority:04}-{}.part",
        component.replace('/', "-")
    )
}
