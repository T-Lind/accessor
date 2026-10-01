use anyhow::Result;
use crossterm::{
    event::{
        self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event, KeyCode, KeyEventKind, KeyModifiers, MouseEventKind,
    },
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap},
    Terminal,
};
use std::{
    collections::VecDeque,
    io::{self, IsTerminal},
    time::Duration,
};

pub fn safe(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect()
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    System,
    User,
    Agent,
    Tool,
    Progress,
    Notice,
}
impl Kind {
    fn label(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::User => "user",
            Self::Agent => "agent",
            Self::Tool => "tool",
            Self::Progress => "progress",
            Self::Notice => "notice",
        }
    }
}

struct Entry {
    kind: Kind,
    text: String,
}

pub struct Ui {
    terminal: Option<Terminal<CrosstermBackend<io::Stdout>>>,
    lines: VecDeque<Entry>,
    input: String,
    status: String,
    scroll: u16,
    dirty: bool,
    secret: bool,
    menu_index: usize,
    settings: Option<String>,
    notice: String,
    chat: String,
    history: InputHistory,
}
impl Ui {
    pub fn new(plain: bool) -> Result<Self> {
        let terminal = if !plain && io::stdout().is_terminal() && io::stdin().is_terminal() {
            terminal::enable_raw_mode()?;
            if let Err(e) = execute!(
                io::stdout(),
                EnterAlternateScreen,
                EnableMouseCapture,
                EnableBracketedPaste
            ) {
                let _ = terminal::disable_raw_mode();
                return Err(e.into());
            }
            match Terminal::new(CrosstermBackend::new(io::stdout())) {
                Ok(t) => Some(t),
                Err(e) => {
                    let _ = execute!(io::stdout(), DisableMouseCapture, LeaveAlternateScreen);
                    let _ = terminal::disable_raw_mode();
                    return Err(e.into());
                }
            }
        } else {
            None
        };
        Ok(Self {
            terminal,
            lines: VecDeque::new(),
            input: String::new(),
            status: String::new(),
            scroll: 0,
            dirty: true,
            secret: false,
            menu_index: 0,
            settings: None,
            notice: String::new(),
            chat: "activity".into(),
            history: InputHistory::default(),
        })
    }
    pub fn set_chat(&mut self, chat: &str) {
        self.chat = chat.to_owned();
        self.dirty = true;
    }
    pub fn interactive(&self) -> bool {
        self.terminal.is_some()
    }
    pub fn secret(&mut self, enabled: bool) {
        self.secret = enabled;
        self.history.position = None;
        self.history.draft.clear();
        self.input.clear();
        if self.interactive() {
            if enabled {
                let _ = execute!(io::stdout(), DisableMouseCapture);
            } else {
                let _ = execute!(io::stdout(), EnableMouseCapture);
            }
        }
        self.dirty = true;
    }
    pub fn clear_private(&mut self) {
        self.lines.clear();
        self.history = InputHistory::default();
        self.input.clear();
        self.settings = None;
        self.notice.clear();
        self.scroll = 0;
        self.dirty = true;
    }
    pub fn suspend(&mut self) -> Result<()> {
        if self.interactive() {
            terminal::disable_raw_mode()?;
            execute!(
                io::stdout(),
                DisableBracketedPaste,
                DisableMouseCapture,
                LeaveAlternateScreen
            )?;
        }
        Ok(())
    }
    pub fn resume(&mut self) -> Result<()> {
        if let Some(t) = &mut self.terminal {
            terminal::enable_raw_mode()?;
            execute!(
                io::stdout(),
                EnterAlternateScreen,
                EnableMouseCapture,
                EnableBracketedPaste
            )?;
            t.clear()?;
        }
        self.dirty = true;
        Ok(())
    }
    pub fn ignored(&mut self, text: &str, reason: &str, note: Option<String>) {
        crate::usage::record_diagnostic("Voice ignored");
        let note = note.map(|note| format!(" · {note}")).unwrap_or_default();
        self.message(format!(
            "Ignored ({reason}{note}): {}",
            if text.trim().is_empty() {
                "[no words recognized]"
            } else {
                text
            }
        ));
    }
    pub fn message(&mut self, text: impl AsRef<str>) {
        self.push(Kind::System, text.as_ref());
    }
    pub fn chat(&mut self, kind: Kind, text: impl AsRef<str>) {
        self.push(kind, text.as_ref());
    }
    fn visible(&self, kind: Kind) -> bool {
        match self.chat.as_str() {
            "off" => matches!(kind, Kind::System | Kind::Tool | Kind::Notice),
            "transcript" => true,
            _ => kind != Kind::Progress,
        }
    }
    fn push(&mut self, kind: Kind, text: &str) {
        let text = safe(text);
        // The master log mirrors everything the UI shows, plus filtered lines,
        // so one file holds the whole session when logging is enabled.
        if crate::masterlog::enabled() {
            crate::masterlog::event("ui", &format!("{}: {text}", kind.label()));
        }
        if !self.visible(kind) {
            return;
        }
        if self.settings.is_some() {
            self.notice = text.lines().take(2).collect::<Vec<_>>().join(" ");
        }
        if self.terminal.is_none() {
            println!("{}", plain_prefix(kind, &text));
            return;
        }
        self.lines.push_back(Entry { kind, text });
        while self.lines.len() > 600 {
            self.lines.pop_front();
        }
        self.scroll = 0;
        self.dirty = true;
    }
    pub fn settings(&mut self, text: Option<String>) {
        if self.terminal.is_none() {
            if let Some(text) = &text {
                println!("{}", safe(text));
            }
        }
        self.settings = text.map(|t| safe(&t));
        self.notice.clear();
        self.scroll = 0;
        self.dirty = true;
    }
    pub fn status(&mut self, line: &str) {
        if line != self.status {
            if self.terminal.is_none() {
                println!("[{line}]");
            }
            self.status = line.to_owned();
            self.dirty = true;
        }
    }
    pub fn input(&mut self) -> Result<Option<String>> {
        if !self.interactive() {
            return Ok(None);
        }
        for _ in 0..1024 {
            if !event::poll(Duration::ZERO)? {
                break;
            }
            match event::read()? {
                Event::Paste(text) => {
                    self.history.position = None;
                    apply_paste(&mut self.input, &text);
                    self.menu_index = 0;
                    self.dirty = true;
                }
                Event::Key(k) if k.kind != KeyEventKind::Release => {
                    self.dirty = true;
                    match k.code {
                        KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                            return Ok(Some("/quit".into()))
                        }
                        KeyCode::Char('v')
                            if k.modifiers
                                .intersects(KeyModifiers::CONTROL | KeyModifiers::SUPER) =>
                        {
                            if let Some(text) = clipboard_text() {
                                self.history.position = None;
                                apply_paste(&mut self.input, &text);
                                self.menu_index = 0;
                            }
                        }
                        KeyCode::Insert if k.modifiers.contains(KeyModifiers::SHIFT) => {
                            if let Some(text) = clipboard_text() {
                                self.history.position = None;
                                apply_paste(&mut self.input, &text);
                                self.menu_index = 0;
                            }
                        }
                        KeyCode::Char(_) if k.modifiers.contains(KeyModifiers::CONTROL) => {}
                        KeyCode::Enter => {
                            if self.settings.is_some() && self.input.is_empty() && !self.secret {
                                return Ok(Some("/settings-nav enter".into()));
                            }
                            if !self.secret {
                                if let Some((name, _)) =
                                    crate::dashboard::matching(&self.input).get(self.menu_index)
                                {
                                    self.input = (*name).into();
                                }
                            }
                            self.menu_index = 0;
                            let text = std::mem::take(&mut self.input);
                            if !self.secret && self.settings.is_none() {
                                self.history.remember(&text);
                            }
                            return Ok(Some(text));
                        }
                        KeyCode::Tab if !self.secret => {
                            if let Some((name, _)) =
                                crate::dashboard::matching(&self.input).get(self.menu_index)
                            {
                                self.input = format!("{name} ");
                            }
                            self.menu_index = 0;
                        }
                        KeyCode::Down
                            if self.settings.is_some() && self.input.is_empty() && !self.secret =>
                        {
                            return Ok(Some("/settings-nav down".into()));
                        }
                        KeyCode::Up
                            if self.settings.is_some() && self.input.is_empty() && !self.secret =>
                        {
                            return Ok(Some("/settings-nav up".into()));
                        }
                        KeyCode::Left
                            if self.settings.is_some() && self.input.is_empty() && !self.secret =>
                        {
                            return Ok(Some("/settings-nav back".into()));
                        }
                        KeyCode::Down
                            if !self.secret
                                && self.history.position.is_none()
                                && !crate::dashboard::matching(&self.input).is_empty() =>
                        {
                            let n = crate::dashboard::matching(&self.input).len();
                            if n > 0 {
                                self.menu_index = (self.menu_index + 1) % n;
                            }
                        }
                        KeyCode::Up
                            if !self.secret
                                && self.history.position.is_none()
                                && !crate::dashboard::matching(&self.input).is_empty() =>
                        {
                            self.menu_index = self.menu_index.saturating_sub(1);
                        }
                        KeyCode::Up if !self.secret && self.settings.is_none() => {
                            self.history.navigate(&mut self.input, true);
                            self.menu_index = 0;
                        }
                        KeyCode::Down if !self.secret && self.settings.is_none() => {
                            self.history.navigate(&mut self.input, false);
                            self.menu_index = 0;
                        }
                        KeyCode::Backspace => {
                            self.history.position = None;
                            self.input.pop();
                            self.menu_index = 0;
                        }
                        KeyCode::Esc => {
                            self.input.clear();
                            self.history.position = None;
                            if self.settings.is_some() && !self.secret {
                                return Ok(Some("/settings-nav back".into()));
                            }
                            return Ok(Some("/cancel".into()));
                        }
                        KeyCode::PageUp => self.scroll_lines(8),
                        KeyCode::PageDown => self.scroll_lines(-8),
                        KeyCode::Char(c) if !c.is_control() && self.input.len() < 4096 => {
                            self.history.position = None;
                            self.input.push(c);
                            self.menu_index = 0;
                        }
                        _ => {}
                    }
                }
                Event::Mouse(m) => {
                    self.dirty = true;
                    match m.kind {
                        MouseEventKind::ScrollUp => self.scroll_lines(3),
                        MouseEventKind::ScrollDown => self.scroll_lines(-3),
                        _ => {}
                    }
                }
                Event::Resize(_, _) => self.dirty = true,
                _ => {}
            }
        }
        Ok(None)
    }
    fn scroll_lines(&mut self, delta: i32) {
        if delta > 0 {
            let step = delta as u16;
            self.scroll = if self.settings.is_some() {
                self.scroll.saturating_sub(step)
            } else {
                self.scroll.saturating_add(step)
            };
        } else {
            let step = delta.unsigned_abs() as u16;
            self.scroll = if self.settings.is_some() {
                self.scroll.saturating_add(step)
            } else {
                self.scroll.saturating_sub(step)
            };
        }
        self.dirty = true;
    }
    pub fn draw(&mut self) -> Result<()> {
        if !self.dirty {
            return Ok(());
        }
        self.dirty = false;
        let Some(terminal) = &mut self.terminal else {
            return Ok(());
        };
        terminal.draw(|f| {
            let layout = Layout::vertical([
                Constraint::Length(4),
                Constraint::Min(3),
                Constraint::Length(3),
                Constraint::Length(1),
            ])
            .split(f.area());
            let color = status_color(&self.status);
            let border = Style::default().fg(color);
            f.render_widget(
                Paragraph::new(status_lines(&self.status))
                    .style(Style::default().fg(color))
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .border_type(BorderType::Rounded)
                            .border_style(border)
                            .title(format!(
                                " ACCESSOR {} · voice & agents ",
                                env!("CARGO_PKG_VERSION")
                            )),
                    ),
                layout[0],
            );
            let width = layout[1].width.saturating_sub(2).max(1) as usize;
            let mut visual: Vec<Line> = Vec::new();
            if self.settings.is_some() {
                let panel = self.settings.clone().unwrap_or_default();
                for line in panel.lines() {
                    if let Some(rest) = line.strip_prefix("hint:") {
                        visual.push(Line::from(Span::styled(
                            rest.to_string(),
                            Style::default().fg(Color::Cyan),
                        )));
                    } else if line.starts_with("› ") {
                        visual.push(Line::from(Span::styled(line.to_string(),Style::default().fg(Color::White).bg(Color::Rgb(35,62,79)).add_modifier(Modifier::BOLD))));
                    } else {
                        visual.push(Line::from(line.to_string()));
                    }
                }
                if !self.notice.is_empty() {
                    visual.push(Line::default());
                    visual.push(Line::from(Span::styled(
                        self.notice.clone(),
                        Style::default().fg(Color::Yellow),
                    )));
                }
                visual = crate::markdown::wrap(visual, width);
            } else {
                for entry in &self.lines {
                    visual.extend(styled_entry(entry, width));
                }
            }
            let available = layout[1].height.saturating_sub(2) as usize;
            let (start, end) = if self.settings.is_some() {
                let selected=visual.iter().position(|l|l.spans.first().is_some_and(|s|s.content.starts_with("› ")));
                let start = settings_scroll(self.scroll as usize,selected,available,visual.len());
                (start, (start + available).min(visual.len()))
            } else {
                let end = visual.len().saturating_sub(self.scroll as usize);
                (end.saturating_sub(available), end)
            };
            let title = if self.settings.is_some() {
                " Settings · microphone paused "
            } else if self.chat == "off" {
                " Tools · chat hidden · /settings "
            } else if self.chat == "transcript" {
                " Transcript · PgUp/PgDn "
            } else {
                " Activity · mouse wheel or PgUp/PgDn "
            };
            f.render_widget(
                Paragraph::new(visual[start..end].to_vec()).block(
                    Block::default()
                        .borders(Borders::ALL)
                            .border_type(BorderType::Rounded)
                        .border_style(border)
                        .title(title),
                ),
                layout[1],
            );
            f.render_widget(
                Paragraph::new(if self.secret {
                    "*".repeat(self.input.chars().count())
                } else {
                    self.input.clone()
                })
                .wrap(Wrap { trim: false })
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                            .border_type(BorderType::Rounded)
                        .border_style(border)
                        .title(if self.secret {
                            " Secret · paste (Ctrl+Shift+V / Shift+Insert) · Enter saves · Esc cancels "
                        } else {
                            " Message or / command · Tab completes · Enter sends "
                        }),
                ),
                layout[2],
            );
            f.render_widget(
                Paragraph::new(if self.settings.is_some() {" ↑/↓ choose · Enter select · Esc back · changes save on confirmation "} else {" ↑/↓ history · / commands · Esc cancel · /repeat · /jobs · /settings "})
                    .style(Style::default().fg(color)),
                layout[3],
            );
            if !self.secret {
                let matches = crate::dashboard::matching(&self.input);
                if !matches.is_empty() && layout[1].height >= 5 {
                    let height = (matches.len() as u16 + 2).min(layout[1].height);
                    let popup = ratatui::layout::Rect::new(
                        layout[1].x,
                        layout[1].bottom() - height,
                        layout[1].width,
                        height,
                    );
                    let menu = matches
                        .iter()
                        .enumerate()
                        .map(|(i, (name, desc))| {
                            format!(
                                "{} {name:14} {desc}",
                                if i == self.menu_index { "›" } else { " " }
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    f.render_widget(Clear, popup);
                    f.render_widget(
                        Paragraph::new(menu)
                            .style(Style::default().fg(Color::Cyan))
                            .block(
                                Block::default()
                                    .borders(Borders::ALL)
                            .border_type(BorderType::Rounded)
                                    .border_style(border)
                                    .title(" Commands · ↑/↓ choose · Enter opens · Tab completes "),
                            ),
                        popup,
                    );
                }
            }
            if layout[2].width > 2 && layout[2].height > 2 {
                f.set_cursor_position((
                    layout[2].x + 1 + (self.input.chars().count() as u16).min(layout[2].width - 3),
                    layout[2].y + 1,
                ));
            }
        })?;
        Ok(())
    }
}
#[derive(Default)]
struct InputHistory {
    entries: VecDeque<String>,
    position: Option<usize>,
    draft: String,
}
impl InputHistory {
    fn remember(&mut self, text: &str) {
        if !text.trim().is_empty() && self.entries.back().is_none_or(|last| last != text) {
            self.entries.push_back(text.to_owned());
            while self.entries.len() > 50 {
                self.entries.pop_front();
            }
        }
        self.position = None;
        self.draft.clear();
    }
    fn navigate(&mut self, input: &mut String, older: bool) {
        if self.entries.is_empty() {
            return;
        }
        if older {
            let next = match self.position {
                Some(index) => index.saturating_sub(1),
                None => {
                    self.draft = input.clone();
                    self.entries.len() - 1
                }
            };
            self.position = Some(next);
            *input = self.entries[next].clone();
        } else if let Some(index) = self.position {
            if index + 1 < self.entries.len() {
                self.position = Some(index + 1);
                *input = self.entries[index + 1].clone();
            } else {
                self.position = None;
                *input = std::mem::take(&mut self.draft);
            }
        }
    }
}

