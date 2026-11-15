use super::{collection_repo, one_name, repo_root, Call, Flags};
use crate::error::Error;
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
    let root = repo_root(here)?;
    let list = crate::model::image::List::editable(&root)
        .map_err(|issues| Error::Operation(issues.plain()))?;
    let wrote = crate::fetch::components(&root, &list.sources, list.audit_enforce, rest)?;
    let tracks = crate::provenance::lock::read(&root)
        .iter()
        .any(|record| record.tracked);
    match (wrote.is_empty(), tracks) {
        (false, _) => crate::report::report(&root, &wrote),
        (true, true) => eprintln!("buildc: every tracked component matches its library"),
        (true, false) => eprintln!("buildc: no component tracks its library"),
    }
    Ok(ExitCode::SUCCESS)
}
