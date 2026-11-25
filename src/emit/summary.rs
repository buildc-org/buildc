use crate::emit::plan::of_target;
use crate::model::image::List;
use common::json::{field, items, strings, text, Json};
use std::fmt::Write as _;

struct Row {
    path: String,
    variant: Option<String>,
    description: String,
    options: Vec<(String, String)>,
    satisfies: Vec<(String, Vec<String>)>,
    refuses: Vec<(String, bool)>,
}

pub fn render(list: &List, target: &str) -> Option<String> {
    let (image, entries) = of_target(list, target)?;
    let lifted: Vec<&str> = image.allows.iter().map(|a| a.rule.as_str()).collect();
    let rows: Vec<Row> = entries
        .iter()
        .map(|entry| {
            let component = entry.component.as_ref();
            Row {
                path: entry.path.clone(),
                variant: entry.variant.clone(),
                description: component.map(|m| m.description.clone()).unwrap_or_default(),
                options: component
                    .map(|m| m.resolved.as_slice())
                    .unwrap_or_default()
                    .iter()
                    .map(|(name, value)| (name.clone(), format!("\"{}\"", cell(value))))
                    .collect(),
                satisfies: component
                    .map(|m| m.satisfies.as_slice())
                    .unwrap_or_default()
                    .iter()
                    .map(|coverage| (coverage.benchmark.clone(), coverage.rules.clone()))
                    .collect(),
                refuses: component
                    .map(|m| m.refuses.as_slice())
                    .unwrap_or_default()
                    .iter()
                    .map(|refusal| {
                        (
                            refusal.rule.clone(),
                            lifted.contains(&refusal.rule.as_str()),
                        )
                    })
                    .collect(),
            }
        })
        .collect();
    Some(table(&image.conforms, &rows))
}

pub fn on_host(target: &Json) -> String {
    let lifted: Vec<String> = items(target, "allows")
        .iter()
        .filter_map(|allow| text(allow, "rule"))
        .collect();
    let rows: Vec<Row> = items(target, "components")
        .iter()
        .map(|component| Row {
            path: text(component, "path").unwrap_or_default(),
            variant: text(component, "variant"),
            description: text(component, "description").unwrap_or_default(),
            options: match field(component, "options") {
                Some(Json::Object(fields)) => fields
                    .iter()
                    .map(|(name, value)| {
                        let written = match value {
                            Json::String(value) => format!("\"{}\"", cell(value)),
                            other @ Json::Null
                            | other @ Json::Bool(_)
                            | other @ Json::Number(_)
                            | other @ Json::Array(_)
                            | other @ Json::Object(_) => cell(
                                other
                                    .render()
                                    .lines()
                                    .map(str::trim_start)
                                    .collect::<Vec<_>>()
                                    .join(" ")
                                    .trim(),
                            ),
                        };
                        (name.clone(), written)
                    })
                    .collect(),
                _ => Vec::new(),
            },
            satisfies: items(component, "satisfies")
                .iter()
                .map(|claim| {
                    (
                        text(claim, "benchmark").unwrap_or_default(),
                        strings(claim, "rules"),
                    )
                })
                .collect(),
            refuses: items(component, "refuses")
                .iter()
                .filter_map(|refusal| text(refusal, "rule"))
                .map(|rule| {
                    let lifted = lifted.contains(&rule);
                    (rule, lifted)
                })
                .collect(),
        })
        .collect();
    table(
        text(target, "conforms").as_deref().unwrap_or_default(),
        &rows,
    )
}

fn table(conforms: &str, rows: &[Row]) -> String {
    let noun = match rows.len() {
        1 => "component",
        _ => "components",
    };
    let mut out = format!("{} {noun}.\n", rows.len());
    if !conforms.is_empty() {
        let _ = write!(out, "\nIt declares `conforms \"{}\"`.\n", cell(conforms));
    }
    out.push_str(
        "\n| Component | Description | Options | Satisfies | Refuses |\n| --- | --- | --- | --- | \
         --- |\n",
    );
    for row in rows {
        let _ = write!(out, "| `{}`", row.path);
        if let Some(variant) = &row.variant {
            let _ = write!(out, " `variant={variant}`");
        }
        let options: Vec<String> = row
            .options
            .iter()
            .map(|(name, value)| format!("`{name}={value}`"))
            .collect();
        let satisfies: Vec<String> = row
            .satisfies
            .iter()
            .map(|(benchmark, rules)| {
                format!(
                    "`{}: {}`",
                    cell(benchmark),
                    rules
                        .iter()
                        .map(|rule| cell(rule))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })
            .collect();
        let refuses: Vec<String> = row
            .refuses
            .iter()
            .map(|(rule, lifted)| match lifted {
                true => format!("`{}` (lifted)", cell(rule)),
                false => format!("`{}`", cell(rule)),
            })
            .collect();
        let _ = writeln!(
            out,
            " | {} | {} | {} | {} |",
            cell(&row.description),
            options.join(" "),
            satisfies.join(" "),
            refuses.join(" ")
        );
    }
    out
}

fn cell(text: &str) -> String {
    text.replace('|', "\\|")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_option_that_is_not_a_string_stays_on_its_row() {
        let component = Json::object([
            ("path", Json::string("one/hello")),
            (
                "options",
                Json::map([
                    ("count".to_string(), Json::number(3)),
                    ("on".to_string(), Json::Bool(true)),
                    ("missing".to_string(), Json::Null),
                    ("names".to_string(), Json::strings(["a|b", "two  spaces"])),
                ]),
            ),
        ]);
        let out = on_host(&Json::object([("components", Json::array([component]))]));
        assert_eq!(
            out.lines().last(),
            Some(
                "| `one/hello` |  | `count=3` `on=true` `missing=null` \
                 `names=[ \"a\\|b\", \"two  spaces\" ]` |  |  |"
            ),
            "{out}"
        );
        assert_eq!(out.lines().count(), 5, "{out}");
    }
}
