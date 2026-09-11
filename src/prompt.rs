use crate::ui::{Answer, Choice};
use std::cell::Cell;
use std::io::{IsTerminal, Write};

fn stem(question: &str) -> &str {
    question.trim_end_matches(':')
}

const SCRIPT: &str = "BUILDC_ANSWERS";

pub struct Prompt {
    ask: bool,
    draw: bool,
    script: Option<(Vec<String>, Cell<usize>)>,
}

impl Prompt {
    pub fn new(no_tui: bool) -> Self {
        if let Some(path) = std::env::var_os(SCRIPT) {
            let text = std::fs::read_to_string(path).unwrap_or_default();
            return Self::scripted(text.lines().map(str::to_string).collect());
        }
        let ask = !no_tui && std::io::stdin().is_terminal();
        Self {
            ask,
            draw: ask && std::io::stdout().is_terminal(),
            script: None,
        }
    }

    pub fn silent() -> Self {
        Self {
            ask: false,
            draw: false,
            script: None,
        }
    }

    pub fn scripted(answers: Vec<String>) -> Self {
        Self {
            ask: true,
            draw: false,
            script: Some((answers, Cell::new(0))),
        }
    }

    pub fn asks(&self) -> bool {
        self.ask
    }

    pub fn draws(&self) -> bool {
        self.draw
    }

    fn read(&self, question: &str, shown: &str) -> Result<String, String> {
        let answer = self.answer(question, shown)?;
        println!();
        Ok(answer)
    }

    fn answer(&self, question: &str, shown: &str) -> Result<String, String> {
        if let Some((answers, at)) = &self.script {
            let answer = answers
                .get(at.get())
                .ok_or_else(|| format!("nothing left in {SCRIPT} to answer `{question}` with"))?;
            at.set(at.get() + 1);
            println!("{shown}{answer}");
            return Ok(answer.trim().to_string());
        }
        print!("{shown}");
        std::io::stdout().flush().map_err(|err| err.to_string())?;
        let mut answer = String::new();
        std::io::stdin()
            .read_line(&mut answer)
            .map_err(|err| err.to_string())?;
        Ok(answer.trim().to_string())
    }

    pub fn text(
        &self,
        given: Option<String>,
        question: &str,
        flag: &str,
        default: Option<&str>,
    ) -> Result<String, String> {
        self.asked(given, question, flag, None, default)
    }

    fn asked(
        &self,
        given: Option<String>,
        question: &str,
        flag: &str,
        prefix: Option<&str>,
        default: Option<&str>,
    ) -> Result<String, String> {
        if let Some(value) = given.filter(|value| !value.is_empty()) {
            return Ok(value);
        }
        let missing = || match default {
            Some(default) => Ok(default.to_string()),
            None => Err(format!(
                "give {flag}, since nothing can be asked here: {}",
                stem(question)
            )),
        };
        if !self.ask {
            return missing();
        }
        if self.draw {
            return self.written(question, prefix.unwrap_or(""), default, missing);
        }
        let labelled = match default {
            Some(default) => format!("{} [{default}]", stem(question)),
            None => question.to_string(),
        };
        let shown = match prefix {
            None => format!("{}: ", stem(&labelled)),
            Some(prefix) => format!("{labelled}\n{prefix}"),
        };
        match self.read(&labelled, &shown)? {
            answer if answer.is_empty() => missing(),
            answer => Ok(answer),
        }
    }

    fn written(
        &self,
        question: &str,
        prefix: &str,
        default: Option<&str>,
        missing: impl Fn() -> Result<String, String>,
    ) -> Result<String, String> {
        let typed = crate::ui::line(stem(question), prefix, default)?;
        let answer = match typed.is_empty() {
            true => missing()?,
            false => typed,
        };
        println!("{}: {prefix}{answer}\n", stem(question));
        Ok(answer)
    }

    pub fn line(
        &self,
        given: Option<String>,
        question: &str,
        flag: &str,
        prefix: &str,
        default: Option<&str>,
    ) -> Result<String, String> {
        self.asked(given, question, flag, Some(prefix), default)
    }

    pub fn confirm(&self, question: &str, yes: &str, no: &str) -> Result<bool, String> {
        if !self.ask {
            return Ok(false);
        }
        self.confirm_current(question, yes, no, true)
    }

    pub fn confirm_current(
        &self,
        question: &str,
        yes: &str,
        no: &str,
        current: bool,
    ) -> Result<bool, String> {
        if !self.ask {
            return Ok(current);
        }
        if self.draw {
            let chosen = crate::ui::confirm_current(question, yes, no, current)?;
            println!("{}: {}\n", stem(question), if chosen { yes } else { no });
            return Ok(chosen);
        }
        let question = format!("{} ({yes}/{no})", stem(question));
        let answer = self.read(&question, &format!("{question}\n"))?;
        if answer.is_empty() {
            return Ok(current);
        }
        let first = |word: &str| word.chars().next().map(|c| c.to_ascii_lowercase());
        Ok(first(&answer) != first(no))
    }

