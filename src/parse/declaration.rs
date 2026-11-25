use crate::layout;
use kdl::{KdlDocument, KdlNode};
use std::path::Path;

pub(crate) fn quotable(value: &str) -> Result<&str, String> {
    match value.contains(['"', '\\', '\n']) {
        true => Err(format!(
            "`{value}` is not writable into a manifest as it is"
        )),
        false => Ok(value),
    }
}

pub(crate) fn placed<'a>(outside: bool, name: &'a str, block: &str) -> (Vec<&'a str>, String) {
    let (parents, last) = name.rsplit_once('/').unwrap_or(("", name));
    (
        crate::parse::image::chain(outside, parents),
        format!("{last}{block}"),
    )
}

pub(crate) fn wrap(blocks: &[&str], leaf: &str) -> String {
    blocks.iter().rev().fold(leaf.to_string(), |inner, node| {
        let inner: String = inner.lines().map(|line| format!("    {line}\n")).collect();
        match *node {
            crate::parse::image::ROOT => format!("component \"/\" {{\n{inner}}}"),
            _ => format!("{node} {{\n{inner}}}"),
        }
    })
}

pub(crate) fn append(file: &Path, chain: &[&str], leaf: &str) -> Result<(), String> {
    let text = std::fs::read_to_string(file).map_err(|err| format!("{}: {err}", file.display()))?;
    let mut doc = document(&text).map_err(|err| format!("{} {err}", file.display()))?;
    grown(&mut doc, chain, leaf)?;
    let out = written(&doc).map_err(|err| format!("{} {err}", file.display()))?;
    std::fs::write(file, out).map_err(|err| format!("{}: {err}", file.display()))
}

fn grown(doc: &mut KdlDocument, chain: &[&str], leaf: &str) -> Result<(), String> {
    let path = longest_held_prefix(doc, chain);
    let mut here = doc;
    for at in &path {
        here = opened_block(&mut here.nodes_mut()[*at]);
    }
    let leading = match path.is_empty() && !chain.is_empty() && !here.nodes().is_empty() {
        true => "\n",
        false => "",
    };
    push(here, path.len(), &wrap(&chain[path.len()..], leaf), leading)
}

fn document(text: &str) -> Result<KdlDocument, String> {
    let mut doc: KdlDocument = text.parse().map_err(|_| "is not valid KDL".to_string())?;
    restored(doc.nodes_mut(), text);
    match doc.to_string() == text {
        true => Ok(doc),
        false => {
            Err("is left as it was because buildc cannot rewrite its layout as written".to_string())
        }
    }
}

/// A node with children parses with an empty terminator and loses the space and terminator after its closing brace. This walk puts them back so that the document prints as it was read.
fn restored(nodes: &mut [KdlNode], text: &str) {
    for node in nodes {
        let Some(children) = node.children_mut() else {
            continue;
        };
        restored(children.nodes_mut(), text);
        let span: crate::diag::Span = node.span().into();
        let mut bare = node.clone();
        let Some(format) = bare.format_mut() else {
            continue;
        };
        if !format.terminator.is_empty() {
            continue;
        }
        format.leading.clear();
        format.trailing.clear();
        let closed = span.offset + bare.to_string().len();
        let end = span.offset + span.len;
        let rest = &text[end..];
        let terminator = match rest.chars().next() {
            Some(';') => ";",
            Some('\n') => "\n",
            _ if rest.starts_with("\r\n") => "\r\n",
            _ if rest.starts_with("//") => rest.find('\n').map_or(rest, |at| &rest[..=at]),
            _ => "",
        };
        if let (Some(gap), Some(format)) = (text.get(closed..end), node.format_mut()) {
            format.before_terminator.push_str(gap);
            format.terminator = terminator.to_string();
        }
    }
}

fn opened_block(node: &mut KdlNode) -> &mut KdlDocument {
    if node.children().is_none() {
        if let Some(format) = node.format_mut() {
            format.before_children = " ".into();
        }
    }
    node.ensure_children()
}

