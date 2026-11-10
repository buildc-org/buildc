use crate::diag::{Issue, Issues, Source};
use crate::model::component::Component;
use crate::model::image::{Entry, Image};
use crate::model::options::{check_values, env_name, env_value, Opt, Value, Variant};

pub fn resolve(
    options: &[Opt],
    variants: &[Variant],
    src: &Source,
    entry: &Entry,
    image: &Image,
    issues: &mut Issues,
) -> Vec<(String, String)> {
    let selected = entry.variant.as_ref();
    let set = &entry.options;
    let component_path = entry.path.as_str();
    let list_src = &image.src;

    let image_id = [("image-id", image.id.as_str())];
    let set_from = [
        ("image-id", image.id.as_str()),
        ("base-url", image.repo_base_url.as_str()),
    ];
    let mut resolved: Vec<(String, Vec<Value>)> = options
        .iter()
        .map(|o| {
            let values = filled(&o.default, &image_id).unwrap_or_else(|name| {
                issues.push(
                    Issue::new(
                        format!("the default of `{}` reads `{{{name}}}`, which a default cannot read", o.name),
                        src,
                    )
                    .at(o.span, "an unknown reference")
                    .help("a default may read `{image-id}`; an image passes any other value as the option"),
                );
                o.default.clone()
            });
            (o.name.clone(), values)
        })
        .collect();

    let find = |name: &str| options.iter().find(|o| o.name == name);

    if let Some(want) = selected {
        match variants.iter().find(|v| &v.name == want) {
            Some(variant) => {
                for (name, values, span) in &variant.sets {
                    let Some(opt) = find(name) else {
                        issues.push(
                            Issue::new(
                                format!("variant `{want}` sets `{name}`, which this component does not declare"),
                                src,
                            )
                            .at(*span, "no such option")
                            .help("a variant may only set options declared in the same manifest"),
                        );
                        continue;
                    };
                    let values = match filled(values, &image_id) {
                        Ok(values) => values,
                        Err(reference) => {
                            issues.push(
                                Issue::new(
                                    format!("variant `{want}` sets `{name}` to a value that reads `{{{reference}}}`. A variant cannot read that value"),
                                    src,
                                )
                                .at(*span, "an unknown reference")
                                .help("a variant may read `{image-id}`; an image passes any other value as the option"),
                            );
                            continue;
                        }
                    };
                    if check_values(name, opt.ty, &values, src, *span, issues) {
                        if let Some(slot) = resolved.iter_mut().find(|(n, _)| n == name) {
                            slot.1 = values;
                        }
                    }
                }
            }
            None => {
                let known: Vec<&str> = variants.iter().map(|v| v.name.as_str()).collect();
                issues.push(
                    Issue::new(
                        format!("`{component_path}` has no variant `{want}`"),
                        list_src,
                    )
                    .help(if known.is_empty() {
                        "this component declares no variants".to_string()
                    } else {
                        format!("declared variants: {}", known.join(", "))
                    }),
                );
            }
        }
    }

    let mut seen: Vec<&str> = Vec::new();
    for (name, values, span) in set {
        let Some(opt) = find(name) else {
            let known: Vec<&str> = options.iter().map(|o| o.name.as_str()).collect();
            issues.push(
                Issue::new(
                    format!("`{component_path}` has no option `{name}`"),
                    list_src,
                )
                .at(*span, "not declared by this component")
                .help(if known.is_empty() {
                    "this component declares no options".to_string()
                } else {
                    format!("declared options: {}", known.join(", "))
                }),
            );
            continue;
        };
        if seen.contains(&name.as_str()) {
            issues.push(
                Issue::new(
                    format!("`{name}` is set twice on `{component_path}`"),
                    list_src,
                )
                .at(*span, "set again here")
                .help("resolution is a single pass, so a second value is an error"),
            );
            continue;
        }
        seen.push(name.as_str());

        let values = &match filled(values, &set_from) {
            Ok(values) => values,
            Err(reference) => {
                issues.push(
                    Issue::new(
                        format!("`{name}` on `{component_path}` reads `{{{reference}}}`, which nothing declares"),
                        list_src,
                    )
                    .at(*span, "an unknown reference")
                    .help(match reference.as_str() {
                        "base-url" => "declare `base-url \"https://host/owner/repository\"` in repo.kdl".to_string(),
                        _ => "an option value may read `{base-url}` and `{image-id}`".to_string(),
                    }),
                );
                continue;
            }
        };
        if check_values(name, opt.ty, values, list_src, *span, issues) {
            if let Some(slot) = resolved.iter_mut().find(|(n, _)| n == name) {
                slot.1 = values.clone();
            }
        }
    }

    options
        .iter()
        .map(|opt| {
            let values = resolved
                .iter()
                .find(|(n, _)| n == &opt.name)
                .map(|(_, v)| v.clone())
                .unwrap_or_default();
            (env_name(&opt.name), env_value(opt.ty, &values))
        })
        .collect()
}

