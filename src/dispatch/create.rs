use super::{one_name, open, Call, Flags, REPO_ERROR};
use crate::error::Error;
use std::path::PathBuf;
use std::process::ExitCode;

pub(super) fn repo(call: Call) -> Result<ExitCode, Error> {
    let Call {
        name,
        rest,
        prompt,
        flags,
        ..
    } = call;
    let Flags {
        root: root_arg,
        owner,
        host,
        images,
        base,
        ..
    } = flags;
    let name = one_name(rest, name)?;
    let image = images.last().cloned();
    if let Some(repo) =
        crate::create::Repo::collect(name, host, owner, image, base, root_arg, prompt)?
    {
        repo.apply()?;
    }
    Ok(ExitCode::SUCCESS)
}

pub(super) fn git(call: Call) -> Result<ExitCode, Error> {
    let Call { flags, .. } = call;
    let Flags { root: root_arg, .. } = flags;
    let root =
        root_arg.unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    crate::create::git_control(&root)?;
    Ok(ExitCode::SUCCESS)
}

pub(super) fn image(call: Call) -> Result<ExitCode, Error> {
    let Call {
        name,
        rest,
        prompt,
        here,
        flags,
        ..
    } = call;
    let Flags { owner, base, .. } = flags;
    let name = one_name(rest, name)?;
    let Some(root) = open(here)? else {
        return Ok(ExitCode::from(REPO_ERROR));
    };
    let repo = crate::layout::named_after_root(&root).unwrap_or_default();
    let url = owner.map(|owner| {
        format!(
            "{}/{repo}",
            crate::forge::origin(crate::forge::HOST, &owner)
        )
    });
    let wrote = crate::create::Image::collect(
        crate::create::Home {
            root: &root,
            repo: &repo,
            url,
            sources_text: "",
        },
        name,
        base,
        "a name argument",
        crate::setup::Field::Image,
        None,
        prompt,
    )?
    .apply(&root)?;
    crate::report::report(&root, &wrote);
    Ok(ExitCode::SUCCESS)
}

pub(super) fn flavour(call: Call) -> Result<ExitCode, Error> {
    let Call {
        name,
        rest,
        prompt,
        here,
        flags,
        ..
    } = call;
    let Flags { images, .. } = flags;
    let name = one_name(rest, name)?;
    let Some(root) = open(here)? else {
        return Ok(ExitCode::from(REPO_ERROR));
    };
    let wrote = crate::create::Flavour::collect(&root, name, images, prompt)?.apply(&root)?;
    crate::report::report(&root, &wrote);
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
    let Flags {
        images, with, pkgs, ..
    } = flags;
    let name = one_name(rest, name)?;
    let Some(root) = open(here)? else {
        return Ok(ExitCode::from(REPO_ERROR));
    };
    let wrote =
        crate::create::Component::collect(&root, name, pkgs, with, images, prompt)?.apply(&root)?;
    crate::report::report(&root, &wrote);
    Ok(ExitCode::SUCCESS)
}

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
        cn,
        ..
    } = flags;
    let kind = one_name(rest, name)?;
    let Some(root) = open(here)? else {
        return Ok(ExitCode::from(REPO_ERROR));
    };
    crate::key::Key::collect(&root, kind, component_arg, cn, prompt)?.apply(&root)?;
    Ok(ExitCode::SUCCESS)
}

pub(super) fn scripts(call: Call) -> Result<ExitCode, Error> {
    let Call {
        rest, prompt, here, ..
    } = call;
    let Some(root) = open(here)? else {
        return Ok(ExitCode::from(REPO_ERROR));
    };
    let scripts = crate::create::Scripts::collect(&root, rest, &[], prompt)?;
    let wrote = scripts.apply(&root)?;
    crate::report::report(&root, &wrote);
    if !wrote.is_empty() {
        println!("{}", crate::copy::SCRIPTS_NEXT);
    }
    Ok(ExitCode::SUCCESS)
}
