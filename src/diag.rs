use miette::{Diagnostic, LabeledSpan, NamedSource, SourceSpan};
use std::fmt;
use std::sync::Arc;

#[derive(Clone)]
pub struct Source(Arc<NamedSource<String>>);

impl Source {
    pub fn new(file: impl AsRef<str>, text: impl Into<String>) -> Self {
        Self(Arc::new(
            NamedSource::new(file, text.into()).with_language("KDL"),
        ))
    }

    pub fn name(&self) -> &str {
        self.0.name()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Span {
    pub offset: usize,
    pub len: usize,
}

impl From<SourceSpan> for Span {
    fn from(span: SourceSpan) -> Self {
        Self {
            offset: span.offset(),
            len: span.len(),
        }
    }
}

impl From<Span> for SourceSpan {
    fn from(span: Span) -> Self {
        (span.offset, span.len).into()
    }
}

pub struct Issue {
    message: String,
    src: Source,
    labels: Vec<LabeledSpan>,
    help: Option<String>,
}

impl Issue {
    pub fn new(message: impl Into<String>, src: &Source) -> Self {
        Self {
            message: message.into(),
            src: src.clone(),
            labels: Vec::new(),
            help: None,
        }
    }

    pub fn at(mut self, span: impl Into<Span>, label: impl Into<String>) -> Self {
        let span = SourceSpan::from(span.into());
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
        Some(&*self.src.0)
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

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
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

    #[cfg(test)]
    pub fn findings(&self) -> Vec<String> {
        self.plain()
            .lines()
            .filter_map(|line| line.strip_prefix("  x "))
            .map(str::to_string)
            .collect()
    }

    pub fn report(self, context: &str) -> bool {
        let found = !self.0.is_empty();
        let count = self.0.len();
        let mut bad: Vec<&str> = Vec::new();
        for issue in &self.0 {
            if !bad.contains(&issue.src.name()) {
                bad.push(issue.src.name());
            }
        }
        let clean: Vec<&str> = match context.contains(", ") {
            false => Vec::new(),
            true => context
                .split(", ")
                .filter(|file| !bad.contains(file))
                .collect(),
        };
        let (bad, clean) = (bad.join(", "), clean.join(", "));
        for issue in self.0 {
            eprintln!("{:?}", miette::Report::new(issue));
        }
        if found {
            eprintln!(
                "buildc: {count} problem{} in {bad}{}",
                if count == 1 { "" } else { "s" },
                match clean.is_empty() {
                    true => String::new(),
                    false => format!("; none in {clean}"),
                }
            );
        }
        found
    }
}
