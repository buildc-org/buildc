use super::{repo_root, Call, Flags, REPO_ERROR};
use crate::error::Error;
use std::path::Path;
use std::process::ExitCode;

pub(super) fn content(call: Call) -> Result<ExitCode, Error> {
    let Call { here, flags, .. } = call;
    let Flags { target, .. } = flags;
    Ok(
        match crate::scap::content(&repo_root(here)?, target.as_deref())? {
            crate::scap::Verdict::Clean => ExitCode::SUCCESS,
            crate::scap::Verdict::Wrong => ExitCode::from(REPO_ERROR),
        },
    )
}

pub(super) fn tailoring(call: Call) -> Result<ExitCode, Error> {
    let Call { here, flags, .. } = call;
    let Flags {
        target, datastream, ..
    } = flags;
    Ok(
        match crate::scap::tailoring_for(
            &repo_root(here)?,
            target.as_deref(),
            datastream.as_deref(),
        )? {
            crate::scap::Verdict::Clean => ExitCode::SUCCESS,
            crate::scap::Verdict::Wrong => ExitCode::from(REPO_ERROR),
        },
    )
}

pub(super) fn rules(call: Call) -> Result<ExitCode, Error> {
    let Call { rest, flags, .. } = call;
    let Flags { datastream, .. } = flags;
    let Some(path) = datastream.as_deref() else {
        return Err(Error::Invocation(
            "`scap rules` needs the content to resolve against: \
                 `buildc scap rules --datastream <file> <number>...`"
                .into(),
        ));
    };
    let content = crate::scap::content_of(path)?;
    let mut lost: Vec<&str> = Vec::new();
    for number in rest {
        match content.rules.get(*number) {
            Some(rule) => println!("{}", crate::scap::rule_name(rule)),
            None => lost.push(number),
        }
    }
    if !lost.is_empty() {
        return Err(Error::Invocation(format!(
            "{} no rule in {}: {}\n\nhelp: a number resolves against the content \
                 the base is measured with, so a claim written for one family does not \
                 resolve against another's",
            match lost.len() {
                1 => "one number reaches".to_string(),
                n => format!("{n} numbers reach"),
            },
            path.display(),
            lost.join(", ")
        )));
    }
    Ok(ExitCode::SUCCESS)
}

pub(super) fn report(call: Call) -> Result<ExitCode, Error> {
    let Call {
        rest, here, flags, ..
    } = call;
    let Flags {
        target,
        datastream,
        baseline,
        base_scan,
        ..
    } = flags;
    let [arf] = rest else {
        return Err(Error::Invocation(
            "`scap` takes one report: `buildc scap <arf.xml>`, or `scap content`".into(),
        ));
    };
    let opts = crate::scap::Options {
        target,
        datastream,
        baseline,
        base: base_scan,
    };
    Ok(
        match crate::scap::run(&repo_root(here)?, Path::new(arf), &opts)? {
            crate::scap::Verdict::Clean => ExitCode::SUCCESS,
            crate::scap::Verdict::Wrong => ExitCode::from(REPO_ERROR),
        },
    )
}