// An empty value counts as undeclared. A reference to an unset `base-url` is refused.
fn filled(values: &[Value], known: &[(&str, &str)]) -> Result<Vec<Value>, String> {
    let mut unknown = None;
    let out = values
        .iter()
        .map(|held| match held {
            Value::String(text) => Value::String(interpolate(text, |name| {
                match known
                    .iter()
                    .find(|(have, value)| *have == name && !value.is_empty())
                {
                    Some((_, value)) => (*value).to_string(),
                    None => {
                        unknown.get_or_insert_with(|| name.to_string());
                        String::new()
                    }
                }
            })),
            other => other.clone(),
        })
        .collect();
    match unknown {
        Some(name) => Err(name),
        None => Ok(out),
    }
}

// A doubled brace writes a literal brace. A value can hold one next to a reference.
pub fn interpolate(text: &str, mut value: impl FnMut(&str) -> String) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find(['{', '}']) {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        let brace = &rest[at..=at];
        if after.starts_with(brace) {
            out.push_str(brace);
            rest = &after[1..];
            continue;
        }
        let name = match (brace, after.split_once('}')) {
            ("{", Some((name, _))) if is_reference(name) => name,
            _ => {
                out.push_str(brace);
                rest = after;
                continue;
            }
        };
        out.push_str(&value(name));
        rest = &after[name.len() + 1..];
    }
    out.push_str(rest);
    out
}

fn is_reference(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_lowercase())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

pub fn apply(component: &mut Component, entry: &Entry, image: &Image, issues: &mut Issues) {
    let resolved = &component.resolved;
    let value = |name: &str| {
        let env = env_name(name);
        resolved
            .iter()
            .find(|(have, _)| *have == env)
            .map(|(_, value)| value.clone())
            .unwrap_or_default()
    };
    for batch in component
        .packages
        .iter_mut()
        .chain(component.groups.iter_mut())
    {
        let mut kept = Vec::new();
        for name in &batch.packages {
            let written = interpolate(name, value);
            if written == *name {
                kept.push(written);
                continue;
            }
            match crate::parse::component::bad_package(&written) {
                Some(problem) => issues.push(
                    Issue::new(
                        format!("`{}` installs `{name}` as `{written}`, which {problem}", entry.path),
                        &image.src,
                    )
                    .at(entry.span, "an option value spoils the package name")
                    .help("the option set on this entry, or its default, fills the package name; it holds letters, digits and . _ + : -"),
                ),
                None => kept.push(written),
            }
        }
        batch.packages = kept;
    }
    for import in &mut component.imports {
        import.path = interpolate(&import.path, value);
        for (_, values) in &mut import.options {
            for held in values {
                if let Value::String(text) = held {
                    *text = interpolate(text, value);
                }
            }
        }
    }
    if let Some(base) = &mut component.base {
        base.image = interpolate(&base.image, value);
    }
}

#[cfg(test)]
mod tests {
    use super::interpolate;

    #[test]
    fn a_reference_reads_the_option_it_names() {
        let written = interpolate("firefox={version}", |name| format!("<{name}>"));
        assert_eq!(written, "firefox=<version>");
    }

    #[test]
    fn a_reference_ends_at_its_closing_brace() {
        let written = interpolate("{a-1}.{b}_c", |name| format!("<{name}>"));
        assert_eq!(written, "<a-1>.<b>_c");
    }

    #[test]
    fn a_doubled_brace_writes_one_brace() {
        assert_eq!(interpolate("{{a}} {{", |_| "x".to_string()), "{a} {");
    }

    #[test]
    fn a_brace_that_holds_no_name_stays() {
        assert_eq!(interpolate("a{1} {B} {", |_| "x".to_string()), "a{1} {B} {");
    }

    #[test]
    fn text_before_a_brace_does_not_hang() {
        // A regression that loops would hang the suite. Wait from outside the loop.
        let (done, finished) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            interpolate("a{version}", |_| "x".to_string());
            done.send(())
                .expect("the test receiver outlives the worker");
        });
        finished
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("interpolation past leading text must terminate");
    }
}
