pub mod table;
pub mod tree;

use crate::copy::{EITHER, LINE_KEYS, NEST, PICK, SECRET_KEYS, TOGGLE};

use ratatui::backend::Backend;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::crossterm::terminal;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, List, ListItem, ListState, Padding, Paragraph};
use ratatui::{DefaultTerminal, Frame, TerminalOptions, Viewport};
use std::io::IsTerminal;
use std::sync::OnceLock;

const NARROWEST: usize = 80;

pub(crate) fn colour() -> bool {
    std::io::stdout().is_terminal()
}

pub(crate) fn width() -> usize {
    if !colour() {
        return NARROWEST;
    }
    std::env::var("COLUMNS")
        .ok()
        .and_then(|cols| parse_width(&cols).map(usize::from))
        .or_else(|| {
            terminal::size()
                .map(|(cols, _)| usize::from(cols))
                .ok()
                .filter(|cols| *cols > 0)
        })
        .unwrap_or(NARROWEST)
}

fn parse_width(width: &str) -> Option<u16> {
    width.parse().ok().filter(|width| *width > 0)
}

pub(crate) fn fits(parts: &[crate::emit::Part]) -> bool {
    let room = width();
    parts.iter().all(|part| match part {
        crate::emit::Part::Table(table) => room >= table::floor(table.header, &table.rows),
        _ => true,
    })
}

