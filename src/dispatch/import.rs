use super::{collection_repo, one_name, open, Call, Flags, REPO_ERROR};
use crate::copy;
use crate::error::Error;
use crate::provenance::lock;
use common::prompt::Prompt;
use common::ui::Choice;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

pub(super) fn component(call: Call) -> Result<ExitCode, Error> {
    let Call {
        name,
        rest,
        prompt,
        here,
        flags,
        ..
    } = call;
    let Flags {
        images,
        datastream,
        dependencies,
        ..
    } = flags;
    let name = one_name(rest, name)?;
    let (root, list) = collection_repo(here)?;
    let mut say = |text: &str| print!("{text}");
    let component = crate::import::Component::collect(
        crate::import::Request {
            name,
            image: images.last().cloned(),
            datastream: datastream.as_deref(),
            dependencies,
        },
        &root,
        &list.sources,
        list.audit_enforce,
        prompt,
        &mut say,
    )?;
    component.apply(&root, &list.sources)?;
    Ok(ExitCode::SUCCESS)
}

pub(super) fn components(call: Call) -> Result<ExitCode, Error> {
    let Call { rest, here, .. } = call;
    let (root, list) = collection_repo(here)?;
    let wrote = crate::fetch::components(&root, &list.sources, list.audit_enforce, rest)?;
    let tracks = crate::provenance::lock::read(&root)
        .iter()
        .any(|record| record.tracked);
    match (wrote.is_empty(), tracks) {
        (false, _) => crate::report::report(&root, &wrote),
        (true, true) => eprintln!("buildc: every tracked component matches its library"),
        (true, false) => eprintln!("buildc: {}", copy::NOTHING_TRACKED),
    }
    Ok(ExitCode::SUCCESS)
}

pub(super) fn detach(call: Call) -> Result<ExitCode, Error> {
    let Call {
        name,
        rest,
        prompt,
        here,
        flags,
        ..
    } = call;
    let Flags { yes, .. } = flags;
    let named = one_name(rest, name)?;
    let Some(root) = open(here)? else {
        return Ok(ExitCode::from(REPO_ERROR));
    };
    // If the lock does not parse, then lock::entry and lock::read find no record.
    lock::parses(&root)?;
    let key = match named {
        Some(path) => lock::key(&path).to_string(),
        None => match choose_tracked(name, &root, prompt)? {
            Some(key) => key,
            None => return Ok(ExitCode::SUCCESS),
        },
    };
    let Some(record) = lock::entry(&root, &key) else {
        return Err(Error::Operation(format!(
            "{}; it has no entry in the lock",
            copy::not_tracking(&key)
        )));
    };
    if !record.tracked {
        eprintln!("buildc: {}", copy::not_tracking(&key));
        return Ok(ExitCode::SUCCESS);
    }
    if !yes {
        if !prompt.asks() {
            return Err(Error::Invocation(format!(
                "`{name}` asks before it detaches; pass `--yes` to answer"
            )));
        }
        if !prompt.confirm(&copy::detach(&key, &record.library), copy::YES, copy::NO)? {
            return Ok(ExitCode::SUCCESS);
        }
    }
    let detached =
        crate::create::Change::Updated(format!("`{key}` detached from `{}`", record.library));
    lock::untrack(&root, record)?;
    crate::report::report(
        &root,
        &[(
            PathBuf::from(crate::layout::COMPONENTS).join(crate::layout::LOCK_FILE),
            detached,
        )],
    );
    Ok(ExitCode::SUCCESS)
}

fn choose_tracked(name: &str, root: &Path, prompt: &Prompt) -> Result<Option<String>, Error> {
    if !prompt.asks() {
        return Err(Error::Invocation(format!(
            "`{name}` takes the component to detach as `<path>`"
        )));
    }
    let tracked = lock::tracked(root)?;
    if tracked.is_empty() {
        eprintln!("buildc: {}", copy::NOTHING_TRACKED);
        return Ok(None);
    }
    let options: Vec<Choice> = tracked
        .iter()
        .map(|record| Choice::new(&record.path, &record.library))
        .collect();
    Ok(prompt
        .choose(copy::WHICH_COMPONENT, &options)?
        .map(|at| tracked[at].path.clone()))
}
