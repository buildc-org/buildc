use super::{repo_root, Call, Flags, REPO_ERROR};
use crate::command::Verb;
use crate::copy;
use crate::emit::Part;
use crate::error::Error;
use crate::Command;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn coverage(
    run: &mut crate::Run,
    json: bool,
    arg: Option<&str>,
    datastream: &Path,
) -> Result<(), Error> {
    let content = crate::scap::content_of(datastream)?;

    let image = match arg {
        Some(id) => match run.reading.list.images.iter().find(|image| image.id == id) {
            Some(image) => image,
            None => return Ok(()),
        },
        None => match run.reading.list.default_image() {
            Some(image) => image,
            None => return Ok(()),
        },
    };
    if image.conforms.is_empty() {
        run.problems.issues.push(
            crate::diag::Issue::new(format!("`{}` declares no `conforms`", image.id), &image.src)
                .at(image.span, "this image is measured against nothing")
                .help(
                    "`conforms \"<profile>\"` is the profile a scan measures it against, and what \
                 this read-out is coverage of",
                ),
        );
        return Ok(());
    }
    let Some(read_out) = crate::emit::coverage::of(image, &content, &run.reading.index) else {
        run.problems.issues.push(
            crate::diag::Issue::new(
                format!(
                    "`{}` conforms to `{}`, which is none of the profiles this datastream carries",
                    image.id, image.conforms
                ),
                &image.src,
            )
            .at(image.span, "this is what it is measured against")
            .help(format!(
                "the datastream carries: {}",
                crate::scap::profile_names(&content)
            )),
        );
        return Ok(());
    };
    let (stdout, parts) = match json {
        true => (read_out.json().render(), read_out.parts()),
        false => (read_out.markdown(), read_out.parts()),
    };
    run.report.stdout = stdout;
    run.report.parts = parts;
    Ok(())
}

fn formatted(command: Command, format: Option<&str>) -> Result<Command, Error> {
    match (command, format) {
        (Command::Graph, None | Some("md")) => Ok(Command::Graph),
        (Command::Graph, Some("json")) => Ok(Command::GraphJson),
        (Command::Why, None | Some("md")) => Ok(Command::Why),
        (Command::Why, Some("json")) => Ok(Command::WhyJson),
        (Command::Coverage, None | Some("md")) => Ok(Command::Coverage),
        (Command::Coverage, Some("json")) => Ok(Command::CoverageJson),
        (Command::Graph | Command::Why | Command::Coverage, Some(other)) => Err(Error::Invocation(
            format!("`--format` is md or json, not `{other}`"),
        )),
        (
            command @ (Command::Plan
            | Command::Check
            | Command::Generate
            | Command::Verify
            | Command::Section
            | Command::GraphJson
            | Command::Summary
            | Command::Sbom
            | Command::WhyJson
            | Command::CoverageJson),
            _,
        ) => Ok(command),
    }
}

