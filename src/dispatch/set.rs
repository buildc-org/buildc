use super::{one_name, open, Call, Flags, REPO_ERROR};
use crate::copy;
use crate::error::Error;
use std::path::PathBuf;
use std::process::ExitCode;

pub(super) fn key(call: Call) -> Result<ExitCode, Error> {
    let Call {
        name,
        rest,
        prompt,
        here,
        flags,
        ..
    } = call;
    let Flags {
        component: component_arg,
        from,
        ..
    } = flags;
    let kind = one_name(rest, name)?;
    let Some(root) = open(here)? else {
        return Ok(ExitCode::from(REPO_ERROR));
    };
    crate::key::Recorded::collect(&root, kind, component_arg, from, prompt)?.apply(&root)?;
    Ok(ExitCode::SUCCESS)
}

pub(super) fn component(call: Call) -> Result<ExitCode, Error> {
    let Call {
        name,
        rest,
        prompt,
        here,
        flags,
        ..
    } = call;
    let Flags { untracked, .. } = flags;
    let path = one_name(rest, name)?.ok_or_else(|| {
        Error::Invocation(format!(
            "`{name}` takes the component to make the repository's own"
        ))
    })?;
    let Some(root) = open(here)? else {
        return Ok(ExitCode::from(REPO_ERROR));
    };
    let key = crate::provenance::lock::key(&path);
    if !untracked {
        if !prompt.asks() {
            return Err(Error::Invocation(format!(
                "`{name}` asks before it stops tracking; pass `--untracked` to answer"
            )));
        }
        if !prompt.confirm(&copy::untrack(key), copy::YES, copy::NO)? {
            return Ok(ExitCode::SUCCESS);
        }
    }
    match crate::provenance::lock::untrack(&root, key)? {
        true => crate::report::report(
            &root,
            &[(
                PathBuf::from(crate::layout::COMPONENTS).join(crate::layout::LOCK_FILE),
                crate::create::Change::Updated(format!("`{key}` untracked")),
            )],
        ),
        false => eprintln!(
            "buildc: `{}/{key}` is already the repository's own",
            crate::layout::COMPONENTS
        ),
    }
    Ok(ExitCode::SUCCESS)
}

pub(super) fn workflows(call: Call) -> Result<ExitCode, Error> {
    let Call {
        name,
        rest,
        prompt,
        here,
        flags,
        ..
    } = call;
    let Flags { .. } = flags;
    if let [word, ..] = rest {
        return Err(Error::Invocation(format!("`{name}` does not take {word}")));
    }
    let Some(root) = open(here)? else {
        return Ok(ExitCode::from(REPO_ERROR));
    };
    if !prompt.asks() {
        return Err(Error::Invocation(crate::setup::BY_HAND.to_string()));
    }
    let list = crate::load(&root).list;
    let on: Vec<&str> = list.workflows.iter().map(|w| w.name.as_str()).collect();
    let mut say = |text: &str| print!("{text}");
    let Some(set) = crate::setup::Workflows::collect(
        &crate::resolve::workflow::Basis::of(&list),
        &on,
        list.workflows_at,
        list.publishes_scheduled,
        list.scans_scheduled,
        crate::setup::Field::Workflows,
        prompt,
        &mut say,
    )?
    else {
        return Ok(ExitCode::SUCCESS);
    };
    let wrote = set.apply(&root)?;
    crate::report::report(&root, &wrote);
    println!("\nnext, in {}:\n\x20 buildc generate\n", root.display());
    Ok(ExitCode::SUCCESS)
}

pub(super) fn library(call: Call) -> Result<ExitCode, Error> {
    let Call {
        prompt,
        here,
        flags,
        ..
    } = call;
    let Flags { .. } = flags;
    let kind = crate::model::remote::Kind::Components;
    let Some(root) = open(here)? else {
        return Ok(ExitCode::from(REPO_ERROR));
    };
    let Some(set) = crate::set::Library::adding(kind, prompt)? else {
        return Ok(ExitCode::SUCCESS);
    };
    let wrote = set.apply(&root)?;
    crate::report::report(&root, &wrote);
    Ok(ExitCode::SUCCESS)
}

pub(super) fn conforms(call: Call) -> Result<ExitCode, Error> {
    let Call {
        name,
        rest,
        prompt,
        here,
        flags,
        ..
    } = call;
    let Flags { datastream, .. } = flags;
    let name = one_name(rest, name)?;
    let Some(root) = open(here)? else {
        return Ok(ExitCode::from(REPO_ERROR));
    };
    if !prompt.asks() {
        return Err(Error::Invocation(
            crate::conformance::CONFORMS_BY_HAND.to_string(),
        ));
    }
    let Some(set) = crate::set::Conforms::collect(&root, name, datastream, prompt, &mut |text| {
        print!("{text}");
    })?
    else {
        return Ok(ExitCode::SUCCESS);
    };
    let wrote = set.apply(&root)?;
    crate::report::report(&root, &wrote);
    Ok(ExitCode::SUCCESS)
}

pub(super) fn claims(call: Call) -> Result<ExitCode, Error> {
    let Call {
        name,
        rest,
        prompt,
        here,
        flags,
        ..
    } = call;
    let Flags { datastream, .. } = flags;
    let named = one_name(rest, name)?
        .ok_or_else(|| Error::Invocation(format!("`{name}` takes the component to claim for")))?;
    let Some(root) = open(here)? else {
        return Ok(ExitCode::from(REPO_ERROR));
    };
    if !prompt.asks() {
        return Err(Error::Invocation(crate::set::CLAIMS_BY_HAND.to_string()));
    }
    let Some(set) = crate::set::Claims::collect(&root, &named, datastream, prompt, &mut |text| {
        print!("{text}");
    })?
    else {
        return Ok(ExitCode::SUCCESS);
    };
    let wrote = set.apply(&root)?;
    crate::report::report(&root, &wrote);
    Ok(ExitCode::SUCCESS)
}
