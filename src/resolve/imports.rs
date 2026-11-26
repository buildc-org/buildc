use crate::diag::{Issue, Issues, Source, Span};
use crate::model::component::{Component, Import};
use crate::model::image::{Entry, Held, Image, Inline, Layer, Setter};
use crate::model::options::{OptType, Setting};
use crate::resolve::library::{shown, Libraries, Unresolved};
use std::collections::VecDeque;

/// A quoted `{option}` segment makes a listed path depend on the merged option values.
/// The cap ends a merge whose values keep changing the tree.
const PASSES: usize = 16;

struct Wanted {
    path: String,
    variant: Option<String>,
    library: Option<String>,
    pin: Option<String>,
    options: Vec<Setting>,
    span: Span,
    organises: bool,
    rooted: bool,
    root_file: bool,
    inline: Option<Vec<Span>>,
    digest: Option<(String, Span)>,
    signed: Option<(bool, Span)>,
    key: Option<(String, Span)>,
    key_sha256: Option<(String, Span)>,
    identity: Option<(String, Span)>,
    issuer: Option<(String, Span)>,
}

struct Importer {
    by: String,
    library: Option<String>,
    held: Option<Held>,
    src: Source,
    span: Span,
    wanted: Vec<Wanted>,
}

impl Importer {
    fn of(entry: &Entry, component: &Component) -> Self {
        Self {
            by: component.path.clone(),
            library: component.library.clone(),
            held: entry.held.clone(),
            src: component.src.clone(),
            span: entry.span,
            wanted: wanted(&component.imports),
        }
    }
}

pub fn place(image: &mut Image, own: &Component, libraries: &Libraries) {
    let mut importer = Importer::of(&image.entries[0], own);
    for want in std::mem::take(&mut importer.wanted) {
        if let Ok(Some(entry)) = listing(want, &importer, true, libraries) {
            image.entries.push(entry);
        }
    }
}

pub fn find_bases(
    image: &Image,
    own: &Entry,
    component: &Component,
    libraries: &Libraries,
    load: &mut impl FnMut(&Entry, &Image, &mut Issues) -> Option<Component>,
    issues: &mut Issues,
) -> Vec<Entry> {
    let mut found = Vec::new();
    let mut seen = vec![own.path.clone()];
    walk_bases(
        image, own, component, libraries, load, &mut seen, &mut found, issues,
    );
    found
}

#[expect(
    clippy::too_many_arguments,
    reason = "one base walk threads its importer, cache and result through each child"
)]
fn walk_bases(
    image: &Image,
    entry: &Entry,
    component: &Component,
    libraries: &Libraries,
    load: &mut impl FnMut(&Entry, &Image, &mut Issues) -> Option<Component>,
    seen: &mut Vec<String>,
    found: &mut Vec<Entry>,
    issues: &mut Issues,
) {
    let mut importer = Importer::of(entry, component);
    for want in std::mem::take(&mut importer.wanted) {
        let Ok(Some(mut child)) = listing(want, &importer, false, libraries) else {
            continue;
        };
        if seen.contains(&child.path) {
            continue;
        }
        seen.push(child.path.clone());
        if let Err(why) = libraries.revise(&mut child, &image.id) {
            issues.push(
                Issue::new(why, &importer.src)
                    .at(child.span, "the base")
                    .help("the build reads a remote component at one commit"),
            );
            continue;
        }
        // The walk uses the loaded component only to find the next `base`. The image loads it again and reports its issues there.
        let Some(child_component) = load(&child, image, &mut Issues::default()) else {
            continue;
        };
        if child_component.base.is_some() {
            found.push(child);
            continue;
        }
        walk_bases(
            image,
            &child,
            &child_component,
            libraries,
            load,
            seen,
            found,
            issues,
        );
    }
}

