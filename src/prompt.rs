use crate::ui::Choice;
use std::io::{IsTerminal, Write};

pub struct Prompt {
    ask: bool,
    draw: bool,
}

impl Prompt {
    pub fn new(no_tui: bool) -> Self {
        let ask = !no_tui && std::io::stdin().is_terminal();
        Self {
            ask,
            draw: ask && std::io::stdout().is_terminal(),
        }
    }

    pub fn silent() -> Self {
        Self {
            ask: false,
            draw: false,
        }
    }

    fn read(&self, question: &str) -> Result<String, String> {
        print!("{question}: ");
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
            Some(default) => self.read(&format!("{question} [{default}]"))?,
            None => self.read(question)?,
        };
        match answer.is_empty() {
            true => missing(),
            false => Ok(answer),
        }
    }

    pub fn confirm(&self, question: &str) -> Result<bool, String> {
        if !self.ask {
            return Ok(false);
        }
        let answer = self.read(&format!("{question} [Y/n]"))?;
        Ok(!answer.starts_with(['n', 'N']))
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
            println!("{question}: {answer}");
            return Ok(chosen);
        }
        for (index, option) in options.iter().enumerate() {
            let line = format!("{}  {}", option.label, option.detail);
            println!("  {}) {}", index + 1, line.trim_end());
        }
        println!("  0) none");
        let answer = self.read(&format!("{question} [0-{}]", options.len()))?;
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
}