pub fn parts(parts: &[crate::emit::Part]) -> String {
    parts
        .iter()
        .map(|part| match part {
            crate::emit::Part::Heading(text) => {
                ratatui::crossterm::style::Stylize::bold(text.as_str()).to_string()
            }
            crate::emit::Part::Text(text) => table::wrap(text, width()).join("\n"),
            crate::emit::Part::Table(table) => {
                table::render(&table.title, table.header, &table.rows)
                    .trim_end()
                    .to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n\n")
        + "\n"
}

pub struct Choice {
    pub label: String,
    pub detail: String,
    pub parent: Option<usize>,
    pub group: String,
    pub available: bool,
    pub dim: bool,
    pub tint: bool,
}

impl Choice {
    pub fn new(label: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            detail: detail.into(),
            parent: None,
            group: String::new(),
            available: true,
            dim: false,
            tint: false,
        }
    }

    pub fn unavailable(mut self) -> Self {
        self.available = false;
        self.dim = true;
        self
    }

    pub fn content(mut self) -> Self {
        self.available = false;
        self
    }

    pub fn tinted(mut self) -> Self {
        self.tint = true;
        self
    }

    pub fn under(mut self, parent: usize) -> Self {
        self.parent = Some(parent);
        self
    }

    pub fn within(mut self, group: impl Into<String>) -> Self {
        self.group = group.into();
        self
    }
}

pub enum Answer {
    Cancelled,
    Chosen(Vec<usize>),
}

const VISIBLE: usize = 8;

pub fn select(question: &str, options: &[Choice]) -> Result<Option<usize>, String> {
    select_current(question, options, 0)
}

pub fn select_current(
    question: &str,
    options: &[Choice],
    at: usize,
) -> Result<Option<usize>, String> {
    inline(height(options.len()), |terminal| {
        pick(terminal, question, options, PICK, at)
    })
}

pub fn confirm(question: &str, yes: &str, no: &str) -> Result<bool, String> {
    confirm_current(question, yes, no, true)
}

pub fn confirm_current(question: &str, yes: &str, no: &str, current: bool) -> Result<bool, String> {
    let options = [Choice::new(yes, ""), Choice::new(no, "")];
    let chosen = inline(height(options.len()), |terminal| {
        pick(terminal, question, &options, EITHER, usize::from(!current))
    })?;
    Ok(chosen == Some(0))
}

pub fn multi(question: &str, options: &[Choice], on: &[usize]) -> Result<Answer, String> {
    if options.iter().any(|choice| !choice.group.is_empty()) {
        let rows = nodes(options).len();
        return inline(height(rows) + 2, |terminal| {
            nest(terminal, question, options, on)
        });
    }
    inline(height(options.len()), |terminal| {
        toggle(terminal, question, options, on)
    })
}

pub enum Field {
    Text { label: String, value: String },
    Secret { label: String, value: String },
    Pick {
        label: String,
        options: Vec<Choice>,
        at: Option<usize>,
    },
    Fixed { label: String, value: String },
}

impl Field {
    pub fn text(label: &str, value: &str) -> Self {
        Self::Text {
            label: label.to_string(),
            value: value.to_string(),
        }
    }

    pub fn secret(label: &str, value: &str) -> Self {
        Self::Secret {
            label: label.to_string(),
            value: value.to_string(),
        }
    }

    pub fn pick(label: &str, options: Vec<Choice>, at: Option<usize>) -> Self {
        Self::Pick {
            label: label.to_string(),
            options,
            at,
        }
    }

    pub fn fixed(label: &str, value: &str) -> Self {
        Self::Fixed {
            label: label.to_string(),
            value: value.to_string(),
        }
    }

    fn answerable(&self) -> bool {
        !matches!(self, Self::Fixed { .. })
    }

    fn label(&self) -> &str {
        match self {
            Self::Text { label, .. }
            | Self::Secret { label, .. }
            | Self::Pick { label, .. }
            | Self::Fixed { label, .. } => label,
        }
    }

    pub fn value(&self) -> String {
        match self {
            Self::Text { value, .. } | Self::Secret { value, .. } | Self::Fixed { value, .. } => {
                value.clone()
            }
            Self::Pick { options, at, .. } => at
                .and_then(|at| options.get(at))
                .map(|choice| choice.label.clone())
                .unwrap_or_default(),
        }
    }

    fn shown(&self) -> String {
        match self {
            Self::Secret { value, .. } if !value.is_empty() => "*".repeat(value.chars().count()),
            _ => match self.value() {
                answer if answer.is_empty() => crate::copy::NOT_SET.to_string(),
                answer => answer,
            },
        }
    }

    fn typing(&self) -> String {
        match self {
            Self::Secret { value, .. } => "*".repeat(value.chars().count()),
            _ => self.value(),
        }
    }

    fn push(&mut self, letter: char) {
        match self {
            Self::Text { value, .. } | Self::Secret { value, .. } => value.push(letter),
            Self::Pick { .. } | Self::Fixed { .. } => {}
        }
    }

    fn pop(&mut self) {
        match self {
            Self::Text { value, .. } | Self::Secret { value, .. } => {
                value.pop();
            }
            Self::Pick { .. } | Self::Fixed { .. } => {}
        }
    }
}

pub enum Filled {
    Took(usize),
    Left,
}

enum Mode {
    Rows,
    Typing,
    Open(usize),
}

pub fn form(
    fields: &mut [Field],
    actions: &[&str],
    blocked: impl Fn(&[Field]) -> Option<String>,
    shown: impl Fn(&[Field]) -> Vec<usize>,
    keys: &str,
) -> Result<Filled, String> {
    let rows = fields.len() + 3;
    inline(rows as u16, |terminal| {
        let mut cursor = 0usize;
        let mut button = 0usize;
        let mut mode = Mode::Rows;
        loop {
            let visible = shown(fields);
            cursor = cursor.min(visible.len());
            let at_row = visible.get(cursor).copied();
            let blocked = blocked(fields);
            let open = match mode {
                Mode::Open(at) => Some(at),
                _ => None,
            };
            let typing = matches!(mode, Mode::Typing);
            let lines = laid_out(
                fields,
                &visible,
                cursor,
                button,
                open,
                typing,
                actions,
                blocked.as_deref(),
            );
            render(terminal, lines.len() as u16 + 1, keys, |frame, area| {
                sheet_of(frame, area, &lines, keys)
            })?;
            let Some(key) = read()? else { continue };
            let Some(row) = at_row else {
                match key {
                    KeyCode::Esc | KeyCode::Char('q') => return Ok(Filled::Left),
                    KeyCode::Up | KeyCode::Char('k') => cursor = cursor.saturating_sub(1),
                    KeyCode::Left => button = button.saturating_sub(1),
                    KeyCode::Right => button = (button + 1).min(actions.len() - 1),
                    KeyCode::Enter if button > 0 || blocked.is_none() => {
                        return Ok(Filled::Took(button))
                    }
                    _ => {}
                }
                continue;
            };
            match &mut mode {
                Mode::Typing => match key {
                    KeyCode::Enter => mode = onward(fields, &visible, &mut cursor),
                    KeyCode::Esc => mode = Mode::Rows,
                    KeyCode::Up => mode = backward(fields, &visible, &mut cursor),
                    KeyCode::Down => mode = onward(fields, &visible, &mut cursor),
                    KeyCode::Backspace => fields[row].pop(),
                    KeyCode::Char(letter) => fields[row].push(letter),
                    _ => {}
                },
                Mode::Open(at) => {
                    let Field::Pick { options, .. } = &fields[row] else {
                        mode = Mode::Rows;
                        continue;
                    };
                    match key {
                        KeyCode::Enter => {
                            let taken = *at;
                            if available(options, taken) {
                                if let Field::Pick { at: held, .. } = &mut fields[row] {
                                    *held = Some(taken);
                                }
                                mode = onward(fields, &shown(fields), &mut cursor);
                            }
                        }
                        KeyCode::Esc | KeyCode::Char('q') => mode = Mode::Rows,
                        KeyCode::Up | KeyCode::Char('k') => *at = at.saturating_sub(1),
                        KeyCode::Down | KeyCode::Char('j') => {
                            *at = (*at + 1).min(options.len().saturating_sub(1))
                        }
                        _ => {}
                    }
                }
                Mode::Rows => match key {
                    KeyCode::Esc | KeyCode::Char('q') => return Ok(Filled::Left),
                    KeyCode::Up | KeyCode::Char('k') => cursor = cursor.saturating_sub(1),
                    KeyCode::Down | KeyCode::Char('j') => cursor = (cursor + 1).min(visible.len()),
                    KeyCode::Enter => mode = opened(&fields[row]),
                    _ => {}
                },
            }
        }
    })
}

fn onward(fields: &[Field], visible: &[usize], cursor: &mut usize) -> Mode {
    loop {
        *cursor = (*cursor + 1).min(visible.len());
        match visible.get(*cursor) {
            Some(row) if !fields[*row].answerable() && *cursor < visible.len() => continue,
            Some(row) => return opened(&fields[*row]),
            None => return Mode::Rows,
        }
    }
}

fn backward(fields: &[Field], visible: &[usize], cursor: &mut usize) -> Mode {
    loop {
        let above = cursor.saturating_sub(1);
        let stuck = above == *cursor;
        *cursor = above;
        match visible.get(*cursor) {
            Some(row) if !fields[*row].answerable() && !stuck => continue,
            Some(row) => return opened(&fields[*row]),
            None => return Mode::Rows,
        }
    }
}

fn opened(field: &Field) -> Mode {
    match field {
        Field::Pick { at, .. } => Mode::Open(at.unwrap_or(0)),
        Field::Fixed { .. } => Mode::Rows,
        _ => Mode::Typing,
    }
}

#[allow(clippy::too_many_arguments)]
fn laid_out(
    fields: &[Field],
    visible: &[usize],
    cursor: usize,
    button: usize,
    open: Option<usize>,
    typing: bool,
    actions: &[&str],
    blocked: Option<&str>,
) -> Vec<Line<'static>> {
    let width = visible
        .iter()
        .map(|row| fields[*row].label().chars().count())
        .max()
        .unwrap_or(0);
    let mut lines: Vec<Line<'static>> = Vec::new();
    for (at, row) in visible.iter().enumerate() {
        let field = &fields[*row];
        let here = at == cursor;
        let unanswered = field.value().is_empty();
        let value = match here && typing {
            true => format!("{}\u{2588}", field.typing()),
            false => field.shown(),
        };
        let label = match (here, field.answerable()) {
            (true, _) => Style::new().fg(HIGHLIGHT).bold(),
            (false, false) => Style::new().dim(),
            (false, true) => Style::new(),
        };
        lines.push(Line::from(vec![
            Span::styled(
                match here {
                    true => "> ",
                    false => "  ",
                },
                label,
            ),
            Span::styled(format!("{:<width$}  ", field.label()), label),
            match unanswered && !typing {
                true => Span::styled(value, Style::new().dim()),
                false => Span::raw(value),
            },
        ]));
        let (Some(open), Field::Pick { options, .. }) = (open.filter(|_| here), field) else {
            continue;
        };
        for (n, choice) in options.iter().enumerate() {
            let on_it = n == open;
            let row = match (choice.available, on_it) {
                (false, _) => Style::new().dim(),
                (true, true) => Style::new().fg(HIGHLIGHT).bold(),
                (true, false) => Style::new(),
            };
            lines.push(Line::from(vec![
                Span::styled(
                    match on_it {
                        true => "    > ",
                        false => "      ",
                    },
                    row,
                ),
                Span::styled(choice.label.clone(), row),
                Span::raw("  "),
                Span::styled(choice.detail.clone(), Style::new().dim()),
            ]));
        }
    }

    lines.push(Line::default());
    let on_actions = cursor == visible.len();
    let mut buttons: Vec<Span<'static>> = vec![Span::raw("  ")];
    for (n, action) in actions.iter().enumerate() {
        let style = match (n == 0 && blocked.is_some(), on_actions && n == button) {
            (true, true) => Style::new().dim().reversed(),
            (true, false) => Style::new().dim(),
            (false, true) => Style::new().bold().reversed(),
            (false, false) => Style::new(),
        };
        buttons.push(Span::styled(format!("  {action}  "), style));
        buttons.push(Span::raw(" "));
    }
    lines.push(Line::from(buttons));
    if let Some(why) = blocked {
        lines.push(Line::from(Span::styled(
            format!("  {why}"),
            Style::new().dim(),
        )));
    }
    lines
}

fn sheet_of(frame: &mut Frame, area: Rect, lines: &[Line<'static>], keys: &str) {
    let keys = hint_row(keys);
    let [body, foot] = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length(u16::from(!keys.is_empty())),
    ])
    .areas(area);
    frame.render_widget(Paragraph::new(lines.to_vec()), body);
    frame.render_widget(Line::from(keys.dim()), foot);
}

pub fn review(
    question: &str,
    rows: &[(String, String)],
    action: &str,
    keys: &str,
    blocked: Option<&str>,
    at: usize,
) -> Result<Option<usize>, String> {
    let options = sheet(rows, action, blocked);
    let chosen = inline((options.len() + 2) as u16, |terminal| {
        pick(
            terminal,
            question,
            &options,
            keys,
            at.min(options.len() - 1),
        )
    })?;
    Ok(chosen.map(|at| at.min(rows.len())))
}

pub fn confirm_over(
    question: &str,
    rows: &[(String, String)],
    yes: &str,
    no: &str,
) -> Result<bool, String> {
    let options = summary_sheet(rows, yes, no);
    let at = options.len() - 2;
    let chosen = inline((options.len() + 2) as u16, |terminal| {
        pick(terminal, question, &options, EITHER, at)
    })?;
    Ok(chosen == Some(at))
}

