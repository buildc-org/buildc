use crate::diag::{Issue, Source, Span};
use crate::model::component::{Component, Import};
use crate::model::image::{Entry, Image};
use crate::model::options::Value;
use crate::parse::disk::Disk;
use std::collections::VecDeque;

struct Wanted {
    path: String,
    options: Vec<(String, Vec<Value>)>,
    span: Span,
}

struct Importer {
    by: String,
    src: Source,
    flavour: Option<String>,
    span: Span,
    wanted: Vec<Wanted>,
}

fn wanted(component: &[Import]) -> Vec<Wanted> {
    component
        .iter()
        .map(|import| Wanted {
            path: import.path.clone(),
            options: import.options.clone(),
            span: import.span,
        })
        .collect()
}

pub fn expand(
    image: &mut Image,
    disk: &Disk,
    mut load: impl FnMut(&Entry, &Image) -> Option<Component>,
) -> Vec<Issue> {
    let mut issues: Vec<Issue> = Vec::new();
    let mut queue: VecDeque<usize> = (0..image.entries.len()).collect();
    if let Some(base) = &image.base {
        let importer = Importer {
            by: base.path.clone(),
            src: base.src.clone(),
            flavour: None,
            span: base.span,
            wanted: wanted(&base.imports),
        };
        bring(image, disk, &mut load, importer, &mut queue, &mut issues);
    }
    while let Some(at) = queue.pop_front() {
        let entry = &image.entries[at];
        let Some(component) = &entry.component else {
            continue;
        };
        let importer = Importer {
            by: component.path.clone(),
            src: component.src.clone(),
            flavour: entry.flavour.clone(),
            span: entry.span,
            wanted: wanted(&component.imports),
        };
        bring(image, disk, &mut load, importer, &mut queue, &mut issues);
    }
    issues.extend(cycles(image));
    reorder(image);
    issues
}

fn bring(
    image: &mut Image,
    disk: &Disk,
    load: &mut impl FnMut(&Entry, &Image) -> Option<Component>,
    importer: Importer,
    queue: &mut VecDeque<usize>,
    issues: &mut Vec<Issue>,
) {
    let Importer {
        by,
        src,
        flavour,
        span,
        wanted,
    } = importer;
    let base = image.base.as_ref().map(|base| base.path.clone());
    for want in wanted {
        if base.as_deref() == Some(want.path.as_str()) {
            issues.push(
                Issue::new(format!("`{by}` imports the base `{}`", want.path), &src)
                    .at(want.span, "the image selects its base")
                    .help("drop the import; the image file names the one base it builds on"),
            );
            continue;
        }
        if want.path.is_empty() {
            issues.push(
                Issue::new(
                    format!("`{by}` imports a path that its option leaves empty"),
                    &src,
                )
                .at(want.span, "no component named")
                .help("set the option on the image entry, or give the option a default"),
            );
            continue;
        }
        match image
            .entries
            .iter()
            .position(|entry| entry.path == want.path)
        {
            Some(held) if reaches(&image.entries, held, &by) => {}
            Some(held) => match image.entries[held].imported_by.clone() {
                None => issues.push(
                    Issue::new(
                        format!("`{}` is listed in the image, and `{by}` imports it", want.path),
                        &image.src,
                    )
                    .at(image.entries[held].span, "listed here")
                    .help(format!(
                        "drop this entry: the import brings `{}` into the image, so it has one source",
                        want.path
                    )),
                ),
                Some(held_by) => {
                    if let Some(first) = disagrees(&image.entries[held], &want.options) {
                        issues.push(
                        Issue::new(
                            format!(
                                "`{by}` and `{held_by}` pass different values for option `{first}` of `{}`",
                                want.path
                            ),
                            &src,
                        )
                        .at(want.span, "a second value")
                        .help("one component builds once with one value per option; make the two imports pass the same value"),
                    );
                    }
                    let entry = &mut image.entries[held];
                    if entry.flavour.is_some() && entry.flavour != flavour {
                        entry.flavour = None;
                        if let Some(component) = entry.component.as_mut() {
                            component.flavour = None;
                        }
                        queue.push_back(held);
                    }
                }
            },
            None if !disk.is_component(&want.path) => {
                issues.push(
                    Issue::new(
                        format!(
                            "`{by}` imports `{}`, which is not under components/",
                            want.path
                        ),
                        &src,
                    )
                    .at(want.span, "not in this repository")
                    .help(format!(
                        "`buildc import component {}` copies it in from a library",
                        want.path
                    )),
                );
            }
            None => {
                let mut entry = Entry {
                    path: want.path,
                    flavour: flavour.clone(),
                    variant: None,
                    options: want
                        .options
                        .into_iter()
                        .map(|(name, values)| (name, values, span))
                        .collect(),
                    span,
                    component: None,
                    imported_by: Some(by.clone()),
                    inline: None,
                    organises: false,
                };
                entry.component = load(&entry, image);
                image.entries.push(entry);
                queue.push_back(image.entries.len() - 1);
            }
        }
    }
}

