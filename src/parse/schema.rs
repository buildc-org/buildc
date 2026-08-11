use crate::diag::{Issue, Issues, Source, Span};
use crate::parse::{bool_arg, int_arg, kids, string_arg, string_args};
use kdl::{KdlDocument, KdlNode};

pub struct Say {
    pub text: &'static str,
    pub label: &'static str,
    pub help: &'static str,
}

impl Say {
    pub const NONE: Say = Say::new("", "", "");

    pub const fn new(text: &'static str, label: &'static str, help: &'static str) -> Say {
        Say { text, label, help }
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

pub enum Arg {
    None,
    Str,
    Bool,
    Int,
    Strs,
    One(&'static [&'static str]),
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
    pub arg: Arg,
    pub arg_say: Say,
    pub missing: Say,
    pub once: bool,
    pub dup_help: &'static str,
    pub unique: Say,
    pub empty: Say,
    pub props: &'static [Prop],
    pub prop_say: Say,
    pub children: &'static [Node],
    pub child_say: Say,
}

impl Node {
    pub const fn new(name: &'static str, desc: &'static str) -> Node {
        Node {
            name,
            desc,
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
        }
    }

    pub const fn arg(mut self, arg: Arg, say: Say) -> Node {
        self.arg = arg;
        self.arg_say = say;
        self
    }

    pub const fn once(mut self, help: &'static str) -> Node {
        self.once = true;
        self.dup_help = help;
        self
    }

    pub const fn unique(mut self, say: Say) -> Node {
        self.unique = say;
        self
    }

    pub const fn missing(mut self, say: Say) -> Node {
        self.missing = say;
        self
    }

    pub const fn empty(mut self, say: Say) -> Node {
        self.empty = say;
        self
    }

    pub const fn props(mut self, props: &'static [Prop], say: Say) -> Node {
        self.props = props;
        self.prop_say = say;
        self
    }

    pub const fn children(mut self, children: &'static [Node], say: Say) -> Node {
        self.children = children;
        self.child_say = say;
        self
    }
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
        Arg::One(set) => {
            let given = string_arg(node);
            if !given.is_some_and(|value| set.contains(&value)) {
                schema
                    .arg_say
                    .raise(given.unwrap_or(about), here, src, issues);
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
                        _ => key,
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
