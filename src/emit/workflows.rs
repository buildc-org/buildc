use crate::model::image::On;

pub fn render(
    body: &str,
    triggers: Option<&[On]>,
    schedule: Option<&str>,
    facts: &[&str],
) -> String {
    let body = match triggers {
        Some(triggers) => retriggered(body, triggers),
        None => body.to_string(),
    };
    let mut out = String::new();
    let mut keep = true;
    for line in body.lines() {
        let trimmed = line.trim();
        if let Some(fact) = trimmed.strip_prefix("# buildc:only ") {
            keep = facts.contains(&fact);
            continue;
        }
        if trimmed == "# buildc:end" {
            keep = true;
            continue;
        }
        if !keep {
            continue;
        }
        match line.trim_end().strip_suffix("# buildc:schedule") {
            Some(cron) => out.push_str(&scheduled(cron.trim_end(), schedule)),
            None => out.push_str(line),
        }
        out.push('\n');
    }
    out
}

/// Each shipped workflow writes a trigger key at a two-space indent with no value on its line.
fn trigger_chunks(section: &[&str]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for line in section.iter().filter(|line| !line.trim().is_empty()) {
        let key = line
            .strip_prefix("  ")
            .filter(|rest| !rest.starts_with(' ') && !rest.starts_with('#'))
            .and_then(|rest| rest.strip_suffix(':'));
        match (key, out.last_mut()) {
            (Some(key), _) => out.push((key.to_string(), format!("{line}\n"))),
            (None, Some((_, text))) => {
                text.push_str(line);
                text.push('\n');
            }
            (None, None) => {}
        }
    }
    out
}