pub(crate) fn workflows(
    text: &str,
    chosen: &[&str],
    entries: &[(&str, String)],
) -> Result<String, String> {
    let mut doc = document(text)?;
    let entry = |stem: &str| {
        entries
            .iter()
            .find(|(name, _)| *name == stem)
            .map(|(_, entry)| entry.as_str())
    };
    let chain = ["github", "workflows"];
    let path = longest_held_prefix(&doc, &chain);
    let [github, block] = path[..] else {
        if !chosen.is_empty() {
            let leaf: Vec<&str> = chosen
                .iter()
                .map(|stem| entry(stem).unwrap_or(stem))
                .collect();
            grown(&mut doc, &chain, &leaf.join("\n"))?;
        }
        return written(&doc);
    };
    let held = opened_block(&mut doc.nodes_mut()[github]);
    if chosen.is_empty() {
        removed(held, block);
        if held.nodes().is_empty() {
            removed(&mut doc, github);
        }
        return written(&doc);
    }
    let listed = opened_block(&mut held.nodes_mut()[block]);
    for at in (0..listed.nodes().len()).rev() {
        if !chosen.contains(&listed.nodes()[at].name().value()) {
            removed(listed, at);
        }
    }
    for stem in chosen {
        let at = listed
            .nodes()
            .iter()
            .position(|node| node.name().value() == *stem);
        match (at, entry(stem)) {
            (Some(at), Some(entry)) => replaced(listed, at, chain.len(), entry)?,
            // A listed workflow with no new entry keeps the triggers written for it.
            (Some(_), None) => {}
            (None, entry) => push(listed, chain.len(), entry.unwrap_or(stem), "")?,
        }
    }
    written(&doc)
}

pub(crate) fn append_inside(
    file: &Path,
    node: crate::diag::Span,
    declaration: &str,
) -> Result<(), String> {
    let text = std::fs::read_to_string(file).map_err(|err| format!("{}: {err}", file.display()))?;
    let mut doc = document(&text).map_err(|err| format!("{} {err}", file.display()))?;
    let (here, at, depth) = spanned(&mut doc, node, 0)
        .ok_or_else(|| format!("{} holds no node where buildc read one", file.display()))?;
    let children = here.nodes_mut()[at].ensure_children();
    push(children, depth + 1, declaration, "")?;
    let out = written(&doc).map_err(|err| format!("{} {err}", file.display()))?;
    std::fs::write(file, out).map_err(|err| format!("{}: {err}", file.display()))
}

/// Returns how many blocks of `chain` the file already holds and the last `library=` that those blocks carry.
pub(crate) fn held(file: &Path, chain: &[&str]) -> Result<(usize, Option<String>), String> {
    let text = std::fs::read_to_string(file).map_err(|err| format!("{}: {err}", file.display()))?;
    let doc: KdlDocument = text
        .parse()
        .map_err(|_| format!("{} is not valid KDL", file.display()))?;
    let path = longest_held_prefix(&doc, chain);
    let mut nodes = doc.nodes();
    let mut library = None;
    for at in &path {
        let node = &nodes[*at];
        if let Some(name) = crate::parse::prop(node, "library") {
            library = Some(name.to_string());
        }
        nodes = node.children().map(KdlDocument::nodes).unwrap_or_default();
    }
    Ok((path.len(), library))
}

fn longest_held_prefix(doc: &KdlDocument, chain: &[&str]) -> Vec<usize> {
    (0..=chain.len())
        .rev()
        .find_map(|kept| blocks(doc.nodes(), &chain[..kept]))
        .unwrap_or_default()
}

fn blocks(nodes: &[KdlNode], chain: &[&str]) -> Option<Vec<usize>> {
    let Some((name, rest)) = chain.split_first() else {
        return Some(Vec::new());
    };
    nodes.iter().enumerate().find_map(|(at, node)| {
        let named = match *name {
            crate::parse::image::ROOT => {
                node.name().value() == crate::parse::tree::CONTEXT.name
                    && crate::parse::string_arg(node) == Some(crate::parse::image::ROOT)
            }
            _ => node.name().value() == *name,
        };
        if !named {
            return None;
        }
        let kids = node.children().map(KdlDocument::nodes).unwrap_or_default();
        let mut found = blocks(kids, rest)?;
        found.insert(0, at);
        Some(found)
    })
}

