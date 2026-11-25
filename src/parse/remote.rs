use crate::diag::{Issue, Issues, Source, Span};
use crate::model::image::is_name;
use crate::model::remote::{At, Collection, DefaultLibrary};
use crate::parse::schema::{Arg, Node, Say};
use crate::parse::{kids, string_arg};
use crate::provenance::{Evidence, Tracker};
use kdl::KdlNode;

const LOCAL_SOURCE: Node = Node::new(
    "source",
    "Where the library lives, as a path from the repository root.",
)
.about(
    "The user edits a local library in place. If `source` is absent, then the library is the \
     folder `./<name>`. A source of `.` is this repository itself.",
)
.example("\".\"")
.arg(
    Arg::Str,
    Say::new(
        "`source` needs a path",
        "not a string",
        "`source \"../library\"`",
    ),
)
.once("");

const REMOTE_SOURCE: Node = Node::new(
    "source",
    "Where the library lives, as an HTTPS Git URL or a path to a Git checkout.",
)
.about(
    "`buildc` reads a remote library at a commit from its cache. A path names a Git checkout \
     on this machine, and `buildc` reads its commits as it reads a URL.",
)
.example("\"https://github.com/buildc-org/library\"")
.arg(
    Arg::Str,
    Say::new(
        "`source` needs a URL or a path",
        "not a string",
        "`source \"https://github.com/owner/library\"`",
    ),
)
.once("")
.missing(Say::new(
    "this library declares no `{}`",
    "a remote library needs one",
    "`source \"https://github.com/owner/library\"`, or a path to a Git checkout",
));

const VERSION: Node = Node::new(
    "version",
    "The branch, tag or commit that a remote library is read at.",
)
.about("If `version` is absent, then the library follows the default branch of the remote.")
.example("\"v1\"")
.arg(
    Arg::Str,
    Say::new(
        "`version` needs a Git ref",
        "not a string",
        "`version \"v1\"`",
    ),
)
.once("");

const NAMED: Say = Say::new(
    "a library takes no arguments",
    "unexpected value",
    "the node name is the library name: `stable { source \"https://github.com/owner/library\" }`",
);

const LIBRARY_ABOUT: &str =
    "The node name is the library's local name, and `library=` on a tree node names the library by its value. A \
     source with a `repo.kdl` keeps its components where its `library` node says, else in \
     `components/`. A source without a `repo.kdl` is a folder of components.";

const LOCAL_LIBRARY: Node = Node::new("", "One local library, named by its node.")
    .about(LIBRARY_ABOUT)
    .example("mine")
    .scaffolds(&["source"])
    .arg(Arg::None, NAMED)
    .children(
        &[LOCAL_SOURCE],
        Say::new(
            "unknown node `{}` in a local library",
            "not part of the schema",
            "a local library accepts `source`; a library read at a version goes in `remote`",
        ),
    );

const REMOTE_LIBRARY: Node = Node::new("", "One remote library, named by its node.")
    .about(LIBRARY_ABOUT)
    .example("stable")
    .scaffolds(&["source"])
    .arg(Arg::None, NAMED)
    .children(
        &[REMOTE_SOURCE, VERSION],
        Say::new(
            "unknown node `{}` in a remote library",
            "not part of the schema",
            "a remote library accepts `source` and `version`",
        ),
    );

pub const LOCAL: Node = Node::new("local", "The libraries that the user edits in place.")
    .about(
        "`buildc` reads a local library from its folder as it stands, so a local library takes \
         no pin and no lock.",
    )
    .scaffolds(&[""])
    .once("a second block would split the local libraries in two")
    .arg(
        Arg::None,
        Say::new(
            "`local` takes no arguments",
            "unexpected value",
            "`local { mine { source \".\" } }`",
        ),
    )
    .children(&[LOCAL_LIBRARY], Say::NONE);