fn retriggered(body: &str, triggers: &[On]) -> String {
    let lines: Vec<&str> = body.lines().collect();
    let Some(start) = lines.iter().position(|line| *line == "on:") else {
        return body.to_string();
    };
    let end = lines[start + 1..]
        .iter()
        .position(|line| !line.is_empty() && !line.starts_with(' ') && !line.starts_with('#'))
        .map_or(lines.len(), |at| start + 1 + at);
    let shipped = trigger_chunks(&lines[start + 1..end]);
    let take = |key: &str| {
        shipped
            .iter()
            .find(|(held, _)| held == key)
            .map(|(_, text)| text.clone())
    };
    let (mut pr, mut pushed, mut schedule, mut tag) = (false, false, false, None);
    for on in triggers {
        match on {
            On::Pr => pr = true,
            On::Push => pushed = true,
            On::Schedule { .. } => schedule = true,
            // YAML doubles a single quote inside a single-quoted scalar.
            On::Tag(glob) => tag = Some(glob.replace('\'', "''")),
        }
    }

    let mut section = String::from("on:\n");
    if pr {
        section.push_str(
            &take("pull_request")
                .unwrap_or_else(|| "  pull_request:\n    branches:\n      - main\n".into()),
        );
    }
    let push = pushed
        .then(|| take("push").unwrap_or_else(|| "  push:\n    branches:\n      - main\n".into()));
    match (push, tag) {
        (Some(push), Some(glob)) => {
            section.push_str(&format!("{push}    tags:\n      - '{glob}'\n"))
        }
        (Some(push), None) => section.push_str(&push),
        (None, Some(glob)) => section.push_str(&format!("  push:\n    tags:\n      - '{glob}'\n")),
        (None, None) => {}
    }
    if schedule {
        section.push_str(&take("schedule").unwrap_or_else(|| {
            // The buildc:schedule marker makes render write the declared cron over this placeholder.
            "  schedule:\n    - cron: '0 0 * * *'  # buildc:schedule\n".into()
        }));
    }
    section.push_str(&take("workflow_dispatch").unwrap_or_else(|| "  workflow_dispatch:\n".into()));

    let mut out: String = lines[..start]
        .iter()
        .map(|line| format!("{line}\n"))
        .collect();
    out.push_str(&section);
    if lines[end - 1].trim().is_empty() {
        out.push('\n');
    }
    for line in &lines[end..] {
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn scheduled(line: &str, schedule: Option<&str>) -> String {
    let Some(cron) = schedule else {
        return line.to_string();
    };
    let Some(open) = line.find('\'') else {
        return line.to_string();
    };
    let Some(close) = line[open + 1..].find('\'').map(|at| open + 1 + at) else {
        return line.to_string();
    };
    let mut out = line.to_string();
    out.replace_range(open + 1..close, cron);
    out
}

#[cfg(test)]
mod tests {
    use super::render;
    use crate::model::image::On;

    const BODY: &str = include_str!("../../assets/.github/workflows/image-build.yml");

    #[test]
    fn the_kernel_input_is_there_only_for_a_repository_that_has_one() {
        let with = render(BODY, None, None, &["kernel"]);
        assert!(with.contains("inputs:"), "{with}");
        assert!(with.contains("    - cron: '45 9 1 * *'"), "{with}");

        let without = render(BODY, None, None, &["no-kernel"]);
        assert!(!without.contains("inputs:"), "{without}");
        assert!(!without.contains("- cron: '45 9 1 * *'"), "{without}");
        assert!(without.contains("  workflow_dispatch:\n"), "{without}");
        assert!(without.contains("  push:\n"), "{without}");
        assert!(without.contains("\n  KERNEL: ''\n"), "{without}");
        assert!(!without.contains("github.event.inputs.kernel"), "{without}");
    }

    #[test]
    fn the_build_job_runs_the_declared_backend() {
        let buildah = render(BODY, None, None, &["no-kernel", "backend-buildah"]);
        assert!(buildah.contains("sudo env \\"), "{buildah}");
        assert!(!buildah.contains("setup-buildx-action"), "{buildah}");
        assert!(!buildah.contains("--oci-output"), "{buildah}");
        let buildx = render(BODY, None, None, &["no-kernel", "backend-buildx"]);
        assert!(buildx.contains("setup-buildx-action"), "{buildx}");
        assert!(
            buildx.contains("args+=(--oci-output \"$archive\")"),
            "{buildx}"
        );
        assert!(
            buildx.contains("containers-storage:localhost/${IMAGE_NAME}:raw"),
            "{buildx}"
        );
        assert!(!buildx.contains("sudo env \\"), "{buildx}");
    }

    #[test]
    fn the_declared_schedule_replaces_the_shipped_one() {
        let out = render(BODY, None, Some("0 5 * * 1"), &["no-kernel"]);
        assert!(out.contains("    - cron: '0 5 * * 1'\n"), "{out}");
        assert!(!out.contains("0 4 * * *"), "{out}");
    }

    #[test]
    fn a_checksum_fixup_says_which_commit_the_later_jobs_build() {
        assert!(BODY.contains(
            "echo \"Checksum fixup commit $(git rev-parse --short HEAD); later jobs build the amended tree.\" >> \"$GITHUB_STEP_SUMMARY\""
        ));
    }

    #[test]
    fn only_a_published_target_asking_to_be_measured_is_scanned() {
        assert!(BODY.contains("target: ${{ fromJson(needs.compute-matrix.outputs.scanned) }}"));
        assert!(BODY.contains("select(.conforms != \"\" and .reference != null) | .name"));
        assert!(BODY.contains("if: needs.compute-matrix.outputs.publishing == 'true'"));
        assert!(!BODY.contains("needs.build_push.outputs"));
        let out = render(BODY, None, None, &["no-kernel"]);
        assert_eq!(
            out.matches("needs.compute-matrix.outputs.scanned != '[]'")
                .count(),
            1,
            "{out}"
        );
    }

    #[test]
    fn listed_triggers_replace_the_shipped_ones_and_keep_dispatch() {
        let out = render(BODY, Some(&[On::Tag("v*".into())]), None, &["no-kernel"]);
        let on = &out[out.find("\non:\n").unwrap()..out.find("\nenv:").unwrap()];
        assert!(on.contains("  push:\n    tags:\n      - 'v*'\n"), "{on}");
        assert!(!on.contains("pull_request"), "{on}");
        assert!(!on.contains("cron"), "{on}");
        assert!(!on.contains("branches"), "{on}");
        assert!(on.contains("  workflow_dispatch:\n"), "{on}");
    }

    #[test]
    fn a_push_and_a_tag_share_one_push_key_with_its_shipped_filters() {
        let out = render(
            BODY,
            Some(&[On::Push, On::Tag("v*".into()), On::Pr]),
            Some("0 1 * * *"),
            &["no-kernel"],
        );
        let on = &out[out.find("\non:\n").unwrap()..out.find("\nenv:").unwrap()];
        assert_eq!(on.matches("  push:\n").count(), 1, "{on}");
        assert!(on.contains("    paths-ignore:\n"), "{on}");
        assert!(on.contains("      - 'v*'\n"), "{on}");
        assert!(
            on.contains("  pull_request:\n    branches:\n      - main\n"),
            "{on}"
        );
        assert!(!on.contains("cron"), "{on}");
    }

    #[test]
    fn a_listed_schedule_on_a_workflow_without_one_takes_the_declared_time() {
        let body = crate::resolve::workflow::find("build-disk").unwrap().body;
        let out = render(
            body,
            Some(&[On::Schedule {
                weekly: None,
                time: None,
            }]),
            Some("0 5 * * 1"),
            &["no-kernel"],
        );
        let on = &out[out.find("\non:\n").unwrap()..out.find("\npermissions:").unwrap()];
        assert!(
            on.contains("  schedule:\n    - cron: '0 5 * * 1'\n"),
            "{on}"
        );
        assert!(!on.contains("pull_request"), "{on}");
        assert!(
            out.contains("  workflow_dispatch:\n\npermissions:"),
            "{out}"
        );
    }

    #[test]
    fn the_rebuilt_section_leaves_every_line_around_it_in_place() {
        let body = "name: x\non:\n  push:\n    branches:\n      - main\n\n  workflow_dispatch:\n\njobs:\n  a:\n    runs-on: x\n";
        assert_eq!(
            render(body, Some(&[On::Push]), None, &[]),
            "name: x\non:\n  push:\n    branches:\n      - main\n  workflow_dispatch:\n\njobs:\n  a:\n    runs-on: x\n"
        );
        assert_eq!(
            render(body, Some(&[On::Tag("it's".into())]), None, &[]),
            "name: x\non:\n  push:\n    tags:\n      - 'it''s'\n  workflow_dispatch:\n\njobs:\n  a:\n    runs-on: x\n"
        );
        let tight = "---\nname: x\non:\n  push:\n  workflow_dispatch:\njobs:\n  a: {}\n";
        assert_eq!(
            render(tight, Some(&[On::Push]), None, &[]),
            "---\nname: x\non:\n  push:\n  workflow_dispatch:\njobs:\n  a: {}\n"
        );
    }

    #[test]
    fn the_smoke_test_fails_on_an_selinux_denial_and_not_on_a_quiet_journal() {
        const MATCHER: &str = "grep -Ei 'avc:[[:space:]]+denied'";
        let body = include_str!("../../assets/.github/workflows/smoke-test.yml");
        assert!(body.contains(MATCHER));
        let denials = |journal: &str| {
            let out = std::process::Command::new("bash")
                .args(["-c", &format!("printf %s \"$1\" | {MATCHER}"), "_", journal])
                .output()
                .unwrap();
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        assert!(!denials("audit: avc:  denied  { execute }").is_empty());
        assert!(denials("").is_empty());
        assert!(denials("nothing to see").is_empty());
    }

    #[test]
    fn no_marker_reaches_a_generated_file() {
        for shipped in crate::resolve::workflow::SHIPPED {
            for facts in [&["kernel"][..], &["no-kernel"][..]] {
                for schedule in [None, Some("0 5 * * 1")] {
                    let out = render(shipped.body, None, schedule, facts);
                    assert!(!out.contains("buildc:"), "{}\n{out}", shipped.stem);
                    if schedule.is_some() && out.contains("- cron: '") {
                        assert!(
                            out.contains("    - cron: '0 5 * * 1'\n"),
                            "{}\n{out}",
                            shipped.stem
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn every_shipped_job_declares_its_own_permissions() {
        let is_job = |line: &str| -> bool {
            let Some(rest) = line.strip_prefix("  ") else {
                return false;
            };
            let Some(name) = rest.strip_suffix(':') else {
                return false;
            };
            !name.is_empty()
                && name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        };
        for shipped in crate::resolve::workflow::SHIPPED {
            let lines: Vec<&str> = shipped.body.lines().collect();
            let Some(jobs_at) = lines.iter().position(|line| *line == "jobs:") else {
                panic!("{}: has no `jobs:` section", shipped.stem);
            };
            assert!(
                lines[..jobs_at].contains(&"permissions: {}"),
                "{}: no top-level `permissions: {{}}`",
                shipped.stem
            );
            let mut job: Option<&str> = None;
            let mut declared = false;
            for line in &lines[jobs_at + 1..] {
                if is_job(line) {
                    if let Some(prev) = job {
                        assert!(
                            declared,
                            "{}: job `{}` declares no permissions",
                            shipped.stem, prev
                        );
                    }
                    job = Some(&line[2..line.len() - 1]);
                    declared = false;
                } else if !declared && *line == "    permissions:" {
                    declared = true;
                }
            }
            if let Some(prev) = job {
                assert!(
                    declared,
                    "{}: job `{}` declares no permissions",
                    shipped.stem, prev
                );
            }
        }
    }

    #[test]
    fn every_checkout_says_whether_it_keeps_the_token() {
        let mut checkouts = 0;
        let mut persist = 0;
        for shipped in crate::resolve::workflow::SHIPPED {
            let lines: Vec<&str> = shipped.body.lines().collect();
            for (at, line) in lines.iter().enumerate() {
                if !line.contains("uses: actions/checkout@") {
                    continue;
                }
                checkouts += 1;
                let indent = line.len() - line.trim_start().len();
                let declared = lines[at + 1..]
                    .iter()
                    .take_while(|next| {
                        let trimmed = next.trim_start();
                        !(trimmed.starts_with("- ") && next.len() - trimmed.len() <= indent)
                    })
                    .any(|next| next.trim().starts_with("persist-credentials:"));
                assert!(
                    declared,
                    "{}: checkout step does not name `persist-credentials:`",
                    shipped.stem
                );
            }
            persist += lines
                .iter()
                .filter(|line| line.trim().starts_with("persist-credentials:"))
                .count();
        }
        assert_eq!(
            checkouts, persist,
            "every checkout names whether it keeps the token"
        );
    }
}
