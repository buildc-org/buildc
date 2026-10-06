use crate::layout;
use crate::parse::schema::{Arg, Kind, Node, Prop};
use crate::parse::{bases, module, repo};
use crate::provenance::record;
use std::fmt::Write as _;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Area {
    Repository,
    Repo,
    Image,
    Modules,
    Module,
    Provenance,
    Bases,
}

impl Area {
    pub fn file(self) -> &'static str {
        match self {
            Area::Repository => "repository.md",
            Area::Repo => "repo.md",
            Area::Image => "image.md",
            Area::Modules => "modules.md",
            Area::Module => "module.md",
            Area::Bases => "bases.md",
            Area::Provenance => "provenance.md",
        }
    }

    fn title(self) -> &'static str {
        match self {
            Area::Repository => "The repository",
            Area::Repo => "The repository file",
            Area::Image => "Image files",
            Area::Modules => "Modules",
            Area::Module => "Module manifests",
            Area::Bases => "The base catalog",
            Area::Provenance => "The import record",
        }
    }
}

struct Section {
    name: &'static str,
    node: &'static Node,
    declared: bool,
    area: Area,
    at: &'static str,
}

#[rustfmt::skip]
const SECTIONS: &[Section] = &[
    Section { name: "repo.kdl", node: &repo::REPO, declared: false, area: Area::Repo,
        at: "The root of the repository." },
    Section { name: "image.kdl", node: &repo::IMAGE_FILE, declared: false, area: Area::Image,
        at: "The root of the repository, as `image.kdl` or `<name>.image.kdl`." },
    Section { name: "bases.kdl", node: &bases::BASES, declared: false, area: Area::Bases,
        at: "Compiled into `buildc`. A `bases.kdl` beside the `buildc` binary replaces it, and one at \
            the root of a collection extends it." },
    Section { name: "module.kdl", node: &module::MODULE, declared: false, area: Area::Module,
        at: "`modules/<module-name>/module.kdl`, in the directory of the module." },
    Section { name: "provenance.kdl", node: &record::RECORD_FILE, declared: false, area: Area::Provenance,
        at: "`modules/<module-name>/provenance.kdl`, beside the `module.kdl` of a copied module." },
];

fn section_of(node: &Node) -> Option<&'static Section> {
    SECTIONS
        .iter()
        .find(|s| s.node.name == node.name && s.node.desc == node.desc)
}

fn named(node: &Node) -> String {
    match node.name.is_empty() {
        true => "`<name>`".to_string(),
        false => format!("`{}`", node.name),
    }
}

fn accepts(node: &Node, fields: &str) -> Option<String> {
    let value = match node.arg {
        Arg::None => None,
        Arg::Str => Some("*string*".to_string()),
        Arg::Bool => Some("*boolean*".to_string()),
        Arg::Int => Some("*number*".to_string()),
        Arg::Strs => Some("*list of strings*".to_string()),
        Arg::StrPair(first, second) => Some(format!("*{first}*, then *{second}*")),
        Arg::One(set) => Some(closed(set)),
        Arg::MaybeOne(set) => Some(format!("optionally {}", closed(set))),
    };
    let block = match (node.children.is_empty(), block_required(node)) {
        (true, _) => None,
        (false, true) => Some(format!("{{&nbsp;[fields]({fields})&nbsp;}}")),
        (false, false) => Some(format!("optionally {{&nbsp;[fields]({fields})&nbsp;}}")),
    };
    match (value, block) {
        (Some(value), Some(block)) => Some(format!("{value}, then {block}")),
        (value, block) => value.or(block),
    }
}

fn block_required(node: &Node) -> bool {
    !node.empty.text.is_empty() || node.children.iter().any(is_minimal)
}

fn presence(node: &Node) -> String {
    let need = match is_minimal(node) && !node.pick {
        true => "required",
        false => "optional",
    };
    match (node.once, node.unique.text.is_empty()) {
        (true, _) => format!("({need})"),
        (false, false) => format!("({need}, unique)"),
        (false, true) => format!("({need}, repeatable)"),
    }
}

fn closed(set: &[&str]) -> String {
    set.iter()
        .map(|v| format!("`{v}`"))
        .collect::<Vec<_>>()
        .join(" or ")
}

fn value(prop: &Prop) -> String {
    match prop.kind {
        Kind::Str => "*string*".to_string(),
        Kind::Bool => "*boolean*".to_string(),
        Kind::Int(low, high) => format!("*number* from {low} to {high}"),
        Kind::One(set) => closed(set),
    }
}

fn href(section: &Section, from: Area) -> String {
    let file = match section.area == from {
        true => "",
        false => section.area.file(),
    };
    format!("{file}#{}", section.name)
}

