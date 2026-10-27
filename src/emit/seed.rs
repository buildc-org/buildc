use crate::layout;
use crate::model::image::{Entry, List, SCHEMA_VERSION};
use crate::model::options::Value;
use crate::model::remote::{At, Collection, Kind as SourceKind};
use crate::provenance::Tracker;
use std::fmt::Write as _;
use std::path::PathBuf;

pub fn file(list: &List) -> Option<(PathBuf, String)> {
    let seed = list.seed.as_ref()?;
    let image = list.images.iter().find(|i| i.id == seed.image)?;
    let base = image.base.as_ref()?;

    let components: Vec<(String, &Entry)> = image
        .entries
        .iter()
        .map(|entry| (entry.qualified(&seed.collection), entry))
        .collect();

    let qualified = match &base.imported {
        Some(record) => format!("{}/{}", record.library, record.upstream),
        None => format!("{}/{}", seed.collection, base.path),
    };
    let mut out = format!(
        "// GENERATED FILE, do not edit. Produced by `buildc generate` from the\n\
         // {} image definition.\n\n\
         schema-version {SCHEMA_VERSION}\n\n\
         base {qualified:?}",
        image.id
    );
    let set: Vec<String> = image
        .base_entry
        .iter()
        .flat_map(|held| &held.entry.options)
        .map(|(name, values, _)| {
            let values: Vec<String> = values.iter().map(Value::kdl).collect();
            format!("    {name} {}\n", values.join(" "))
        })
        .collect();
    match set.is_empty() {
        true => out.push('\n'),
        false => {
            let _ = write!(out, " {{\n{}}}\n", set.concat());
        }
    }

    let owners: Vec<&str> = components
        .iter()
        .map(|(name, _)| name.as_str())
        .chain(std::iter::once(qualified.as_str()))
        .filter_map(|name| name.split('/').next())
        .collect();
    let sources: Vec<&Collection> = list
        .sources
        .iter()
        .filter(|c| c.kind == SourceKind::Components && owners.contains(&c.name.as_str()))
        .collect();
    if !sources.is_empty() {
        out.push_str("\nsources {\n");
        for collection in sources {
            source(collection, &mut out);
        }
        out.push_str("}\n");
    }

    out.push('\n');
    for (name, entry) in components {
        let _ = write!(out, "component {name:?}");
        for (prop, value) in [("flavour", &entry.flavour), ("variant", &entry.variant)] {
            if let Some(value) = value {
                let _ = write!(out, " {prop}={value:?}");
            }
        }
        out.push('\n');
    }

    Some((PathBuf::from(layout::GENERATED).join("seed.kdl"), out))
}

fn source(collection: &Collection, out: &mut String) {
    let name = &collection.name;
    match &collection.at {
        At::Dir(path) => {
            let _ = writeln!(out, "\x20   components {name:?} {{");
            let _ = writeln!(out, "\x20       dir {path:?}");
            out.push_str("\x20   }\n");
        }
        At::Git(pin) => {
            let _ = writeln!(out, "\x20   components {name:?} {{");
            if let Tracker::Unpinned(why) = &pin.tracker {
                if !why.is_empty() {
                    let _ = writeln!(out, "\x20       unpinned {why:?}");
                }
            }
            let _ = writeln!(
                out,
                "\x20       url {:?}",
                pin.url.clone().unwrap_or_default()
            );
            if let Some(version) = &pin.version {
                let _ = writeln!(out, "\x20       version {version:?}");
            }
            if let Some(sha256) = &pin.sha256 {
                let _ = writeln!(out, "\x20       sha256 {sha256:?}");
            }
            if let Some(path) = &pin.path {
                let _ = writeln!(out, "\x20       path {path:?}");
            }
            out.push_str("\x20   }\n");
        }
    }
}