pub fn offer_over(
    question: &str,
    rows: Vec<Choice>,
    action: &str,
    keys: &str,
) -> Result<bool, String> {
    let mut options = rows;
    options.push(Choice::new("", ""));
    options.push(Choice::new(action, ""));
    let at = options.len() - 1;
    let chosen = inline((options.len() + 2) as u16, |terminal| {
        pick(terminal, question, &options, keys, at)
    })?;
    Ok(chosen == Some(at))
}

fn summary_sheet(rows: &[(String, String)], yes: &str, no: &str) -> Vec<Choice> {
    let width = rows
        .iter()
        .map(|(label, _)| label.chars().count())
        .max()
        .unwrap_or(0);
    let mut options: Vec<Choice> = rows
        .iter()
        .map(|(label, value)| Choice::new(format!("{label:<width$}"), value).unavailable())
        .collect();
    options.push(Choice::new("", ""));
    options.push(Choice::new(yes, ""));
    options.push(Choice::new(no, ""));
    options
}

fn sheet(rows: &[(String, String)], action: &str, blocked: Option<&str>) -> Vec<Choice> {
    let width = rows
        .iter()
        .map(|(label, _)| label.chars().count())
        .max()
        .unwrap_or(0);
    let mut options: Vec<Choice> = rows
        .iter()
        .map(|(label, value)| Choice::new(format!("{label:<width$}"), value))
        .collect();
    options.push(Choice::new("", ""));
    options.push(match blocked {
        None => Choice::new(action, ""),
        Some(why) => Choice::new(action, why).unavailable(),
    });
    options
}

pub fn secret(question: &str) -> Result<String, String> {
    inline(3, |terminal| {
        let mut typed = String::new();
        loop {
            render(terminal, 3, SECRET_KEYS, |frame, area| {
                masked(frame, area, question, typed.chars().count())
            })?;
            let Some(key) = read()? else { continue };
            match key {
                KeyCode::Enter => return Ok(typed),
                KeyCode::Esc => return Ok(String::new()),
                KeyCode::Backspace => {
                    typed.pop();
                }
                KeyCode::Char(letter) => typed.push(letter),
                _ => {}
            }
        }
    })
}

pub fn line(question: &str, prefix: &str, default: Option<&str>) -> Result<String, String> {
    inline(3, |terminal| {
        let mut typed = String::new();
        loop {
            render(terminal, 3, LINE_KEYS, |frame, area| {
                written(frame, area, question, prefix, &typed, default)
            })?;
            let Some(key) = read()? else { continue };
            match key {
                KeyCode::Enter => return Ok(typed),
                KeyCode::Esc => return Ok(String::new()),
                KeyCode::Backspace => {
                    typed.pop();
                }
                KeyCode::Char(letter) => typed.push(letter),
                _ => {}
            }
        }
    })
}

fn written(
    frame: &mut Frame,
    area: Rect,
    question: &str,
    prefix: &str,
    typed: &str,
    default: Option<&str>,
) {
    let [head, body, foot] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(area);
    frame.render_widget(Line::from(question.bold().cyan()), head);
    let answer = match (typed.is_empty(), default) {
        (true, Some(default)) => Span::styled(default, Style::new().dim()),
        _ => Span::raw(typed),
    };
    frame.render_widget(Line::from(vec![Span::raw(prefix), answer]), body);
    frame.render_widget(Line::from(hint_row(LINE_KEYS).dim()), foot);
}

fn masked(frame: &mut Frame, area: Rect, question: &str, typed: usize) {
    let [head, body, foot] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(area);
    frame.render_widget(Line::from(question.bold().cyan()), head);
    frame.render_widget(Line::from("*".repeat(typed)), body);
    frame.render_widget(Line::from(hint_row(SECRET_KEYS).dim()), foot);
}

const NOTED: usize = 5;

const ROWS: u16 = NOTED as u16 + 4;

pub struct Progress {
    terminal: DefaultTerminal,
    pct: u16,
    flight: u16,
    within: u32,
    turn: usize,
    step: String,
    notes: Vec<String>,
    foot: String,
}

impl Progress {
    pub fn open(foot: &str) -> Result<Self, String> {
        let terminal = open(ROWS)?;
        let _ = terminal::disable_raw_mode();
        Ok(Self {
            terminal,
            pct: 0,
            flight: 0,
            within: 0,
            turn: 0,
            step: String::new(),
            notes: Vec::new(),
            foot: foot.to_string(),
        })
    }

    pub fn step(&mut self, pct: u16, flight: u16, name: &str) -> Result<(), String> {
        self.pct = pct.min(100);
        self.flight = flight.min(100);
        self.within = 0;
        self.step = name.to_string();
        self.notes.clear();
        self.show()
    }

    fn at(&self) -> u16 {
        crept(self.pct, self.flight, self.within)
    }

    pub fn tick(&mut self) -> Result<(), String> {
        self.turn = self.turn.wrapping_add(1);
        self.show()
    }

    pub fn note(&mut self, text: &str) -> Result<(), String> {
        self.within += 1;
        if self.notes.len() == NOTED {
            self.notes.remove(0);
        }
        self.notes.push(text.to_string());
        self.show()
    }

    fn show(&mut self) -> Result<(), String> {
        let (pct, turn, step, notes, foot) = (
            self.at(),
            self.turn,
            self.step.as_str(),
            self.notes.as_slice(),
            self.foot.as_str(),
        );
        render(&mut self.terminal, ROWS, foot, |frame, area| {
            working(frame, area, pct, turn, step, notes, foot)
        })
    }

    pub fn close(self) {
        close(self.terminal);
    }
}

fn working(
    frame: &mut Frame,
    area: Rect,
    pct: u16,
    turn: usize,
    step: &str,
    notes: &[String],
    foot: &str,
) {
    let [meter, name, body, tail] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(area);
    frame.render_widget(bar(pct, meter.width), meter);
    frame.render_widget(
        Line::from(vec![
            Span::styled(TURNING[turn % TURNING.len()], Style::new().fg(HIGHLIGHT)),
            Span::raw(" "),
            Span::styled(step.to_string(), Style::new().fg(HIGHLIGHT).bold()),
        ]),
        name,
    );
    let pane = Block::new()
        .borders(Borders::TOP)
        .border_style(Style::new().dim())
        .title(Line::from(" output ".dim()));
    let inside = pane.inner(body);
    frame.render_widget(pane, body);
    frame.render_widget(Paragraph::new(notes.join("\n")).dim(), inside);
    frame.render_widget(Line::from(hint_row(foot).dim()), tail);
}

const COLD: (u8, u8, u8) = (0x5a, 0x56, 0xe0);
const HOT: (u8, u8, u8) = (0xee, 0x6f, 0xf8);

fn crept(pct: u16, flight: u16, within: u32) -> u16 {
    let left = HELD.powi(within.min(400) as i32);
    pct + (f64::from(flight) * (1.0 - left)) as u16
}

const HELD: f64 = 0.97;

fn bar<'a>(pct: u16, width: u16) -> Line<'a> {
    let label = format!(" {pct:>3}%");
    let room = usize::from(width).saturating_sub(label.chars().count());
    let done = room * usize::from(pct.min(100)) / 100;
    let mut spans: Vec<Span> = (0..room)
        .map(|at| match at < done {
            true => Span::styled("\u{2588}", Style::new().fg(blend(at, room))),
            false => Span::styled("\u{2588}", Style::new().fg(TRACK)),
        })
        .collect();
    spans.push(Span::styled(label, Style::new().bold()));
    Line::from(spans)
}