fn listing(
    want: Wanted,
    importer: &Importer,
    by_build: bool,
    libraries: &Libraries,
) -> Result<Option<Entry>, Issue> {
    let mut home = None;
    let held = match &want.inline {
        Some(_) => importer.held.clone(),
        None if want.root_file => match libraries.root_file(&want.path) {
            Some(at) => {
                home = Some(at);
                None
            }
            None if want.organises => return Ok(None),
            None => {
                return Err(Issue::new(
                    format!(
                        "`{}` names `{}`, which is no component at the repository root",
                        importer.by, want.path
                    ),
                    &importer.src,
                )
                .at(want.span, "not a component there")
                .help(
                    "a `component` path in a file outside every library reads the repository \
                     root, as `component \"./<name>\"`",
                ))
            }
        },
        None => match libraries.resolve(
            &want.path,
            want.library.as_deref(),
            want.rooted,
            importer.library.as_deref(),
        ) {
            Ok(held) => held,
            // Only an absent path lets an organising segment through.
            // Two holders and an undeclared `library=` stay refusals because a child's own `library=` never settles them.
            Err(Unresolved::Absent { .. }) if want.organises => return Ok(None),
            // An unwritten path that the build lists falls back to the local library so that the load can name what the directory lacks.
            Err(Unresolved::Absent {
                library: None,
                unread,
            }) if by_build && unread.is_empty() && libraries.reads_own() => None,
            Err(why) => {
                return Err(unresolved(
                    &importer.by,
                    &want.path,
                    why,
                    &importer.src,
                    want.span,
                ))
            }
        },
    };
    let by = (!by_build).then(|| importer.by.clone());
    let setter = Setter {
        by: by.clone(),
        variant: want.variant.clone(),
        pin: want.pin.clone(),
        settings: want.options,
        src: importer.src.clone(),
        span: want.span,
    };
    Ok(Some(Entry {
        path: want.path,
        variant: want.variant,
        pin: want.pin,
        digest: want.digest,
        signed: want.signed,
        key: want.key,
        key_sha256: want.key_sha256,
        identity: want.identity,
        issuer: want.issuer,
        options: vec![setter.layer()],
        setters: vec![setter],
        span: match by_build {
            true => want.span,
            false => importer.span,
        },
        imported_by: by,
        inline: want.inline.map(|spans| Inline {
            src: importer.src.clone(),
            spans,
        }),
        held,
        home,
        ..Entry::default()
    }))
}

fn unresolved(by: &str, path: &str, why: Unresolved, src: &Source, span: Span) -> Issue {
    match why {
        Unresolved::Undeclared(name) => Issue::new(
            format!("`{by}` reads `{path}` from `{name}`, which `libraries` does not declare"),
            src,
        )
        .at(span, "no such library")
        .help(format!(
            "declare `{name}` in `libraries` in repo.kdl, or drop `library=\"{name}\"`"
        )),
        Unresolved::Absent { library, unread } => match (&library, unread.as_slice()) {
            (Some(name), []) => Issue::new(
                format!("`{by}` reads `{path}` from library `{name}`, which does not hold it"),
                src,
            )
            .at(span, "not in that library")
            .help(format!(
                "a relative path stays in the library that lists it; write `component \"/\"` \
                 to read `{path}` from a library root, or name the `library=` that holds it \
                 after declaring that library in `libraries` in repo.kdl"
            )),
            (Some(name), _) => Issue::new(
                format!(
                    "`{by}` reads `{path}` from library `{name}`, which has no components folder that buildc can read"
                ),
                src,
            )
            .at(span, "not readable")
            .help(
                "a local library names a folder of this repository; a remote library is read from \
                 its cache, which `buildc generate` fills",
            ),
            (None, []) => Issue::new(
                format!("`{by}` lists `{path}`, which no library holds"),
                src,
            )
            .at(span, "not in a library")
            .help(
                "a path names a component below the components folder of a library that \
                 repo.kdl declares, or of this repository",
            ),
            (None, names) => {
                let names: Vec<String> = names.iter().map(|name| format!("`{name}`")).collect();
                Issue::new(
                    format!("`{by}` lists `{path}`, which no library holds"),
                    src,
                )
                .at(span, "not in a library")
                .help(format!(
                    "buildc cannot read the components folder of {}. The path may be there",
                    crate::said(&names)
                ))
            }
        },
        Unresolved::Ambiguous(holders) => {
            let held: Vec<String> = holders
                .iter()
                .map(|library| shown(library.as_deref()))
                .collect();
            let fixes: Vec<String> = holders
                .iter()
                .flatten()
                .map(|name| format!("`library=\"{name}\"`"))
                .collect();
            Issue::new(
                format!("`{by}` lists `{path}`, which {} each hold", crate::said(&held)),
                src,
            )
            .at(span, "more than one library")
            .help(format!(
                "{} on the segment chooses the library, and `libraries default=` settles every such path",
                fixes.join(" or ")
            ))
        }
    }
}

