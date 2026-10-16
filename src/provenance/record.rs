use crate::diag::{Issues, Source};
use crate::layout;
use crate::parse::schema::{check_doc, Arg, Node, Say, NEEDS_VALUE};
use crate::parse::{kids, string_arg, syntax_issue, text};
use crate::provenance::evidence::{self, Role, PIN};
use crate::provenance::{Evidence, Tracker};
use kdl::KdlDocument;
use std::path::Path;

pub const RECORD: &str = "provenance.kdl";

#[rustfmt::skip]
pub const IMPORTED: Node = Node::new("imported",
    "Where this module was copied from, and what its content hashed to then.").example("\"buildc-org\"")
    .arg(Arg::Str, Say::new("`imported` needs a collection name", "no collection given",
        "`imported \"buildc-org\"`, the name the collection is declared under in `sources`"))
    .once("")
    .missing(Say::new("`{}` declares no `imported`", "nothing recorded",
        "the file records one import; delete it if the module was not imported"))
    .children(&[
        Node::new("content", "The hash that the module directory had at import, which excludes `provenance.kdl`.").example("\"b7c232b0e8249d8e55a40beb79c5c43a7d370f3f9408bd215deb0170daeaadf3\"")
            .arg(Arg::Str, NEEDS_VALUE).once("")
            .missing(Say::new("`{}` records no `content` hash", "nothing to compare against",
                "the sha256 of the imported directory, which is what makes a later edit visible")),
        PIN,
    ], Say::new("unknown node `{}` in an import record", "not part of the schema",
        "an import record holds `content` and the `pin` the collection was fetched at"))
    .notes(&[
        "`buildc copy module` writes this record to `provenance.kdl` beside the module's \
         `module.kdl`. The module's author does not maintain it.",
        "The record stays out of `module.kdl`. A rewrite of the author's file would fork it from \
         upstream and break the hash comparison.",
        "`plan.json` carries the same hash for each module, so `buildc verify` fails on a module \
         edited without a new `buildc generate`.",
        "If a module's content no longer matches its record, then `buildc check` names the module. \
         The mismatch is an error only under `audit { enforce #true }`, because a fork of an \
         imported module is otherwise allowed.",
    ]);

pub(crate) const RECORD_FILE: Node = Node::new(
    "provenance file",
    "The import record that `buildc copy module` writes beside a copied module.",
)
.about(
    "When `buildc copy module` copies a module into this repository, it writes this record beside \
     the module. The record keeps where the module came from and what its content hashed to, so \
     `buildc check` can show whether the copy has changed since.",
)
.children(&[IMPORTED], Say::NONE);

pub struct Record {
    pub collection: String,
    pub content: String,
    pub pin: Evidence,
}

pub fn read(dir: &Path, issues: &mut Issues) -> Option<Record> {
    let path = dir.join(RECORD);
    let raw = std::fs::read_to_string(&path).ok()?;
    let name = path.display().to_string();
    let src = Source::new(&name, raw.clone());
    let doc: KdlDocument = match raw.parse() {
        Ok(doc) => doc,
        Err(err) => {
            issues.push(syntax_issue(&err, &name, &src));
            return None;
        }
    };
    check_doc(&doc, &RECORD_FILE, &src, issues);

    let node = doc
        .nodes()
        .iter()
        .find(|n| n.name().value() == "imported")?;
    let pin = kids(node)
        .iter()
        .find(|n| n.name().value() == "pin")
        .map_or_else(
            || Evidence::new(node.name().span().into()),
            |n| evidence::read(n, Role::Record, &src, issues),
        );

    Some(Record {
        collection: string_arg(node).unwrap_or_default().to_string(),
        content: text(node, "content"),
        pin,
    })
}

