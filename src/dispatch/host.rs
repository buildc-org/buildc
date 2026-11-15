use super::read::{fits, rendered};
use crate::command::Verb;
use crate::error::Error;
use common::prompt::Prompt;
use std::path::Path;
use std::process::ExitCode;

fn this_target(named: Option<&str>, scope: &crate::emit::why::Scope) -> Result<(), Error> {
    use crate::emit::why::Scope;
    let Some(named) = named else {
        return Ok(());
    };
    match scope {
        Scope::Built(name) if name == named => Ok(()),
        Scope::Built(name) => Err(Error::Invocation(format!(
            "this image was built as `{name}`, so it cannot answer for `{named}`"
        ))),
        Scope::Repository | Scope::EveryTarget => Err(Error::Invocation(
            "the build record does not name a target, so this image can answer only for \
             itself"
                .to_string(),
        )),
    }
}

fn unscoped(name: &str) -> String {
    format!(
        "the build record does not name a target and the baked manifest holds more than one, so \
         `{name}` cannot say which of them is running\n\nhelp: `buildc why <component>` still answers, \
         and says that it read across all of them"
    )
}

pub(super) fn on_host(
    verb: Verb,
    name: &str,
    rest: &[&str],
    format: Option<&str>,
    target: Option<&str>,
    prompt: &Prompt,
) -> Result<ExitCode, Error> {
    use crate::emit::why::{built_as, image_of};
    use crate::provenance::build::{MANIFEST, RECORD};

    let unwanted = || Error::Invocation(format!("`{name}` does not take {}", rest.join(" ")));
    if verb == Verb::Plan {
        if !matches!(rest, [] | ["--json"]) {
            return Err(unwanted());
        }
        let raw = std::fs::read_to_string(MANIFEST)
            .map_err(|err| Error::Invocation(format!("{MANIFEST}: {err}")))?;
        print!("{raw}");
        return Ok(ExitCode::SUCCESS);
    }

    let (manifest, record) = crate::emit::why::baked(Path::new(MANIFEST), Path::new(RECORD))
        .map_err(Error::Invocation)?;
    let (targets, scope) = built_as(&manifest, record.as_ref());

    if verb == Verb::Why {
        return why_on_host(&manifest, record.as_ref(), rest, format, prompt);
    }

    this_target(
        match (verb, rest) {
            (Verb::ScapContent, []) => target,
            (Verb::Summary, []) => None,
            (Verb::Summary, [named]) => Some(*named),
            _ => return Err(unwanted()),
        },
        &scope,
    )?;
    let [target] = targets.as_slice() else {
        return Err(Error::Invocation(unscoped(name)));
    };
    if verb == Verb::Summary {
        print!("{}", crate::emit::summary::on_host(target));
        return Ok(ExitCode::SUCCESS);
    }
    if verb == Verb::ScapContent {
        let image = image_of(&manifest, target).ok_or_else(|| {
            Error::Invocation("the manifest names no image for this target".to_string())
        })?;
        return Ok(crate::scap::content_on_host(image, target)?.into());
    }
    unreachable!("{verb:?} is not answered on a host");
}

fn why_on_host(
    manifest: &common::json::Json,
    record: Option<&common::json::Json>,
    rest: &[&str],
    format: Option<&str>,
    prompt: &Prompt,
) -> Result<ExitCode, Error> {
    let json = match format {
        Some("json") => true,
        None | Some("md") => false,
        Some(other) => {
            return Err(Error::Invocation(format!(
                "`--format` is md or json, not `{other}`"
            )))
        }
    };
    let [given] = rest else {
        return Err(Error::Invocation(
            "`why` needs a component: `buildc why <component>`".into(),
        ));
    };

    let known = crate::emit::why::known_on_host(manifest, record);
    let path = match crate::emit::why::matching(&known, given).as_slice() {
        [path] => path.clone(),
        [] => {
            return Err(Error::Invocation(format!(
                "`{given}` is not a component this image carries\n\nmodules: {}",
                crate::emit::why::display(&known).join(", ")
            )))
        }
        paths => {
            return Err(Error::Invocation(format!(
                "`{given}` names more than one component\n\nmodules: {}",
                paths.join(", ")
            )))
        }
    };
    let why = crate::emit::why::on_host(manifest, record, &path)
        .expect("the resolved component is one this image carries");
    let parts = why.parts(true);
    print!(
        "{}",
        match json {
            true => why.json().render(),
            false if prompt.draws() && fits(&parts) => rendered(&parts),
            false => why.markdown(),
        }
    );
    Ok(ExitCode::SUCCESS)
}
