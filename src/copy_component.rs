use crate::create::Change;
use crate::diag::Span;
use crate::error::Error;
use crate::layout;
use crate::model::image::{Image, List, Setter};
use crate::model::remote::{At, Collection};
use crate::parse::declaration::{carried, Carried};
use crate::provenance::origins;
use common::prompt::Prompt;
use common::ui::Choice;
use std::path::{Path, PathBuf};

pub struct Request {
    pub paths: Vec<String>,
    pub from: Option<String>,
    pub build: Option<String>,
}

struct Dest {
    /// `None` is this repository's own components folder.
    held: Option<String>,
    written: Option<String>,
    components: PathBuf,
}

struct Piece {
    library: String,
    path: String,
    components: PathBuf,
    origin: String,
    pin: Option<String>,
    nested: Vec<String>,
}

impl Piece {
    fn paths(&self) -> impl Iterator<Item = &String> {
        std::iter::once(&self.path).chain(&self.nested)
    }

    fn dir(&self) -> PathBuf {
        self.components.join(&self.path)
    }
}

struct Rewrite {
    file: PathBuf,
    node: Span,
}

pub struct Copy {
    dest: Dest,
    pieces: Vec<Piece>,
    rewrites: Vec<Rewrite>,
}

impl Copy {
    pub fn collect(
        request: &Request,
        root: &Path,
        prompt: &Prompt,
        say: crate::report::Say<'_>,
    ) -> Result<Option<Self>, Error> {
        let loaded = crate::load(root);
        if !loaded.issues.is_empty() {
            return Err(Error::Operation(
                "the builds do not load, so nothing is copied; `buildc check` names the problems"
                    .to_string(),
            ));
        }
        let list = &loaded.list;
        let named_from: Vec<&str> = request
            .from
            .iter()
            .map(String::as_str)
            .chain(request.paths.iter().filter_map(|path| {
                let (first, _) = path.split_once('/')?;
                list.libraries
                    .iter()
                    .any(|library| library.name == first)
                    .then_some(first)
            }))
            .collect();
        let dest = destination(root, list, &named_from, prompt)?;
        let mut sources: Vec<&Collection> = list
            .libraries
            .iter()
            .filter(|library| Some(library.name.as_str()) != dest.held.as_deref())
            .collect();
        if let Some(from) = &request.from {
            sources.retain(|library| library.name == *from);
            if sources.is_empty() {
                return Err(Error::Operation(format!(
                    "`{from}` is no library to copy from; `--from` names a library that repo.kdl declares, other than the one that takes the copy"
                )));
            }
        }
        let build = request
            .build
            .as_deref()
            .map(|name| build_named(list, name))
            .transpose()?;
        let wanted = match (request.paths.as_slice(), build) {
            ([], Some(image)) => read_remote(image, &sources)?,
            ([], None) => chosen(root, &sources, prompt)?,
            (paths, _) => paths
                .iter()
                .map(|path| named(path, request.from.is_some(), &sources, root, prompt))
                .collect::<Result<_, _>>()?,
        };
        let pieces = outermost(
            wanted
                .iter()
                .map(|(library, path)| piece(root, list, library, path, build))
                .collect::<Result<_, _>>()?,
        )?;
        vacant(root, list, &dest, &pieces)?;
        let rewrites = rewrites(root, list, &dest, &pieces)?;

        let mut shown = String::from("Components to be copied:\n");
        for piece in &pieces {
            for path in piece.paths() {
                let to = layout::under(root, &dest.components.join(path));
                shown.push_str(&format!("{}/{path} > ./{}\n", piece.library, to.display()));
            }
        }
        let mut files: Vec<PathBuf> = rewrites
            .iter()
            .map(|rewrite| layout::under(root, &rewrite.file))
            .collect();
        files.sort();
        files.dedup();
        if !files.is_empty() {
            shown.push_str("Component references to update:\n");
            for file in &files {
                shown.push_str(&format!("{}\n", file.display()));
            }
        }
        say(&format!("{shown}\n"));
        if !prompt.confirm_current(crate::copy::COPY_COMPONENTS, "Confirm", "Cancel", true)? {
            return Ok(None);
        }
        Ok(Some(Self {
            dest,
            pieces,
            rewrites,
        }))
    }