fn push(
    doc: &mut KdlDocument,
    depth: usize,
    declaration: &str,
    leading: &str,
) -> Result<(), String> {
    let added = |text: String| {
        text.parse::<KdlDocument>()
            .map(|added| added.nodes().to_vec())
            .map_err(|err| format!("buildc wrote a declaration that is not KDL: {err}"))
    };
    let closing = match depth {
        0 => {
            ended(doc);
            Close::Lines
        }
        _ => opened(doc),
    };
    let mut nodes = match closing {
        Close::OneLine(closing) => {
            let mut nodes = added(one_lined(declaration))?;
            if let Some(last) = nodes.last_mut().and_then(KdlNode::format_mut) {
                last.before_terminator = closing;
            }
            if let Some(first) = nodes.first_mut().and_then(KdlNode::format_mut) {
                first.leading = " ".into();
            }
            nodes
        }
        Close::Lines => {
            let indent = "    ".repeat(depth);
            added(
                declaration
                    .lines()
                    .map(|line| format!("{indent}{line}\n"))
                    .collect(),
            )?
        }
    };
    if let Some(format) = nodes.first_mut().and_then(KdlNode::format_mut) {
        format.leading = format!("{leading}{}", format.leading);
    }
    doc.nodes_mut().extend(nodes);
    Ok(())
}

fn ended(doc: &mut KdlDocument) {
    if let Some(last) = doc.nodes_mut().last_mut().and_then(KdlNode::format_mut) {
        if !last.terminator.contains('\n') {
            last.terminator.push('\n');
        }
    }
}

enum Close {
    OneLine(String),
    Lines,
}

fn opened(doc: &mut KdlDocument) -> Close {
    match doc.nodes_mut().last_mut().and_then(KdlNode::format_mut) {
        Some(last) if !last.terminator.contains('\n') && !last.trailing.contains('\n') => {
            last.terminator = ";".into();
            Close::OneLine(std::mem::take(&mut last.before_terminator))
        }
        Some(_) => Close::Lines,
        None => {
            // An empty block holds the space inside it in the block's own format.
            let Some(format) = doc.format_mut() else {
                return Close::Lines;
            };
            let space = format!("{}{}", format.leading, format.trailing);
            match space.rfind('\n') {
                Some(at) => {
                    format.trailing = space[at + 1..].to_string();
                    format.leading = space[..=at].to_string();
                    Close::Lines
                }
                None => {
                    format.leading.clear();
                    format.trailing.clear();
                    Close::OneLine(" ".into())
                }
            }
        }
    }
}

fn written(doc: &KdlDocument) -> Result<String, String> {
    let mut out = doc.to_string();
    if !out.ends_with('\n') {
        out.push('\n');
    }
    let bare = |doc: &KdlDocument| {
        let mut doc = doc.clone();
        doc.clear_format_recursive();
        doc
    };
    match out.parse::<KdlDocument>() {
        Ok(back) if bare(&back) == bare(doc) => Ok(out),
        _ => Err(
            "is left as it was because buildc's edit did not read back as the KDL it meant to write"
                .to_string(),
        ),
    }
}

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
            (end, format!("{gap}{}", one_lined(declaration)))
        }
    };
    out.insert_str(at, &line);
    out
}