/// Clipboard text is passed on stdin, never interpolated into a shell command.
pub async fn copy_text(text: String) -> Result<String> {
    use tokio::io::AsyncWriteExt;
    let candidates: &[&[&str]] = if cfg!(windows) {
        &[&["powershell", "-NoProfile", "-NonInteractive", "-Command", "[Console]::InputEncoding = [Text.UTF8Encoding]::new(); Set-Clipboard -Value ([Console]::In.ReadToEnd())"]]
    } else if cfg!(target_os = "macos") {
        &[&["pbcopy"]]
    } else {
        &[
            &["wl-copy"],
            &["xclip", "-selection", "clipboard"],
            &["xsel", "--clipboard", "--input"],
        ]
    };
    for args in candidates {
        let mut command = tokio::process::Command::new(args[0]);
        command
            .args(&args[1..])
            .kill_on_drop(true)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let Ok(mut child) = command.spawn() else {
            continue;
        };
        let result = tokio::time::timeout(Duration::from_secs(5), async {
            if let Some(mut stdin) = child.stdin.take() {
                stdin.write_all(text.as_bytes()).await?;
            }
            child.wait().await
        })
        .await;
        if matches!(result, Ok(Ok(status)) if status.success()) {
            return Ok("Copied the last answer to the clipboard.".into());
        }
    }
    anyhow::bail!(
        "Clipboard unavailable. Linux needs wl-copy, xclip, or xsel in a desktop session."
    )
}