    pub fn apply(&self, root: &Path) -> Result<Vec<(PathBuf, Change)>, String> {
        let origins_file = self.dest.components.join(origins::FILE);
        let mut kept: Vec<(PathBuf, Option<String>)> = Vec::new();
        for file in
            std::iter::once(&origins_file).chain(self.rewrites.iter().map(|rewrite| &rewrite.file))
        {
            if file.is_file() && !kept.iter().any(|(held, _)| held == file) {
                let text = std::fs::read_to_string(file)
                    .map_err(|err| format!("{}: {err}", file.display()))?;
                kept.push((file.clone(), Some(text)));
            }
        }
        if !origins_file.is_file() {
            kept.push((origins_file.clone(), None));
        }
        let mut created: Vec<PathBuf> = Vec::new();
        let done = self
            .write(root, &origins_file, &mut created)
            .and_then(|wrote| self.verify(root).map(|()| wrote));
        done.map_err(|err| {
            let failed: Vec<String> = created
                .iter()
                .rev()
                .map(|path| crate::init::remove(path))
                .chain(kept.iter().map(|(file, text)| match text {
                    Some(text) => crate::init::put(file, text),
                    None => crate::init::remove(file),
                }))
                .filter_map(Result::err)
                .collect();
            crate::init::undone(err, &failed)
        })
    }

    fn write(
        &self,
        root: &Path,
        origins_file: &Path,
        created: &mut Vec<PathBuf>,
    ) -> Result<Vec<(PathBuf, Change)>, String> {
        let mut wrote: Vec<(PathBuf, Change)> = Vec::new();
        for piece in &self.pieces {
            let from = piece.dir();
            let to = self.dest.components.join(&piece.path);
            if from.is_dir() {
                created.push(first_missing(&to));
                for under in crate::init::copy_component_tree(&from, &to)? {
                    wrote.push((layout::under(root, &to.join(under)), Change::Created));
                }
            }
            let sibling = layout::sibling(&from);
            if sibling.is_file() {
                let target = layout::sibling(&to);
                created.push(first_missing(&target));
                let text = std::fs::read_to_string(&sibling)
                    .map_err(|err| format!("{}: {err}", sibling.display()))?;
                crate::init::put(&target, &text)?;
                wrote.push((layout::under(root, &target), Change::Created));
            }
        }

        let recorded: Vec<origins::Origin> = self
            .pieces
            .iter()
            .flat_map(|piece| {
                piece.paths().filter_map(|path| {
                    Some(origins::Origin {
                        origin: piece.origin.clone(),
                        path: path.clone(),
                        name: path.clone(),
                        pin: piece.pin.clone(),
                        hash: origins::hash(&piece.components.join(path))?,
                    })
                })
            })
            .collect();
        if !recorded.is_empty() {
            let existed = origins_file.is_file();
            let text = origins::with_at(origins_file, &recorded)?;
            crate::init::put(origins_file, &text)?;
            let names: Vec<String> = recorded
                .iter()
                .map(|copy| format!("`{}`", copy.name))
                .collect();
            wrote.push((
                layout::under(root, origins_file),
                match existed {
                    true => Change::Updated(format!("{} recorded", crate::said(&names))),
                    false => Change::Created,
                },
            ));
        }

        let mut files: Vec<&Path> = self
            .rewrites
            .iter()
            .map(|rewrite| rewrite.file.as_path())
            .collect();
        files.sort();
        files.dedup();
        for file in files {
            let mut text = std::fs::read_to_string(file)
                .map_err(|err| format!("{}: {err}", file.display()))?;
            let mut here: Vec<&Rewrite> = self
                .rewrites
                .iter()
                .filter(|rewrite| rewrite.file == file)
                .collect();
            // Each rewrite runs from the end of the file so that the spans before it stay valid.
            here.sort_by_key(|rewrite| std::cmp::Reverse(rewrite.node.offset));
            for rewrite in here {
                text = crate::parse::declaration::reference(&text, rewrite.node)
                    .map_err(|err| format!("{} {err}", file.display()))?;
            }
            std::fs::write(file, text).map_err(|err| format!("{}: {err}", file.display()))?;
            let shown = layout::under(root, file);
            if !wrote.iter().any(|(path, _)| *path == shown) {
                wrote.push((
                    shown,
                    Change::Updated("its references read the copy".to_string()),
                ));
            }
        }
        Ok(wrote)
    }

