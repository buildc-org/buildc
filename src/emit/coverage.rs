use crate::emit::json::Json;
use crate::emit::{Part, Table};
use crate::model::image::{Entry, Image};
use crate::provider::Index;
use crate::scap::{ordinal, reached, Content, Profile};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

struct Row {
    rule: String,
    numbers: Vec<String>,
    title: String,
    claimed: Vec<String>,
    would: Vec<String>,
}

pub struct Coverage<'a> {
    image: &'a Image,
    profile: &'a Profile,
    rows: Vec<Row>,
}

pub fn of<'a>(image: &'a Image, content: &'a Content, index: &Index) -> Option<Coverage<'a>> {
    let profile = content.profiles.iter().find(|p| p.is(&image.conforms))?;
    let selected = content.selected(&profile.id);

    let mut claimed: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for module in image.modules() {
        let numbers = module
            .satisfies
            .iter()
            .flat_map(|coverage| coverage.rules.iter());
        for rule in reached(content, numbers) {
            claimed.entry(rule).or_default().insert(module.path.clone());
        }
    }
    let open: BTreeSet<String> = selected
        .iter()
        .filter(|rule| !claimed.contains_key(*rule))
        .cloned()
        .collect();
    let listed: BTreeSet<String> = image
        .entries
        .iter()
        .chain(&image.suppressed)
        .map(Entry::dir)
        .collect();
    let mut would: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for provider in index.claiming(&content.numbering(&open)) {
        if listed.contains(&provider.dir()) {
            continue;
        }
        for rule in reached(content, provider.declares.satisfies.iter()) {
            if open.contains(&rule) {
                would.entry(rule).or_default().push(provider.qualified());
            }
        }
    }

    let mut rows: Vec<Row> = selected
        .iter()
        .map(|rule| Row {
            numbers: content
                .numbers
                .get(rule)
                .into_iter()
                .flatten()
                .cloned()
                .collect(),
            title: content.titles.get(rule).unwrap_or(rule).clone(),
            claimed: claimed.get(rule).into_iter().flatten().cloned().collect(),
            would: would.remove(rule).unwrap_or_default(),
            rule: rule.clone(),
        })
        .collect();
    rows.sort_by(|a, b| {
        let key = |row: &Row| {
            (
                row.numbers.is_empty(),
                row.numbers.first().map(|n| ordinal(n)).unwrap_or_default(),
                row.rule.clone(),
            )
        };
        key(a).cmp(&key(b))
    });
    Some(Coverage {
        image,
        profile,
        rows,
    })
}

const HEADER: &[&str] = &["Number", "Rule", "Claimed by", "Would claim"];

impl Coverage<'_> {
    fn claimed(&self) -> usize {
        self.rows
            .iter()
            .filter(|row| !row.claimed.is_empty())
            .count()
    }

    pub fn markdown(&self) -> String {
        let named = match self.profile.title.is_empty() {
            true => format!("`{}`", self.profile.name()),
            false => format!("`{}` ({})", self.profile.name(), self.profile.title),
        };
        let unnamed = match self
            .rows
            .iter()
            .filter(|row| row.numbers.is_empty())
            .count()
        {
            0 => String::new(),
            1 => " One of them carries no number, so no `satisfies` can name it.".into(),
            n => format!(" {n} of them carry no number, so no `satisfies` can name them."),
        };
        let mut out = format!(
            "# {} coverage of `{}`\n\n\
             {named} selects {} rules, and what `{}` lists claims {} of them.{unnamed}\n\n\
             | Number | Rule | Claimed by | Would claim |\n|---|---|---|---|\n",
            self.image.name,
            self.profile.name(),
            self.rows.len(),
            self.image.id,
            self.claimed(),
        );
        for row in &self.rows {
            let _ = writeln!(
                out,
                "| {} | {} | {} | {} |",
                code(&row.numbers),
                row.title,
                code(&row.claimed),
                code(&row.would),
            );
        }
        out
    }

    pub fn parts(&self) -> Vec<Part> {
        vec![Part::Table(Table {
            title: format!("{} coverage of {}", self.image.id, self.profile.name()),
            header: HEADER,
            rows: self
                .rows
                .iter()
                .map(|row| {
                    (
                        vec![
                            row.numbers.join(", "),
                            row.title.clone(),
                            row.claimed.join(", "),
                            row.would.join(", "),
                        ],
                        row.claimed.is_empty(),
                    )
                })
                .collect(),
        })]
    }

    pub fn json(&self) -> Json {
        Json::object([
            ("image", Json::string(&self.image.id)),
            ("profile", Json::string(self.profile.name())),
            ("title", Json::string(&self.profile.title)),
            (
                "rules",
                Json::array(self.rows.iter().map(|row| {
                    Json::object([
                        ("rule", Json::string(&row.rule)),
                        ("numbers", Json::strings(row.numbers.iter().cloned())),
                        ("title", Json::string(&row.title)),
                        ("claimed_by", Json::strings(row.claimed.iter().cloned())),
                        ("would_claim", Json::strings(row.would.iter().cloned())),
                    ])
                })),
            ),
        ])
    }
}

fn code(names: &[String]) -> String {
    names
        .iter()
        .map(|name| format!("`{name}`"))
        .collect::<Vec<String>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn fixture(name: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join(name)
    }

    #[test]
    fn a_module_the_image_lists_is_not_offered_as_one_that_would_claim() {
        let root = fixture("tests/scap/listed-claimant");
        let loaded = crate::load(&root);
        let content = crate::scap::content_of(&fixture("tests/scap/datastream.xml"))
            .expect("the fixture datastream reads");
        let image = loaded.list.images.first().expect("one image");
        let out = of(image, &content, &loaded.index)
            .expect("the datastream carries `standard`")
            .markdown();
        assert!(
            out.contains("| `4.1.3.1` | Record Attempts to Alter Logon and Logout Events |  |  |"),
            "{out}"
        );
        assert!(!out.contains("one/broken"), "{out}");
    }
}
