use miette::{Diagnostic, LabeledSpan, NamedSource, SourceSpan};
use std::fmt;

pub struct Issue {
    message: String,
    src: NamedSource<String>,
    labels: Vec<LabeledSpan>,
    help: Option<String>,
}

impl Issue {
    pub fn new(message: impl Into<String>, file: &str, text: &str) -> Self {
        Self {
            message: message.into(),
            src: NamedSource::new(file, text.to_string()).with_language("KDL"),
            labels: Vec::new(),
            help: None,
        }
    }

    pub fn at(mut self, span: SourceSpan, label: impl Into<String>) -> Self {
        self.labels
            .push(LabeledSpan::new_with_span(Some(label.into()), span));
        self
    }

    pub fn help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }
}

impl fmt::Display for Issue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl fmt::Debug for Issue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Issue {}

impl Diagnostic for Issue {
    fn source_code(&self) -> Option<&dyn miette::SourceCode> {
        Some(&self.src)
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = LabeledSpan> + '_>> {
        if self.labels.is_empty() {
            None
        } else {
            Some(Box::new(self.labels.iter().cloned()))
        }
    }

    fn help(&self) -> Option<Box<dyn fmt::Display + '_>> {
        self.help
            .as_ref()
            .map(|h| Box::new(h) as Box<dyn fmt::Display>)
    }
}

#[derive(Default)]
pub struct Issues(Vec<Issue>);

impl Issues {
    pub fn push(&mut self, issue: Issue) {
        self.0.push(issue);
    }

    pub fn plain(&self) -> String {
        let handler = miette::GraphicalReportHandler::new_themed(miette::GraphicalTheme::none())
            .with_width(80);
        let mut out = String::new();
        for issue in &self.0 {
            let _ = handler.render_report(&mut out, issue);
            out.push('\n');
        }
        out
    }

    pub fn report(self, context: &str) -> bool {
        let found = !self.0.is_empty();
        let count = self.0.len();
        for issue in self.0 {
            eprintln!("{:?}", miette::Report::new(issue));
        }
        if found {
            eprintln!(
                "manifest: {count} problem{} in {context}",
                if count == 1 { "" } else { "s" }
            );
        }
        found
    }
}