fn at(entries: &[Entry], path: &str) -> Option<usize> {
    entries.iter().position(|entry| entry.path == path)
}

fn cycles(image: &Image) -> Vec<Issue> {
    let mut issues: Vec<Issue> = Vec::new();
    let mut state = vec![Walk::Unvisited; image.entries.len()];
    let mut path: Vec<usize> = Vec::new();
    for from in 0..image.entries.len() {
        if state[from] == Walk::Unvisited {
            walk(&image.entries, from, &mut state, &mut path, &mut issues);
        }
    }
    issues
}

#[derive(Clone, Copy, PartialEq)]
enum Walk {
    Unvisited,
    OnPath,
    Done,
}

fn walk(
    entries: &[Entry],
    from: usize,
    state: &mut [Walk],
    path: &mut Vec<usize>,
    issues: &mut Vec<Issue>,
) {
    state[from] = Walk::OnPath;
    path.push(from);
    if let Some(component) = &entries[from].component {
        for import in &component.imports {
            let Some(to) = at(entries, &import.path) else {
                continue;
            };
            match state[to] {
                Walk::Unvisited => walk(entries, to, state, path, issues),
                Walk::OnPath => {
                    let start = path
                        .iter()
                        .position(|&held| held == to)
                        .expect("an entry on the walk is on the path");
                    let members: Vec<String> = path[start..]
                        .iter()
                        .map(|&held| format!("`{}`", entries[held].path))
                        .collect();
                    let said = match members.as_slice() {
                        [one] => format!("{one} imports itself"),
                        many => format!("{} import each other in a cycle", crate::said(many)),
                    };
                    issues.push(
                        Issue::new(said, &component.src)
                            .at(import.span, "closes the cycle")
                            .help(
                                "a component cannot be built from itself; drop one of the imports",
                            ),
                    );
                }
                Walk::Done => {}
            }
        }
    }
    path.pop();
    state[from] = Walk::Done;
}

fn reaches(entries: &[Entry], from: usize, path: &str) -> bool {
    let mut seen = vec![false; entries.len()];
    let mut todo = vec![from];
    while let Some(held_at) = todo.pop() {
        if entries[held_at].path == path {
            return true;
        }
        if std::mem::replace(&mut seen[held_at], true) {
            continue;
        }
        if let Some(component) = &entries[held_at].component {
            todo.extend(
                component
                    .imports
                    .iter()
                    .filter_map(|import| at(entries, &import.path)),
            );
        }
    }
    false
}

fn reorder(image: &mut Image) {
    let mut placed: Vec<usize> = Vec::new();
    let mut seen = vec![false; image.entries.len()];
    if let Some(base) = &image.base {
        for import in &base.imports {
            if let Some(to) = at(&image.entries, &import.path) {
                place(&image.entries, to, &mut seen, &mut placed);
            }
        }
    }
    for from in 0..image.entries.len() {
        place(&image.entries, from, &mut seen, &mut placed);
    }
    let mut taken: Vec<Option<Entry>> = std::mem::take(&mut image.entries)
        .into_iter()
        .map(Some)
        .collect();
    image.entries = placed
        .into_iter()
        .filter_map(|from| taken[from].take())
        .collect();
}

fn place(entries: &[Entry], from: usize, seen: &mut [bool], placed: &mut Vec<usize>) {
    if seen[from] {
        return;
    }
    seen[from] = true;
    if let Some(component) = &entries[from].component {
        for import in &component.imports {
            if let Some(to) = at(entries, &import.path) {
                place(entries, to, seen, placed);
            }
        }
    }
    placed.push(from);
}

fn disagrees(entry: &Entry, passed: &[(String, Vec<Value>)]) -> Option<String> {
    let held = |name: &str| {
        entry
            .options
            .iter()
            .find(|(have, ..)| have == name)
            .map(|(_, values, _)| values)
    };
    let given = |name: &str| {
        passed
            .iter()
            .find(|(have, _)| have == name)
            .map(|(_, values)| values)
    };
    entry
        .options
        .iter()
        .map(|(name, ..)| name)
        .chain(passed.iter().map(|(name, _)| name))
        .find(|name| held(name) != given(name))
        .cloned()
}