const TRACK: Color = Color::DarkGray;

const TURNING: [&str; 10] = [
    "\u{280b}", "\u{2819}", "\u{2839}", "\u{2838}", "\u{283c}", "\u{2834}", "\u{2826}", "\u{2827}",
    "\u{2807}", "\u{280f}",
];

fn blend(at: usize, room: usize) -> Color {
    let step = |cold: u8, hot: u8| {
        let (cold, hot) = (i32::from(cold), i32::from(hot));
        (cold + (hot - cold) * at as i32 / room.max(1) as i32) as u8
    };
    Color::Rgb(
        step(COLD.0, HOT.0),
        step(COLD.1, HOT.1),
        step(COLD.2, HOT.2),
    )
}

fn lifted(base: (u8, u8, u8)) -> Color {
    let lift = |channel: u8| (u16::from(channel) + (0xff - u16::from(channel)) * 2 / 3) as u8;
    Color::Rgb(lift(base.0), lift(base.1), lift(base.2))
}

fn tinted(label: &str) -> Vec<Span<'static>> {
    label
        .chars()
        .map(|letter| {
            let colour = match letter {
                _ if letter.is_ascii_digit() => lifted(HOT),
                _ if letter.is_alphabetic() => lifted(COLD),
                _ => Color::White,
            };
            Span::styled(letter.to_string(), Style::new().fg(colour))
        })
        .collect()
}

fn height(rows: usize) -> u16 {
    (rows.min(VISIBLE) + 2) as u16
}

fn give_size() {
    if !unsized_tty(terminal::size().ok()) {
        return;
    }
    let size = libc::winsize {
        ws_row: 24,
        ws_col: NARROWEST as u16,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    unsafe { libc::ioctl(libc::STDOUT_FILENO, libc::TIOCSWINSZ, &size) };
}

fn unsized_tty(size: Option<(u16, u16)>) -> bool {
    !matches!(size, Some((cols, rows)) if cols > 0 && rows > 0)
}

static CHROME: OnceLock<String> = OnceLock::new();

pub fn own_screen(title: impl Into<String>) {
    let _ = CHROME.set(title.into());
}

fn open(height: u16) -> Result<DefaultTerminal, String> {
    give_size();
    let viewport = match CHROME.get() {
        Some(_) => Viewport::Fullscreen,
        None => Viewport::Inline(height),
    };
    ratatui::try_init_with_options(TerminalOptions { viewport }).map_err(|err| err.to_string())
}

fn close(mut terminal: DefaultTerminal) {
    let origin = terminal.get_frame().area().as_position();
    let _ = terminal.clear();
    let _ = terminal.set_cursor_position(origin);
    let _ = terminal.show_cursor();
    let _ = terminal::disable_raw_mode();
}

fn inline<T>(
    height: u16,
    body: impl FnOnce(&mut DefaultTerminal) -> Result<T, String>,
) -> Result<T, String> {
    let mut terminal = open(height)?;
    let out = body(&mut terminal);
    close(terminal);
    out
}

fn render<B: Backend>(
    terminal: &mut ratatui::Terminal<B>,
    rows: u16,
    keys: &str,
    body: impl FnOnce(&mut Frame, Rect),
) -> Result<(), String> {
    terminal
        .draw(|frame| {
            let area = chrome(frame, CHROME.get().map(String::as_str), rows, keys);
            body(frame, area);
        })
        .map(|_| ())
        .map_err(|err| err.to_string())
}

const WIDEST: u16 = 76;

const ACCENT: Color = Color::Rgb(COLD.0, COLD.1, COLD.2);
const HIGHLIGHT: Color = Color::Rgb(HOT.0, HOT.1, HOT.2);

const PAD_X: u16 = 3;
const PAD_Y: u16 = 1;

fn chrome(frame: &mut Frame, title: Option<&str>, rows: u16, keys: &str) -> Rect {
    let Some(title) = title else {
        return frame.area();
    };
    let full = frame.area();
    let box_area = centred(
        full,
        WIDEST.min(full.width),
        rows.saturating_add(2 + 2 * PAD_Y),
    );
    let block = Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(ACCENT))
        .padding(Padding::symmetric(PAD_X, PAD_Y))
        .title(Line::from(vec![
            Span::styled("\u{2500} ", Style::new().fg(ACCENT)),
            Span::styled(title.trim().to_string(), Style::new().bold()),
            Span::raw(" "),
        ]))
        .title_bottom(
            Line::from(vec![
                Span::styled("\u{2500} ", Style::new().fg(ACCENT)),
                Span::styled(keys.to_string(), Style::new().fg(TRACK)),
                Span::raw(" "),
            ])
            .left_aligned(),
        );
    let inside = block.inner(box_area);
    frame.render_widget(block, box_area);
    inside
}

fn hint_row(keys: &str) -> &str {
    match CHROME.get() {
        Some(_) => "",
        None => keys,
    }
}

fn centred(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    }
}

fn pick<B: Backend>(
    terminal: &mut ratatui::Terminal<B>,
    question: &str,
    options: &[Choice],
    hint: &str,
    selected: usize,
) -> Result<Option<usize>, String> {
    let mut state = ListState::default().with_selected(Some(selected));
    loop {
        render(terminal, height(options.len()), hint, |frame, area| {
            draw(frame, area, question, options, None, hint, &mut state)
        })?;
        let Some(key) = read()? else { continue };
        match key {
            KeyCode::Enter => match state.selected() {
                Some(at) if !available(options, at) => {}
                chosen => return Ok(chosen),
            },
            KeyCode::Esc | KeyCode::Char('q') => return Ok(None),
            code => {
                move_by(code, &mut state);
                skip_spacers(code, options, &mut state);
            }
        }
    }
}

fn skip_spacers(code: KeyCode, options: &[Choice], state: &mut ListState) {
    for _ in 0..options.len() {
        match state.selected() {
            Some(at) if options.get(at).is_some_and(|row| row.label.is_empty()) => {
                move_by(code, state)
            }
            _ => return,
        }
    }
}

fn toggle<B: Backend>(
    terminal: &mut ratatui::Terminal<B>,
    question: &str,
    options: &[Choice],
    held: &[usize],
) -> Result<Answer, String> {
    let mut state = ListState::default().with_selected(Some(0));
    let mut on: Vec<usize> = held.to_vec();
    loop {
        render(terminal, height(options.len()), TOGGLE, |frame, area| {
            draw(
                frame,
                area,
                question,
                options,
                Some(&on),
                TOGGLE,
                &mut state,
            )
        })?;
        let Some(key) = read()? else { continue };
        match key {
            KeyCode::Enter => return Ok(Answer::Chosen(on)),
            KeyCode::Char(' ') => match state.selected() {
                Some(at) if available(options, at) => flip(&mut on, at, options),
                _ => {}
            },
            KeyCode::Esc | KeyCode::Char('q') => return Ok(Answer::Cancelled),
            code => move_by(code, &mut state),
        }
    }
}

fn available(options: &[Choice], at: usize) -> bool {
    options.get(at).is_some_and(|choice| choice.available)
}

fn flip(on: &mut Vec<usize>, at: usize, options: &[Choice]) {
    if let Some(held) = on.iter().position(|held| *held == at) {
        on.remove(held);
        return;
    }
    let parent = options[at].parent;
    on.retain(|held| Some(*held) != parent && options[*held].parent != Some(at));
    on.push(at);
}

fn branch(options: &[Choice], at: usize) -> &'static str {
    let Some(parent) = options[at].parent else {
        return "";
    };
    match options[at + 1..]
        .iter()
        .any(|choice| choice.parent == Some(parent))
    {
        true => "\u{251c}\u{2500} ",
        false => "\u{2514}\u{2500} ",
    }
}

