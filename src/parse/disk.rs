use crate::diag::{Issues, Source};
use crate::layout;
use crate::model::module::Key;
use kdl::KdlDocument;
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Default)]
pub struct Disk {
    pub keys: BTreeMap<String, Vec<(String, Key)>>,
    pub collectors: BTreeMap<String, String>,
    pub overlays: BTreeMap<String, Vec<String>>,
}

impl Disk {
    pub fn modules(&self) -> impl Iterator<Item = &String> {
        self.overlays.keys()
    }

    pub fn scan(root: &Path) -> Self {
        let mut out = Disk::default();

        let modules = layout::modules(root);
        let mut dirs = vec![modules.clone()];
        while let Some(dir) = dirs.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() || path.file_name().is_some_and(|n| n == "_template") {
                    continue;
                }
                let manifest = path.join(layout::MODULE_FILE);
                if !manifest.is_file() {
                    dirs.push(path);
                    continue;
                }
                let name = path
                    .strip_prefix(&modules)
                    .unwrap_or(&path)
                    .display()
                    .to_string();
                out.overlays
                    .insert(name.clone(), overlay_paths(&path.join(layout::OVERLAY)));

                let Ok(text) = std::fs::read_to_string(&manifest) else {
                    continue;
                };
                let Ok(doc) = text.parse::<KdlDocument>() else {
                    continue;
                };
                for node in doc.nodes() {
                    let args = || {
                        node.entries()
                            .iter()
                            .filter(|e| e.name().is_none())
                            .filter_map(|e| e.value().as_string())
                    };
                    match node.name().value() {
                        "collects" => {
                            if let Some(file) = args().next() {
                                out.collectors.insert(file.to_string(), name.clone());
                            }
                        }
                        "key" => {
                            let src = Source::new(manifest.display().to_string(), text.clone());
                            let Some(key) =
                                crate::parse::module::parse_key(node, &src, &mut Issues::default())
                            else {
                                continue;
                            };
                            out.keys
                                .entry(key.kind.clone())
                                .or_default()
                                .push((name.clone(), key));
                        }
                        _ => {}
                    }
                }
            }
        }

        for declaring in out.keys.values_mut() {
            declaring.sort_by(|a, b| a.0.cmp(&b.0));
        }
        out
    }
}

fn overlay_paths(overlay: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut dirs = vec![overlay.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = path.symlink_metadata() else {
                continue;
            };
            if meta.is_dir() {
                dirs.push(path);
            } else if let Ok(rel) = path.strip_prefix(overlay) {
                out.push(format!("/{}", rel.display()));
            }
        }
    }
    out
}