fn props(node: &Node, out: &mut String) {
    if node.props.is_empty() {
        return;
    }
    let _ = writeln!(
        out,
        "| Property | Accepts | Description |\n| --- | --- | --- |"
    );
    for prop in node.props {
        let need = match prop.missing.text.is_empty() {
            true => "optional",
            false => "required",
        };
        let _ = writeln!(
            out,
            "| `{}=` ({need}) | {} | {} |",
            prop.name,
            value(prop),
            prop.desc
        );
    }
    out.push('\n');
}

fn cases(node: &Node, out: &mut String) {
    lists(node.lists, out);
    if node.values.is_empty() {
        return;
    }
    out.push_str("| Value | Effect |\n| --- | --- |\n");
    for (label, effect) in node.values {
        let _ = writeln!(out, "| {label} | {effect} |");
    }
    out.push('\n');
}

fn lists(lists: &[(&str, &[&str])], out: &mut String) {
    for (lead, items) in lists {
        let _ = writeln!(out, "{lead}\n");
        for item in *items {
            let _ = writeln!(out, "- {item}");
        }
        out.push('\n');
    }
}

fn alert(notes: &[&str], out: &mut String) {
    if notes.is_empty() {
        return;
    }
    out.push_str("> [!NOTE]\n");
    for line in notes.join("\n\n").lines() {
        match line.is_empty() {
            true => out.push_str(">\n"),
            false => {
                let _ = writeln!(out, "> {line}");
            }
        }
    }
    out.push('\n');
}

fn either(set: &[&str]) -> String {
    set.iter()
        .map(|v| format!("\"{v}\""))
        .collect::<Vec<_>>()
        .join("|")
}

fn head(node: &Node) -> String {
    let mut line = match node.name.is_empty() {
        true => "<name>".to_string(),
        false => node.name.to_string(),
    };
    let arg = match node.arg {
        Arg::None => String::new(),
        Arg::Str => " \"…\"".into(),
        Arg::Bool => " #true|#false".into(),
        Arg::Int => " <number>".into(),
        Arg::Strs => " \"…\" …".into(),
        Arg::StrPair(..) => " \"…\" \"…\"".into(),
        Arg::One(set) | Arg::MaybeOne(set) => format!(" {}", either(set)),
    };
    line.push_str(&arg);
    for prop in node.props {
        let value = match prop.kind {
            Kind::Str => "\"…\"".to_string(),
            Kind::Bool => "#true|#false".to_string(),
            Kind::Int(..) => "<number>".to_string(),
            Kind::One(set) => either(set),
        };
        let _ = write!(line, " {}={value}", prop.name);
    }
    line
}

fn intro(node: &Node) -> &'static str {
    match node.about.is_empty() {
        true => node.desc,
        false => node.about,
    }
}

fn is_minimal(node: &Node) -> bool {
    node.minimal || !node.missing.text.is_empty()
}