pub const INTERRUPTED: &str = "interrupted";

fn read() -> Result<Option<KeyCode>, String> {
    let Event::Key(key) = event::read().map_err(|err| err.to_string())? else {
        return Ok(None);
    };
    if key.kind != KeyEventKind::Press {
        return Ok(None);
    }
    match key.code {
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            Err(INTERRUPTED.to_string())
        }
        code => Ok(Some(code)),
    }
}

fn move_by(code: KeyCode, state: &mut ListState) {
    match code {
        KeyCode::Up | KeyCode::Char('k') => state.select_previous(),
        KeyCode::Down | KeyCode::Char('j') => state.select_next(),
        KeyCode::Home => state.select_first(),
        KeyCode::End => state.select_last(),
        _ => {}
    }
}

fn draw(
    frame: &mut Frame,
    area: Rect,
    question: &str,
    options: &[Choice],
    on: Option<&[usize]>,
    hint_text: &str,
    state: &mut ListState,
) {
    let [head, body, foot] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(area);

    frame.render_widget(Line::from(question.bold().cyan()), head);

    let items: Vec<ListItem> = options
        .iter()
        .enumerate()
        .map(|(at, choice)| {
            let mark = match on {
                None => "",
                Some(on) if on.contains(&at) => "[x] ",
                Some(_) => "[ ] ",
            };
            let row = match choice.dim {
                false => Style::new(),
                true => Style::new().dim(),
            };
            let mut spans = vec![
                Span::styled(mark, row),
                Span::styled(branch(options, at), row),
            ];
            match choice.tint {
                true => spans.extend(tinted(&choice.label)),
                false => spans.push(Span::styled(choice.label.clone(), row)),
            }
            spans.push(Span::raw("  "));
            spans.push(Span::styled(choice.detail.clone(), Style::new().dim()));
            ListItem::new(Line::from(spans))
        })
        .collect();
    let rows = items.len();
    frame.render_stateful_widget(List::new(items).highlight_symbol("> "), body, state);
    frame.render_widget(
        Line::from(hint(hint_row(hint_text), state, body.height, rows).dim()),
        foot,
    );
}

fn hint(hint: &str, state: &ListState, height: u16, rows: usize) -> String {
    let seen = (state.offset() + usize::from(height)).min(rows);
    match seen < rows || state.offset() > 0 {
        true => format!("{hint}  {seen} of {rows}"),
        false => hint.to_string(),
    }
}

struct Node {
    label: String,
    depth: usize,
    at: Option<usize>,
}

fn nodes(options: &[Choice]) -> Vec<Node> {
    let mut nodes = Vec::new();
    let mut open: Vec<&str> = Vec::new();
    for (at, choice) in options.iter().enumerate() {
        let path: Vec<&str> = match choice.group.is_empty() {
            true => Vec::new(),
            false => choice.group.split('.').collect(),
        };
        let same = open
            .iter()
            .zip(&path)
            .take_while(|(held, part)| held == part)
            .count();
        open.truncate(same);
        for part in &path[same..] {
            open.push(part);
            nodes.push(Node {
                label: open.join("."),
                depth: open.len() - 1,
                at: None,
            });
        }
        nodes.push(Node {
            label: choice.label.clone(),
            depth: open.len(),
            at: Some(at),
        });
    }
    nodes
}

fn leaves(nodes: &[Node], at: usize) -> Vec<usize> {
    if let Some(option) = nodes[at].at {
        return vec![option];
    }
    let depth = nodes[at].depth;
    nodes[at + 1..]
        .iter()
        .take_while(|node| node.depth > depth)
        .filter_map(|node| node.at)
        .collect()
}

fn check(on: &mut Vec<usize>, nodes: &[Node], at: usize) {
    let leaves = leaves(nodes, at);
    if leaves.iter().all(|leaf| on.contains(leaf)) {
        on.retain(|held| !leaves.contains(held));
        return;
    }
    for leaf in leaves {
        if !on.contains(&leaf) {
            on.push(leaf);
        }
    }
}

fn checkbox(on: &[usize], nodes: &[Node], at: usize) -> &'static str {
    let leaves = leaves(nodes, at);
    match leaves.iter().filter(|leaf| on.contains(leaf)).count() {
        0 => "[ ] ",
        held if held == leaves.len() => "[x] ",
        _ => "[-] ",
    }
}

fn shown(nodes: &[Node], options: &[Choice], open: &[bool], filter: &str) -> Vec<usize> {
    if !filter.is_empty() {
        let filter = filter.to_lowercase();
        let matches = |choice: &Choice| {
            choice.label.to_lowercase().contains(&filter)
                || choice.detail.to_lowercase().contains(&filter)
        };
        return (0..nodes.len())
            .filter(|at| {
                nodes[*at]
                    .at
                    .is_some_and(|option| matches(&options[option]))
            })
            .collect();
    }
    let mut rows = Vec::new();
    let mut hidden: Option<usize> = None;
    for (at, node) in nodes.iter().enumerate() {
        match hidden {
            Some(depth) if node.depth > depth => continue,
            _ => hidden = None,
        }
        rows.push(at);
        if node.at.is_none() && !open[at] {
            hidden = Some(node.depth);
        }
    }
    rows
}