fn one_lined(declaration: &str) -> String {
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

pub(crate) fn packages(
    text: &str,
    node: crate::diag::Span,
    names: &[String],
) -> Result<String, String> {
    let mut doc = document(text)?;
    let (held, at, depth) = spanned(&mut doc, node, 0)
        .ok_or_else(|| "holds no node where buildc read one".to_string())?;
    let packages = &mut held.nodes_mut()[at];
    let quoted: Vec<String> = names.iter().map(|name| format!("{name:?}")).collect();
    let positional = packages
        .entries()
        .iter()
        .rposition(|entry| entry.name().is_none());
    match packages.children_mut() {
        Some(block) if positional.is_none() => push(block, depth + 1, &quoted.join("\n"), "")?,
        _ => {
            let added: KdlDocument = format!("packages {}", quoted.join(" "))
                .parse()
                .map_err(|_| "buildc wrote package names that are not KDL".to_string())?;
            let at = positional.map_or(0, |at| at + 1);
            let entries = added.nodes()[0].entries().iter().cloned();
            packages.entries_mut().splice(at..at, entries);
        }
    }
    written(&doc)
}

/// A property that the named node holds keeps its value.
pub(crate) fn property(file: &Path, node: &str, name: &str, value: &str) -> Result<(), String> {
    let text = std::fs::read_to_string(file).map_err(|err| format!("{}: {err}", file.display()))?;
    let mut doc = document(&text).map_err(|err| format!("{} {err}", file.display()))?;
    let held = doc
        .nodes_mut()
        .iter_mut()
        .find(|held| held.name().value() == node)
        .ok_or_else(|| format!("{} holds no `{node}`", file.display()))?;
    if held.get(name).is_some() {
        return Ok(());
    }
    let entry = format!("{node} {name}={value:?}")
        .parse::<KdlDocument>()
        .ok()
        .and_then(|added| added.nodes().first()?.entries().first().cloned())
        .ok_or_else(|| format!("buildc wrote a `{name}=` that is not KDL"))?;
    held.entries_mut().push(entry);
    let out = written(&doc).map_err(|err| format!("{} {err}", file.display()))?;
    std::fs::write(file, out).map_err(|err| format!("{}: {err}", file.display()))
}

pub(crate) fn argument(
    text: &str,
    node: &str,
    value: &str,
    stays_above: fn(&str) -> bool,
) -> Result<String, String> {
    let mut doc = document(text)?;
    let mut added = format!("{node} {value:?}\n")
        .parse::<KdlDocument>()
        .ok()
        .and_then(|added| added.nodes().first().cloned())
        .ok_or_else(|| format!("buildc wrote a `{node}` that is not KDL"))?;
    match doc
        .nodes_mut()
        .iter_mut()
        .find(|held| held.name().value() == node)
    {
        Some(held) => {
            let value = added.entries()[0].clone();
            match held
                .entries()
                .iter()
                .position(|entry| entry.name().is_none())
            {
                Some(at) => held.entries_mut()[at] = value,
                None => held.entries_mut().insert(0, value),
            }
        }
        None => {
            let at = doc
                .nodes()
                .iter()
                .position(|held| !stays_above(held.name().value()))
                .unwrap_or(doc.nodes().len());
            if let Some(next) = doc.nodes_mut().get_mut(at).and_then(KdlNode::format_mut) {
                if let Some(format) = added.format_mut() {
                    let comment = next.leading.split_off(own_lines(&next.leading));
                    format.leading = std::mem::replace(&mut next.leading, format!("\n{comment}"));
                }
            }
            doc.nodes_mut().insert(at, added);
        }
    }
    written(&doc)
}

pub(crate) fn unlisting(
    root: &Path,
    file: &Path,
    text: &str,
    name: &str,
    outside: bool,
) -> Result<String, String> {
    crate::parse::image::unlisted(text, &crate::parse::image::chain(outside, name), |path| {
        layout::is_component(&layout::component(root, path))
    })
    .map_err(|err| format!("{} {err}", file.display()))
}

pub(crate) fn splice(
    text: &str,
    was: Option<crate::diag::Span>,
    block: &str,
) -> Result<String, String> {
    let mut doc = document(text)?;
    match was {
        Some(was) => {
            let (held, at, depth) = spanned(&mut doc, was, 0)
                .ok_or_else(|| "holds no node where buildc read one".to_string())?;
            match block.is_empty() {
                true => removed(held, at),
                false => replaced(held, at, depth, block)?,
            }
        }
        None => {
            let leading = match doc.nodes().is_empty() {
                true => "",
                false => "\n",
            };
            push(&mut doc, 0, block, leading)?;
        }
    }
    written(&doc)
}

pub(crate) fn reference(text: &str, node: crate::diag::Span) -> Result<String, String> {
    let mut doc = document(text)?;
    let (held, at, _) = spanned(&mut doc, node, 0)
        .ok_or_else(|| "holds no node where buildc read one".to_string())?;
    let held = &mut held.nodes_mut()[at];
    held.entries_mut().retain(|entry| {
        !entry
            .name()
            .is_some_and(|key| ["library", "pin"].contains(&key.value()))
    });
    written(&doc)
}

pub(crate) enum Carried {
    Own,
    Above {
        node: String,
        line: usize,
        props: String,
    },
    Bare,
}

pub(crate) fn carried(text: &str, node: crate::diag::Span) -> Carried {
    let Ok(doc) = text.parse::<KdlDocument>() else {
        return Carried::Bare;
    };
    let mut chain: Vec<&KdlNode> = Vec::new();
    if !ancestry(doc.nodes(), node.offset, &mut chain) {
        return Carried::Bare;
    }
    let props = |held: &KdlNode| -> Vec<String> {
        held.entries()
            .iter()
            .filter(|entry| {
                entry
                    .name()
                    .is_some_and(|key| ["library", "pin"].contains(&key.value()))
            })
            .map(|entry| format!("`{}`", entry.to_string().trim()))
            .collect()
    };
    let (own, above) = chain.split_last().expect("the walk found the node");
    if let Some(held) = above.iter().rev().find(|held| !props(held).is_empty()) {
        let offset = held.span().offset();
        return Carried::Above {
            node: match crate::parse::string_arg(held) {
                Some(arg) => format!("{} {arg:?}", held.name().value()),
                None => held.name().value().to_string(),
            },
            line: text[..offset].matches('\n').count() + 1,
            props: crate::said(&props(held)),
        };
    }
    match props(own).is_empty() {
        true => Carried::Bare,
        false => Carried::Own,
    }
}

fn ancestry<'a>(nodes: &'a [KdlNode], offset: usize, chain: &mut Vec<&'a KdlNode>) -> bool {
    for node in nodes {
        chain.push(node);
        if node.span().offset() == offset {
            return true;
        }
        if node
            .children()
            .is_some_and(|kids| ancestry(kids.nodes(), offset, chain))
        {
            return true;
        }
        chain.pop();
    }
    false
}

