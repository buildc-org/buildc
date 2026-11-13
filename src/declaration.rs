use crate::layout;
use std::path::Path;

fn listed_in(flavour: Option<&str>) -> Vec<(&str, Option<&str>)> {
    flavour
        .map(|flavour| ("flavour", Some(flavour)))
        .into_iter()
        .collect()
}

pub(crate) fn chain<'a>(
    flavour: Option<&'a str>,
    name: &'a str,
) -> Vec<(&'a str, Option<&'a str>)> {
    let mut chain = listed_in(flavour);
    chain.extend(crate::parse::image::segments(name));
    chain
}

pub(crate) fn placed<'a>(
    flavour: Option<&'a str>,
    name: &'a str,
    block: &str,
) -> (Vec<(&'a str, Option<&'a str>)>, String) {
    let mut chain = listed_in(flavour);
    let (parents, last) = name.rsplit_once('/').unwrap_or(("", name));
    chain.extend(
        parents
            .split('/')
            .filter(|part| !part.is_empty())
            .map(|part| (part, None)),
    );
    (chain, format!("{last}{block}"))
}

pub(crate) fn wrap(blocks: &[(&str, Option<&str>)], leaf: &str) -> String {
    blocks
        .iter()
        .rev()
        .fold(leaf.to_string(), |inner, (node, arg)| {
            let arg = arg.map_or(String::new(), |arg| format!(" \"{arg}\""));
            let inner: String = inner.lines().map(|line| format!("    {line}\n")).collect();
            format!("{node}{arg} {{\n{inner}}}")
        })
}

pub(crate) fn append(
    file: &Path,
    image: &str,
    chain: &[(&str, Option<&str>)],
    leaf: &str,
) -> Result<(), String> {
    let text = std::fs::read_to_string(file).map_err(|err| format!("{}: {err}", file.display()))?;
    let (kept, close) = (0..=chain.len())
        .rev()
        .find_map(|kept| {
            crate::parse::image::block_close(&text, image, &chain[..kept]).map(|at| (kept, at))
        })
        .ok_or_else(|| format!("{} declares no image `{image}`", file.display()))?;
    let declaration = wrap(&chain[kept..], leaf);
    let out = inside(&text, close, &declaration);
    std::fs::write(file, out).map_err(|err| format!("{}: {err}", file.display()))
}

/// `close` is the byte offset of a block's closing brace in `text`.
pub(crate) fn inside(text: &str, close: usize, declaration: &str) -> String {
    let mut out = text.to_string();
    let start = text[..close].rfind('\n').map_or(0, |at| at + 1);
    let indent = &text[start..close];
    let (at, line) = match indent.trim().is_empty() {
        true => (
            start,
            declaration
                .lines()
                .map(|line| format!("{indent}    {line}\n"))
                .collect(),
        ),
        false => {
            let end = text[..close].trim_end().len();
            let gap = match text[..end].ends_with(['{', ';']) {
                true => " ",
                false => "; ",
            };
            (end, format!("{gap}{}", one_line(declaration)))
        }
    };
    out.insert_str(at, &line);
    out
}

// KDL needs a semicolon between two nodes on one line.
fn one_line(declaration: &str) -> String {
    let mut out = String::new();
    for line in declaration.lines().map(str::trim) {
        if !out.is_empty() {
            out.push_str(match out.ends_with('{') || line.starts_with('}') {
                true => " ",
                false => "; ",
            });
        }
        out.push_str(line);
    }
    out
}

pub(crate) fn unlisting(
    file: &Path,
    text: &str,
    image: &str,
    flavour: Option<&str>,
    name: &str,
) -> Result<String, String> {
    let root = file.parent().unwrap_or(Path::new("."));
    let chain = chain(flavour, name);
    crate::parse::image::unlisted(text, image, &chain, |path| {
        layout::is_component(&layout::component(root, path))
    })
    .map_err(|err| format!("{} {err}", file.display()))
}

pub(crate) fn splice(text: &str, was: Option<crate::diag::Span>, block: &str) -> String {
    let mut out = text.to_string();
    match was {
        Some(was) => {
            let end = was.offset + was.len;
            let start = match block.is_empty() {
                true => text[..was.offset].trim_end_matches(['\n', ' ']).len(),
                false => was.offset,
            };
            out.replace_range(start..end, block);
        }
        None if block.is_empty() => {}
        None => {
            if !out.ends_with('\n') {
                out.push('\n');
            }
            out.push_str(&format!("\n{block}\n"));
        }
    }
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

pub(crate) fn line_indent(text: &str, at: usize) -> &str {
    let start = text[..at].rfind('\n').map_or(0, |found| found + 1);
    let line = &text[start..at];
    &line[..line.len() - line.trim_start().len()]
}