fn wanted(component: &[Import]) -> Vec<Wanted> {
    component
        .iter()
        .map(|import| Wanted {
            path: import.path.clone(),
            variant: import.variant.clone(),
            library: import.library.clone(),
            pin: import.pin.clone(),
            options: import.options.clone(),
            span: import.span,
            organises: import.organises,
            rooted: import.rooted,
            root_file: import.root_file,
            inline: import.inline.clone(),
            digest: import.digest.clone(),
            signed: import.signed,
            key: import.key.clone(),
            key_sha256: import.key_sha256.clone(),
            identity: import.identity.clone(),
            issuer: import.issuer.clone(),
        })
        .collect()
}

#[derive(PartialEq)]
struct Merged {
    path: String,
    variant: Option<String>,
    variant_at: Option<usize>,
    pin: Option<String>,
    options: Vec<Layer>,
}

pub fn expand(
    image: &mut Image,
    libraries: &Libraries,
    mut load: impl FnMut(&Entry, &Image, &mut Issues) -> Option<Component>,
) -> Issues {
    let mut seeded: Vec<Merged> = Vec::new();
    let mut pass = 0;
    let mut issues = loop {
        pass += 1;
        let mut issues = Issues::default();
        let found = pass_once(image, libraries, &mut load, &seeded, &mut issues);
        for issue in found {
            issues.push(issue);
        }
        let (merged, conflicts) = merge(image);
        let settled = merged.iter().all(|merged| {
            image.entries.iter().any(|entry| {
                entry.path == merged.path
                    && entry.variant == merged.variant
                    && entry.pin == merged.pin
                    && entry.options == merged.options
            })
        });
        if settled {
            for issue in conflicts {
                issues.push(issue);
            }
            break issues;
        }
        if pass == PASSES {
            issues.push(
                Issue::new(
                    format!("the option values of `{}` do not settle", image.id),
                    &image.src,
                )
                .at(image.span, "the merge keeps changing")
                .help(
                    "an option-valued child selects a component whose own values change that \
                     option again; set the option in the component that the build names",
                ),
            );
            break issues;
        }
        seeded = merged;
    };
    for issue in cycles(image)
        .into_iter()
        .chain(one_directory(image, libraries))
    {
        issues.push(issue);
    }
    reorder(image);
    issues
}