    fn verify(&self, root: &Path) -> Result<(), String> {
        let loaded = crate::load(root);
        if !loaded.issues.is_empty() {
            return Err(format!(
                "the copy is undone, because the builds no longer load with it:\n\n{}",
                loaded.issues.plain()
            ));
        }
        let local = Local::of(root, &loaded.list);
        for image in &loaded.list.images {
            for entry in &image.entries {
                let copied = self
                    .pieces
                    .iter()
                    .any(|piece| piece.paths().any(|path| *path == entry.path));
                let referenced = entry
                    .setters
                    .iter()
                    .any(|setter| local.holds(Path::new(setter.src.name())));
                if copied && referenced && entry.library() != self.dest.held.as_deref() {
                    return Err(format!(
                        "the copy is undone, because `{}` still reads `{}` from {}",
                        image.id,
                        entry.path,
                        crate::resolve::library::shown(entry.library())
                    ));
                }
            }
        }
        Ok(())
    }
}

fn destination(root: &Path, list: &List, from: &[&str], prompt: &Prompt) -> Result<Dest, Error> {
    let mut all: Vec<Dest> = Vec::new();
    if list.resolver.reads_own() {
        all.push(Dest {
            held: None,
            written: list.root_library.clone(),
            components: layout::components(root),
        });
    }
    for library in &list.libraries {
        if let At::Dir(dir) = &library.at {
            all.push(Dest {
                held: Some(library.name.clone()),
                written: Some(library.name.clone()),
                components: layout::library_components(&root.join(dir)),
            });
        }
    }
    all.retain(|dest| {
        !dest
            .written
            .as_deref()
            .is_some_and(|name| from.contains(&name))
    });
    if all.is_empty() {
        let other = match from.is_empty() {
            true => String::new(),
            false => format!(" other than `{}`", from.join("`, `")),
        };
        return Err(Error::Operation(format!(
            "`libraries` declares no `local` library{other} to take the copy; add one with `buildc set library`"
        )));
    }
    if all.len() == 1 {
        return Ok(all.remove(0));
    }
    let options: Vec<Choice> = all
        .iter()
        .map(|dest| {
            Choice::new(
                dest.written.as_deref().unwrap_or("this repository"),
                layout::under(root, &dest.components).display().to_string(),
            )
        })
        .collect();
    match prompt.choose(crate::copy::COPY_INTO, &options)? {
        Some(at) => Ok(all.swap_remove(at)),
        None => Err(Error::Operation(
            "`libraries` declares more than one `local` library; run `buildc copy component` in a terminal to choose the one that takes the copy"
                .to_string(),
        )),
    }
}

fn build_named<'a>(list: &'a List, name: &str) -> Result<&'a Image, Error> {
    list.images
        .iter()
        .find(|image| image.id == name)
        .ok_or_else(|| Error::Operation(format!("`builds` in repo.kdl names no build `{name}`")))
}

fn shelf(root: &Path, library: &Collection) -> Option<(PathBuf, PathBuf)> {
    let tree = crate::collection::cached(root, library)?;
    let components = layout::library_components(&tree);
    Some((tree, components))
}

fn contained_shelf(root: &Path, library: &Collection) -> Option<PathBuf> {
    let (tree, components) = shelf(root, library)?;
    let real = components.canonicalize().ok()?;
    real.starts_with(tree.canonicalize().ok()?)
        .then_some(components)
}

fn read_remote<'a>(
    image: &Image,
    sources: &[&'a Collection],
) -> Result<Vec<(&'a Collection, String)>, Error> {
    let out: Vec<(&Collection, String)> = image
        .entries
        .iter()
        .filter(|entry| {
            entry.inline.is_none()
                && entry
                    .held
                    .as_ref()
                    .is_some_and(|held| held.remote.is_some())
        })
        .filter_map(|entry| {
            let library = sources
                .iter()
                .find(|library| Some(library.name.as_str()) == entry.library())?;
            Some((*library, entry.path.clone()))
        })
        .collect();
    match out.is_empty() {
        true => Err(Error::Operation(format!(
            "`{}` reads no remote component",
            image.id
        ))),
        false => Ok(out),
    }
}

