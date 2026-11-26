use crate::diag::{Issue, Issues, Source, Span};
use crate::model::image::is_name;
use crate::parse::schema::{Arg, Kind, Node, Prop, Say};
use crate::parse::{kids, options};
use kdl::KdlNode;

#[rustfmt::skip]
const LIBRARY: Prop = Prop { name: "library", kind: Kind::Str,
    desc: "The library that holds this path and every path below it.",
    say: Say::NONE, missing: Say::NONE };

#[rustfmt::skip]
const PIN: Prop = Prop { name: "pin", kind: Kind::Str,
    desc: "The tag or full commit of a remote library that this path and every path below it read.",
    say: Say::NONE, missing: Say::NONE };

#[rustfmt::skip]
const VARIANT: Prop = Prop { name: "variant", kind: Kind::Str,
    desc: "The variant of the component that this path builds.",
    say: Say::NONE, missing: Say::NONE };

#[rustfmt::skip]
pub(crate) const DIGEST: Prop = Prop { name: "digest", kind: Kind::Str,
    desc: "The digest of the base image reference.",
    say: Say::NONE, missing: Say::NONE };

#[rustfmt::skip]
pub(crate) const SIGNED: Prop = Prop { name: "signed", kind: Kind::Bool,
    desc: "Whether the base image publishes a cosign signature.",
    say: Say::NONE, missing: Say::NONE };

#[rustfmt::skip]
pub(crate) const KEY: Prop = Prop { name: "key", kind: Kind::Str,
    desc: "The repository path of the cosign public key that verifies the base.",
    say: Say::NONE, missing: Say::NONE };

#[rustfmt::skip]
pub(crate) const KEY_SHA256: Prop = Prop { name: "key-sha256", kind: Kind::Str,
    desc: "The sha256 that pins a local cosign public key's content.",
    say: Say::NONE, missing: Say::NONE };

#[rustfmt::skip]
pub(crate) const IDENTITY: Prop = Prop { name: "identity", kind: Kind::Str,
    desc: "The certificate identity that verifies a keyless signature of the base.",
    say: Say::NONE, missing: Say::NONE };

#[rustfmt::skip]
pub(crate) const ISSUER: Prop = Prop { name: "issuer", kind: Kind::Str,
    desc: "The OIDC issuer of the certificate identity; it defaults to GitHub's.",
    say: Say::NONE, missing: Say::NONE };

#[rustfmt::skip]
pub(crate) const BASE_PROPS: &[Prop] = &[
    DIGEST,
    SIGNED,
    KEY,
    KEY_SHA256,
    IDENTITY,
    ISSUER,
];

#[rustfmt::skip]
pub(crate) const PROPS: &[Prop] = &[
    VARIANT,
    LIBRARY,
    PIN,
    DIGEST,
    SIGNED,
    KEY,
    KEY_SHA256,
    IDENTITY,
    ISSUER,
];