pub fn write(collection: &str, pin: Option<&Evidence>, content: &str) -> String {
    let mut body = format!("    content {}\n", quoted(content));
    let Some(pin) = pin else {
        return format!("imported {} {{\n{body}}}\n", quoted(collection));
    };
    body.push_str("    pin {\n");
    match &pin.tracker {
        Tracker::Renovate {
            datasource,
            dep_name,
        } => body.push_str(&format!(
            "        renovate datasource={} depName={}\n",
            quoted(datasource),
            quoted(dep_name)
        )),
        Tracker::Manual(why) => body.push_str(&format!("        manual {}\n", quoted(why))),
        Tracker::Unpinned(why) => body.push_str(&format!("        unpinned {}\n", quoted(why))),
        Tracker::None => {}
    }
    for (name, value) in [
        ("version", pin.version.as_deref()),
        ("url", pin.url.as_deref()),
        ("sha256", pin.sha256.as_deref()),
        ("path", pin.path.as_deref()),
    ] {
        if let Some(value) = value {
            body.push_str(&format!("        {name} {}\n", quoted(value)));
        }
    }
    body.push_str("    }\n");
    format!("imported {} {{\n{body}}}\n", quoted(collection))
}

fn quoted(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

pub fn hash(dir: &Path) -> Option<String> {
    let record = dir.join(RECORD);
    let mut listed: Vec<String> = Vec::new();
    for path in crate::tracked(dir) {
        if path == record {
            continue;
        }
        let rel = path.strip_prefix(dir).ok()?;
        listed.push(rel.to_string_lossy().into_owned());
    }
    if listed.is_empty() {
        return None;
    }
    crate::runtime::sha256_tree(dir, &listed).ok()
}

pub fn modified(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let found = layout::module_dirs(
        &layout::modules(root),
        |path| path.join(layout::MODULE_FILE).is_file(),
        |name| name == crate::model::remote::REMOTE_DIR,
    );
    for dir in found {
        let Ok(raw) = std::fs::read_to_string(dir.join(RECORD)) else {
            continue;
        };
        let Some(recorded) = raw.parse::<KdlDocument>().ok().and_then(|doc| {
            let node = doc
                .nodes()
                .iter()
                .find(|n| n.name().value() == "imported")?;
            Some(text(node, "content"))
        }) else {
            continue;
        };
        if hash(&dir).is_some_and(|found| found != recorded) {
            let name = dir.strip_prefix(root).unwrap_or(&dir);
            out.push(name.to_string_lossy().into_owned());
        }
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provenance::ShaFrom;

    #[test]
    fn a_written_pin_reads_back_as_what_was_written() {
        let dir = std::env::temp_dir().join(format!("buildc-record-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let written = Evidence {
            url: Some("https://host/owner/modules/{version}.tar.gz".into()),
            version: Some("v1.0.0".into()),
            sha256: Some("b7c2".into()),
            from: ShaFrom::Asset,
            path: Some("modules".into()),
            tracker: Tracker::Renovate {
                datasource: "github-tags".into(),
                dep_name: "owner/modules".into(),
            },
            span: Default::default(),
        };
        std::fs::write(
            dir.join(RECORD),
            write("buildc-org", Some(&written), "abc"),
        )
        .unwrap();

        let mut issues = Issues::default();
        let read = read(&dir, &mut issues).expect("the record is there");
        assert!(issues.is_empty(), "{}", issues.plain());
        assert_eq!(read.collection, "buildc-org");
        assert_eq!(read.content, "abc");
        assert_eq!(read.pin.url, written.url);
        assert_eq!(read.pin.version, written.version);
        assert_eq!(read.pin.sha256, written.sha256);
        assert_eq!(read.pin.path, written.path);
        assert_eq!(read.pin.tracker.as_str(), "renovate");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_collection_record_is_not_a_local_edit() {
        let root = std::env::temp_dir().join(format!("buildc-remote-record-{}", std::process::id()));
        let dir = root.join("modules/.remote/one/hello");
        crate::init::put(&dir.join(layout::MODULE_FILE), "description \"changed\"\n").unwrap();
        crate::init::put(&dir.join(RECORD), &write("one", None, "different")).unwrap();
        assert!(modified(&root).is_empty());
        let _ = std::fs::remove_dir_all(root);
    }
}