/// Each pass lists the tree again from the build's own component with the option values that the last pass merged.
fn pass_once(
    image: &mut Image,
    libraries: &Libraries,
    load: &mut impl FnMut(&Entry, &Image, &mut Issues) -> Option<Component>,
    seeded: &[Merged],
    issues: &mut Issues,
) -> Vec<Issue> {
    image.entries.retain(|entry| entry.path == image.component);
    let mut entries = std::mem::take(&mut image.entries);
    for entry in &mut entries {
        entry.setters.truncate(1);
        settle(entry, seeded);
        revise(entry, libraries, image, issues);
        entry.component = load(entry, image, issues);
    }
    image.entries = entries;
    let mut found: Vec<Issue> = Vec::new();
    let mut queue: VecDeque<usize> = (0..image.entries.len()).collect();
    if let Some(base) = &image.base {
        let importer = Importer {
            by: base.path.clone(),
            library: base.library.clone(),
            held: image
                .base_entry
                .as_ref()
                .and_then(|based| based.entry.held.clone()),
            src: base.src.clone(),
            span: base.span,
            wanted: wanted(&base.imports),
        };
        bring(
            image, libraries, load, importer, seeded, &mut queue, &mut found, issues,
        );
    }
    while let Some(at) = queue.pop_front() {
        let entry = &image.entries[at];
        let Some(component) = &entry.component else {
            continue;
        };
        let importer = Importer::of(entry, component);
        bring(
            image, libraries, load, importer, seeded, &mut queue, &mut found, issues,
        );
    }
    found
}

pub fn inherit(image: &mut Image) {
    let mut by = image
        .base_entry
        .as_ref()
        .and_then(|held| held.entry.imported_by.clone());
    let mut chain = Vec::new();
    while let Some(current) = by {
        chain.push(current.clone());
        by = image
            .entries
            .iter()
            .find(|entry| entry.path == current)
            .and_then(|entry| entry.imported_by.clone());
    }
    // The chain runs from the base outward. The reverse of that order lets the component nearest the build fill a field first.
    for path in chain.iter().rev() {
        let Some(component) = image
            .entries
            .iter()
            .find(|entry| &entry.path == path)
            .and_then(|entry| entry.component.as_ref())
        else {
            continue;
        };
        let description = component.description.clone();
        let keywords = component.keywords.clone();
        let logo_url = component.logo_url.clone();
        let conforms = component.conforms.clone();
        let allows = component.allows.clone();
        if image.description.is_empty() && !description.is_empty() {
            image.description = description;
            image.field_source("description", path);
        }
        if image.keywords.is_empty() && !keywords.is_empty() {
            image.keywords = keywords;
            image.field_source("keywords", path);
        }
        if image.logo_url.is_empty() && !logo_url.is_empty() {
            image.logo_url = logo_url;
            image.field_source("logo-url", path);
        }
        if image.conforms.is_empty() && !conforms.is_empty() {
            image.conforms = conforms;
            image.field_source("conforms", path);
            image.allows = allows;
        }
    }
}

fn revise(entry: &mut Entry, libraries: &Libraries, image: &Image, issues: &mut Issues) {
    if let Err(why) = libraries.revise(entry, &image.id) {
        issues.push(
            Issue::new(why, &image.src)
                .at(entry.span, "this component")
                .help("the build reads a remote component at one commit"),
        );
    }
}

