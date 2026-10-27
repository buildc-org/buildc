use crate::model::image::List;
use crate::model::options::VersionTracker;
use common::json::Json;
use std::path::{Path, PathBuf};

const TEMPLATE: &str = include_str!("renovate.json5");
const POLICY: &str = "  // buildc:policy\n\n";
const MANAGERS: &str = "    // buildc:managers\n";

pub fn path() -> PathBuf {
    PathBuf::from(".github/renovate.json5")
}

pub fn file(list: &List) -> Option<String> {
    let policy: String = list
        .renovate
        .as_ref()?
        .iter()
        .map(|(key, value)| format!("  {}: {},\n", quoted(key), indented(value, "  ")))
        .collect();
    let policy = match policy.is_empty() {
        true => policy,
        false => format!("{policy}\n"),
    };
    let managers: String = managers(list)
        .iter()
        .map(|manager| format!("    {},\n", indented(manager, "    ")))
        .collect();
    Some(
        TEMPLATE
            .replace(POLICY, &policy)
            .replace(MANAGERS, &managers),
    )
}

fn managers(list: &List) -> Vec<Json> {
    let mut seen: Vec<(String, &str)> = Vec::new();
    let mut managers = Vec::new();
    for image in &list.images {
        let file = Path::new(image.src.name())
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .expect("an image source is named by its file");
        let based = image.base_entry.as_ref().zip(image.base.as_ref());
        if let Some((held, tracker)) = based
            .filter(|(held, _)| {
                held.entry
                    .options
                    .iter()
                    .any(|(name, ..)| name == "version")
            })
            .and_then(|(held, base)| Some((held, base.tracker.as_ref()?)))
        {
            managers.push(manager(
                &format!("`version` of the base {} in {file}", held.entry.path),
                &file,
                vec![
                    entry_match(&held.entry.path, "version", "currentValue", "[^\"]+"),
                    entry_match(
                        &held.entry.path,
                        "digest",
                        "currentDigest",
                        "sha256:[0-9a-f]+",
                    ),
                ],
                tracker,
            ));
        }
        for entry in &image.entries {
            if !entry.options.iter().any(|(name, ..)| name == "version") {
                continue;
            }
            let Some(tracker) = entry
                .component
                .as_ref()
                .and_then(|component| component.options.iter().find(|opt| opt.name == "version"))
                .and_then(|opt| opt.renovate.as_ref())
            else {
                continue;
            };
            if seen.iter().any(|(f, p)| *f == file && *p == entry.path) {
                continue;
            }
            seen.push((file.clone(), &entry.path));
            managers.push(manager(
                &format!("`version` of {} in {file}", entry.path),
                &file,
                vec![entry_match(
                    &entry.path,
                    "version",
                    "currentValue",
                    "[^\"]+",
                )],
                tracker,
            ));
        }
    }
    managers
}

fn manager(description: &str, file: &str, patterns: Vec<String>, tracker: &VersionTracker) -> Json {
    let mut fields = vec![
        ("customType", Json::string("regex")),
        ("description", Json::string(description)),
        (
            "managerFilePatterns",
            Json::strings([format!("/^{}$/", escaped(file))]),
        ),
        ("matchStrings", Json::strings(patterns.clone())),
        ("datasourceTemplate", Json::string(&tracker.datasource)),
        ("depNameTemplate", Json::string(&tracker.dep_name)),
    ];
    if let Some(extract) = &tracker.extract_version {
        fields.push(("extractVersionTemplate", Json::string(extract)));
    }
    if patterns.len() > 1 {
        fields.push(("matchStringsStrategy", Json::string("combination")));
    }
    Json::object(fields)
}

fn entry_match(path: &str, field: &str, group: &str, value: &str) -> String {
    let segment = |part: &str| {
        let part = escaped(part);
        format!("(?:^|[\\s{{;])(?:\"{part}\"|{part})(?:[ \\t][^{{\\n;]*)?\\{{")
    };
    let parts: Vec<&str> = path.split('/').collect();
    let (last, parents) = parts.split_last().expect("a component path has a segment");
    let mut pattern: String = parents
        .iter()
        .map(|part| format!("{}{SIBLINGS}", segment(part)))
        .collect();
    pattern.push_str(&segment(last));
    pattern.push_str(&format!(
        "(?:{SIBLINGS}[;\\n])?\\s*{field}\\s+\"(?<{group}>{value})\""
    ));
    pattern
}

// Renovate runs RE2, which has no lookaround, so the nested braces are matched by explicit repetition.
const SIBLINGS: &str = r"(?:[^{}]|\{(?:[^{}]|\{(?:[^{}]|\{[^{}]*\})*\})*\})*?";

fn escaped(text: &str) -> String {
    text.chars()
        .map(|c| match "\\.^$|?*+()[]{}".contains(c) {
            true => format!("\\{c}"),
            false => c.to_string(),
        })
        .collect()
}

fn quoted(key: &str) -> String {
    Json::string(key).render().trim_end().to_string()
}

fn indented(value: &Json, indent: &str) -> String {
    value
        .render()
        .trim_end()
        .lines()
        .collect::<Vec<_>>()
        .join(&format!("\n{indent}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_template_holds_both_markers() {
        assert!(TEMPLATE.contains(POLICY));
        assert!(TEMPLATE.contains(MANAGERS));
    }

    #[test]
    fn a_pattern_holds_no_lookaround() {
        let pattern = entry_match("apps/browser", "version", "currentValue", "[^\"]+");
        for lookaround in ["(?=", "(?!", "(?<=", "(?<!"] {
            assert!(!pattern.contains(lookaround), "{pattern}");
        }
    }

    #[test]
    fn only_a_metacharacter_is_escaped() {
        assert_eq!(
            escaped("core/image-viewer_2@1.x"),
            "core/image-viewer_2@1\\.x"
        );
    }
}
