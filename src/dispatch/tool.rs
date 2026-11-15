use super::{Call, Flags};
use crate::error::Error;
use std::process::ExitCode;

pub(super) fn upgrade(call: Call) -> Result<ExitCode, Error> {
    let Call {
        name, rest, flags, ..
    } = call;
    let Flags { .. } = flags;
    if let [word, ..] = rest {
        return Err(Error::Invocation(format!("`{name}` does not take {word}")));
    }
    crate::upgrade::run()?;
    Ok(ExitCode::SUCCESS)
}

pub(super) fn build_record(call: Call) -> Result<ExitCode, Error> {
    let Call { .. } = call;
    crate::provenance::build::write()?;
    Ok(ExitCode::SUCCESS)
}

pub(super) fn validate_image(call: Call) -> Result<ExitCode, Error> {
    let Call { .. } = call;
    crate::runtime::validate_image()?;
    Ok(ExitCode::SUCCESS)
}

pub(super) fn fetch(call: Call) -> Result<ExitCode, Error> {
    let Call { rest, .. } = call;
    crate::runtime::fetch(rest)?;
    Ok(ExitCode::SUCCESS)
}