fn settle(entry: &mut Entry, seeded: &[Merged]) {
    if let Some(merged) = seeded.iter().find(|merged| merged.path == entry.path) {
        entry.variant.clone_from(&merged.variant);
        entry.variant_at = merged.variant_at;
        entry.pin.clone_from(&merged.pin);
        entry.options.clone_from(&merged.options);
    } else if let Some(setter) = entry.setters.first() {
        entry.variant.clone_from(&setter.variant);
        entry.variant_at = setter.variant.is_some().then_some(0);
        entry.pin.clone_from(&setter.pin);
        entry.options = vec![setter.layer()];
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "one expansion pass threads its state through each importer"
)]
fn bring(
    image: &mut Image,
    libraries: &Libraries,
    load: &mut impl FnMut(&Entry, &Image, &mut Issues) -> Option<Component>,
    mut importer: Importer,
    seeded: &[Merged],
    queue: &mut VecDeque<usize>,
    found: &mut Vec<Issue>,
    issues: &mut Issues,
) {
    let wanted = std::mem::take(&mut importer.wanted);
    let by = importer.by.clone();
    let src = importer.src.clone();
    let by_build = by == image.component;
    for (at, want) in wanted.iter().enumerate() {
        let first = wanted[..at]
            .iter()
            .find(|first| first.path == want.path && !first.organises);
        if let Some(first) = first.filter(|_| !want.organises) {
            found.push(
                Issue::new(format!("`{}` is listed twice", want.path), &src)
                    .at(first.span, "first here")
                    .at(want.span, "and again here")
                    .help("a component builds once in an image, so list it once"),
            );
        }
    }
    let base = image.base.as_ref().map(|base| base.path.clone());
    let holds_base = image
        .base_entry
        .as_ref()
        .is_some_and(|held| held.entry.imported_by.as_deref() == Some(by.as_str()));
    for want in wanted {
        if base.as_deref() == Some(want.path.as_str()) {
            if !by_build && !holds_base {
                found.push(
                    Issue::new(format!("`{by}` lists the base `{}`", want.path), &src)
                        .at(want.span, "only the build's component lists the base")
                        .help("drop the path; the component that the build names lists the one base it builds on"),
                );
            }
            continue;
        }
        if want.path.is_empty() {
            found.push(
                Issue::new(
                    format!("`{by}` imports a path that its option leaves empty"),
                    &src,
                )
                .at(want.span, "no component named")
                .help("set the option on the image entry, or give the option a default"),
            );
            continue;
        }
        if let (Some(_), Some(pin)) = (&want.inline, &want.pin) {
            found.push(
                Issue::new(
                    format!("`{by}` defines `{}` inline at `pin=\"{pin}\"`", want.path),
                    &src,
                )
                .at(want.span, "an inline definition")
                .help(format!(
                    "an inline definition reads at the revision of `{by}`; drop `pin=`"
                )),
            );
            continue;
        }
        let span = want.span;
        let mut entry = match listing(want, &importer, by_build, libraries) {
            Ok(Some(entry)) => entry,
            Ok(None) => continue,
            Err(issue) => {
                found.push(issue);
                continue;
            }
        };
        if image
            .base_entry
            .as_ref()
            .is_some_and(|held| held.entry.path == entry.path)
        {
            continue;
        }
        let from = entry.library();
        match image
            .entries
            .iter()
            .position(|held| held.path == entry.path)
        {
            Some(held) if image.entries[held].library() != from => {
                let held = &image.entries[held];
                found.push(
                    Issue::new(
                        format!(
                            "`{by}` reads `{}` from {}, and `{}` reads it from {}",
                            entry.path,
                            shown(from),
                            held.imported_by.as_deref().unwrap_or(&image.id),
                            shown(held.library()),
                        ),
                        &src,
                    )
                    .at(span, "a second library")
                    .help(
                        "an image holds one component at each path; set the same `library=` on \
                         both references",
                    ),
                );
            }
            Some(held) if reaches(&image.entries, held, &by) => {}
            Some(held) => image.entries[held].setters.append(&mut entry.setters),
            None => {
                settle(&mut entry, seeded);
                if entry.inline.is_none() {
                    revise(&mut entry, libraries, image, issues);
                }
                entry.component = load(&entry, image, issues);
                image.entries.push(entry);
                queue.push_back(image.entries.len() - 1);
            }
        }
    }
}

fn nearer(entries: &[Entry], a: &Setter, b: &Setter) -> bool {
    match (&a.by, &b.by) {
        (None, Some(_)) => true,
        (Some(near), Some(far)) => {
            near != far && at(entries, near).is_some_and(|from| reaches(entries, from, far))
        }
        (_, None) => false,
    }
}

fn named<'a>(setter: &'a Setter, image: &'a Image) -> &'a str {
    setter.by.as_deref().unwrap_or(&image.id)
}