#[rustfmt::skip]
pub(crate) const SEGMENT: Node = Node::new("", "One part of a component path, which lists the component at that path or holds further parts.")
    .about("A segment takes no argument. Its name takes lowercase letters, digits, dashes and \
        dots, starts with a letter, and allows one `@<tag>` on the last part. Every other setting is a child node. A child with \
        an argument sets an option of the component. The option value may read `{image-id}` and \
        `{base-url}`.")
    .example("desktop-environment")
    .props(PROPS, Say::NONE)
    .notes(&[
        "In a manifest, a segment inside an `on` block builds only on the families that the \
         gate names.",
        "A segment is read below the current context. At the top of a manifest the context is \
         the component's own path, and `component \"<path>\"` sets another.",
        "A component that another component defines inline must have no manifest file. \
         `buildc check` refuses the pair and names both.",
        "`library=` and `pin=` hold for every path below the segment that sets them.",
        "`pin=` reads the component's own directory at the pin, and the paths that the component \
         lists read at the library `version`. `buildc check` refuses `pin=` on a component of a \
         local library and a pin that is neither a tag nor a full 40-character commit.",
        "If two components pin one path, then the pin of the component that lists the other wins.",
        "If two components set one option of a path, then the value of the component that \
         lists the other wins. A list value appends to the value that it inherits, and the \
         default is part of that value.",
        "A `remove` child of a list value takes a value out of what the list inherits, as in \
         `flatpaks \"a\" { remove \"b\" }`. `buildc check` refuses a `remove` of a value that \
         the list does not hold.",
        "`buildc check` refuses two components that set different values if neither component \
         lists the other. `buildc why` reports each value that each component applies.",
        "The base is the component that declares `base`. A source-composed image lists it as a \
         segment, and a base-derived component is the build's own component. Only a base entry \
         takes `digest=`, `signed=`, `key=`, `key-sha256=`, `identity=` and `issuer=`.",
        "`key=` names a cosign public key at a repository path. `identity=` names a keyless \
         certificate identity, and `issuer=` defaults to GitHub's OIDC issuer. Either one \
         verifies the base's signature, and each needs `signed=#true`.",
        "`buildc add component` adds a reference to a library component. `buildc copy \
         component` copies one into a local library and records where it came from in that \
         library's `origins.kdl`.",
    ]);

#[rustfmt::skip]
pub(crate) const CONTEXT: Node = Node::new("component", "An instance of the component at a path, or a context that the segments in the block are read below.")
    .about("A path that names a component lists it, and the block holds that component's option \
        values and nested components. A path of `/`, `.` or `..` sets the context alone and takes \
        no `variant=` or base facts. A path that starts with `/` is read from the library root. \
        Any other path is read from the current context, and `..` goes up one segment. At the top \
        of a manifest the context is the component's own path. A file outside every library reads \
        the repository root in place of a library root.")
    .example("\"/hardware\"")
    .arg(Arg::Str, Say::new("`component` needs a path", "no path given",
        "`component \"/hardware\"` lists `hardware`, and `component \"/\" { … }` reads the block from the library root"))
    .props(PROPS, Say::new("`{}` is not a `component` property", "not part of the schema",
        "`component` takes the same properties as a segment"))
    .children(&[SEGMENT], Say::NONE)
    .notes(&[
        "In a file outside every library, `component \"./<name>\"` and `component \"/<name>\"` \
         both read the component file at the repository root.",
        "A bare child of a file outside every library reads from the library root.",
    ]);

pub(crate) fn join(parent: &str, name: &str) -> String {
    match parent.is_empty() {
        true => name.to_string(),
        false => format!("{parent}/{name}"),
    }
}

pub(crate) fn context(at: &str, path: &str) -> Result<String, String> {
    let (mut held, rest): (Vec<&str>, &str) = match path.strip_prefix('/') {
        Some(rest) => (Vec::new(), rest),
        None => (
            at.split('/').filter(|part| !part.is_empty()).collect(),
            path,
        ),
    };
    for part in rest.split('/').filter(|part| !part.is_empty()) {
        match part {
            "." => {}
            ".." if held.pop().is_none() => {
                return Err(format!("`{path}` climbs above the library root"));
            }
            ".." => {}
            _ if crate::model::image::is_local_path(part) => held.push(part),
            _ => return Err(format!("`{path}` is not a component path")),
        }
    }
    Ok(held.join("/"))
}

#[derive(Clone, Default)]
pub(crate) struct Props {
    pub variant: Option<String>,
    pub library: Option<String>,
    pub pin: Option<String>,
    pub digest: Option<(String, Span)>,
    pub signed: Option<(bool, Span)>,
    pub key: Option<(String, Span)>,
    pub key_sha256: Option<(String, Span)>,
    pub identity: Option<(String, Span)>,
    pub issuer: Option<(String, Span)>,
    pub rooted: bool,
    pub root_file: bool,
}

impl Props {
    pub(crate) fn below(&self) -> Self {
        Self {
            library: self.library.clone(),
            pin: self.pin.clone(),
            rooted: self.rooted,
            ..Self::default()
        }
    }