pub const REMOTE: Node = Node::new("remote", "The libraries that buildc reads at a commit.")
    .about(
        "`buildc` reads a remote library from its cache at a commit. `repo.lock.kdl` records \
         the commit and the content hash of each component that a build reads.",
    )
    .scaffolds(&[""])
    .once("a second block would split the remote libraries in two")
    .arg(
        Arg::None,
        Say::new(
            "`remote` takes no arguments",
            "unexpected value",
            "`remote { stable { source \"https://github.com/owner/library\" } }`",
        ),
    )
    .children(&[REMOTE_LIBRARY], Say::NONE);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Local,
    Remote,
}

impl Kind {
    pub fn block(self) -> &'static str {
        match self {
            Self::Local => LOCAL.name,
            Self::Remote => REMOTE.name,
        }
    }
}

pub fn declared(libraries: &KdlNode) -> impl Iterator<Item = (Kind, &KdlNode)> {
    kids(libraries).iter().flat_map(|block| {
        let kind = [Kind::Local, Kind::Remote]
            .into_iter()
            .find(|kind| kind.block() == block.name().value());
        kind.into_iter()
            .flat_map(move |kind| kids(block).iter().map(move |library| (kind, library)))
    })
}

fn value<'a>(node: &'a KdlNode, name: &str) -> Option<(&'a str, Span)> {
    kids(node)
        .iter()
        .find(|child| child.name().value() == name)
        .and_then(|child| {
            string_arg(child)
                .filter(|value| !value.is_empty())
                .map(|value| (value, child.name().span().into()))
        })
}

pub fn parse_library(
    node: &KdlNode,
    kind: Kind,
    src: &Source,
    issues: &mut Issues,
) -> Option<Collection> {
    let name = node.name().value().to_string();
    let span: Span = node.name().span().into();
    if !is_name(&name) {
        issues.push(
            Issue::new(format!("`{name}` is not a library name"), src)
                .at(span, "lowercase, digits and dashes, starting with a letter")
                .help("a tree node's `library=` names the library by it"),
        );
        return None;
    }

    let declared = value(node, "source");
    if kind == Kind::Local {
        if let Some((source, at)) = declared.filter(|(source, _)| source.contains("://")) {
            issues.push(
                Issue::new(format!("`{name}` is a local library"), src)
                    .at(at, "a URL")
                    .help(format!(
                        "a local library is a folder of this repository; move `{name}` under \
                         `remote` to read `{source}` at a commit"
                    )),
            );
            return None;
        }
        return Some(Collection {
            name: name.clone(),
            at: At::Dir(
                declared.map_or_else(|| format!("./{name}"), |(source, _)| source.to_string()),
            ),
            span,
        });
    }

    let (source, source_span) = declared?;
    if source.contains("://") && !source.starts_with("https://") {
        issues.push(
            Issue::new(format!("`{name}` does not use an HTTPS Git URL"), src)
                .at(source_span, "not HTTPS")
                .help("`source \"https://host/owner/repository\"`; credentials and host policy stay with Git"),
        );
        return None;
    }
    if source.contains(['{', '}']) {
        issues.push(
            Issue::new(format!("`{name}` puts a template in its Git URL"), src)
                .at(source_span, "a repository URL is fixed")
                .help("put the branch, tag or commit in `version`; `source` names the repository alone"),
        );
    }

    let mut pin = Evidence::new(span);
    pin.url = Some(source.trim_end_matches('/').to_string());
    pin.version = value(node, "version").map(|(value, _)| value.to_string());
    pin.tracker = Tracker::Unpinned(String::new());
    Some(Collection {
        name,
        at: At::Git(pin),
        span,
    })
}

pub fn defaults(text: &str) -> Vec<DefaultLibrary> {
    let Ok(doc) = text.parse::<kdl::KdlDocument>() else {
        return Vec::new();
    };
    doc.nodes()
        .iter()
        .filter(|node| is_name(node.name().value()))
        .filter_map(|node| {
            Some(DefaultLibrary {
                name: node.name().value().to_string(),
                source: value(node, "source")?.0.to_string(),
                about: value(node, "description")
                    .map_or("", |(about, _)| about)
                    .to_string(),
            })
        })
        .collect()
}