fn chosen<'a>(
    root: &Path,
    sources: &[&'a Collection],
    prompt: &Prompt,
) -> Result<Vec<(&'a Collection, String)>, Error> {
    let mut offered: Vec<(&Collection, String)> = Vec::new();
    for library in sources {
        let Some(components) = contained_shelf(root, library) else {
            continue;
        };
        for dir in layout::component_dirs(&components, layout::is_component, |name| {
            name.starts_with('.')
        }) {
            let Ok(relative) = dir.strip_prefix(&components) else {
                continue;
            };
            let path = relative.to_string_lossy().replace('\\', "/");
            if crate::model::image::is_local_path(&path) {
                offered.push((*library, path));
            }
        }
    }
    let options: Vec<Choice> = offered
        .iter()
        .map(|(library, path)| Choice::new(format!("{}/{path}", library.name), ""))
        .collect();
    let picked = match prompt.choose_many(crate::copy::WHICH_COMPONENTS, &options, &[])? {
        common::ui::Answer::Chosen(picked) => picked,
        common::ui::Answer::Cancelled => Vec::new(),
    };
    match picked.is_empty() {
        true => Err(Error::Operation(
            "no component chosen; `buildc copy component <library>/<path>` names one".to_string(),
        )),
        false => Ok(picked.into_iter().map(|at| offered[at].clone()).collect()),
    }
}

fn named<'a>(
    arg: &str,
    from: bool,
    sources: &[&'a Collection],
    root: &Path,
    prompt: &Prompt,
) -> Result<(&'a Collection, String), Error> {
    if !from {
        let prefixed = arg.split_once('/').and_then(|(first, rest)| {
            let library = sources.iter().find(|library| library.name == first)?;
            Some((*library, rest.to_string()))
        });
        if let Some(prefixed) = prefixed {
            return Ok(prefixed);
        }
    }
    let holders: Vec<&Collection> = sources
        .iter()
        .filter(|library| {
            contained_shelf(root, library)
                .is_some_and(|components| locate(&components, arg) != Located::Absent)
        })
        .copied()
        .collect();
    match holders.as_slice() {
        [only] => Ok((*only, arg.to_string())),
        [] => Err(Error::Operation(format!(
            "no library holds `{arg}`; `buildc copy component <library>/<path>` names the library"
        ))),
        many => {
            let options: Vec<Choice> = many
                .iter()
                .map(|library| Choice::new(format!("{}/{arg}", library.name), ""))
                .collect();
            let names: Vec<String> = many
                .iter()
                .map(|library| format!("`{}`", library.name))
                .collect();
            let at = prompt
                .choose(&crate::copy::which_library(arg), &options)?
                .ok_or_else(|| {
                    format!(
                        "{} each hold `{arg}`; `--from <library>` names one",
                        crate::said(&names)
                    )
                })?;
            Ok((many[at], arg.to_string()))
        }
    }
}

#[derive(PartialEq)]
enum Located {
    Component,
    Inline(String),
    Absent,
}

fn locate(components: &Path, path: &str) -> Located {
    if !crate::model::image::is_local_path(path) {
        return Located::Absent;
    }
    if layout::is_component(&components.join(path)) {
        return Located::Component;
    }
    let mut at = path;
    while let Some((parent, _)) = at.rsplit_once('/') {
        at = parent;
        let dir = components.join(parent);
        if !layout::is_component(&dir) {
            continue;
        }
        let found = crate::parse::local::inline_in(&layout::manifest_at(&dir), parent)
            .into_iter()
            .find(|listed| listed.path == path);
        if let Some(found) = found {
            return Located::Inline(found.held_by);
        }
    }
    Located::Absent
}

