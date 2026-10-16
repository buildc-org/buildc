use crate::diag::{Issue, Issues, Source, Span};
use crate::parse::{bool_arg, int_arg, kids, string_arg, string_args};
use kdl::{KdlDocument, KdlNode};

#[rustfmt::skip]
pub const FILE_SCHEMA_VERSION: Node = Node::new("schema-version",
    "The schema release that this file is written against.").about("The schema version tells `buildc` which reader to use for this file. A file written against an earlier release keeps using that release's reader, while `repo.kdl` sets the newest schema the repository accepts.").example("1")
    .arg(Arg::Int, Say::new("`{}` needs a number", "not a version", "`schema-version 1`"))
    .missing(Say::new("this file declares no `{}`", "no schema version",
        "`schema-version 1`, so the repository and every file state which reader they need"))
    .once("")
    .notes(&[
        "A file cannot declare a schema newer than `repo.kdl`.",
        "`buildc create repo`, `buildc create image` and `buildc create module` write the version, and `buildc` never changes it.",
    ]);

pub struct Say {
    pub text: &'static str,
    pub label: &'static str,
    pub help: &'static str,
}

impl Say {
    pub const NONE: Self = Self::new("", "", "");

    pub const fn new(text: &'static str, label: &'static str, help: &'static str) -> Self {
        Self { text, label, help }
    }

    const fn silent(&self) -> bool {
        self.text.is_empty()
    }

    fn raise(&self, about: &str, span: impl Into<Span>, src: &Source, issues: &mut Issues) {
        if self.silent() {
            return;
        }
        let issue = Issue::new(self.text.replace("{}", about), src).at(span, self.label);
        issues.push(match self.help.is_empty() {
            true => issue,
            false => issue.help(self.help.replace("{}", about)),
        });
    }
}

pub const NEEDS_VALUE: Say = Say::new("`{}` needs a value", "nothing given", "");

pub enum Arg {
    None,
    Str,
    Bool,
    Int,
    Strs,
    StrPair(&'static str, &'static str),
    One(&'static [&'static str]),
    MaybeOne(&'static [&'static str]),
}

pub enum Kind {
    Str,
    Bool,
    Int(i128, i128),
    One(&'static [&'static str]),
}

pub struct Prop {
    pub name: &'static str,
    pub desc: &'static str,
    pub kind: Kind,
    pub say: Say,
    pub missing: Say,
}

pub struct Node {
    pub name: &'static str,
    pub desc: &'static str,
    pub about: &'static str,
    pub arg: Arg,
    pub arg_say: Say,
    pub missing: Say,
    pub once: bool,
    pub dup_help: &'static str,
    pub unique: Say,
    pub empty: Say,
    pub props: &'static [Prop],
    pub prop_say: Say,
    pub children: &'static [Self],
    pub child_say: Say,
    pub notes: &'static [&'static str],
    pub values: &'static [(&'static str, &'static str)],
    pub lists: &'static [(&'static str, &'static [&'static str])],
    pub example: &'static str,
    pub minimal: bool,
    pub pick: bool,
    pub scaffolds: &'static [&'static str],
}

impl Node {
    pub const fn new(name: &'static str, desc: &'static str) -> Self {
        Self {
            name,
            desc,
            about: "",
            arg: Arg::None,
            arg_say: Say::NONE,
            missing: Say::NONE,
            once: false,
            dup_help: "",
            unique: Say::NONE,
            empty: Say::NONE,
            props: &[],
            prop_say: Say::NONE,
            children: &[],
            child_say: Say::NONE,
            notes: &[],
            values: &[],
            lists: &[],
            example: "",
            minimal: false,
            pick: false,
            scaffolds: &[],
        }
    }

    pub const fn scaffolds(mut self, names: &'static [&'static str]) -> Self {
        self.scaffolds = names;
        self
    }

    pub const fn about(mut self, about: &'static str) -> Self {
        self.about = about;
        self
    }

    pub const fn example(mut self, example: &'static str) -> Self {
        self.example = example;
        self
    }

    pub const fn minimal(mut self) -> Self {
        self.minimal = true;
        self
    }

    pub const fn pick(mut self) -> Self {
        self.minimal = true;
        self.pick = true;
        self
    }

    pub const fn notes(mut self, notes: &'static [&'static str]) -> Self {
        self.notes = notes;
        self
    }

    pub const fn values(mut self, values: &'static [(&'static str, &'static str)]) -> Self {
        self.values = values;
        self
    }

    pub const fn lists(
        mut self,
        lists: &'static [(&'static str, &'static [&'static str])],
    ) -> Self {
        self.lists = lists;
        self
    }

    pub const fn arg(mut self, arg: Arg, say: Say) -> Self {
        self.arg = arg;
        self.arg_say = say;
        self
    }

    pub const fn once(mut self, help: &'static str) -> Self {
        self.once = true;
        self.dup_help = help;
        self
    }

    pub const fn unique(mut self, say: Say) -> Self {
        self.unique = say;
        self
    }

    pub const fn missing(mut self, say: Say) -> Self {
        self.missing = say;
        self
    }

    pub const fn empty(mut self, say: Say) -> Self {
        self.empty = say;
        self
    }

    pub const fn props(mut self, props: &'static [Prop], say: Say) -> Self {
        self.props = props;
        self.prop_say = say;
        self
    }

    pub const fn children(mut self, children: &'static [Self], say: Say) -> Self {
        self.children = children;
        self.child_say = say;
        self
    }
}

