pub mod asset;
pub mod disk;
pub mod image;
pub mod module;
pub mod options;
pub mod remote;
pub mod repo;
pub mod schema;

use crate::diag::{Issue, Source, Span};
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

pub(crate) fn prop<'a>(node: &'a KdlNode, key: &str) -> Option<&'a str> {
    node.entries()
        .iter()
        .find(|e| e.name().map(|n| n.value()) == Some(key))
        .and_then(|e| e.value().as_string())
}

pub(crate) fn boolean(node: &KdlNode, key: &str) -> Option<bool> {
    node.entries()
        .iter()
        .find(|e| e.name().map(|n| n.value()) == Some(key))
        .and_then(|e| e.value().as_bool())
}

pub(crate) fn flag(node: &KdlNode, key: &str) -> bool {
    boolean(node, key).unwrap_or(false)
}

pub(crate) fn prop_span(node: &KdlNode, key: &str) -> Option<Span> {
    node.entries()
        .iter()
        .find(|e| e.name().map(|n| n.value()) == Some(key))
        .map(|e| e.span().into())
}

pub(crate) fn kids(node: &KdlNode) -> &[KdlNode] {
    node.children().map(|c| c.nodes()).unwrap_or_default()
}

pub(crate) fn child<'a>(node: &'a KdlNode, name: &str) -> Option<&'a KdlNode> {
    kids(node).iter().find(|c| c.name().value() == name)
}

pub(crate) fn text(node: &KdlNode, name: &str) -> String {
    child(node, name)
        .and_then(string_arg)
        .unwrap_or_default()
        .to_string()
}