fn example(node: &Node) -> String {
    match (node.example.is_empty(), node.name.is_empty()) {
        (true, _) => head(node),
        (false, true) => node.example.to_string(),
        (false, false) => format!("{} {}", node.name, node.example),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Show {
    Minimal,
    Scaffold,
    #[cfg(test)]
    All,
}

impl Show {
    fn keeps(self, parent: &Node, node: &Node) -> bool {
        match self {
            Show::Minimal => is_minimal(node),
            Show::Scaffold => match parent.scaffolds.is_empty() {
                true => is_minimal(node),
                false => parent.scaffolds.contains(&node.name) || !node.missing.text.is_empty(),
            },
            #[cfg(test)]
            Show::All => true,
        }
    }
}

fn kdl(
    node: &Node,
    depth: usize,
    show: Show,
    from: Area,
    pointer: Option<String>,
    out: &mut String,
) {
    let pad = "    ".repeat(depth);
    let comment = pointer.map(|p| format!("  // {p}")).unwrap_or_default();
    let mut shown: Vec<&Node> = node
        .children
        .iter()
        .filter(|c| show.keeps(node, c))
        .collect();
    if shown.is_empty() && !node.empty.text.is_empty() {
        shown.extend(node.children.first());
    }
    if shown.is_empty() {
        let _ = writeln!(out, "{pad}{}{comment}", example(node));
        return;
    }
    let _ = writeln!(out, "{pad}{} {{{comment}", example(node));
    for child in shown {
        match section_of(child) {
            None => kdl(child, depth + 1, show, from, None, out),
            Some(s) => {
                let page = match s.area == from {
                    true => String::new(),
                    false => format!(" in {}", s.area.file()),
                };
                let pointer = format!("see {}{page}", s.name);
                kdl(child, depth + 1, Show::Minimal, from, Some(pointer), out);
            }
        }
    }
    let _ = writeln!(out, "{pad}}}");
}

fn file(section: &Section, show: Show) -> String {
    let mut out = String::new();
    match section.declared {
        true => kdl(section.node, 0, show, section.area, None, &mut out),
        false => {
            for child in section
                .node
                .children
                .iter()
                .filter(|c| show.keeps(section.node, c))
            {
                kdl(child, 0, show, section.area, None, &mut out);
            }
        }
    }
    out
}

pub fn scaffold(area: Area) -> Vec<String> {
    let mut out = Vec::new();
    for s in SECTIONS.iter().filter(|s| s.area == area && !s.declared) {
        scaffolded(s.node, "", &mut out);
    }
    out
}

fn scaffolded(parent: &Node, prefix: &str, out: &mut Vec<String>) {
    for child in parent
        .children
        .iter()
        .filter(|c| parent.scaffolds.contains(&c.name))
    {
        let name = match child.name.is_empty() {
            true => "*",
            false => child.name,
        };
        let path = match prefix.is_empty() {
            true => name.to_string(),
            false => format!("{prefix}/{name}"),
        };
        match section_of(child) {
            Some(_) => out.push(format!("{path}/**")),
            None => {
                scaffolded(child, &path, out);
                out.push(path);
            }
        }
    }
}

fn snippet(path: &[&Node], node: &Node, from: Area, out: &mut String) {
    out.push_str("```kdl\n");
    for (depth, outer) in path.iter().enumerate() {
        let _ = writeln!(out, "{}{} {{", "    ".repeat(depth), example(outer));
    }
    kdl(node, path.len(), Show::Minimal, from, None, out);
    for depth in (0..path.len()).rev() {
        let _ = writeln!(out, "{}}}", "    ".repeat(depth));
    }
    out.push_str("```\n\n");
}

fn anchor(path: &[&Node], node: &Node) -> String {
    path.iter()
        .copied()
        .chain(std::iter::once(node))
        .map(|n| match n.name.is_empty() {
            true => "name",
            false => n.name,
        })
        .collect::<Vec<_>>()
        .join("-")
}

fn table(
    node: &'static Node,
    path: &[&'static Node],
    from: Area,
    id: Option<&str>,
    seen: &mut Vec<(&'static str, String)>,
    out: &mut String,
) -> Vec<&'static Node> {
    let mut here: Vec<&'static Node> = Vec::new();
    if node.children.is_empty() {
        return here;
    }
    if let Some(id) = id {
        let _ = writeln!(out, "<a id=\"{id}-fields\"></a>\n");
    }
    out.push_str("| Field | Accepts | Description |\n| --- | --- | --- |\n");
    for child in node.children {
        let link = match (
            section_of(child),
            seen.iter().find(|(desc, _)| *desc == child.desc),
        ) {
            (Some(s), _) => href(s, from),
            (None, Some((_, at))) => format!("#{at}"),
            (None, None) => {
                let at = anchor(path, child);
                seen.push((child.desc, at.clone()));
                here.push(child);
                format!("#{at}")
            }
        };
        let _ = writeln!(
            out,
            "| [{}]({link}) {} | {} | {} |",
            named(child),
            presence(child).replace(' ', "&nbsp;"),
            accepts(child, &format!("{link}-fields")).unwrap_or_default(),
            child.desc
        );
    }
    out.push('\n');
    here
}

fn node_section(
    node: &'static Node,
    path: &mut Vec<&'static Node>,
    depth: usize,
    from: Area,
    seen: &mut Vec<(&'static str, String)>,
    out: &mut String,
) {
    let id = anchor(path, node);
    let _ = writeln!(
        out,
        "<a id=\"{id}\"></a>\n\n{} {} {}\n\n{}\n",
        "#".repeat(depth.min(6)),
        named(node),
        presence(node),
        intro(node)
    );
    snippet(path, node, from, out);
    if let Some(what) = accepts(node, &format!("#{id}-fields")) {
        let _ = writeln!(out, "Accepts: {what}\n");
    }
    cases(node, out);
    props(node, out);
    path.push(node);
    let here = table(node, path, from, Some(&id), seen, out);
    alert(node.notes, out);
    for child in here {
        node_section(child, path, depth + 1, from, seen, out);
    }
    path.pop();
}

fn section(section: &Section, depth: usize, out: &mut String) {
    let root = section.node;
    let _ = writeln!(
        out,
        "<a id=\"{}\"></a>\n\n{} `{}`\n\nLocation: {}\n\n{}\n",
        section.name,
        "#".repeat(depth),
        section.name,
        section.at,
        intro(root)
    );
    let _ = writeln!(out, "```kdl\n{}```\n", file(section, Show::Scaffold));
    let mut path: Vec<&'static Node> = Vec::new();
    if section.declared {
        if let Some(what) = accepts(root, &format!("#{}-fields", section.name)) {
            let _ = writeln!(out, "Accepts: {what}\n");
        }
        cases(root, out);
        props(root, out);
        path.push(root);
    }
    let id = section.declared.then_some(section.name);
    let seen = &mut Vec::new();
    let here = table(root, &path, section.area, id, seen, out);
    if !section.declared {
        cases(root, out);
    }
    alert(root.notes, out);
    for child in here {
        node_section(child, &mut path, depth + 1, section.area, seen, out);
    }
}

const AREAS: [Area; 7] = [
    Area::Repository,
    Area::Repo,
    Area::Image,
    Area::Modules,
    Area::Module,
    Area::Provenance,
    Area::Bases,
];

pub fn areas() -> Vec<Area> {
    AREAS.to_vec()
}

#[derive(Default)]
struct Dir {
    children: std::collections::BTreeMap<String, Dir>,
    dir: bool,
    ignored: bool,
}

fn drawn(root: &str, places: &[layout::Place]) -> String {
    let mut top = Dir::default();
    for place in places {
        let path = place.path.concat();
        let parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
        let mut node = &mut top;
        for (at, part) in parts.iter().enumerate() {
            node = node.children.entry(part.to_string()).or_default();
            node.dir |= at + 1 < parts.len() || path.ends_with('/');
            if at + 1 == parts.len() {
                node.ignored = !place.tracked;
            }
        }
    }
    let indent = root.trim_end_matches('/').rfind('/').map_or(0, |at| at + 1);
    let mut out = format!("```text\n{root}\n");
    branch(&top, &" ".repeat(indent), &mut out);
    out.push_str("```\n\n");
    out
}

fn branch(node: &Dir, prefix: &str, out: &mut String) {
    let mut names: Vec<&String> = node.children.keys().collect();
    names.sort_by_key(|name| !node.children[*name].dir);
    for (index, name) in names.iter().enumerate() {
        let child = &node.children[*name];
        let (fork, carry) = match index + 1 == names.len() {
            true => ("└── ", "    "),
            false => ("├── ", "│   "),
        };
        let slash = match child.dir {
            true => "/",
            false => "",
        };
        let untracked = match child.ignored {
            true => "  (not in git)",
            false => "",
        };
        let _ = writeln!(out, "{prefix}{fork}{name}{slash}{untracked}");
        branch(child, &format!("{prefix}{carry}"), out);
    }
}

fn tree_page(title: &str, tree: &layout::Tree) -> String {
    let mut out = format!("# {title}\n\n{}\n\n", tree.intro);
    out.push_str(&drawn(tree.root, tree.places));
    let writer = tree.places.iter().any(|place| !place.writer.is_empty());
    match writer {
        true => out
            .push_str("| Path | Written by | In git | Description |\n| --- | --- | --- | --- |\n"),
        false => out.push_str("| Path | Description |\n| --- | --- |\n"),
    }
    for place in tree.places {
        let path = place.path.concat();
        let _ = match writer {
            true => writeln!(
                out,
                "| `{path}` | {} | {} | {} |",
                place.writer,
                match place.tracked {
                    true => "yes",
                    false => "no",
                },
                place.what
            ),
            false => writeln!(out, "| `{path}` | {} |", place.what),
        };
    }
    out.push('\n');
    lists(tree.lists, &mut out);
    alert(tree.notes, &mut out);
    let trimmed = out.trim_end().len();
    out.truncate(trimmed);
    out.push('\n');
    out
}

pub fn page(area: Area) -> String {
    match area {
        Area::Repository => return tree_page(area.title(), &layout::REPOSITORY),
        Area::Modules => return tree_page(area.title(), &layout::MODULE_DIR),
        Area::Repo | Area::Image | Area::Module | Area::Provenance | Area::Bases => {}
    }
    let schemas: Vec<&Section> = SECTIONS.iter().filter(|s| s.area == area).collect();
    let mut out = String::new();
    let depth = match schemas.len() {
        1 => 1,
        _ => {
            let _ = writeln!(out, "# {}\n", area.title());
            2
        }
    };
    for s in schemas {
        section(s, depth, &mut out);
    }
    let trimmed = out.trim_end().len();
    out.truncate(trimmed);
    out.push('\n');
    out
}

#[cfg(test)]
mod tests;