fn merge(image: &Image) -> (Vec<Merged>, Vec<Issue>) {
    let mut merged = Vec::new();
    let mut issues = Vec::new();
    let near = |a: &Setter, b: &Setter| nearer(&image.entries, a, b);
    for entry in image
        .entries
        .iter()
        .filter(|entry| !entry.setters.is_empty())
    {
        let ordered = farthest_first(&entry.setters, near);
        let chose: Vec<&Setter> = ordered
            .iter()
            .copied()
            .filter(|s| s.variant.is_some())
            .collect();
        let variants = nearest(&chose, near);
        if let Some((a, b)) = differs(&variants, |s| &s.variant) {
            issues.push(
                Issue::new(
                    format!(
                        "`{}` and `{}` list different variants of `{}`",
                        named(a, image),
                        named(b, image),
                        entry.path
                    ),
                    &b.src,
                )
                .at(b.span, "a second variant")
                .help(
                    "neither lists the other, so neither is nearer the build; select the \
                     variant in a component that lists both",
                ),
            );
        }
        let pinned: Vec<&Setter> = ordered
            .iter()
            .copied()
            .filter(|s| s.pin.is_some())
            .collect();
        let pins = nearest(&pinned, near);
        if let Some((a, b)) = differs(&pins, |s| &s.pin) {
            issues.push(
                Issue::new(
                    format!(
                        "`{}` and `{}` pin `{}` at different revisions",
                        named(a, image),
                        named(b, image),
                        entry.path
                    ),
                    &b.src,
                )
                .at(b.span, "a second pin")
                .help(
                    "neither lists the other, so neither is nearer the build; set `pin=` in a \
                     component that lists both",
                ),
            );
        }
        let mut names: Vec<&str> = Vec::new();
        for setting in ordered.iter().flat_map(|s| &s.settings) {
            if !names.contains(&setting.name.as_str()) {
                names.push(&setting.name);
            }
        }
        for name in names {
            if let Some(issue) = clash(image, entry, &ordered, name, near) {
                issues.push(issue);
            }
        }
        merged.push(Merged {
            path: entry.path.clone(),
            variant: variants.last().and_then(|s| s.variant.clone()),
            variant_at: variants.last().and_then(|chosen| {
                ordered
                    .iter()
                    .position(|setter| std::ptr::eq(*setter, *chosen))
            }),
            pin: pins.last().and_then(|s| s.pin.clone()),
            options: layers(&ordered, near),
        });
    }
    (merged, issues)
}

fn farthest_first(setters: &[Setter], near: impl Fn(&Setter, &Setter) -> bool) -> Vec<&Setter> {
    let mut left: Vec<&Setter> = setters.iter().collect();
    let mut ordered = Vec::new();
    while !left.is_empty() {
        // A cycle of components leaves no farthest setter.
        // The first setter then stands in.
        let farthest = (0..left.len())
            .find(|&at| !left.iter().any(|other| near(left[at], other)))
            .unwrap_or(0);
        ordered.push(left.remove(farthest));
    }
    ordered
}

fn nearest<'a>(setters: &[&'a Setter], near: impl Fn(&Setter, &Setter) -> bool) -> Vec<&'a Setter> {
    setters
        .iter()
        .copied()
        .filter(|s| !setters.iter().any(|other| near(other, s)))
        .collect()
}

fn differs<'a, T: PartialEq>(
    setters: &[&'a Setter],
    value: impl Fn(&Setter) -> &T,
) -> Option<(&'a Setter, &'a Setter)> {
    let (first, rest) = setters.split_first()?;
    rest.iter()
        .find(|other| value(other) != value(first))
        .map(|other| (*first, *other))
}