pub(super) fn reading(verb: Verb, call: Call) -> Result<ExitCode, Error> {
    let Call {
        name,
        rest,
        prompt,
        here,
        flags,
    } = call;
    let Flags {
        format, datastream, ..
    } = flags;
    let format = format.as_deref();
    let command = formatted(verb.reads().expect("a command run reads"), format)?;

    let arg = match rest {
        [] => None,
        [one] if command.arg().is_some() => Some(*one),
        _ => {
            return Err(Error::Invocation(format!(
                "`{name}` does not take {}",
                rest.join(" ")
            )));
        }
    };

    if matches!(command, Command::Coverage | Command::CoverageJson) && datastream.is_none() {
        return Err(Error::Invocation(
            "`coverage` reads the profile out of a SCAP datastream, and never probes this machine \
             for one: `buildc coverage --datastream <file>`\n\nhelp: `buildc scap content` prints the \
             path a scan of this repository uses"
                .into(),
        ));
    }

    let root = repo_root(here)?;
    let picks = matches!(
        command,
        Command::Why | Command::WhyJson | Command::Coverage | Command::CoverageJson
    ) && arg.is_none()
        && prompt.draws();
    let mut chosen = None;
    let mut run = if picks {
        let loaded = crate::load(&root);
        let (question, known, shown) = match command {
            Command::Coverage | Command::CoverageJson => {
                let known: Vec<String> = loaded.list.images.iter().map(|i| i.id.clone()).collect();
                (copy::WHICH_IMAGE, known.clone(), known)
            }
            Command::Plan
            | Command::Check
            | Command::Generate
            | Command::Verify
            | Command::Section
            | Command::Graph
            | Command::GraphJson
            | Command::Summary
            | Command::Sbom
            | Command::Why
            | Command::WhyJson => {
                let known = crate::emit::why::known(&loaded.list);
                let shown = crate::emit::why::display(&known);
                (copy::WHICH_COMPONENT, known, shown)
            }
        };
        if known.is_empty() {
            crate::run_loaded(command, None, &root, loaded)
        } else {
            let options = picker_options(&shown);
            let Some(at) = prompt.choose(question, &options)? else {
                return Ok(ExitCode::SUCCESS);
            };
            chosen = Some(known[at].clone());
            crate::run_loaded(command, chosen.as_deref(), &root, loaded)
        }
    } else {
        crate::run(command, arg, &root)
    };
    let arg = chosen.as_deref().or(arg);

    if matches!(command, Command::Coverage | Command::CoverageJson) {
        let disk = crate::parse::disk::Disk::scan(&root);
        run.reading.index =
            crate::provider::Index::scan(&root, &run.reading.list.sources, &disk, false);
        coverage(
            &mut run,
            command == Command::CoverageJson,
            arg,
            datastream
                .as_deref()
                .expect("coverage checked its datastream"),
        )?;
    }
    let problems = run.problems.report();
    if command == Command::Check {
        for name in &run.problems.unpinned {
            eprintln!(
                "buildc: `{name}` is unpinned, so a fetch from it takes whatever its ref holds \
                 then, unverified"
            );
        }
        if let Some(version) = crate::parse::repo::pinned_unverified(&root) {
            eprintln!(
                "buildc: `buildc-version` names {version} with no sha256, so the binary it fetches \
                 is whatever the release holds, unverified"
            );
        }
        for name in &run.problems.ungoverned {
            eprintln!(
                "buildc: `{name}` ships a Containerfile fragment, and the script rule does not \
                 reach a fragment's own RUN lines"
            );
        }
        for line in crate::init::drifted(&root) {
            eprintln!("buildc: {line}");
        }
        for line in
            crate::scap::conformance(&run.reading.list, &run.reading.index, datastream.as_deref())?
        {
            eprintln!("buildc: {line}");
        }
    }
    if problems {
        return Ok(ExitCode::from(REPO_ERROR));
    }
    if command == Command::Generate {
        let wrote: Vec<(PathBuf, crate::create::Change)> = run
            .files
            .iter()
            .map(|(path, _)| path)
            .chain(run.copies.iter().map(|copied| &copied.path))
            .map(|path| {
                let change = match root.join(path).exists() {
                    true => crate::create::Change::Updated(String::new()),
                    false => crate::create::Change::Created,
                };
                (path.clone(), change)
            })
            .collect();
        crate::write_generated(&root, &run.files, &run.copies)?;
        if prompt.draws() {
            crate::report::report(&root, &wrote);
            run.report.stdout.clear();
        }
    }
    print!(
        "{}",
        match matches!(command, Command::Graph | Command::Why | Command::Coverage)
            && prompt.draws()
            && fits(&run.report.parts)
        {
            false => run.report.stdout,
            true => rendered(&run.report.parts),
        }
    );
    if command == Command::Check {
        match run.counts.images {
            0 => eprintln!("buildc: no image yet; `buildc create image <name>` writes one"),
            _ => eprintln!(
                "buildc: {} images, {} components, {} flavours{}",
                run.counts.images,
                run.counts.components,
                run.counts.flavours,
                match run.counts.suppressed {
                    0 => String::new(),
                    n => format!(", {n} the base already provides"),
                }
            ),
        }
    }
    if matches!(command, Command::Coverage | Command::CoverageJson) {
        if let Some(table) = run.report.parts.iter().find_map(|part| match part {
            crate::emit::Part::Table(table) => Some(table),
            crate::emit::Part::Heading(_) | crate::emit::Part::Text(_) => None,
        }) {
            eprintln!(
                "buildc: {} of {} rules are unclaimed",
                table.rows.iter().filter(|(_, defect)| *defect).count(),
                table.rows.len()
            );
        }
        match run.reading.index.unsearched() {
            clause if clause.is_empty() => {}
            clause => eprintln!("buildc: {clause}"),
        }
    }
    if command == Command::Generate && run.files.is_empty() {
        eprintln!("buildc: nothing to generate; `buildc create image <name>` writes an image");
    }
    if command == Command::Verify {
        let count = run.files.len() + run.copies.len();
        eprintln!(
            "buildc: {count} generated file{} match the manifests",
            if count == 1 { "" } else { "s" }
        );
    }
    Ok(ExitCode::SUCCESS)
}

pub(crate) fn picker_options(shown: &[String]) -> Vec<common::ui::Choice> {
    shown
        .iter()
        .map(|name| common::ui::Choice::new(name, ""))
        .collect()
}

pub(super) fn fits(parts: &[Part]) -> bool {
    let room = common::ui::width();
    parts.iter().all(|part| match part {
        Part::Table(table) => room >= common::ui::table::floor(table.header, &table.rows),
        Part::Heading(_) | Part::Text(_) => true,
    })
}

pub(super) fn rendered(parts: &[Part]) -> String {
    parts
        .iter()
        .map(|part| match part {
            Part::Heading(text) => common::ui::bold(text),
            Part::Text(text) => common::ui::table::wrap(text, common::ui::width()).join("\n"),
            Part::Table(table) => {
                common::ui::table::render(&table.title, table.header, &table.rows)
                    .trim_end()
                    .to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n\n")
        + "\n"
}