fn replaced(doc: &mut KdlDocument, at: usize, depth: usize, block: &str) -> Result<(), String> {
    let old = doc.nodes_mut().remove(at);
    let mut held = KdlDocument::new();
    push(&mut held, depth, block, "")?;
    let mut new = held.nodes().to_vec();
    if let Some(old) = old.format() {
        if let Some(first) = new.first_mut().and_then(KdlNode::format_mut) {
            first.leading = old.leading.clone();
        }
        if let Some(last) = new.last_mut().and_then(KdlNode::format_mut) {
            last.terminator = old.terminator.clone();
            last.trailing = old.trailing.clone();
        }
    }
    doc.nodes_mut().splice(at..at, new);
    Ok(())
}

// A block's first node holds the line break after the opening brace in its leading space.
fn removed(doc: &mut KdlDocument, at: usize) {
    let old = doc.nodes_mut().remove(at);
    if at > 0 {
        return;
    }
    let leading = old.format().map_or("", |format| format.leading.as_str());
    match doc.nodes_mut().first_mut().and_then(KdlNode::format_mut) {
        Some(next) => {
            let own = next.leading.split_off(own_lines(&next.leading));
            next.leading = format!("{}{own}", &leading[..own_lines(leading)]);
        }
        None => {
            if let Some(format) = doc.format_mut() {
                let broken = leading.rfind('\n').map_or("", |end| &leading[..=end]);
                format.trailing = format!("{broken}{}", format.trailing);
            }
        }
    }
}

fn own_lines(leading: &str) -> usize {
    let text = leading
        .find(|c: char| !c.is_whitespace())
        .unwrap_or(leading.len());
    leading[..text].rfind('\n').map_or(0, |at| at + 1)
}