fn nest<B: Backend>(
    terminal: &mut ratatui::Terminal<B>,
    question: &str,
    options: &[Choice],
    held: &[usize],
) -> Result<Answer, String> {
    let tree = nodes(options);
    let mut open = vec![false; tree.len()];
    let mut on = held.to_vec();
    let mut filter = String::new();
    let mut state = ListState::default().with_selected(Some(0));
    loop {
        let rows = shown(&tree, options, &open, &filter);
        render(terminal, height(tree.len()) + 2, NEST, |frame, area| {
            nested(
                frame, area, question, options, &tree, &rows, &open, &on, &filter, &mut state,
            )
        })?;
        let Some(key) = read()? else { continue };
        let row = state.selected().and_then(|at| rows.get(at)).copied();
        match key {
            KeyCode::Enter => {
                on.sort_unstable();
                return Ok(Answer::Chosen(on));
            }
            KeyCode::Esc if !filter.is_empty() => filter.clear(),
            KeyCode::Esc => return Ok(Answer::Cancelled),
            KeyCode::Backspace => {
                filter.pop();
            }
            KeyCode::Char(' ') | KeyCode::Left | KeyCode::Right => {
                if let Some(at) = row {
                    match (tree[at].at.is_some(), key) {
                        (true, KeyCode::Left | KeyCode::Right) => {}
                        (true, _) => check(&mut on, &tree, at),
                        (false, KeyCode::Char(' ')) => check(&mut on, &tree, at),
                        (false, KeyCode::Left) => open[at] = false,
                        (false, _) => open[at] = true,
                    }
                }
            }
            KeyCode::Char(letter) => filter.push(letter),
            code => move_by(code, &mut state),
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn nested(
    frame: &mut Frame,
    area: Rect,
    question: &str,
    options: &[Choice],
    nodes: &[Node],
    rows: &[usize],
    open: &[bool],
    on: &[usize],
    filter: &str,
    state: &mut ListState,
) {
    let [head, body, detail, foot] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(2),
        Constraint::Length(1),
    ])
    .areas(area);

    frame.render_widget(Line::from(question.bold().cyan()), head);
    let under = state
        .selected()
        .and_then(|at| rows.get(at))
        .and_then(|at| nodes[*at].at)
        .map_or("", |at| options[at].detail.as_str());
    frame.render_widget(
        Paragraph::new(table::wrap(under, usize::from(detail.width)).join("\n")).dim(),
        detail,
    );

    let items: Vec<ListItem> = rows
        .iter()
        .map(|at| {
            let node = &nodes[*at];
            let sign = match (node.at.is_some(), open[*at]) {
                (true, _) => "",
                (false, true) => "\u{25be} ",
                (false, false) => "\u{25b8} ",
            };
            ListItem::new(Line::from(vec![
                Span::raw(checkbox(on, nodes, *at)),
                Span::raw("  ".repeat(node.depth)),
                Span::raw(sign),
                Span::raw(&node.label),
            ]))
        })
        .collect();
    let shown = items.len();
    frame.render_stateful_widget(List::new(items).highlight_symbol("> "), body, state);
    frame.render_widget(
        Line::from(vec![
            Span::raw(match filter.is_empty() {
                true => String::new(),
                false => format!("{filter}  "),
            }),
            Span::styled(hint(NEST, state, body.height, shown), Style::new().dim()),
        ]),
        foot,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::style::Modifier;
    use ratatui::Terminal;

    #[test]
    fn a_terminal_that_reports_no_size_is_given_one() {
        assert!(unsized_tty(None));
        assert!(unsized_tty(Some((0, 0))));
        assert!(unsized_tty(Some((80, 0))));
        assert!(!unsized_tty(Some((80, 24))));
    }

    #[test]
    fn the_completion_screen_draws_the_recovery_key() {
        const KEY: &str = "6f1b4c0d2a9e83f57b6c1d40e2578a93bb0e4f21c7d68a5039e1b74c2f8d605a";
        let options: Vec<Choice> = vec![
            Choice::new(crate::copy::WRITE_DOWN, "").content(),
            Choice::new(KEY, "").content().tinted(),
            Choice::new(crate::copy::KEY_NOT_LOGGED, "").content(),
            Choice::new(
                crate::copy::logging(Some(std::path::Path::new("/var/log/bootc-anywhere.log"))),
                "",
            )
            .content(),
            Choice::new("", ""),
            Choice::new(crate::copy::RESTART, ""),
        ];
        let rows = 4;
        for at in 0..rows {
            assert!(!available(&options, at), "row {at} is landable");
            assert!(!options[at].dim, "row {at} is dim");
        }
        assert!(available(&options, options.len() - 1));

        let at = options.len() - 1;
        let mut state = ListState::default().with_selected(Some(at));
        let mut terminal = Terminal::new(TestBackend::new(72, options.len() as u16 + 2)).unwrap();
        terminal
            .draw(|frame| {
                draw(
                    frame,
                    frame.area(),
                    crate::copy::INSTALL_DONE,
                    &options,
                    None,
                    crate::copy::DONE_KEYS,
                    &mut state,
                )
            })
            .unwrap();
        let drawn = terminal.backend().to_string();
        assert!(drawn.contains(KEY), "the key is not on the screen: {drawn}");
        assert!(drawn.contains(crate::copy::KEY_NOT_LOGGED), "{drawn}");
        assert!(drawn.contains("/var/log/bootc-anywhere.log"), "{drawn}");
        assert!(drawn.contains(crate::copy::RESTART), "{drawn}");

        let buffer = terminal.backend().buffer();
        let row = KEY_ROW;
        let lit = (0..buffer.area.width)
            .map(|column| &buffer[(column, row)])
            .filter(|cell| !cell.symbol().trim().is_empty())
            .collect::<Vec<_>>();
        assert_eq!(
            lit.len(),
            KEY.len(),
            "the key's row is not the key: {drawn}"
        );
        for cell in &lit {
            assert!(
                !cell.modifier.contains(Modifier::DIM),
                "the recovery key is drawn dim: {drawn}"
            );
        }

        let tint = |class: fn(&char) -> bool| {
            KEY.chars()
                .zip(&lit)
                .filter(|(letter, _)| class(letter))
                .map(|(_, cell)| cell.fg)
                .collect::<std::collections::HashSet<_>>()
        };
        let digits = tint(|letter| letter.is_ascii_digit());
        let letters = tint(|letter| letter.is_alphabetic());
        assert_eq!(digits.len(), 1, "digits take one tint: {digits:?}");
        assert_eq!(letters.len(), 1, "letters take one tint: {letters:?}");
        assert!(
            digits.is_disjoint(&letters),
            "letters and digits share a tint: {digits:?} {letters:?}"
        );
    }

    const KEY_ROW: u16 = 2;

    #[test]
    fn the_confirmation_shows_the_answers_and_lands_only_on_a_choice() {
        let rows = [
            ("disk".to_string(), "/dev/vda".to_string()),
            ("computer name".to_string(), "deb2".to_string()),
            (
                "password".to_string(),
                crate::copy::PASSWORD_SET.to_string(),
            ),
        ];
        let options = summary_sheet(&rows, crate::copy::CONTINUE, crate::copy::GO_BACK);
        for at in 0..rows.len() {
            assert!(!available(&options, at), "row {at} is landable");
        }
        assert!(available(&options, options.len() - 2));
        assert!(available(&options, options.len() - 1));

        let mut state = ListState::default().with_selected(Some(options.len() - 2));
        let mut terminal = Terminal::new(TestBackend::new(60, options.len() as u16 + 2)).unwrap();
        let question = crate::copy::erasing("/dev/vda");
        terminal
            .draw(|frame| {
                draw(
                    frame,
                    frame.area(),
                    &question,
                    &options,
                    None,
                    EITHER,
                    &mut state,
                )
            })
            .unwrap();
        let drawn = terminal.backend().to_string();
        assert!(drawn.contains("will be erased. Are you sure?"), "{drawn}");
        assert!(
            drawn.contains("/dev/vda") && drawn.contains("deb2"),
            "{drawn}"
        );
        assert!(drawn.contains(crate::copy::CONTINUE), "{drawn}");
        assert!(drawn.contains(crate::copy::GO_BACK), "{drawn}");
    }

    #[test]
    fn a_secret_is_drawn_as_its_length_and_never_as_itself() {
        let mut terminal = Terminal::new(TestBackend::new(60, 3)).unwrap();
        terminal
            .draw(|frame| {
                masked(
                    frame,
                    frame.area(),
                    crate::copy::LUKS_PASSPHRASE,
                    "hunter2".len(),
                )
            })
            .unwrap();
        let drawn = terminal.backend().to_string();
        assert!(drawn.contains(crate::copy::LUKS_PASSPHRASE), "{drawn}");
        assert!(drawn.contains("*******"), "{drawn}");
        assert!(!drawn.contains("hunter2"), "{drawn}");
        assert!(drawn.contains(SECRET_KEYS), "{drawn}");
    }

    fn typed_line(prefix: &str, typed: &str, default: Option<&str>) -> String {
        let mut terminal = Terminal::new(TestBackend::new(60, 3)).unwrap();
        terminal
            .draw(|frame| written(frame, frame.area(), "who owns it", prefix, typed, default))
            .unwrap();
        terminal.backend().to_string()
    }

    #[test]
    fn a_default_is_shown_until_something_is_typed_over_it() {
        let empty = typed_line("github.com/", "", Some("someone"));
        assert!(empty.contains("github.com/someone"), "{empty}");
        assert!(empty.contains(LINE_KEYS), "{empty}");

        let over = typed_line("github.com/", "else", Some("someone"));
        assert!(over.contains("github.com/else"), "{over}");
        assert!(!over.contains("someone"), "{over}");

        let bare = typed_line("", "", None);
        assert!(bare.contains("who owns it"), "{bare}");
        let answer = bare.lines().nth(1).unwrap().replace('"', "");
        assert_eq!(answer.trim(), "", "{bare}");
    }

    #[test]
    fn a_command_that_owns_the_screen_draws_one_box_around_the_form() {
        let fields = [
            Field::pick(
                "disk",
                vec![Choice::new("/dev/vda", "64G  QEMU HARDDISK")],
                None,
            ),
            Field::text("computer name", "deb2"),
            Field::secret("password", ""),
        ];
        let visible: Vec<usize> = (0..fields.len()).collect();
        let shown = laid_out(
            &fields,
            &visible,
            0,
            0,
            None,
            false,
            &[crate::copy::INSTALL, crate::copy::SHUT_DOWN],
            Some("still needs a disk, a password"),
        );
        let mut terminal = Terminal::new(TestBackend::new(64, 16)).unwrap();
        terminal
            .draw(|frame| {
                let area = chrome(
                    frame,
                    Some("Buildc installer"),
                    shown.len() as u16,
                    crate::copy::INSTALL_KEYS,
                );
                sheet_of(frame, area, &shown, "")
            })
            .unwrap();
        let drawn = terminal.backend().to_string();
        let rows: Vec<&str> = drawn.lines().collect();
        assert!(rows[0].replace('"', "").trim().is_empty(), "{drawn}");
        let top = rows
            .iter()
            .position(|row| row.contains("Buildc installer"))
            .unwrap_or_else(|| panic!("{drawn}"));
        assert!(rows[top].contains('\u{256d}'), "{drawn}");
        assert!(rows[top + 2].contains("disk"), "{drawn}");
        assert!(drawn.contains(crate::copy::INSTALL), "{drawn}");
        assert!(drawn.contains(crate::copy::SHUT_DOWN), "{drawn}");
        assert!(drawn.contains("still needs a disk, a password"), "{drawn}");
        assert_eq!(
            drawn.matches(crate::copy::INSTALL_KEYS).count(),
            1,
            "{drawn}"
        );
        let last = rows
            .iter()
            .rposition(|row| row.contains('\u{2570}'))
            .unwrap();
        assert!(rows[last].contains(crate::copy::INSTALL_KEYS), "{drawn}");
    }

    #[test]
    fn the_bar_climbs_through_a_step_and_stops_short_of_the_next() {
        let seen: Vec<u16> = (0..400).map(|within| crept(2, 87, within)).collect();
        assert_eq!(seen[0], 2);
        assert!(
            seen.windows(2).all(|pair| pair[1] >= pair[0]),
            "{:?}",
            &seen[..40]
        );
        assert!(seen[10] > 10, "{}", seen[10]);
        assert!(seen[50] > 45, "{}", seen[50]);
        assert!(seen.iter().all(|at| *at < 89), "{}", seen[399]);
    }

    #[test]
    fn the_progress_region_draws_the_step_the_messages_and_the_line_beneath() {
        let mut terminal = Terminal::new(TestBackend::new(40, 8)).unwrap();
        let notes = ["first".to_string(), "second".to_string()];
        terminal
            .draw(|frame| {
                working(
                    frame,
                    frame.area(),
                    50,
                    0,
                    "7/12 install OS",
                    &notes,
                    "log: /run/bootc-anywhere.log",
                )
            })
            .unwrap();
        let drawn = terminal.backend().to_string();
        assert!(drawn.contains("50%"), "{drawn}");
        assert!(drawn.contains("7/12 install OS"), "{drawn}");
        assert!(drawn.contains(TURNING[0]), "{drawn}");
        assert_eq!(
            TURNING
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len(),
            10
        );
        assert!(
            drawn.contains("first") && drawn.contains("second"),
            "{drawn}"
        );
        assert!(drawn.contains("log: /run/bootc-anywhere.log"), "{drawn}");
    }

    #[test]
    fn a_form_shows_every_field_and_opens_a_list_where_it_stands() {
        let disks = vec![
            Choice::new("/dev/sda", "512G  Samsung SSD"),
            Choice::new("/dev/sdb", "1T  WD  removable"),
        ];
        let fields = [
            Field::pick("disk", disks, None),
            Field::text("computer name", "debian-bootc"),
            Field::text("username", ""),
            Field::secret("password", "hunter2"),
            Field::secret("confirm", "hunter2"),
            Field::pick(
                "encryption",
                vec![Choice::new("none", "not encrypted")],
                Some(0),
            ),
        ];
        let visible: Vec<usize> = (0..fields.len()).collect();
        let shown = laid_out(
            &fields,
            &visible,
            0,
            0,
            Some(1),
            false,
            &["Install", "Shut down"],
            Some("still needs a disk, a username"),
        );
        let mut terminal = Terminal::new(TestBackend::new(64, shown.len() as u16 + 1)).unwrap();
        terminal
            .draw(|frame| sheet_of(frame, frame.area(), &shown, crate::copy::INSTALL_KEYS))
            .unwrap();
        let drawn = terminal.backend().to_string();
        eprintln!("SHOT\n{drawn}SHOT");
        let rows: Vec<&str> = drawn.lines().collect();
        assert!(rows[0].contains("> disk"), "{drawn}");
        assert!(rows[1].contains("/dev/sda"), "{drawn}");
        assert!(rows[2].contains("> /dev/sdb"), "{drawn}");
        assert!(rows[3].contains("computer name"), "{drawn}");
        assert!(
            drawn.contains("*******") && !drawn.contains("hunter2"),
            "{drawn}"
        );
        assert!(drawn.contains("not set"), "{drawn}");
        assert!(
            drawn.contains("Install") && drawn.contains("Shut down"),
            "{drawn}"
        );
        assert!(drawn.contains("still needs a disk, a username"), "{drawn}");
    }

    #[test]
    fn columns_width_must_be_nonzero_and_fit_the_renderer() {
        assert_eq!(parse_width("0"), None);
        assert_eq!(parse_width("65535"), Some(u16::MAX));
        assert_eq!(parse_width("65536"), None);
    }

    fn drawn(options: &[Choice], on: Option<&[usize]>, hint: &str, at: usize) -> String {
        let mut state = ListState::default().with_selected(Some(at));
        let height = options.len() as u16 + 3;
        let mut terminal = Terminal::new(TestBackend::new(60, height)).unwrap();
        terminal
            .draw(|frame| {
                draw(
                    frame,
                    frame.area(),
                    "which module",
                    options,
                    on,
                    hint,
                    &mut state,
                )
            })
            .unwrap();
        terminal.backend().to_string()
    }

    fn tree() -> Vec<Choice> {
        vec![
            Choice::new("linux-desktop", ""),
            Choice::new("dx", "").under(0),
            Choice::new("gaming", "").under(0),
            Choice::new("linux-server", ""),
        ]
    }

    #[test]
    fn every_label_is_drawn_with_its_detail_and_the_selection_marked() {
        let options = [
            Choice::new("gaming", "steam and the rest"),
            Choice::new("starship", "requires shell-config"),
        ];
        let drawn = drawn(&options, None, PICK, 1);
        assert!(drawn.contains("which module"), "{drawn}");
        assert!(drawn.contains("  gaming  steam and the rest"), "{drawn}");
        assert!(
            drawn.contains("> starship  requires shell-config"),
            "{drawn}"
        );
        assert!(drawn.contains(PICK), "{drawn}");
    }

    #[test]
    fn a_toggled_option_is_drawn_held_and_the_rest_are_not() {
        let options = [Choice::new("gaming", ""), Choice::new("starship", "")];
        let drawn = drawn(&options, Some(&[1]), TOGGLE, 0);
        assert!(drawn.contains("> [ ] gaming"), "{drawn}");
        assert!(drawn.contains("  [x] starship"), "{drawn}");
    }

    #[test]
    fn a_question_taking_one_answer_marks_nothing() {
        let options = [Choice::new("Yes", ""), Choice::new("No", "")];
        let drawn = drawn(&options, None, EITHER, 0);
        assert!(drawn.contains("> Yes"), "{drawn}");
        assert!(!drawn.contains('['), "{drawn}");
    }

    #[test]
    fn every_hint_fits_the_narrowest_terminal() {
        let mut state = ListState::default().with_selected(Some(0));
        for hint in [PICK, TOGGLE, EITHER] {
            let mut terminal = Terminal::new(TestBackend::new(60, 4)).unwrap();
            terminal
                .draw(|frame| {
                    draw(
                        frame,
                        frame.area(),
                        "which module",
                        &[Choice::new("gaming", "")],
                        None,
                        hint,
                        &mut state,
                    )
                })
                .unwrap();
            let drawn = terminal.backend().to_string();
            assert!(drawn.contains(hint), "{drawn}");
        }
        let drawn = nest_drawn_at(60, &rules(), &[true; 7], &[], "", 0);
        assert!(drawn.contains(NEST), "{drawn}");
    }

    #[test]
    fn a_list_longer_than_the_window_says_how_far_down_it_is() {
        let short = ListState::default();
        assert_eq!(hint(PICK, &short, 8, 3), PICK);
        assert_eq!(hint(PICK, &short, 8, 21), format!("{PICK}  8 of 21"));
        let scrolled = ListState::default().with_offset(13);
        assert_eq!(hint(PICK, &scrolled, 8, 21), format!("{PICK}  21 of 21"));
    }

    #[test]
    fn a_child_is_drawn_under_its_parent_and_the_last_one_closes_the_branch() {
        let drawn = drawn(&tree(), Some(&[]), TOGGLE, 0);
        assert!(drawn.contains("> [ ] linux-desktop"), "{drawn}");
        assert!(drawn.contains("  [ ] \u{251c}\u{2500} dx"), "{drawn}");
        assert!(drawn.contains("  [ ] \u{2514}\u{2500} gaming"), "{drawn}");
        assert!(drawn.contains("  [ ] linux-server"), "{drawn}");
    }

    #[test]
    fn a_parent_and_a_child_cannot_both_be_on() {
        let options = tree();
        let mut on = vec![0];
        flip(&mut on, 1, &options);
        assert_eq!(on, vec![1]);
        flip(&mut on, 0, &options);
        assert_eq!(on, vec![0]);
    }

    #[test]
    fn two_children_of_one_parent_can() {
        let options = tree();
        let mut on = Vec::new();
        flip(&mut on, 1, &options);
        flip(&mut on, 2, &options);
        flip(&mut on, 3, &options);
        assert_eq!(on, vec![1, 2, 3]);
    }

    #[test]
    fn flipping_a_held_row_turns_it_off_and_touches_nothing_else() {
        let options = tree();
        let mut on = vec![1, 3];
        flip(&mut on, 1, &options);
        assert_eq!(on, vec![3]);
    }

    fn rules() -> Vec<Choice> {
        vec![
            Choice::new("1.1.1 tmp", "a separate partition").within("1.1"),
            Choice::new("1.1.2 nodev", "no device files there").within("1.1"),
            Choice::new("1.2.1 gpgcheck", "signatures are checked").within("1.2"),
            Choice::new("RHEL-09-232010", "numbered by nothing"),
        ]
    }

    fn shape(nodes: &[Node]) -> Vec<(&str, usize, Option<usize>)> {
        nodes
            .iter()
            .map(|node| (node.label.as_str(), node.depth, node.at))
            .collect()
    }

    fn nest_drawn(
        options: &[Choice],
        open: &[bool],
        on: &[usize],
        filter: &str,
        at: usize,
    ) -> String {
        nest_drawn_at(60, options, open, on, filter, at)
    }

    fn nest_drawn_at(
        width: u16,
        options: &[Choice],
        open: &[bool],
        on: &[usize],
        filter: &str,
        at: usize,
    ) -> String {
        let nodes = nodes(options);
        let rows = shown(&nodes, options, open, filter);
        let mut state = ListState::default().with_selected(Some(at));
        let height = rows.len() as u16 + 5;
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| {
                nested(
                    frame,
                    frame.area(),
                    "which rules",
                    options,
                    &nodes,
                    &rows,
                    open,
                    on,
                    filter,
                    &mut state,
                )
            })
            .unwrap();
        terminal.backend().to_string()
    }

    #[test]
    fn a_branch_is_drawn_for_every_dotted_part_and_a_row_with_no_group_stays_at_the_top() {
        assert_eq!(
            shape(&nodes(&rules())),
            vec![
                ("1", 0, None),
                ("1.1", 1, None),
                ("1.1.1 tmp", 2, Some(0)),
                ("1.1.2 nodev", 2, Some(1)),
                ("1.2", 1, None),
                ("1.2.1 gpgcheck", 2, Some(2)),
                ("RHEL-09-232010", 0, Some(3)),
            ]
        );
    }

    #[test]
    fn a_branch_turns_on_everything_under_it_and_off_again() {
        let nodes = nodes(&rules());
        let mut on = Vec::new();
        check(&mut on, &nodes, 0);
        assert_eq!(on, vec![0, 1, 2]);
        check(&mut on, &nodes, 0);
        assert!(on.is_empty());
    }

    #[test]
    fn a_branch_holding_some_of_what_is_on_is_neither_on_nor_off() {
        let nodes = nodes(&rules());
        let mut on = Vec::new();
        check(&mut on, &nodes, 2);
        assert_eq!(checkbox(&on, &nodes, 1), "[-] ");
        check(&mut on, &nodes, 3);
        assert_eq!(checkbox(&on, &nodes, 1), "[x] ");
        assert_eq!(checkbox(&on, &nodes, 4), "[ ] ");
    }

    #[test]
    fn a_closed_branch_hides_what_it_contains_and_an_open_one_shows_it() {
        let options = rules();
        let nodes = nodes(&options);
        let mut open = vec![false; nodes.len()];
        assert_eq!(shown(&nodes, &options, &open, ""), vec![0, 6]);
        open[0] = true;
        assert_eq!(shown(&nodes, &options, &open, ""), vec![0, 1, 4, 6]);
        open[1] = true;
        assert_eq!(shown(&nodes, &options, &open, ""), vec![0, 1, 2, 3, 4, 6]);
    }

    #[test]
    fn a_filtered_answer_names_the_option_chosen_and_not_the_row_it_was_on() {
        let options = rules();
        let nodes = nodes(&options);
        let open = vec![false; nodes.len()];
        let rows = shown(&nodes, &options, &open, "signatures");
        assert_eq!(rows, vec![5]);
        let mut on = Vec::new();
        check(&mut on, &nodes, rows[0]);
        assert_eq!(on, vec![2]);
        assert_eq!(options[on[0]].label, "1.2.1 gpgcheck");
    }

    #[test]
    fn the_detail_of_the_highlighted_row_is_drawn_under_the_tree() {
        let options = rules();
        let open = vec![true; nodes(&options).len()];
        let drawn = nest_drawn(&options, &open, &[1], "", 3);
        assert!(drawn.contains("> [x]     1.1.2 nodev"), "{drawn}");
        assert!(drawn.contains("[-]   \u{25be} 1.1"), "{drawn}");
        assert!(drawn.contains("no device files there"), "{drawn}");
        assert!(drawn.contains(NEST), "{drawn}");
    }
}
