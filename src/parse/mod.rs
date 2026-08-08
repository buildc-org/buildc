pub mod asset;
pub mod disk;
pub mod image;
pub mod module;
pub mod options;
pub mod remote;
pub mod repo;

use crate::diag::{Issue, Source};
use kdl::KdlNode;

pub(crate) fn syntax_issue(err: &kdl::KdlError, file: &str, src: &Source) -> Issue {
    let mut issue = Issue::new(format!("{file} is not valid KDL"), src);
    for found in &err.diagnostics {
        let label = found
            .message
            .clone()
            .or_else(|| found.label.clone())
            .unwrap_or_else(|| "here".into());
        issue = issue.at(found.span, label);
    }
    match err.diagnostics.iter().find_map(|d| d.help.clone()) {
        Some(help) => issue.help(help),
        None => issue,
    }
}

pub(crate) fn string_arg(node: &KdlNode) -> Option<&str> {
    node.entries()
        .iter()
        .find(|e| e.name().is_none())
        .and_then(|e| e.value().as_string())
}

pub(crate) fn bool_arg(node: &KdlNode) -> Option<bool> {
    node.entries()
        .iter()
        .find(|e| e.name().is_none())
        .and_then(|e| e.value().as_bool())
}

pub(crate) fn string_args(node: &KdlNode) -> Vec<&str> {
    node.entries()
        .iter()
        .filter(|e| e.name().is_none())
        .filter_map(|e| e.value().as_string())
        .collect()
}