fn spanned(
    doc: &mut KdlDocument,
    was: crate::diag::Span,
    depth: usize,
) -> Option<(&mut KdlDocument, usize, usize)> {
    let at = doc
        .nodes()
        .iter()
        .position(|node| node.span().offset() == was.offset);
    match at {
        Some(at) => Some((doc, at, depth)),
        None => doc
            .nodes_mut()
            .iter_mut()
            .filter_map(|node| node.children_mut().as_mut())
            .find_map(|kids| spanned(kids, was, depth + 1)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn appended(name: &str, text: &str, chain: &[&str], leaf: &str) -> String {
        let file = std::env::temp_dir().join(format!(
            "buildc-declaration-{name}-{}.kdl",
            std::process::id()
        ));
        std::fs::write(&file, text).unwrap();
        append(&file, chain, leaf).unwrap();
        let out = std::fs::read_to_string(&file).unwrap();
        std::fs::remove_file(&file).unwrap();
        out
    }

    #[test]
    fn a_comment_after_a_closing_brace_is_kept() {
        for text in [
            "a {\n    b\n} // c\n",
            "a {\n    b\n} // c\nd\n",
            "a { b } /* c */\nd\n",
            "a {\n    b {\n    } // c\n} // d\n",
            "a {\n}  ;d\n",
            "a {\n} // c",
            "a {\r\n} \r\nd\r\n",
        ] {
            assert_eq!(
                document(text).map(|doc| doc.to_string()),
                Ok(text.to_string())
            );
        }
    }

    #[test]
    fn a_packages_node_with_names_and_a_block_takes_the_new_name_after_its_last_name() {
        let text = "packages \"a\" {\n    \"b\"\n}\n";
        let span = crate::diag::Span {
            offset: 0,
            len: text.trim_end().len(),
        };
        assert_eq!(
            packages(text, span, &["c".to_string()]).unwrap(),
            "packages \"a\" \"c\" {\n    \"b\"\n}\n"
        );
    }

    #[test]
    fn a_new_node_lands_after_a_comment_on_a_closing_brace() {
        assert_eq!(
            appended("brace-comment", "a {\n    b\n} // c\n", &[], "d"),
            "a {\n    b\n} // c\nd\n"
        );
    }

    #[test]
    fn a_rendering_that_reads_back_as_other_nodes_is_refused() {
        let mut doc: KdlDocument = "a\nb\n".parse().unwrap();
        if let Some(format) = doc.nodes_mut()[0].format_mut() {
            format.terminator = String::new();
        }
        assert!(written(&doc).is_err());
    }

    #[test]
    fn an_empty_block_takes_the_new_node() {
        assert_eq!(
            appended("empty", "apps {}\n", &["apps"], "packages \"htop\""),
            "apps { packages \"htop\" }\n"
        );
    }

    #[test]
    fn a_file_without_a_final_newline_takes_the_new_node_on_its_own_line() {
        assert_eq!(
            appended("unended", "a \"1\"", &[], "b \"2\""),
            "a \"1\"\nb \"2\"\n"
        );
    }

    #[test]
    fn an_end_of_file_comment_without_a_final_newline_moves_as_with_one() {
        assert_eq!(
            appended("unended-comment", "a \"1\"\n// tail", &[], "b \"2\""),
            appended("ended-comment", "a \"1\"\n// tail\n", &[], "b \"2\"")
        );
    }

    #[test]
    fn a_trailing_comment_without_a_final_newline_is_kept() {
        assert_eq!(
            appended("unended-trailing", "a \"1\" // tail", &[], "b \"2\""),
            "a \"1\" // tail\nb \"2\"\n"
        );
    }

    #[test]
    fn a_new_argument_node_leaves_the_next_node_its_comment() {
        assert_eq!(
            argument(
                "schema-version 1\n\n// about b\nb\n",
                "conforms",
                "x",
                |name| name == "schema-version"
            )
            .unwrap(),
            "schema-version 1\n\nconforms \"x\"\n\n// about b\nb\n"
        );
    }

    #[test]
    fn a_removed_first_node_takes_its_comment_and_leaves_the_next_its_own() {
        let mut doc = document("a {\n    // about b\n    b\n    // about c\n    c\n}\n").unwrap();
        removed(doc.nodes_mut()[0].children_mut().as_mut().unwrap(), 0);
        assert_eq!(doc.to_string(), "a {\n    // about c\n    c\n}\n");
    }

    #[test]
    fn a_new_argument_node_takes_the_blank_line_before_the_next_node() {
        assert_eq!(
            argument("schema-version 1\n\nb\n", "conforms", "x", |name| {
                name == "schema-version"
            })
            .unwrap(),
            "schema-version 1\n\nconforms \"x\"\n\nb\n"
        );
    }

    #[test]
    fn a_later_block_of_one_name_is_found_when_the_first_lacks_the_path() {
        assert_eq!(
            appended(
                "later",
                "apps {\n    one\n}\napps {\n    two {\n    }\n}\n",
                &["apps", "two"],
                "packages \"htop\""
            ),
            "apps {\n    one\n}\napps {\n    two {\n        packages \"htop\"\n    }\n}\n"
        );
    }

    #[test]
    fn the_root_context_is_found_past_another_context() {
        assert_eq!(
            appended(
                "root",
                "component \"other\" {\n    one\n}\ncomponent \"/\" {\n    two\n}\n",
                &[crate::parse::image::ROOT],
                "three"
            ),
            "component \"other\" {\n    one\n}\ncomponent \"/\" {\n    two\n    three\n}\n"
        );
    }

    #[test]
    fn a_bare_node_takes_the_new_node_as_its_block() {
        assert_eq!(
            appended("bare", "tools library=\"shelf\"\n", &["tools"], "podman"),
            "tools library=\"shelf\" {\n    podman\n}\n"
        );
    }

    #[test]
    fn a_bare_node_keeps_its_trailing_comment_when_it_takes_a_block() {
        assert_eq!(
            appended(
                "bare-comment",
                "a\ntools library=\"shelf\" // kept\nb\n",
                &["tools"],
                "podman"
            ),
            "a\ntools library=\"shelf\" {\n    podman\n} // kept\nb\n"
        );
    }

    #[test]
    fn a_new_block_in_a_one_line_block_stays_on_the_line() {
        assert_eq!(
            appended(
                "one-line",
                "apps { description \"Apps\" }\n",
                &["apps", "two"],
                "packages \"htop\""
            ),
            "apps { description \"Apps\"; two { packages \"htop\" } }\n"
        );
    }

    #[test]
    fn a_nested_node_is_replaced_in_place() {
        let text = "builds {\n    one {\n        publish \"a\"\n    }\n}\n";
        let span = crate::diag::Span {
            offset: text.find("publish").unwrap(),
            len: "publish \"a\"".len(),
        };
        assert_eq!(
            splice(text, Some(span), "publish \"b\"").unwrap(),
            "builds {\n    one {\n        publish \"b\"\n    }\n}\n"
        );
    }

    #[test]
    fn a_nested_block_is_replaced_at_its_depth() {
        let text = "builds {\n    one {\n        install {\n            filesystem \"a\"\n        }\n    }\n}\n";
        let span = crate::diag::Span {
            offset: text.find("install").unwrap(),
            len: "install {\n            filesystem \"a\"\n        }".len(),
        };
        assert_eq!(
            splice(text, Some(span), "install {\n    filesystem \"b\"\n}").unwrap(),
            "builds {\n    one {\n        install {\n            filesystem \"b\"\n        }\n    }\n}\n"
        );
    }

    #[test]
    fn a_removed_nested_node_leaves_its_block_closed_on_its_own_line() {
        let text = "builds {\n    one {\n        publish \"a\"\n    }\n}\n";
        let span = crate::diag::Span {
            offset: text.find("publish").unwrap(),
            len: "publish \"a\"".len(),
        };
        assert_eq!(
            splice(text, Some(span), "").unwrap(),
            "builds {\n    one {\n    }\n}\n"
        );
    }
}