    pub(crate) fn read(
        node: &KdlNode,
        path: &str,
        above: &Self,
        src: &Source,
        issues: &mut Issues,
    ) -> Self {
        let mut out = above.below();
        let mut seen: Vec<(&str, Span)> = Vec::new();
        for entry in node.entries() {
            let Some(key) = entry.name().map(|key| key.value()) else {
                continue;
            };
            let span: Span = entry.span().into();
            if let Some((_, first)) = seen.iter().find(|(held, _)| *held == key) {
                issues.push(
                    Issue::new(format!("`{key}` is declared twice"), src)
                        .at(*first, "first here")
                        .at(span, "and again here"),
                );
                continue;
            }
            seen.push((key, span));
            let text = entry.value().as_string().map(str::to_string);
            let help = match key {
                "variant" => match text {
                    Some(text) => {
                        out.variant = Some(text);
                        continue;
                    }
                    None => "`variant=\"<name>\"` names one variant the component declares",
                },
                "library" => match text.filter(|text| is_name(text)) {
                    Some(text) => {
                        out.library = Some(text);
                        continue;
                    }
                    None => "`library=\"<name>\"` names a library that `repo.kdl` declares",
                },
                "pin" => match text.filter(|text| !text.is_empty()) {
                    Some(text) => {
                        out.pin = Some(text);
                        continue;
                    }
                    None => "`pin=\"<tag or commit>\"` names the library revision to read",
                },
                "digest" => match text {
                    Some(text) => {
                        out.digest = Some((text, span));
                        continue;
                    }
                    None => {
                        "`digest=\"sha256:...\"` records the digest of the base image reference"
                    }
                },
                "signed" => match entry.value().as_bool() {
                    Some(signed) => {
                        out.signed = Some((signed, span));
                        continue;
                    }
                    None => "`signed=#false` records that the base publishes no cosign signature",
                },
                "key" => match text.filter(|text| key_path(text)) {
                    Some(text) => {
                        out.key = Some((text, span));
                        continue;
                    }
                    None => {
                        "`key=\"<path>\"` names a cosign public key at a repository path, as \
                         `key=\"keys/public/etc/pki/containers/cosign.pub\"`"
                    }
                },
                "key-sha256" => match text.filter(|text| is_sha256(text)) {
                    Some(text) => {
                        out.key_sha256 = Some((text, span));
                        continue;
                    }
                    None => {
                        "`key-sha256=\"<64 hex>\"` pins a local public key, lowercase as \
                         sha256sum writes it"
                    }
                },
                "identity" => match text.filter(|text| !text.is_empty()) {
                    Some(text) => {
                        out.identity = Some((text, span));
                        continue;
                    }
                    None => "`identity=\"<identity>\"` names the certificate identity of a keyless signature",
                },
                "issuer" => match text.filter(|text| !text.is_empty()) {
                    Some(text) => {
                        out.issuer = Some((text, span));
                        continue;
                    }
                    None => "`issuer=\"<url>\"` names the OIDC issuer of the certificate identity",
                },
                _ => {
                    issues.push(
                        Issue::new(format!("`{key}` is not a property of `{path}`"), src)
                            .at(span, "not part of the schema")
                            .help(
                                "a segment accepts `variant=`, `library=`, `pin=`, `digest=`, \
                                 `signed=`, `key=`, `key-sha256=`, `identity=` and `issuer=`; \
                                 options are set as child nodes",
                            ),
                    );
                    continue;
                }
            };
            issues.push(
                Issue::new(format!("`{key}` on `{path}` has the wrong value"), src)
                    .at(span, "not a value it takes")
                    .help(help),
            );
        }
        check_trust(&out, src, issues);
        out
    }
}

fn key_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains("://")
        && path.split('/').all(|part| !part.is_empty() && part != "..")
}

fn is_sha256(text: &str) -> bool {
    text.len() == 64
        && text
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
}