fn settings_scroll(
    scroll: usize,
    selected: Option<usize>,
    available: usize,
    total: usize,
) -> usize {
    let max = total.saturating_sub(available);
    let mut start = scroll.min(max);
    if let Some(row) = selected {
        if row < start {
            start = row;
        } else if row + 2 >= start + available {
            start = (row + 3).saturating_sub(available).min(max);
        }
    }
    start
}
fn status_lines(status: &str) -> String {
    let parts: Vec<_> = status.split(" | ").collect();
    let voice = parts
        .first()
        .copied()
        .unwrap_or(status)
        .replace(
            "WHITE: waiting for wake code",
            "Asleep · say your wake code",
        )
        .replace("GREEN: conversation open", "Listening · conversation open")
        .replace(
            "BLUE: waiting for reply",
            "Thinking · listening for your follow-up",
        );
    let identity = parts.get(2).copied().unwrap_or("");
    let task = parts
        .get(1)
        .copied()
        .unwrap_or("")
        .replace("BLUE: agent working", "Working")
        .replace("AMBER: approval pending", "Approval needed");
    format!("{voice}  ·  {task}\nMain harness: {identity}")
}
fn plain_prefix(kind: Kind, text: &str) -> String {
    match kind {
        Kind::User => format!("You: {text}"),
        Kind::Agent => format!("Agent: {text}"),
        Kind::Tool => format!("Tool: {text}"),
        Kind::Progress => format!("… {text}"),
        Kind::Notice => format!("! {text}"),
        Kind::System => text.to_string(),
    }
}
fn styled_entry(entry: &Entry, width: usize) -> Vec<Line<'static>> {
    let (label, color) = match entry.kind {
        Kind::User => ("You", Color::Green),
        Kind::Agent => ("Agent", Color::Cyan),
        Kind::Tool => ("Tool", Color::Yellow),
        Kind::Progress => ("…", Color::DarkGray),
        Kind::Notice => ("Notice", Color::Magenta),
        Kind::System => ("", Color::Gray),
    };
    let body = if entry.kind == Kind::Agent {
        crate::markdown::render(&entry.text)
    } else {
        entry
            .text
            .lines()
            .map(|l| Line::from(l.to_string()))
            .collect::<Vec<_>>()
    };
    let mut out = Vec::new();
    if label.is_empty() {
        return crate::markdown::wrap(body, width);
    }
    for (i, mut line) in body.into_iter().enumerate() {
        if i == 0 {
            let mut spans = vec![Span::styled(
                format!("{label:<5} "),
                Style::default().fg(color),
            )];
            spans.extend(line.spans);
            line = Line::from(spans);
        } else {
            let mut spans = vec![Span::raw("      ")];
            spans.extend(line.spans);
            line = Line::from(spans);
        }
        out.push(line);
    }
    crate::markdown::wrap(out, width)
}
impl Drop for Ui {
    fn drop(&mut self) {
        if self.terminal.is_some() {
            let _ = execute!(
                io::stdout(),
                DisableBracketedPaste,
                DisableMouseCapture,
                LeaveAlternateScreen
            );
            let _ = terminal::disable_raw_mode();
        }
    }
}
fn apply_paste(input: &mut String, pasted: &str) {
    let chunk = pasted.lines().next().unwrap_or(pasted).trim();
    for c in chunk.chars() {
        if !c.is_control() && input.len() < 4096 {
            input.push(c);
        }
    }
}
fn clipboard_text() -> Option<String> {
    let candidates: &[&[&str]] = if cfg!(windows) {
        &[&[
            "powershell",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Get-Clipboard",
        ]]
    } else if cfg!(target_os = "macos") {
        &[&["pbpaste"]]
    } else {
        &[
            &["wl-paste", "-n"],
            &["xclip", "-selection", "clipboard", "-o"],
            &["xsel", "-ob"],
        ]
    };
    for args in candidates {
        let (bin, rest) = args.split_first()?;
        let Ok(output) = std::process::Command::new(bin).args(rest).output() else {
            continue;
        };
        if !output.status.success() {
            continue;
        }
        let text = String::from_utf8_lossy(&output.stdout);
        let text = text.lines().next().unwrap_or("").trim();
        if !text.is_empty() {
            return Some(text.to_string());
        }
    }
    None
}
fn status_color(status: &str) -> Color {
    if status.contains("SPEAKING") {
        Color::Magenta
    } else if status.contains("AMBER") || status.contains("SETTINGS") {
        Color::Yellow
    } else if status.contains("BLUE") {
        Color::LightBlue
    } else if status.contains("GREEN") {
        Color::Green
    } else {
        Color::Gray
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn history_restores_draft_and_bounds_duplicates() {
        let mut history = InputHistory::default();
        history.remember("first");
        history.remember("second");
        history.remember("second");
        let mut input = "unfinished draft".to_string();
        history.navigate(&mut input, true);
        assert_eq!(input, "second");
        history.navigate(&mut input, true);
        assert_eq!(input, "first");
        history.navigate(&mut input, true);
        assert_eq!(input, "first");
        history.navigate(&mut input, false);
        assert_eq!(input, "second");
        history.navigate(&mut input, false);
        assert_eq!(input, "unfinished draft");
        for n in 0..100 {
            history.remember(&format!("message {n}"));
        }
        assert_eq!(history.entries.len(), 50);
        assert_eq!(history.entries.front().unwrap(), "message 50");
    }
    #[test]
    fn locking_clears_input_history_and_secret_draft() {
        let mut ui = Ui::new(true).unwrap();
        ui.history.remember("private conversation");
        ui.history.draft = "sensitive draft".into();
        ui.input = "private input".into();
        ui.secret(true);
        assert!(ui.history.draft.is_empty());
        assert!(ui.input.is_empty());
        ui.clear_private();
        assert!(ui.history.entries.is_empty());
    }
    #[test]
    fn settings_selection_stays_visible_in_short_terminals() {
        assert_eq!(settings_scroll(0, Some(17), 8, 25), 12);
        assert_eq!(settings_scroll(12, Some(2), 8, 25), 2);
        assert!(
            status_lines("GREEN: conversation open | idle | codex · luna")
                .contains("Main harness: codex · luna")
        );
    }
    #[test]
    fn speaking_paints_accessor_purple() {
        assert_eq!(
            status_color("SPEAKING: listening for interruptions"),
            Color::Magenta
        );
        assert_eq!(status_color("GREEN: conversation open"), Color::Green);
        assert_eq!(status_color("BLUE: agent working"), Color::LightBlue);
        assert_eq!(status_color("SETTINGS: mic paused"), Color::Yellow);
    }
    #[test]
    fn paste_takes_one_trimmed_line() {
        let mut input = String::new();
        apply_paste(&mut input, "  sk-test-key\nignored\n");
        assert_eq!(input, "sk-test-key");
    }
}
