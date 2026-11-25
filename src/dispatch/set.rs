use super::{one_name, open, Call, Flags, REPO_ERROR};
use crate::error::Error;
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

pub(super) fn workflows(call: Call) -> Result<ExitCode, Error> {
    let Call {
        name,
        rest,
        prompt,
        here,
        flags,
        ..
    } = call;
    let Flags {
        on,
        workflow_builds,
        ..
    } = flags;
    let workflow = one_name(rest, name)?;
    let Some(root) = open(here)? else {
        return Ok(ExitCode::from(REPO_ERROR));
    };
    let list = crate::load(&root).list;
    let set = match workflow {
        Some(stem) => {
            let file = root.join(crate::layout::REPO_FILE);
            let text = std::fs::read_to_string(&file)
                .map_err(|err| Error::Operation(format!("{}: {err}", file.display())))?;
            crate::setup::Workflows::named(&text, &list, &stem, &on, &workflow_builds)?
        }
        None => {
            if !prompt.asks() {
                return Err(Error::Invocation(crate::setup::BY_HAND.to_string()));
            }
            let listed: Vec<&str> = list.workflows.iter().map(|w| w.name.as_str()).collect();
            let mut say = |text: &str| print!("{text}");
            let Some(mut set) = crate::setup::Workflows::collect(
                &crate::resolve::workflow::Basis::of(&list),
                &listed,
                crate::setup::Field::Workflows,
                prompt,
                &mut say,
            )?
            else {
                return Ok(ExitCode::SUCCESS);
            };
            let builds: Vec<String> = list.builds.iter().map(|build| build.name.clone()).collect();
            if !set.ask_builds(&listed, &builds, prompt)? {
                return Ok(ExitCode::SUCCESS);
            }
            set
        }
    };
    let wrote = set.apply(&root)?;
    crate::report::report(&root, &wrote);
    println!("\nnext, in {}:\n\x20 buildc generate\n", root.display());
    Ok(ExitCode::SUCCESS)
}

pub(super) fn library(call: Call) -> Result<ExitCode, Error> {
    let Call {
        name,
        rest,
        prompt,
        here,
        flags,
        ..
    } = call;
    let library = one_name(rest, name)?;
    let Some(root) = open(here)? else {
        return Ok(ExitCode::from(REPO_ERROR));
    };
    let Some(set) = crate::set::Library::adding(
        &root,
        crate::init::assets()
            .map(|assets| crate::collection::defaults(&assets))
            .unwrap_or_default(),
        library,
        flags.source,
        prompt,
    )?
    else {
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
    let mut say = |text: &str| print!("{text}");
    let Some(set) = crate::set::Conforms::collect(&root, name, datastream, prompt, &mut say)?
    else {
        return Ok(ExitCode::SUCCESS);
    };
    let wrote = set.apply(&root, prompt, &mut say)?;
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