fn piece(
    root: &Path,
    list: &List,
    library: &Collection,
    path: &str,
    build: Option<&Image>,
) -> Result<Piece, Error> {
    let reads: Vec<(&str, &crate::model::image::Held)> = list
        .images
        .iter()
        .flat_map(|image| {
            image
                .entries
                .iter()
                .filter(|entry| {
                    entry.path == path
                        && entry.inline.is_none()
                        && entry.library() == Some(library.name.as_str())
                })
                .filter_map(move |entry| Some((image.id.as_str(), entry.held.as_ref()?)))
        })
        .collect();
    let reads: Vec<(&str, &crate::model::image::Held)> = match build {
        Some(build) if reads.iter().any(|(id, _)| *id == build.id) => reads
            .into_iter()
            .filter(|(id, _)| *id == build.id)
            .collect(),
        _ => reads,
    };
    let mut folders: Vec<&PathBuf> = reads.iter().map(|(_, held)| &held.components).collect();
    folders.sort();
    folders.dedup();
    let (tree, components, origin, pin) = match (folders.as_slice(), reads.first()) {
        ([], _) | (_, None) => {
            let (tree, components) = shelf(root, library).ok_or_else(|| {
                format!(
                    "the cache holds no copy of library `{}`; `buildc generate` fetches it",
                    library.name
                )
            })?;
            let (origin, pin) = origins::source(root, library);
            (tree, components, origin, pin)
        }
        ([_], Some((_, held))) => match &held.remote {
            Some(revision) => (
                held.tree.clone(),
                held.components.clone(),
                revision.source.clone(),
                Some(revision.commit.clone()),
            ),
            None => {
                let (origin, pin) = origins::source(root, library);
                (held.tree.clone(), held.components.clone(), origin, pin)
            }
        },
        (_, Some(_)) => {
            let at: Vec<String> = reads
                .iter()
                .map(|(id, held)| {
                    let commit = held
                        .remote
                        .as_ref()
                        .map_or("", |revision| revision.commit.as_str());
                    format!("`{id}` at {commit}")
                })
                .collect();
            return Err(Error::Operation(format!(
                "the builds read `{path}` at different commits, {}; `--build <name>` names the build whose commit to copy",
                crate::said(&at)
            )));
        }
    };
    match locate(&components, path) {
        Located::Component => {}
        Located::Inline(definer) => {
            return Err(Error::Operation(format!(
                "`{path}` is defined inline in `{definer}` of library `{}`; copy `{definer}`, which holds it",
                library.name
            )))
        }
        Located::Absent => {
            return Err(Error::Operation(format!(
                "library `{}` holds no component `{path}`",
                library.name
            )))
        }
    }
    // A link in a remote library could point out of its tree and bring a host file into the copy.
    layout::contained(&tree, &components.join(path), &library.name, path)?;
    let nested = layout::held_components(&components.join(path))
        .iter()
        .map(|relative| format!("{path}/{}", relative.to_string_lossy().replace('\\', "/")))
        .collect();
    Ok(Piece {
        library: library.name.clone(),
        path: path.to_string(),
        components,
        origin,
        pin,
        nested,
    })
}

fn outermost(mut pieces: Vec<Piece>) -> Result<Vec<Piece>, Error> {
    // A parent path sorts before each path below it and reaches the output first.
    pieces.sort_by(|a, b| a.path.cmp(&b.path));
    let mut out: Vec<Piece> = Vec::new();
    for piece in pieces {
        let Some(held) = out
            .iter()
            .find(|held| held.paths().any(|path| *path == piece.path))
        else {
            out.push(piece);
            continue;
        };
        if held.library != piece.library {
            return Err(Error::Operation(format!(
                "`{}` is asked for from `{}` and from `{}`; one path holds one copy",
                piece.path, held.library, piece.library
            )));
        }
    }
    Ok(out)
}

fn vacant(root: &Path, list: &List, dest: &Dest, pieces: &[Piece]) -> Result<(), Error> {
    let inline: Vec<crate::parse::local::Listed> = match dest.held.is_none() {
        true => crate::parse::local::all(root, list)
            .into_iter()
            .filter(|listed| matches!(listed.place, crate::parse::local::Place::Inline { .. }))
            .collect(),
        false => Vec::new(),
    };
    for piece in pieces {
        let at = dest.components.join(&piece.path);
        for taken in [&at, &layout::sibling(&at)] {
            if taken.exists() {
                return Err(Error::Operation(format!(
                    "`{}` already exists, so `{}` is not copied",
                    layout::under(root, taken).display(),
                    piece.path
                )));
            }
        }
        for path in piece.paths() {
            if let Some(listed) = inline.iter().find(|listed| listed.path == *path) {
                return Err(Error::Operation(format!(
                    "`{}` defines `{path}` inline, so `{}` is not copied",
                    listed.held_by, piece.path
                )));
            }
        }
    }
    Ok(())
}

