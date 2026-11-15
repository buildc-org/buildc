use super::Call;
use crate::error::Error;
use std::process::ExitCode;

pub(super) fn upgrade(call: Call) -> Result<ExitCode, Error> {
    let Call { name, rest, .. } = call;
    if let [word, ..] = rest {
        return Err(Error::Invocation(format!("`{name}` does not take {word}")));
    }
    crate::upgrade::run()?;
    Ok(ExitCode::SUCCESS)
}

pub(super) fn build_record(_: Call) -> Result<ExitCode, Error> {
    crate::provenance::build::write()?;
    Ok(ExitCode::SUCCESS)
}

pub(super) fn validate_image(_: Call) -> Result<ExitCode, Error> {
    crate::runtime::validate_image()?;
    Ok(ExitCode::SUCCESS)
}

pub(super) fn fetch(call: Call) -> Result<ExitCode, Error> {
    let Call { rest, .. } = call;
    crate::runtime::fetch(rest)?;
    Ok(ExitCode::SUCCESS)
}
