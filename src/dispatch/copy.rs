use super::{collection_repo, Call};
use crate::error::Error;
use std::process::ExitCode;

pub(super) fn copy_component(call: Call) -> Result<ExitCode, Error> {
    let Call {
        rest,
        prompt,
        here,
        flags,
        ..
    } = call;
    let (root, _) = collection_repo(here)?;
    let request = crate::copy_component::Request {
        paths: rest.iter().map(ToString::to_string).collect(),
        from: flags.from,
        build: flags.builds.last().cloned(),
    };
    let mut say = |text: &str| print!("{text}");
    let Some(copy) = crate::copy_component::Copy::collect(&request, &root, prompt, &mut say)?
    else {
        return Ok(ExitCode::SUCCESS);
    };
    let wrote = copy.apply(&root)?;
    crate::report::report(&root, &wrote);
    Ok(ExitCode::SUCCESS)
}
