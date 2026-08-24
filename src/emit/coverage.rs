use crate::emit::json::Json;
use crate::emit::Table;
use crate::model::image::{Entry, Image};
use crate::provider::Index;
use crate::scap::{Content, Profile};
use std::collections::BTreeSet;
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
    let listed: BTreeSet<String> = image.entries.iter().map(Entry::dir).collect();
    let mut rows: Vec<Row> = content
        .selected(&profile.id)
        .iter()
        .map(|rule| {
            let numbers: Vec<String> = content
                .numbers
                .get(rule)
                .into_iter()
                .flatten()
                .cloned()
                .collect();
            let claimed: BTreeSet<String> = image
                .modules()
                .filter(|module| {
                    module
                        .satisfies
                        .iter()
                        .flat_map(|coverage| coverage.rules.iter())
                        .any(|number| content.rules.get(number) == Some(rule))
                })
                .map(|module| module.path.clone())
                .collect();
            let would = match claimed.is_empty() {
                false => Vec::new(),
                true => index
                    .claiming(&numbers.iter().cloned().collect())
                    .into_iter()
                    .filter(|provider| !listed.contains(&provider.dir()))
                    .map(|provider| provider.qualified())
                    .collect(),
            };
            Row {
                rule: rule.clone(),
                numbers,
                title: content.titles.get(rule).unwrap_or(rule).clone(),
                claimed: claimed.into_iter().collect(),
                would,
            }
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

fn ordinal(number: &str) -> Vec<u64> {
    number
        .split('.')
        .map(|part| part.parse().unwrap_or(u64::MAX))
        .collect()
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

    pub fn tables(&self) -> Vec<Table> {
        vec![Table {
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
        }]
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
