use crate::layout;
use crate::model::image::{Entry, List, SCHEMA_VERSION};
use crate::model::options::Value;
use crate::model::remote::{At, Collection};
use std::fmt::Write as _;
use std::path::PathBuf;

pub fn file(list: &List) -> Option<(PathBuf, String)> {
    let seed = list.seed.as_ref()?;
    let image = list.images.iter().find(|i| i.id == seed.image)?;
    let base = image.base.as_ref()?;

    let components: Vec<(String, &Entry)> = image
        .entries
        .iter()
        // The build's own component is the image. The seed lists only the components it holds.
        .filter(|entry| entry.path != image.component)
        .map(|entry| (entry.qualified(&seed.collection), entry))
        .collect();

    let qualified = match &base.library {
        Some(library) => format!("{library}/{}", base.path),
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
        .flat_map(|held| held.entry.settings())
        .map(|setting| {
            let values: Vec<String> = setting.values.iter().map(Value::kdl).collect();
            format!("    {} {}\n", setting.name, values.join(" "))
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
        .libraries
        .iter()
        .filter(|c| owners.contains(&c.name.as_str()))
        .collect();
    if !sources.is_empty() {
        out.push_str("\nlibraries {\n");
        for (block, remote) in [("local", false), ("remote", true)] {
            let held: Vec<&&Collection> = sources
                .iter()
                .filter(|collection| collection.pin().is_some() == remote)
                .collect();
            if held.is_empty() {
                continue;
            }
            let _ = writeln!(out, "\x20   {block} {{");
            for collection in held {
                out.push_str(&source(collection));
            }
            out.push_str("\x20   }\n");
        }
        out.push_str("}\n");
    }

    out.push('\n');
    for (name, entry) in components {
        let _ = write!(out, "component {name:?}");
        if let Some(variant) = &entry.variant {
            let _ = write!(out, " variant={variant:?}");
        }
        out.push('\n');
    }

    Some((PathBuf::from(layout::GENERATED).join("seed.kdl"), out))
}

fn source(collection: &Collection) -> String {
    let name = &collection.name;
    match &collection.at {
        At::Dir(path) => format!("\x20       {name} {{ source {path:?} }}\n"),
        At::Git(pin) => {
            let url = pin.url.clone().unwrap_or_default();
            match &pin.version {
                Some(version) => format!(
                    "\x20       {name} {{\n\x20           source {url:?}\n\x20           version {version:?}\n\x20       }}\n"
                ),
                None => format!("\x20       {name} {{ source {url:?} }}\n"),
            }
        }
    }
}