fn clash(
    image: &Image,
    entry: &Entry,
    ordered: &[&Setter],
    name: &str,
    near: impl Fn(&Setter, &Setter) -> bool + Copy,
) -> Option<Issue> {
    let ty = entry
        .component
        .as_ref()?
        .options
        .iter()
        .find(|opt| opt.name == name)?
        .ty;
    let setting = |s: &Setter| s.settings.iter().find(|held| held.name == name).cloned();
    let setters: Vec<&Setter> = ordered
        .iter()
        .copied()
        .filter(|s| setting(s).is_some())
        .collect();
    let unrelated = |a: &Setter, b: &Setter| !near(a, b) && !near(b, a);
    let same = |a: &Setter, b: &Setter| {
        setting(a)
            .zip(setting(b))
            .is_some_and(|(a, b)| a.same_values(&b))
    };
    let (pair, help) = match ty {
        OptType::List => (
            setters.iter().enumerate().find_map(|(at, a)| {
                setters[at + 1..]
                    .iter()
                    .find(|b| unrelated(a, b) && !same(a, b))
                    .map(|b| (*a, *b))
            }),
            "neither lists the other, so the order of their values is unsettled; set the values \
             in one of them",
        ),
        OptType::String | OptType::Bool => {
            let nearest_setters = nearest(&setters, near);
            (
                nearest_setters
                    .split_first()
                    .and_then(|(a, rest)| rest.iter().find(|b| !same(a, b)).map(|b| (*a, *b))),
                "neither lists the other, so neither is nearer the build; set the value in a \
                 component that lists both",
            )
        }
    };
    let (a, b) = pair?;
    Some(
        Issue::new(
            format!(
                "`{}` and `{}` set different values for option `{name}` of `{}`",
                named(a, image),
                named(b, image),
                entry.path
            ),
            &b.src,
        )
        .at(b.span, "a second value")
        .help(help),
    )
}

/// Two unrelated setters may write one identical setting.
/// It applies once because a list would otherwise append its values twice.
fn layers(ordered: &[&Setter], near: impl Fn(&Setter, &Setter) -> bool) -> Vec<Layer> {
    let unrelated = |a: &Setter, b: &Setter| !near(a, b) && !near(b, a);
    ordered
        .iter()
        .enumerate()
        .map(|(at, setter)| Layer {
            by: setter.by.clone(),
            settings: setter
                .settings
                .iter()
                .filter(|held| {
                    !ordered[..at].iter().any(|earlier| {
                        unrelated(earlier, setter)
                            && earlier.settings.iter().any(|other| other.same_values(held))
                    })
                })
                .cloned()
                .collect(),
        })
        .collect()
}

fn one_directory(image: &Image, libraries: &Libraries) -> Vec<Issue> {
    let mut seen: Vec<(std::path::PathBuf, &Entry)> = Vec::new();
    let mut issues = Vec::new();
    for entry in image.entries.iter().filter(|entry| entry.inline.is_none()) {
        let dir = libraries.dir(entry);
        let dir = std::fs::canonicalize(&dir).unwrap_or(dir);
        match seen.iter().find(|(held, _)| *held == dir) {
            Some((_, first)) => {
                let mut issue = Issue::new(
                    format!(
                        "`{}` and `{}` are one component directory",
                        first.path, entry.path
                    ),
                    &image.src,
                );
                // The span of an imported entry points into the file of the component that imports it.
                // `image.src` is not that file.
                if entry.imported_by.is_none() {
                    issue = issue.at(entry.span, "reached a second time");
                }
                issues.push(issue.help(format!(
                    "a nested library gives the directory a second path; keep one of `{}` and `{}`",
                    first.path, entry.path
                )));
            }
            None => seen.push((dir, entry)),
        }
    }
    issues
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
                visit(&image.entries, to, &mut seen, &mut placed);
            }
        }
    }
    for from in 0..image.entries.len() {
        visit(&image.entries, from, &mut seen, &mut placed);
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

fn visit(entries: &[Entry], from: usize, seen: &mut [bool], placed: &mut Vec<usize>) {
    if seen[from] {
        return;
    }
    seen[from] = true;
    if let Some(component) = &entries[from].component {
        for import in &component.imports {
            if let Some(to) = at(entries, &import.path) {
                visit(entries, to, seen, placed);
            }
        }
    }
    placed.push(from);
}