    pub fn choose(&self, question: &str, options: &[Choice]) -> Result<Option<usize>, String> {
        self.pick_one(question, options, None)
    }

    pub fn choose_current(
        &self,
        question: &str,
        options: &[Choice],
        current: usize,
    ) -> Result<Option<usize>, String> {
        self.pick_one(question, options, Some(current))
    }

    fn pick_one(
        &self,
        question: &str,
        options: &[Choice],
        current: Option<usize>,
    ) -> Result<Option<usize>, String> {
        if !self.ask || options.is_empty() {
            return Ok(current);
        }
        if self.draw {
            let chosen = crate::ui::select_current(question, options, current.unwrap_or(0))?;
            if let Some(index) = chosen {
                println!("{}: {}\n", stem(question), options[index].label);
            }
            return Ok(chosen);
        }
        self.numbered(options, current.as_slice());
        let question = format!("{} [{}, 0 for none]", stem(question), range(options));
        let answer = self.read(&question, &format!("{question}: "))?;
        if let (true, Some(current)) = (answer.is_empty(), current) {
            return Ok(Some(current));
        }
        match answer.parse::<usize>() {
            Ok(0) => Ok(None),
            Ok(number) if number <= options.len() => Ok(Some(number - 1)),
            _ => options
                .iter()
                .position(|option| option.label == answer)
                .map(Some)
                .ok_or_else(|| format!("`{answer}` is not one of them")),
        }
    }

    pub fn choose_many(
        &self,
        question: &str,
        options: &[Choice],
        on: &[usize],
    ) -> Result<Answer, String> {
        if !self.ask || options.is_empty() {
            return Ok(Answer::Chosen(on.to_vec()));
        }
        if self.draw {
            let answer = crate::ui::multi(question, options, on)?;
            println!("{}: {}\n", stem(question), said(&answer, options));
            return Ok(answer);
        }
        self.numbered(options, on);
        let several = match options.len() {
            1 => "",
            _ => ", several",
        };
        let question = format!(
            "{} [{}{several}, 0 for none]",
            stem(question),
            range(options)
        );
        let answer = self.read(&question, &format!("{question}: "))?;
        if answer.is_empty() {
            return Ok(Answer::Chosen(on.to_vec()));
        }

        let mut chosen: Vec<usize> = Vec::new();
        for word in answer.split([' ', ',']).filter(|word| !word.is_empty()) {
            let at = match word.parse::<usize>() {
                Ok(0) => return Ok(Answer::Chosen(Vec::new())),
                Ok(number) if number <= options.len() => number - 1,
                _ => options
                    .iter()
                    .position(|option| option.label == word)
                    .ok_or_else(|| format!("`{word}` is not one of them"))?,
            };
            if !chosen.contains(&at) {
                chosen.push(at);
            }
        }
        Ok(Answer::Chosen(chosen))
    }

    fn numbered(&self, options: &[Choice], on: &[usize]) {
        for (index, option) in options.iter().enumerate() {
            let mark = match (on.is_empty(), on.contains(&index)) {
                (true, _) => "",
                (false, true) => "[x] ",
                (false, false) => "[ ] ",
            };
            let line = format!("{mark}{}  {}", option.label, option.detail);
            println!("  {}) {}", index + 1, line.trim_end());
        }
    }
}

const LISTED: usize = 8;

fn said(answer: &Answer, options: &[Choice]) -> String {
    let Answer::Chosen(chosen) = answer else {
        return "none".to_string();
    };
    match chosen.len() {
        0 => "none".to_string(),
        many if many > LISTED => format!("{many} chosen"),
        _ => chosen
            .iter()
            .map(|at| options[*at].label.as_str())
            .collect::<Vec<_>>()
            .join(", "),
    }
}

fn range(options: &[Choice]) -> String {
    match options.len() {
        1 => "1".to_string(),
        n => format!("1-{n}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(count: usize) -> Vec<Choice> {
        (0..count)
            .map(|at| Choice::new(format!("rule {at}"), ""))
            .collect()
    }

    #[test]
    fn a_short_answer_reads_back_as_itself_and_a_long_one_as_a_count() {
        let options = options(20);
        let short = Answer::Chosen((0..LISTED).collect());
        assert!(said(&short, &options).starts_with("rule 0, rule 1,"));
        let long = Answer::Chosen((0..LISTED + 1).collect());
        assert_eq!(said(&long, &options), format!("{} chosen", LISTED + 1));
        assert_eq!(said(&Answer::Chosen(Vec::new()), &options), "none");
        assert_eq!(said(&Answer::Cancelled, &options), "none");
    }
}