#[cfg(test)]
pub fn check_text(text: &str, schema: &Node, declared: bool) -> Result<Issues, String> {
    let doc: KdlDocument = text.parse().map_err(|err| format!("{err}"))?;
    let src = Source::new(schema.name, text);
    let mut issues = Issues::default();
    match (declared, doc.nodes().first()) {
        (true, Some(node)) => check(node, schema, &src, &mut issues),
        (true, None) => return Err(format!("`{}` writes no node", schema.name)),
        (false, _) => check_doc(&doc, schema, &src, &mut issues),
    }
    Ok(issues)
}

pub fn check_doc(doc: &KdlDocument, schema: &Node, src: &Source, issues: &mut Issues) {
    walk(doc.nodes(), schema, Span::default(), src, issues);
}

pub fn check(node: &KdlNode, schema: &Node, src: &Source, issues: &mut Issues) {
    let here: Span = node.name().span().into();
    let about = match schema.name.is_empty() {
        true => node.name().value(),
        false => schema.name,
    };
    match schema.arg {
        Arg::None => {
            if let Some(stray) = string_arg(node) {
                schema.arg_say.raise(stray, here, src, issues);
            }
        }
        Arg::Str => {
            if string_arg(node).is_none_or(str::is_empty) {
                schema.arg_say.raise(about, here, src, issues);
            }
        }
        Arg::Bool => {
            if bool_arg(node).is_none() {
                schema.arg_say.raise(about, here, src, issues);
            }
        }
        Arg::Int => {
            if int_arg(node).is_none() {
                schema.arg_say.raise(about, here, src, issues);
            }
        }
        Arg::Strs => {
            if string_args(node).is_empty() {
                schema.arg_say.raise(about, here, src, issues);
            }
        }
        Arg::StrPair(..) => {
            let args: Vec<_> = node
                .entries()
                .iter()
                .filter(|entry| entry.name().is_none())
                .collect();
            if args.len() != 2 || args.iter().any(|entry| entry.value().as_string().is_none()) {
                schema.arg_say.raise(about, here, src, issues);
            }
        }
        Arg::One(set) => {
            let given = string_arg(node);
            if !given.is_some_and(|value| set.contains(&value)) {
                schema
                    .arg_say
                    .raise(given.unwrap_or(about), here, src, issues);
            }
        }
        Arg::MaybeOne(set) => {
            let positional = node.entries().iter().find(|entry| entry.name().is_none());
            if let Some(entry) = positional {
                let given = entry.value().to_string();
                let given = entry.value().as_string().unwrap_or(&given);
                if !set.contains(&given) {
                    schema.arg_say.raise(given, here, src, issues);
                }
            }
        }
    }

    for entry in node.entries() {
        let Some(key) = entry.name().map(|n| n.value()) else {
            continue; 
        };
        match schema.props.iter().find(|p| p.name == key) {
            Some(prop) => {
                let ok = match prop.kind {
                    Kind::Str => entry.value().as_string().is_some(),
                    Kind::Bool => entry.value().as_bool().is_some(),
                    Kind::Int(low, high) => entry
                        .value()
                        .as_integer()
                        .is_some_and(|v| (low..=high).contains(&v)),
                    Kind::One(set) => entry.value().as_string().is_some_and(|v| set.contains(&v)),
                };
                if !ok {
                    let about = match prop.kind {
                        Kind::One(_) => entry.value().as_string().unwrap_or(key),
                        Kind::Str | Kind::Bool | Kind::Int(..) => key,
                    };
                    prop.say.raise(about, entry.span(), src, issues);
                }
            }
            None => schema.prop_say.raise(key, entry.span(), src, issues),
        }
    }

    for prop in schema.props {
        if prop.missing.silent()
            || node
                .entries()
                .iter()
                .any(|e| e.name().map(|n| n.value()) == Some(prop.name))
        {
            continue;
        }
        prop.missing.raise(node.name().value(), here, src, issues);
    }

    walk(kids(node), schema, here, src, issues);
}

fn walk(children: &[KdlNode], schema: &Node, here: Span, src: &Source, issues: &mut Issues) {
    if children.is_empty() {
        schema.empty.raise(schema.name, here, src, issues);
    }

    let mut seen: Vec<(&str, &str, Span)> = Vec::new();
    for child in children {
        let name = child.name().value();
        let span: Span = child.name().span().into();
        let Some(sub) = schema
            .children
            .iter()
            .find(|c| c.name == name)
            .or_else(|| schema.children.iter().find(|c| c.name.is_empty()))
        else {
            schema.child_say.raise(name, span, src, issues);
            continue;
        };
        let key = match sub.once {
            true => Some(name),
            false => match sub.unique.silent() {
                true => None,
                false if sub.name.is_empty() => (!string_args(child).is_empty()).then_some(name),
                false => string_arg(child),
            },
        };
        if let Some(key) = key {
            if let Some((_, _, first)) = seen.iter().find(|(n, k, _)| *n == sub.name && *k == key) {
                match sub.once {
                    true => {
                        let issue = Issue::new(format!("`{name}` is declared twice"), src)
                            .at(*first, "first here")
                            .at(span, "and again here");
                        issues.push(match sub.dup_help.is_empty() {
                            true => issue,
                            false => issue.help(sub.dup_help),
                        });
                    }
                    false => sub.unique.raise(key, span, src, issues),
                }
                continue;
            }
            seen.push((sub.name, key, span));
        }
        check(child, sub, src, issues);
    }

    for sub in schema.children {
        if sub.missing.silent() || children.iter().any(|c| c.name().value() == sub.name) {
            continue;
        }
        sub.missing.raise(sub.name, here, src, issues);
    }
}