fn check_trust(props: &Props, src: &Source, issues: &mut Issues) {
    if props.key_sha256.is_some() && props.key.is_none() {
        let span = props
            .key_sha256
            .as_ref()
            .map(|(_, span)| *span)
            .unwrap_or_default();
        issues.push(
            Issue::new("`key-sha256=` pins a key with no `key=`", src)
                .at(span, "no key to hash")
                .help("add `key=\"<repository path>\"`, or drop `key-sha256=`"),
        );
    }
    if let (Some((_, key)), Some((_, identity))) = (&props.key, &props.identity) {
        issues.push(
            Issue::new("the base names a `key=` and an `identity=`", src)
                .at(*key, "one verifier")
                .at(*identity, "and another")
                .help("a key verifies a key signature, and an identity verifies a keyless one; name one"),
        );
    }
    if props.identity.is_none() && props.issuer.is_some() {
        let span = props
            .issuer
            .as_ref()
            .map(|(_, span)| *span)
            .unwrap_or_default();
        issues.push(
            Issue::new("`issuer=` names an issuer with no `identity=`", src)
                .at(span, "no identity to issue")
                .help("add `identity=\"<certificate identity>\"`, or drop `issuer=`"),
        );
    }
    if !matches!(props.signed, Some((true, _))) && (props.key.is_some() || props.identity.is_some())
    {
        let span = props
            .key
            .as_ref()
            .or(props.identity.as_ref())
            .map(|(_, span)| *span)
            .unwrap_or_default();
        issues.push(
            Issue::new("a verifier names a base that is not declared signed", src)
                .at(span, "nothing to verify")
                .help("`signed=#true` records that the base publishes a cosign signature"),
        );
    }
}

pub(crate) fn refuse_base_facts(
    path: &str,
    facts: &[(&str, Option<Span>)],
    src: &Source,
    issues: &mut Issues,
) {
    for (key, span) in facts
        .iter()
        .filter_map(|(key, span)| span.map(|span| (*key, span)))
    {
        issues.push(
            Issue::new(
                format!("`{path}` is not the base, so it takes no `{key}=`"),
                src,
            )
            .at(span, "only the base takes it")
            .help(
                "`digest=`, `signed=`, `key=`, `key-sha256=`, `identity=` and `issuer=` record the \
                 base image on a `base` node or the base segment",
            ),
        );
    }
}

pub(crate) struct Body<'a> {
    pub(crate) settings: Vec<&'a KdlNode>,
    pub(crate) inline: Option<Vec<Span>>,
    pub(crate) nested: Vec<&'a KdlNode>,
}

/// One manifest node in a segment's body makes the whole body an inline definition.
/// Its other children then read as manifest nodes and set no option.
pub(crate) fn body(node: &KdlNode) -> Body<'_> {
    let (own, nested): (Vec<&KdlNode>, Vec<&KdlNode>) =
        kids(node).iter().partition(|kid| !is_segment(kid));
    match own.iter().any(|kid| declares(kid)) {
        true => Body {
            settings: Vec::new(),
            inline: Some(own.iter().map(|kid| Span::from(kid.span())).collect()),
            nested,
        },
        false => Body {
            settings: own,
            inline: None,
            nested,
        },
    }
}

/// `library=` and `pin=` set the paths below a segment and leave it organising.
pub(crate) fn organises(node: &KdlNode, body: &Body) -> bool {
    !body.nested.is_empty()
        && body.settings.is_empty()
        && body.inline.is_none()
        && !node.entries().iter().any(|entry| {
            matches!(
                entry.name().map(|name| name.value()),
                Some("variant" | "digest" | "signed" | "key" | "identity" | "issuer")
            )
        })
}

fn declares(kid: &KdlNode) -> bool {
    crate::parse::image::is_manifest_node(kid.name().value())
        && (!kid.entries().is_empty() || kid.children().is_some())
}

pub(crate) fn is_segment(kid: &KdlNode) -> bool {
    !options::is_setting(kid) && !declares(kid)
}

pub(crate) fn is_context(node: &KdlNode) -> bool {
    node.name().value() == CONTEXT.name && crate::parse::string_arg(node).is_some()
}
