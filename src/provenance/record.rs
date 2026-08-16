use crate::diag::{Issues, Source};
use crate::parse::schema::{check_doc, Arg, Node, Say, NEEDS_VALUE};
use crate::parse::{kids, string_arg, syntax_issue, text};
use crate::provenance::evidence::{self, Role, PIN};
use crate::provenance::{Evidence, Tracker};
use kdl::KdlDocument;
use std::path::Path;

pub const RECORD: &str = "provenance.kdl";

#[rustfmt::skip]
pub const IMPORTED: Node = Node::new("imported",
    "Where this module was copied from, and what its content hashed to then. Written by \
     `buildc import module`; the module's author does not maintain it.")
    .arg(Arg::Str, Say::new("`imported` needs a collection name", "no collection given",
        "`imported \"buildc-org\"`, the name the collection is declared under in `sources`"))
    .once("")
    .missing(Say::new("`{}` declares no `imported`", "nothing recorded",
        "the file records one import; delete it if the module was not imported"))
    .children(&[
        Node::new("content",
            "What the module directory hashed to when it was imported, every file in it except \
             this one.")
            .arg(Arg::Str, NEEDS_VALUE).once("")
            .missing(Say::new("`{}` records no `content` hash", "nothing to compare against",
                "the sha256 of the imported directory, which is what makes a later edit visible")),
        PIN,
    ], Say::new("unknown node `{}` in an import record", "not part of the schema",
        "an import record holds `content` and the `pin` the collection was fetched at"));

const RECORD_FILE: Node = Node::new("", "").children(&[IMPORTED], Say::NONE);

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
        .map(|n| evidence::read(n, Role::Record, &src, issues))
        .unwrap_or_else(|| Evidence::new(node.name().span().into()));

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
    for dir in dirs(&root.join("modules")) {
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

fn dirs(from: &Path) -> Vec<std::path::PathBuf> {
    let Ok(entries) = std::fs::read_dir(from) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for path in entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()) {
        match path.join("module.kdl").is_file() {
            true => out.push(path),
            false => out.extend(dirs(&path)),
        }
    }
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
}
