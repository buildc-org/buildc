use crate::ui::Choice;
use std::cell::Cell;
use std::io::{IsTerminal, Write};

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
        if let Some(value) = given.filter(|value| !value.is_empty()) {
            return Ok(value);
        }
        let missing = || match default {
            Some(default) => Ok(default.to_string()),
            None => Err(format!(
                "give {flag}, since nothing can be asked here: {question}"
            )),
        };
        if !self.ask {
            return missing();
        }
        let answer = match default {
            Some(default) => {
                let question = format!("{question} [{default}]");
                self.read(&question, &format!("{question}: "))?
            }
            None => self.read(question, &format!("{question}: "))?,
        };
        match answer.is_empty() {
            true => missing(),
            false => Ok(answer),
        }
    }

    pub fn line(
        &self,
        given: Option<String>,
        question: &str,
        flag: &str,
        prefix: &str,
        default: Option<&str>,
    ) -> Result<String, String> {
        if let Some(value) = given.filter(|value| !value.is_empty()) {
            return Ok(value);
        }
        let missing = || match default {
            Some(default) => Ok(default.to_string()),
            None => Err(format!(
                "give {flag}, since nothing can be asked here: {question}"
            )),
        };
        if !self.ask {
            return missing();
        }
        let question = match default {
            Some(default) => format!("{question} [{default}]"),
            None => question.to_string(),
        };
        match self.read(&question, &format!("{question}\n{prefix}"))? {
            answer if answer.is_empty() => missing(),
            answer => Ok(answer),
        }
    }

    pub fn confirm(&self, question: &str, yes: &str, no: &str) -> Result<bool, String> {
        if !self.ask {
            return Ok(false);
        }
        if self.draw {
            let chosen = crate::ui::confirm(question, yes, no)?;
            println!("{question}: {}\n", if chosen { yes } else { no });
            return Ok(chosen);
        }
        let question = format!("{question} ({yes}/{no})");
        let answer = self.read(&question, &format!("{question}\n"))?;
        let first = |word: &str| word.chars().next().map(|c| c.to_ascii_lowercase());
        Ok(first(&answer) != first(no))
    }

    pub fn choose(&self, question: &str, options: &[Choice]) -> Result<Option<usize>, String> {
        if !self.ask || options.is_empty() {
            return Ok(None);
        }
        if self.draw {
            let chosen = crate::ui::select(question, options)?;
            let answer = match chosen {
                Some(index) => options[index].label.as_str(),
                None => "none",
            };
            println!("{question}: {answer}\n");
            return Ok(chosen);
        }
        self.numbered(options);
        let question = format!("{question} [{}, 0 for none]", range(options));
        let answer = self.read(&question, &format!("{question}: "))?;
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

    pub fn choose_many(&self, question: &str, options: &[Choice]) -> Result<Vec<usize>, String> {
        if !self.ask || options.is_empty() {
            return Ok(Vec::new());
        }
        if self.draw {
            let chosen = match crate::ui::multi(question, options)? {
                crate::ui::Answer::Cancelled => Vec::new(),
                crate::ui::Answer::Chosen(chosen) => chosen,
            };
            let answer = match chosen.is_empty() {
                true => "none".to_string(),
                false => chosen
                    .iter()
                    .map(|at| options[*at].label.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
            };
            println!("{question}: {answer}\n");
            return Ok(chosen);
        }
        self.numbered(options);
        let several = match options.len() {
            1 => "",
            _ => ", several",
        };
        let question = format!("{question} [{}{several}, 0 for none]", range(options));
        let answer = self.read(&question, &format!("{question}: "))?;

        let mut chosen: Vec<usize> = Vec::new();
        for word in answer.split([' ', ',']).filter(|word| !word.is_empty()) {
            let at = match word.parse::<usize>() {
                Ok(0) => return Ok(Vec::new()),
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
        Ok(chosen)
    }

    fn numbered(&self, options: &[Choice]) {
        for (index, option) in options.iter().enumerate() {
            let line = format!("{}  {}", option.label, option.detail);
            println!("  {}) {}", index + 1, line.trim_end());
        }
    }
}

fn range(options: &[Choice]) -> String {
    match options.len() {
        1 => "1".to_string(),
        n => format!("1-{n}"),
    }
}
