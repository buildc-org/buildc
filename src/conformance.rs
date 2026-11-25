use crate::parse;
use crate::scap::Content;
use common::ui::tree::Change;
use common::ui::Choice;
use std::path::{Path, PathBuf};

pub(crate) fn profile_options(content: &Content) -> Vec<Choice> {
    content
        .profiles
        .iter()
        .map(|profile| Choice::new(profile.name(), &profile.title))
        .collect()
}

pub const CONFORMS_BY_HAND: &str = "nothing can be asked here; write `conforms \"<profile>\"` \
                                    into the image, and `buildc scap content` prints the \
                                    datastream it is then measured with";

pub(crate) fn cost(measured: &str, enforce: bool) -> String {
    match enforce {
        false => format!(
            "A `conforms` turns the scan on: every build measures {measured} against the profile \
             and publishes the score."
        ),
        true => format!(
            "A `conforms` turns the scan on, and this repository enforces: every build measures \
             {measured} against the profile, and a rule it fails fails the build."
        ),
    }
}

pub struct Conforms {
    file: PathBuf,
    profile: String,
    brought: Vec<String>,
}

impl Conforms {
    pub(crate) fn declaring(
        image: &crate::model::image::Image,
        profile: &str,
        brought: Vec<String>,
    ) -> Self {
        Self {
            file: PathBuf::from(image.src.name()),
            profile: profile.to_string(),
            brought,
        }
    }

    pub(crate) fn apply(&self, root: &Path) -> Result<Vec<(PathBuf, Change)>, String> {
        let file = root.join(&self.file);
        let text =
            std::fs::read_to_string(&file).map_err(|err| format!("{}: {err}", file.display()))?;
        let spliced = self.spliced(&text)?;
        std::fs::write(&file, spliced).map_err(|err| format!("{}: {err}", file.display()))?;
        Ok(vec![(
            self.file.clone(),
            Change::Updated(match self.brought.is_empty() {
                true => format!("measured against `{}`", self.profile),
                false => format!(
                    "measured against `{}`, and {} listed",
                    self.profile,
                    crate::said(&self.brought)
                ),
            }),
        )])
    }

    pub(crate) fn spliced(&self, text: &str) -> Result<String, String> {
        parse::declaration::argument(text, "conforms", &self.profile, parse::image::is_header)
            .map_err(|err| format!("{} {err}", self.file.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conforms(profile: &str) -> Conforms {
        Conforms {
            file: PathBuf::from("components/example.kdl"),
            profile: profile.to_string(),
            brought: Vec::new(),
        }
    }

    #[test]
    fn the_profile_replaces_the_one_that_was_there() {
        let text = "schema-version 1\n\nconforms \"standard\"\n\nx {\n}\n";
        assert_eq!(
            conforms("ospp").spliced(text).unwrap(),
            text.replace("\"standard\"", "\"ospp\"")
        );
    }

    #[test]
    fn a_new_profile_keeps_the_allowances_under_the_old_one() {
        let text = "schema-version 1\nconforms \"standard\" {\n    allow-remediation \"r\" because=\"b\"\n}\nx\n";
        assert_eq!(
            conforms("ospp").spliced(text).unwrap(),
            text.replace("\"standard\"", "\"ospp\"")
        );
    }

    #[test]
    fn an_image_with_nothing_to_stand_in_front_of_still_takes_one() {
        assert_eq!(
            conforms("standard").spliced("schema-version 1\n").unwrap(),
            "schema-version 1\nconforms \"standard\"\n"
        );
    }

    #[test]
    fn a_new_profile_stands_in_front_of_the_first_component() {
        assert_eq!(
            conforms("standard")
                .spliced("schema-version 1\n\nx {\n}\n")
                .unwrap(),
            "schema-version 1\n\nconforms \"standard\"\n\nx {\n}\n"
        );
    }
}
