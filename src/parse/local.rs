use crate::diag::{Source, Span};
use crate::layout;
use crate::model::image::List;
use kdl::KdlNode;
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct Listed {
    pub path: String,
    /// An empty context is the library root.
    pub context: String,
    pub place: Place,
    pub build: Option<String>,
    /// The component whose file defines this one.
    /// An image file outside every library names its build instead.
    pub held_by: String,
    pub hidden: bool,
}

#[derive(Clone)]
pub enum Place {
    Directory {
        file: PathBuf,
    },
    Inline {
        file: PathBuf,
        src: Source,
        spans: Vec<Span>,
        node: Span,
    },
}

impl Place {
    pub fn file(&self) -> &Path {
        match self {
            Self::Directory { file } | Self::Inline { file, .. } => file,
        }
    }
}

pub fn all(root: &Path, list: &List) -> Vec<Listed> {
    let components = layout::components(root);
    let mut folders = vec![components.clone()];
    if let Ok(home) = list.resolver.home() {
        if home != components {
            // The default library's folder comes first so that its component wins a path that both folders hold.
            folders.insert(0, home);
        }
    }
    let mut out: Vec<Listed> = Vec::new();
    let dirs = folders.iter().flat_map(|components| {
        layout::component_dirs(components, layout::is_component, layout::is_dot)
            .into_iter()
            .map(move |dir| (components, dir))
    });
    for (components, dir) in dirs {
        let Ok(relative) = dir.strip_prefix(components) else {
            continue;
        };
        let path = relative.to_string_lossy().replace('\\', "/");
        if !crate::model::image::is_local_path(&path) || out.iter().any(|held| held.path == path) {
            continue;
        }
        let file = layout::manifest_at(&dir);
        let build = build_of(list, &path);
        out.push(Listed {
            path: path.clone(),
            context: path.clone(),
            place: Place::Directory { file: file.clone() },
            build: build.clone(),
            held_by: path.clone(),
            hidden: crate::parse::component::hidden(&file),
        });
        definitions(&file, &path, &path, build.as_deref(), &mut out);
    }
    for image in &list.images {
        if !image.outside {
            continue;
        }
        let path = image
            .component
            .strip_prefix("./")
            .unwrap_or(&image.component);
        let file = PathBuf::from(image.src.name());
        out.push(Listed {
            path: path.to_string(),
            context: String::new(),
            place: Place::Directory { file: file.clone() },
            build: Some(image.id.clone()),
            held_by: image.id.clone(),
            hidden: crate::parse::component::hidden(&file),
        });
        definitions(&file, "", path, Some(&image.id), &mut out);
    }
    out.sort_by_cached_key(|listed| {
        (
            listed.build.is_none(),
            listed.path.clone(),
            listed.build.clone(),
        )
    });
    out
}

fn build_of(list: &List, path: &str) -> Option<String> {
    list.builds
        .iter()
        .find(|build| {
            build
                .component
                .strip_prefix("./")
                .unwrap_or(&build.component)
                == path
        })
        .map(|build| build.name.clone())
}

pub fn named<'a>(local: &'a [Listed], path: &str) -> Result<&'a Listed, String> {
    match local
        .iter()
        .filter(|listed| listed.path == path)
        .collect::<Vec<_>>()
        .as_slice()
    {
        [] => Err(format!(
            "`{path}` is not a component this repository holds; `--in` takes a local component \
             and `buildc copy component` makes a local copy of a remote one"
        )),
        [one] => Ok(one),
        _ => Err(format!(
            "`{path}` is defined twice; `buildc check` refuses the pair"
        )),
    }
}

pub fn inline_in(file: &Path, path: &str) -> Vec<Listed> {
    let mut out = Vec::new();
    definitions(file, path, path, None, &mut out);
    out
}

fn definitions(file: &Path, top: &str, held: &str, build: Option<&str>, out: &mut Vec<Listed>) {
    let Ok(text) = std::fs::read_to_string(file) else {
        return;
    };
    let Ok(doc) = text.parse::<kdl::KdlDocument>() else {
        return;
    };
    let src = Source::new(file.display().to_string(), text);
    walk(doc.nodes(), top, held, build, &src, out);
}

fn walk<'a>(
    nodes: impl IntoIterator<Item = &'a KdlNode>,
    parent: &str,
    held: &str,
    build: Option<&str>,
    src: &Source,
    out: &mut Vec<Listed>,
) {
    for node in nodes {
        if crate::parse::tree::is_context(node) {
            let Some(arg) = crate::parse::string_arg(node) else {
                continue;
            };
            let Ok(context) = crate::parse::tree::context(parent, arg) else {
                continue;
            };
            walk(crate::parse::kids(node), &context, held, build, src, out);
            continue;
        }
        if crate::parse::image::is_manifest_node(node.name().value()) {
            continue;
        }
        let path = crate::parse::tree::join(parent, node.name().value());
        let crate::parse::tree::Body { inline, nested, .. } = crate::parse::tree::body(node);
        if let Some(spans) = inline.filter(|_| crate::model::image::is_local_path(&path)) {
            out.push(Listed {
                path: path.clone(),
                context: path.clone(),
                hidden: crate::parse::component::inline_hidden(src, &spans),
                place: Place::Inline {
                    file: PathBuf::from(src.name()),
                    src: src.clone(),
                    spans,
                    node: node.name().span().into(),
                },
                build: build.map(str::to_string),
                held_by: match parent.is_empty() {
                    true => held.to_string(),
                    false => parent.to_string(),
                },
            });
        }
        walk(nested, &path, held, build, src, out);
    }
}
