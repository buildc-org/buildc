use super::{colour, width};
use ratatui::crossterm::style::Stylize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const GAP: usize = 2;

const LEAST: usize = 12;

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

    let column = lines
        .iter()
        .filter(|(_, _, desc)| !desc.is_empty())
        .map(|(plain, _, _)| plain.chars().count() + GAP)
        .max()
        .unwrap_or(0);
    let room = width().saturating_sub(column);

    println!("\n{label}/");
    for (plain, shown, desc) in &lines {
        match fit(desc, room) {
            "" => println!("{shown}"),
            desc => println!(
                "{shown}{}{desc}",
                " ".repeat(column - plain.chars().count())
            ),
        }
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
    out: &mut Vec<(String, String, String)>,
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
        let (plain, shown) = match child.dir {
            true => {
                let base = format!("{prefix_shown}{name}/");
                (base.clone(), base)
            }
            false => {
                let change = child.change.clone().unwrap_or(Change::Created);
                let bare = match change {
                    Change::Created => "+ ",
                    Change::Updated(_) => "~ ",
                };
                (
                    format!("{prefix_shown}{bare}{name}"),
                    format!("{prefix_shown}{}{name}", mark(&change)),
                )
            }
        };
        out.push((plain, shown, describe(&path, child.change.as_ref())));
        walk(child, &path, &format!("{prefix}{carry}"), describe, out);
    }
}

fn fit(desc: &str, room: usize) -> &str {
    if desc.chars().count() <= room {
        return desc;
    }
    let cut = desc
        .char_indices()
        .take(room + 1)
        .filter(|(_, c)| *c == ' ')
        .last()
        .map_or(0, |(at, _)| at);
    match cut >= LEAST {
        true => &desc[..cut],
        false => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn phrase(path: &Path, change: Option<&Change>) -> String {
        if let Some(Change::Updated(edit)) = change {
            if !edit.is_empty() {
                return edit.clone();
            }
        }
        match path.to_string_lossy().as_ref() {
            "modules" => "every module the repo holds",
            "repo.kdl" => "what the repo pins",
            _ => "",
        }
        .to_string()
    }

    fn drawn(wrote: &[(&str, Change)]) -> Vec<(String, String)> {
        let wrote: Vec<(PathBuf, Change)> = wrote
            .iter()
            .map(|(path, change)| (PathBuf::from(path), change.clone()))
            .collect();
        let mut lines = Vec::new();
        walk(&of(&wrote), Path::new(""), "", phrase, &mut lines);
        lines
            .into_iter()
            .map(|(plain, _, desc)| (plain, desc))
            .collect()
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
        assert_eq!(lines[0].1, "every module the repo holds");
        assert_eq!(lines[2].1, "rewritten by hand");
        assert_eq!(lines[3].1, "");
        assert_eq!(lines[4].1, "what the repo pins");
    }

    #[test]
    fn a_description_is_cut_at_a_word_and_dropped_before_it_says_nothing() {
        assert_eq!(fit("what the repo pins", 18), "what the repo pins");
        assert_eq!(fit("what the repo pins", 17), "what the repo");
        assert_eq!(fit("what the repo pins", 13), "what the repo");
        assert_eq!(fit("what the repo pins", 12), "");
        assert_eq!(fit("what the repo pins", 0), "");
    }
}
