use super::{one_name, repo_root, Call, Flags, REPO_ERROR};
use crate::error::Error;
use std::process::ExitCode;

pub(super) fn image(call: Call) -> Result<ExitCode, Error> {
    let Call {
        name,
        rest,
        here,
        flags,
        ..
    } = call;
    let Flags {
        target,
        tags,
        kernel,
        backend,
        oci_output,
        secrets,
        cache_to,
        no_cache_from,
        ..
    } = flags;
    let opts = crate::build::Options {
        target: one_name(rest, name)?.or(target),
        kernel,
        tags,
        secrets,
        backend,
        oci_output,
        no_cache_from,
        cache_to,
    };
    Ok(match crate::build::run(&repo_root(here)?, &opts)? {
        crate::build::Stopped::Repository => ExitCode::from(REPO_ERROR),
        crate::build::Stopped::Built => ExitCode::SUCCESS,
    })
}

pub(super) fn validate(call: Call) -> Result<ExitCode, Error> {
    let Call {
        name,
        rest,
        here,
        flags,
        ..
    } = call;
    let Flags { target, .. } = flags;
    let root = repo_root(here)?;
    let image = one_name(rest, name)?.ok_or_else(|| {
        Error::Invocation("`buildc validate` takes the built image to check".to_string())
    })?;
    Ok(
        match crate::build::validate(&root, &image, target.as_deref())? {
            crate::build::Stopped::Repository => ExitCode::from(REPO_ERROR),
            crate::build::Stopped::Built => ExitCode::SUCCESS,
        },
    )
}

pub(super) fn attribute(call: Call) -> Result<ExitCode, Error> {
    let Call {
        name,
        rest,
        here,
        flags,
        ..
    } = call;
    let Flags { target, .. } = flags;
    let root = repo_root(here)?;
    let image = one_name(rest, name)?.ok_or_else(|| {
        Error::Invocation("`buildc attribute` takes the built image to map".to_string())
    })?;
    Ok(
        match crate::attribute::run(&root, &image, target.as_deref())? {
            crate::build::Stopped::Repository => ExitCode::from(REPO_ERROR),
            crate::build::Stopped::Built => ExitCode::SUCCESS,
        },
    )
}

pub(super) fn vm(call: Call) -> Result<ExitCode, Error> {
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
        target,
        tags,
        ram,
        rebuild,
        ..
    } = flags;
    let root = repo_root(here)?;
    let opts = crate::vm::Options {
        target,
        image: images.last().cloned(),
        tag: tags.last().cloned(),
        ram,
        rebuild,
    };
    let noun = name.rsplit(' ').next().expect("a vm command has a noun");
    crate::vm::run(
        &root,
        noun,
        name,
        one_name(rest, name)?.as_deref(),
        &opts,
        prompt,
    )?;
    Ok(ExitCode::SUCCESS)
}

pub(super) fn media(call: Call) -> Result<ExitCode, Error> {
    let Call {
        name,
        rest,
        here,
        flags,
        ..
    } = call;
    let Flags {
        images,
        target,
        tags,
        ram,
        rebuild,
        ..
    } = flags;
    let root = repo_root(here)?;
    let opts = crate::vm::Options {
        target,
        image: images.last().cloned(),
        tag: tags.last().cloned(),
        ram,
        rebuild,
    };
    one_name(rest, name)?;
    let noun = name.rsplit(' ').next().expect("a media command has a noun");
    crate::media::run(&root, noun, &opts)?;
    Ok(ExitCode::SUCCESS)
}

pub(super) fn registry_namespace(call: Call) -> Result<ExitCode, Error> {
    let Call { here, .. } = call;
    println!("{}", crate::registry::namespace(&repo_root(here)?)?);
    Ok(ExitCode::SUCCESS)
}

pub(super) fn registry_ref(call: Call) -> Result<ExitCode, Error> {
    let Call { here, flags, .. } = call;
    let Flags { target, tags, .. } = flags;
    let root = repo_root(here)?;
    let (list, issues, context) = crate::declarations(&root);
    if issues.report(&context) {
        return Ok(ExitCode::from(REPO_ERROR));
    }
    println!(
        "{}",
        crate::registry::reference(
            &list,
            &root,
            target.as_deref(),
            tags.last().map(String::as_str)
        )?
    );
    Ok(ExitCode::SUCCESS)
}

pub(super) fn recipe(call: Call) -> Result<ExitCode, Error> {
    let Call { here, flags, .. } = call;
    let Flags {
        images,
        target,
        tags,
        ..
    } = flags;
    let root = repo_root(here)?;
    let loaded = crate::load(&root);
    let (list, issues, context) = (loaded.list, loaded.issues, loaded.context);
    if issues.report(&context) {
        return Ok(ExitCode::from(REPO_ERROR));
    }
    let selected = match target.as_deref() {
        Some(name) => list.find_target(name)?,
        None => list
            .ungated_target()
            .ok_or_else(|| "no default image to take a target from".to_string())?,
    }
    .to_string();
    let imgref = crate::registry::reference(
        &list,
        &root,
        Some(&selected),
        tags.last().map(String::as_str),
    )?;
    let image = images.last().map_or(imgref.as_str(), String::as_str);
    let recipe = crate::emit::recipe::build(&list, &selected, image, &imgref, &[])
        .ok_or_else(|| Error::Invocation(crate::emit::recipe::refusal(&list, &selected)))?;
    print!("{}", recipe.render());
    Ok(ExitCode::SUCCESS)
}
