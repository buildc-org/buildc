use super::colour;
use ratatui::crossterm::style::Stylize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub enum Change {
    Created,
    Updated(String),
}

fn mark(change: &Change) -> String {
    let symbol = match change {
        Change::Created => "+",
        Change::Updated(_) => "~",
    };
    match colour() {
        false => format!("{symbol} "),
        true => match change {
            Change::Created => format!("{} ", symbol.green()),
            Change::Updated(_) => format!("{} ", symbol.yellow()),
        },
    }
}

#[derive(Default)]
struct Node {
    children: BTreeMap<String, Node>,
    dir: bool,
    change: Option<Change>,
}

pub fn print(
    label: &str,
    wrote: &[(PathBuf, Change)],
    describe: fn(&Path, Option<&Change>) -> String,
) {
    let mut lines = Vec::new();
    walk(&of(wrote), Path::new(""), "", describe, &mut lines);
    println!("\n{label}/");
    for (shown, said) in &lines {
        match said.is_empty() {
            true => println!("{shown}"),
            false => println!("{shown}  {}", phrase(said)),
        }
    }
}

fn phrase(said: &str) -> String {
    match colour() {
        false => said.to_string(),
        true => said.dim().to_string(),
    }
}

fn of(wrote: &[(PathBuf, Change)]) -> Node {
    let mut tree = Node::default();
    for (path, change) in wrote {
        let parts: Vec<_> = path
            .components()
            .filter(|part| !matches!(part, std::path::Component::CurDir))
            .collect();
        let depth = parts.len();
        let mut node = &mut tree;
        for (at, part) in parts.into_iter().enumerate() {
            node = node
                .children
                .entry(part.as_os_str().to_string_lossy().into_owned())
                .or_default();
            node.dir |= at + 1 < depth;
            if at + 1 == depth {
                node.change = Some(change.clone());
            }
        }
    }
    tree
}

fn walk(
    node: &Node,
    at: &Path,
    prefix: &str,
    describe: fn(&Path, Option<&Change>) -> String,
    out: &mut Vec<(String, String)>,
) {
    let mut names: Vec<&String> = node.children.keys().collect();
    names.sort_by_key(|name| !node.children[*name].dir);
    for (index, name) in names.iter().enumerate() {
        let child = &node.children[*name];
        let (branch, carry) = match index + 1 == names.len() {
            true => ("└── ", "    "),
            false => ("├── ", "│   "),
        };
        let path = at.join(name);
        let prefix_shown = format!("{prefix}{branch}");
        let shown = match child.dir {
            true => format!("{prefix_shown}{name}/"),
            false => {
                let change = child.change.clone().unwrap_or(Change::Created);
                format!("{prefix_shown}{}{name}", mark(&change))
            }
        };
        out.push((shown, describe(&path, child.change.as_ref())));
        walk(child, &path, &format!("{prefix}{carry}"), describe, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said(_path: &Path, change: Option<&Change>) -> String {
        match change {
            Some(Change::Updated(edit)) => edit.clone(),
            _ => String::new(),
        }
    }

    fn drawn(wrote: &[(&str, Change)]) -> Vec<(String, String)> {
        let wrote: Vec<(PathBuf, Change)> = wrote
            .iter()
            .map(|(path, change)| (PathBuf::from(path), change.clone()))
            .collect();
        let mut lines = Vec::new();
        walk(&of(&wrote), Path::new(""), "", said, &mut lines);
        lines
    }

    #[test]
    fn directories_come_first_and_carry_the_branch_their_children_hang_off() {
        let lines = drawn(&[
            ("repo.kdl", Change::Updated(String::new())),
            (
                "modules/mine/module.kdl",
                Change::Updated("rewritten by hand".into()),
            ),
            ("README.md", Change::Created),
        ]);
        let branches: Vec<&str> = lines.iter().map(|(branch, _)| branch.as_str()).collect();
        assert_eq!(
            branches,
            [
                "├── modules/",
                "│   └── mine/",
                "│       └── ~ module.kdl",
                "├── + README.md",
                "└── ~ repo.kdl",
            ]
        );
        assert_eq!(lines[0].1, "");
        assert_eq!(lines[2].1, "rewritten by hand");
        assert_eq!(lines[3].1, "");
        assert_eq!(lines[4].1, "");
    }
}