struct Local {
    trees: Vec<PathBuf>,
    cache: PathBuf,
}

impl Local {
    fn of(root: &Path, list: &List) -> Self {
        Self {
            trees: std::iter::once(root.to_path_buf())
                .chain(
                    list.libraries
                        .iter()
                        .filter_map(|library| match &library.at {
                            At::Dir(dir) => Some(root.join(dir)),
                            At::Git(_) => None,
                        }),
                )
                .collect(),
            cache: root.join(".cache"),
        }
    }

    /// The remote library cache sits below the repository root and holds no local file.
    fn holds(&self, file: &Path) -> bool {
        !file.starts_with(&self.cache) && self.trees.iter().any(|tree| file.starts_with(tree))
    }
}

fn moved(file: &Path, pieces: &[Piece], dest: &Dest) -> Option<PathBuf> {
    pieces.iter().find_map(|piece| {
        let dir = piece.dir();
        let to = dest.components.join(&piece.path);
        if file == layout::sibling(&dir) {
            return Some(layout::sibling(&to));
        }
        file.strip_prefix(&dir)
            .ok()
            .map(|relative| to.join(relative))
    })
}

fn rewrites(
    root: &Path,
    list: &List,
    dest: &Dest,
    pieces: &[Piece],
) -> Result<Vec<Rewrite>, Error> {
    let local = Local::of(root, list);
    let mut out: Vec<Rewrite> = Vec::new();
    for image in &list.images {
        for entry in &image.entries {
            let Some(piece) = pieces.iter().find(|piece| {
                entry.library() == Some(piece.library.as_str())
                    && piece.paths().any(|path| *path == entry.path)
            }) else {
                continue;
            };
            let mut edits: Vec<(&Setter, PathBuf)> = Vec::new();
            let mut stays: Option<&Setter> = None;
            for setter in &entry.setters {
                let file = PathBuf::from(setter.src.name());
                match moved(&file, pieces, dest)
                    .or_else(|| local.holds(&file).then(|| file.clone()))
                {
                    Some(placed) => edits.push((setter, placed)),
                    None => stays = stays.or(Some(setter)),
                }
            }
            if edits.is_empty() {
                continue;
            }
            if let Some(setter) = stays {
                let by = setter.by.as_deref().unwrap_or(&image.component);
                return Err(Error::Operation(format!(
                    "`{by}` lists `{}` and stays in `{}`, so `{}` would read `{}` from two libraries; copy `{by}` as well",
                    entry.path, piece.library, image.id, entry.path
                )));
            }
            for (setter, file) in edits {
                let text = setter.src.text();
                match carried(text, setter.span) {
                    Carried::Above { node, line, props } => {
                        return Err(Error::Operation(format!(
                            "{}:{line} sets {props} on `{node}`, which holds for every path below it, so `{}` would not read the copy; move `{}` out of that block, then copy again",
                            layout::under(root, Path::new(setter.src.name())).display(),
                            entry.path,
                            entry.path
                        )));
                    }
                    Carried::Bare => {}
                    Carried::Own => out.push(Rewrite {
                        file,
                        node: setter.span,
                    }),
                }
            }
        }
    }
    Ok(out)
}

fn first_missing(path: &Path) -> PathBuf {
    let mut out = path.to_path_buf();
    while let Some(parent) = out.parent().filter(|parent| !parent.exists()) {
        out = parent.to_path_buf();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn piece(library: &str, path: &str, nested: &[&str]) -> Piece {
        Piece {
            library: library.to_string(),
            path: path.to_string(),
            components: PathBuf::new(),
            origin: String::new(),
            pin: None,
            nested: nested.iter().map(ToString::to_string).collect(),
        }
    }

    #[test]
    fn a_nested_path_joins_its_parent_only_from_the_same_library() {
        let kept = outermost(vec![
            piece("stable", "podman/extra", &[]),
            piece("stable", "podman", &["podman/extra"]),
        ])
        .expect("one library holds both");
        let paths: Vec<&str> = kept.iter().map(|piece| piece.path.as_str()).collect();
        assert_eq!(paths, ["podman"]);
        let refused = outermost(vec![
            piece("other", "podman/extra", &[]),
            piece("stable", "podman", &["podman/extra"]),
        ]);
        assert!(refused.is_err());
    }
}
