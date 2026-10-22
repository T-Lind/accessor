use crate::mascot::{bear, Mood};
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
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap},
    Terminal,
};
use std::{
    collections::VecDeque,
    io::{self, IsTerminal},
    time::{Duration, Instant},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

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
    cursor: usize,
    status: String,
    scroll: u16,
    dirty: bool,
    secret: bool,
    menu_index: usize,
    menu_chosen: bool,
    settings: Option<String>,
    notice: String,
    chat: String,
    history: InputHistory,
    auto_mood: Mood,
    mood_hold: Option<MoodHold>,
    bear: bear::Animator,
    animation_started: Instant,
    last_frame: Instant,
    reduced_motion: bool,
    color: bool,
    clipboard: Option<std::sync::mpsc::Receiver<Option<String>>>,
}
struct MoodHold {
    mood: Mood,
    hold: Duration,
    until: Option<Instant>,
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
        let reduced_motion = std::env::var("ACC_REDUCED_MOTION").is_ok_and(|value| {
            !matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "" | "0" | "false" | "no" | "off"
            )
        });
        let color = std::env::var_os("NO_COLOR").is_none_or(|value| value.is_empty());
        let mut bear = bear::Animator::new(Mood::Asleep);
        bear.set_reduced_motion(reduced_motion);
        bear.set_color(color);
        Ok(Self {
            terminal,
            lines: VecDeque::new(),
            input: String::new(),
            cursor: 0,
            status: String::new(),
            scroll: 0,
            dirty: true,
            secret: false,
            menu_index: 0,
            menu_chosen: false,
            settings: None,
            notice: String::new(),
            chat: "activity".into(),
            history: InputHistory::default(),
            auto_mood: Mood::Asleep,
            mood_hold: None,
            bear,
            animation_started: Instant::now(),
            last_frame: Instant::now(),
            reduced_motion,
            color,
            clipboard: None,
        })
    }
    /// Update the phase-derived mood. Tone overrides apply between active phases.
    pub fn set_mood(&mut self, mood: Mood) {
        if self.auto_mood != mood {
            self.auto_mood = mood;
            self.dirty = true;
        }
    }
    /// Hold a specific mood on screen for `hold`, then fall back to the phase.
    /// The hold starts when the pose is actually visible, so reply tone is not
    /// consumed while Accessor is still thinking or speaking.
    pub fn override_mood(&mut self, mood: Mood, hold: Duration) {
        self.mood_hold = Some(MoodHold {
            mood,
            hold,
            until: None,
        });
        self.dirty = true;
    }
    /// Return to the live phase immediately, e.g. when a new turn starts.
    pub fn clear_mood_override(&mut self) {
        if self.mood_hold.take().is_some() {
            self.dirty = true;
        }
    }
    /// The mood currently on screen, for status lines and commands.
    pub fn mood_label(&self) -> &'static str {
        self.displayed_mood().label()
    }
    /// The automatic phase, ignoring any pending or visible tone override.
    pub fn phase_mood_label(&self) -> &'static str {
        self.auto_mood.label()
    }
    fn displayed_mood(&self) -> Mood {
        if let Some(hold) = &self.mood_hold {
            if matches!(self.auto_mood, Mood::Idle | Mood::Asleep) {
                return hold.mood;
            }
        }
        self.auto_mood
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
        self.cursor = 0;
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
        self.cursor = 0;
        self.settings = None;
        self.notice.clear();
        self.scroll = 0;
        self.mood_hold = None;
        self.auto_mood = Mood::Locked;
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
        if self.settings.is_some() {
            self.notice = text.lines().take(2).collect::<Vec<_>>().join(" ");
        }
        if self.terminal.is_none() {
            if self.visible(kind) {
                println!("{}", plain_prefix(kind, &text));
            }
            return;
        }
        // Keep filtered entries as well: changing the chat view should reveal
        // or hide earlier lines immediately, in both directions.
        self.lines.push_back(Entry { kind, text });
        while self.lines.len() > 600 {
            self.lines.pop_front();
        }
        // Reading older output should not be yanked away by the next tool call.
        self.dirty = true;
    }
    pub fn settings(&mut self, text: Option<String>) {
        if self.terminal.is_none() {
            if let Some(text) = &text {
                println!("{}", safe(text));
            }
        }
        let was_open = self.settings.is_some();
        self.settings = text.map(|t| safe(&t));
        if !was_open || self.settings.is_none() {
            self.notice.clear();
        }
        self.scroll = 0;
        self.dirty = true;
    }
    pub fn status(&mut self, line: &str) {
        // Harness/model names are configuration, not trusted terminal output.
        let line: String = line
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect();
        if line != self.status {
            if self.terminal.is_none() {
                println!("[{line}]");
            }
            self.status = line;
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
                    apply_paste(&mut self.input, &text, self.secret);
                    self.cursor = self.input.chars().count();
                    self.menu_index = 0;
                    self.menu_chosen = false;
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
                            self.request_clipboard();
                        }
                        KeyCode::Insert if k.modifiers.contains(KeyModifiers::SHIFT) => {
                            self.request_clipboard();
                        }
                        KeyCode::Char('a') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                            self.cursor = 0;
                        }
                        KeyCode::Char('e') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                            self.cursor = self.input.chars().count();
                        }
                        KeyCode::Char('u') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                            let after: String = self.input.chars().skip(self.cursor).collect();
                            self.input = after;
                            self.cursor = 0;
                            self.history.position = None;
                            self.menu_index = 0;
                        }
                        KeyCode::Char('k') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                            self.input = self.input.chars().take(self.cursor).collect();
                            self.history.position = None;
                            self.menu_index = 0;
                        }
                        KeyCode::Char('w') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                            self.delete_word_before();
                            self.history.position = None;
                            self.menu_index = 0;
                        }
                        KeyCode::Char(_) if k.modifiers.contains(KeyModifiers::CONTROL) => {}
                        KeyCode::Enter => {
                            if self.settings.is_some() && self.input.is_empty() && !self.secret {
                                return Ok(Some("/settings-nav enter".into()));
                            }
                            if !self.secret {
                                if let Some(name) =
                                    completion(&self.input, self.menu_index, self.menu_chosen)
                                {
                                    self.input = name;
                                }
                            }
                            self.menu_index = 0;
                            self.menu_chosen = false;
                            self.cursor = 0;
                            let text = std::mem::take(&mut self.input);
                            if !self.secret && self.settings.is_none() {
                                self.history.remember(&text);
                            }
                            return Ok(Some(text));
                        }
                        KeyCode::Tab if !self.secret => {
                            // Complete only a unique prefix (or a row the user
                            // explicitly highlighted) so Tab on "/" cannot turn
                            // into a destructive command by habit.
                            if let Some(name) =
                                completion(&self.input, self.menu_index, self.menu_chosen)
                            {
                                self.input = format!("{name} ");
                                self.cursor = self.input.chars().count();
                            }
                            self.menu_index = 0;
                            self.menu_chosen = false;
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
                                self.menu_chosen = true;
                            }
                        }
                        KeyCode::Up
                            if !self.secret
                                && self.history.position.is_none()
                                && !crate::dashboard::matching(&self.input).is_empty() =>
                        {
                            self.menu_index = self.menu_index.saturating_sub(1);
                            self.menu_chosen = true;
                        }
                        KeyCode::Up if !self.secret && self.settings.is_none() => {
                            self.history.navigate(&mut self.input, true);
                            self.cursor = self.input.chars().count();
                            self.menu_index = 0;
                            self.menu_chosen = false;
                        }
                        KeyCode::Down if !self.secret && self.settings.is_none() => {
                            self.history.navigate(&mut self.input, false);
                            self.cursor = self.input.chars().count();
                            self.menu_index = 0;
                            self.menu_chosen = false;
                        }
                        KeyCode::Backspace => {
                            self.history.position = None;
                            self.delete_before();
                            self.menu_index = 0;
                            self.menu_chosen = false;
                        }
                        KeyCode::Delete => {
                            self.history.position = None;
                            self.delete_after();
                            self.menu_index = 0;
                            self.menu_chosen = false;
                        }
                        KeyCode::Left => {
                            self.cursor = self.cursor.saturating_sub(1);
                        }
                        KeyCode::Right => {
                            self.cursor = (self.cursor + 1).min(self.input.chars().count());
                        }
                        KeyCode::Home => self.cursor = 0,
                        KeyCode::End => self.cursor = self.input.chars().count(),
                        KeyCode::Esc => {
                            self.input.clear();
                            self.cursor = 0;
                            self.history.position = None;
                            self.menu_index = 0;
                            self.menu_chosen = false;
                            if self.settings.is_some() && !self.secret {
                                return Ok(Some("/settings-nav back".into()));
                            }
                            return Ok(Some("/cancel".into()));
                        }
                        KeyCode::PageUp => self.scroll_lines(8),
                        KeyCode::PageDown => self.scroll_lines(-8),
                        KeyCode::Char(c) if !c.is_control() && self.input.len() < 4096 => {
                            self.history.position = None;
                            self.insert_char(c);
                            self.menu_index = 0;
                            self.menu_chosen = false;
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
        self.poll_clipboard();
        Ok(None)
    }
    /// Read the clipboard on a worker thread. Clipboard helpers can block on a
    /// desktop portal or a missing selection owner; the input loop must keep
    /// drawing and handling keys while that happens.
    fn request_clipboard(&mut self) {
        if self.clipboard.is_some() {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(clipboard_text());
        });
        self.clipboard = Some(rx);
        self.dirty = true;
    }
    fn poll_clipboard(&mut self) {
        let Some(result) = self
            .clipboard
            .as_ref()
            .map(std::sync::mpsc::Receiver::try_recv)
        else {
            return;
        };
        match result {
            Ok(Some(text)) => {
                self.clipboard = None;
                self.history.position = None;
                apply_paste(&mut self.input, &text, self.secret);
                self.cursor = self.input.chars().count();
                self.menu_index = 0;
                self.menu_chosen = false;
                self.dirty = true;
            }
            Ok(None) | Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.clipboard = None;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
        }
    }
    fn insert_char(&mut self, c: char) {
        let mut chars: Vec<char> = self.input.chars().collect();
        let at = self.cursor.min(chars.len());
        chars.insert(at, c);
        self.input = chars.into_iter().collect();
        self.cursor = at + 1;
    }
    fn delete_before(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let mut chars: Vec<char> = self.input.chars().collect();
        chars.remove(self.cursor - 1);
        self.input = chars.into_iter().collect();
        self.cursor -= 1;
    }
    fn delete_after(&mut self) {
        let mut chars: Vec<char> = self.input.chars().collect();
        if self.cursor < chars.len() {
            chars.remove(self.cursor);
            self.input = chars.into_iter().collect();
        }
    }
    fn delete_word_before(&mut self) {
        let chars: Vec<char> = self.input.chars().collect();
        let mut start = self.cursor.min(chars.len());
        while start > 0 && chars[start - 1].is_whitespace() {
            start -= 1;
        }
        while start > 0 && !chars[start - 1].is_whitespace() {
            start -= 1;
        }
        let rest: String = chars[self.cursor.min(chars.len())..].iter().collect();
        self.input = chars[..start].iter().collect::<String>() + &rest;
        self.cursor = start;
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
    /// Resolve the mood to show, retiring an expired override. Marks the frame
    /// dirty when the face changes so an override visibly times out.
    fn effective_mood(&mut self) -> Mood {
        let now = Instant::now();
        if let Some(hold) = &mut self.mood_hold {
            let visible = matches!(self.auto_mood, Mood::Idle | Mood::Asleep);
            if !visible {
                // Pause the countdown while live activity owns the pose.
                if let Some(until) = hold.until.take() {
                    hold.hold = until.saturating_duration_since(now);
                }
            } else if let Some(until) = hold.until {
                if now >= until {
                    self.mood_hold = None;
                    self.dirty = true;
                }
            } else {
                hold.until = Some(now + hold.hold);
            }
        }
        self.displayed_mood()
    }
    pub fn draw(&mut self) -> Result<()> {
        let mood = self.effective_mood();
        if !self.dirty
            && (self.reduced_motion
                || self.last_frame.elapsed() < Duration::from_millis(bear::FRAME_MS))
        {
            return Ok(());
        }
        self.dirty = false;
        let Some(mut terminal) = self.terminal.take() else {
            return Ok(());
        };
        let now = self.animation_started.elapsed().as_secs_f64() * 1000.0;
        self.bear.set_mood(mood, now);
        let frame = self.bear.frame(now);
        self.last_frame = Instant::now();
        let result = terminal.draw(|f| self.render(f, mood, &frame)).map(|_| ());
        self.terminal = Some(terminal);
        result?;
        Ok(())
    }
    fn render(&self, f: &mut ratatui::Frame<'_>, mood: Mood, frame: &bear::Bear) {
        let area = f.area();
        if area.width < 44 || area.height < 12 {
            let message = vec![
                Line::from(Span::styled(
                    format!("{} · {}", crate::mascot::NAME, mood.label()),
                    Style::default()
                        .fg(paint(self.color, mood.color()))
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from("Terminal too small"),
                Line::from("Need at least 48 × 12"),
                Line::from("Use --plain for a simple view."),
            ];
            f.render_widget(
                Paragraph::new(message)
                    .alignment(Alignment::Center)
                    .wrap(Wrap { trim: true }),
                area,
            );
            return;
        }
        let layout = view_layout(area, self.settings.is_some());
        let color = paint(self.color, status_color(&self.status));
        let border = Style::default().fg(color);
        f.render_widget(
            Paragraph::new(status_lines(
                &self.status,
                layout.status.width.saturating_sub(2) as usize,
            ))
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
            layout.status,
        );
        if let Some(mascot_area) = layout.mascot {
            let tint = Style::default().fg(paint(self.color, mood.color()));
            let title = if mascot_area.width < 24 {
                format!(" {} ", crate::mascot::NAME)
            } else {
                format!(" {} · {} ", crate::mascot::NAME, mood.label())
            };
            let block = Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(tint)
                .title(Span::styled(title, tint.add_modifier(Modifier::BOLD)));
            let inner = block.inner(mascot_area);
            f.render_widget(block, mascot_area);
            if inner.width >= bear::WIDTH && inner.height >= bear::HEIGHT {
                f.render_widget(frame, inner);
            } else {
                let face: Vec<Line> = mood
                    .face()
                    .iter()
                    .map(|row| Line::from(Span::styled(row.to_string(), tint)))
                    .collect();
                f.render_widget(Paragraph::new(face).alignment(Alignment::Center), inner);
            }
        }
        let width = layout.activity.width.saturating_sub(2).max(1) as usize;
        let mut visual: Vec<Line> = Vec::new();
        if self.settings.is_some() {
            let panel = self.settings.as_deref().unwrap_or_default();
            for line in panel.lines() {
                if let Some(rest) = line.strip_prefix("hint:") {
                    visual.push(Line::from(Span::styled(
                        rest.to_string(),
                        Style::default().fg(paint(self.color, Color::Cyan)),
                    )));
                } else if line.starts_with("› ") {
                    visual.push(Line::from(Span::styled(
                        line.to_string(),
                        Style::default()
                            .fg(paint(self.color, Color::White))
                            .bg(paint(self.color, Color::Rgb(35, 62, 79)))
                            .add_modifier(Modifier::BOLD),
                    )));
                } else {
                    visual.push(Line::from(line.to_string()));
                }
            }
            visual = crate::markdown::wrap(visual, width);
        } else {
            for entry in self.lines.iter().filter(|entry| self.visible(entry.kind)) {
                visual.extend(styled_entry(entry, width, self.color));
            }
        }
        let available = layout.activity.height.saturating_sub(2) as usize;
        let (start, end) = if self.settings.is_some() {
            let selected = visual
                .iter()
                .position(|l| l.spans.first().is_some_and(|s| s.content.starts_with("› ")));
            let start = settings_scroll(self.scroll as usize, selected, available, visual.len());
            (start, (start + available).min(visual.len()))
        } else {
            // Clamp stale scroll offsets so paging past the top cannot leave a
            // blank pane that needs matching PageDown presses to recover.
            let scroll = (self.scroll as usize).min(visual.len().saturating_sub(available));
            let end = visual.len().saturating_sub(scroll);
            (end.saturating_sub(available), end)
        };
        let title = if self.settings.is_some() {
            " Settings · microphone paused "
        } else if self.chat == "off" {
            " Tools · chat hidden · /settings "
        } else if self.chat == "transcript" {
            " Transcript · PgUp/PgDn "
        } else {
            " Activity · PgUp/PgDn "
        };
        f.render_widget(
            Paragraph::new(visual[start..end].to_vec()).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(border)
                    .title(title),
            ),
            layout.activity,
        );
        let input_full = if self.secret {
            "*".repeat(self.input.chars().count())
        } else {
            self.input.clone()
        };
        let cursor_chars = self.cursor.min(input_full.chars().count());
        let before: String = input_full.chars().take(cursor_chars).collect();
        let before_width = UnicodeWidthStr::width(before.as_str());
        let input_width = layout.input.width.saturating_sub(4).max(1) as usize;
        let start_col = before_width.saturating_sub(input_width.saturating_sub(1));
        let input_body = slice_display(&input_full, start_col, input_width);
        let cursor_col = before_width.saturating_sub(start_col) as u16;
        let prompt_style = Style::default().fg(color).add_modifier(Modifier::BOLD);
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(if self.secret { "# " } else { "❯ " }, prompt_style),
                Span::raw(input_body),
            ]))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(border)
                    .title(if self.secret && layout.input.width >= 60 {
                        " Secret · paste (Ctrl+Shift+V / Shift+Insert) · Enter saves · Esc cancels "
                    } else if self.secret {
                        " Secret · Enter saves · Esc cancels "
                    } else if self.settings.is_some() {
                        " Settings · value or ↑/↓ · Esc back "
                    } else if layout.input.width < 60 {
                        " Message · Enter sends "
                    } else {
                        " Message or / command · Tab completes · Enter sends "
                    }),
            ),
            layout.input,
        );
        let footer_text = if self.settings.is_some() {
            if !self.notice.is_empty() {
                self.notice.lines().next().unwrap_or("").to_string()
            } else if area.width < 60 {
                " ↑/↓ · Enter · Esc back ".into()
            } else {
                " ↑/↓ choose · Enter select · Esc back · changes apply when chosen ".into()
            }
        } else if area.width < 60 {
            " / commands · Esc cancel ".into()
        } else {
            " ↑/↓ history · / commands · Esc cancel · /repeat · /jobs · /settings ".into()
        };
        let footer_style = if self.settings.is_some() && !self.notice.is_empty() {
            Style::default().fg(paint(self.color, Color::Yellow))
        } else {
            Style::default().fg(color)
        };
        f.render_widget(
            Paragraph::new(footer_text).style(footer_style),
            layout.footer,
        );
        if !self.secret {
            let matches = crate::dashboard::matching(&self.input);
            if !matches.is_empty() && layout.activity.height >= 5 {
                let height = (matches.len() as u16 + 2).min(layout.activity.height);
                let popup = Rect::new(
                    layout.activity.x,
                    layout.activity.bottom() - height,
                    layout.activity.width,
                    height,
                );
                let items: Vec<Line> = matches
                    .iter()
                    .enumerate()
                    .map(|(i, (name, desc))| {
                        let selected = i == self.menu_index;
                        let lead = if selected {
                            Style::default()
                                .fg(paint(self.color, Color::Black))
                                .bg(paint(self.color, Color::Cyan))
                        } else {
                            Style::default().fg(paint(self.color, Color::Cyan))
                        };
                        let desc_style = if selected {
                            lead.add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(paint(self.color, Color::DarkGray))
                        };
                        Line::from(vec![
                            Span::styled(if selected { "› " } else { "  " }, lead),
                            Span::styled(format!("{name:<14}"), lead),
                            Span::styled(desc.to_string(), desc_style),
                        ])
                    })
                    .collect();
                f.render_widget(Clear, popup);
                f.render_widget(
                    Paragraph::new(items).block(
                        Block::default()
                            .borders(Borders::ALL)
                            .border_type(BorderType::Rounded)
                            .border_style(border)
                            .title(if layout.activity.width < 60 {
                                " Commands · ↑/↓ · Enter "
                            } else {
                                " Commands · ↑/↓ choose · Enter opens · Tab completes "
                            }),
                    ),
                    popup,
                );
            }
        }
        if layout.input.width > 3 && layout.input.height > 2 {
            f.set_cursor_position((
                layout.input.x + 3 + cursor_col.min(layout.input.width.saturating_sub(4)),
                layout.input.y + 1,
            ));
        }
    }
}

struct ViewLayout {
    status: Rect,
    activity: Rect,
    mascot: Option<Rect>,
    input: Rect,
    footer: Rect,
}
fn view_layout(area: Rect, settings: bool) -> ViewLayout {
    let wide = !settings && area.width >= 76 && area.height >= 24;
    let tall = !settings && !wide && area.width >= 44 && area.height >= 33;
    let compact = !settings && !wide && !tall && area.width >= 48 && area.height >= 12;
    // A full-width status needs four rows above the full-size companion. In a
    // tight 24-row window, keep the original side-by-side split instead; the
    // compact status formatter shortens the identity line to fit.
    if tall || (wide && area.height >= 28) {
        let rows = Layout::vertical([
            Constraint::Length(4),
            Constraint::Length(if tall { bear::HEIGHT + 2 } else { 0 }),
            Constraint::Min(0),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(area);
        let (activity, mascot) = if wide {
            let split =
                Layout::horizontal([Constraint::Min(34), Constraint::Length(bear::WIDTH + 2)])
                    .split(rows[2]);
            (split[0], Some(split[1]))
        } else {
            (rows[2], Some(rows[1]))
        };
        return ViewLayout {
            status: rows[0],
            activity,
            mascot,
            input: rows[3],
            footer: rows[4],
        };
    }
    let outer = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(3),
        Constraint::Length(1),
    ])
    .split(area);
    if wide {
        let split = Layout::horizontal([Constraint::Min(34), Constraint::Length(bear::WIDTH + 2)])
            .split(outer[0]);
        let rows = Layout::vertical([Constraint::Length(4), Constraint::Min(0)]).split(split[0]);
        return ViewLayout {
            status: rows[0],
            activity: rows[1],
            mascot: Some(split[1]),
            input: outer[1],
            footer: outer[2],
        };
    }
    let rows = Layout::vertical([
        Constraint::Length(if compact { 5 } else { 4 }),
        Constraint::Min(0),
    ])
    .split(outer[0]);
    let (status, mascot) = if compact {
        let split =
            Layout::horizontal([Constraint::Min(24), Constraint::Length(13)]).split(rows[0]);
        (split[0], Some(split[1]))
    } else {
        (rows[0], None)
    };
    ViewLayout {
        status,
        activity: rows[1],
        mascot,
        input: outer[1],
        footer: outer[2],
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
fn status_lines(status: &str, width: usize) -> String {
    if status.trim().is_empty() {
        return String::new();
    }
    let parts: Vec<_> = status.splitn(3, " | ").collect();
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
    let voice = if width < 50 {
        let short = if voice.starts_with("ALARM") {
            "Alarm ringing".to_string()
        } else if voice.starts_with("SETTINGS") {
            "Settings · microphone paused".to_string()
        } else if voice.starts_with("SPEAKING") {
            "Speaking · say wake code".to_string()
        } else if voice.starts_with("MICROPHONE UNAVAILABLE") {
            "Microphone unavailable".to_string()
        } else if voice.starts_with("CONNECTING") {
            "Connecting microphone".to_string()
        } else if voice.starts_with("LOCKED") {
            "Locked · /unlock".to_string()
        } else {
            voice
        };
        short.replace("Thinking · listening for your follow-up", "Thinking")
    } else {
        voice
    };
    if parts.len() < 2 {
        return voice;
    }
    let identity = parts.get(2).copied().unwrap_or("");
    let task = parts
        .get(1)
        .copied()
        .unwrap_or("")
        .replace("BLUE: agent working", "Working")
        .replace("AMBER: approval pending", "Approval needed");
    if identity.is_empty() {
        format!("{voice}  ·  {task}")
    } else if width < 50 {
        // The narrow side-by-side layout gets the short identity form so the
        // harness and model are not clipped away on a standard 80-column screen.
        let first = if task == "idle" {
            voice
        } else {
            format!("{voice}  ·  {task}")
        };
        format!("{first}\n{identity}")
    } else {
        format!("{voice}  ·  {task}\nMain harness: {identity}")
    }
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
fn paint(enabled: bool, color: Color) -> Color {
    if enabled {
        color
    } else {
        Color::Reset
    }
}

fn styled_entry(entry: &Entry, width: usize, color_enabled: bool) -> Vec<Line<'static>> {
    let (label, color) = match entry.kind {
        Kind::User => ("You", Color::Green),
        Kind::Agent => ("Agent", Color::Cyan),
        Kind::Tool => ("Tool", Color::Yellow),
        Kind::Progress => ("…", Color::DarkGray),
        Kind::Notice => ("Notice", Color::Magenta),
        Kind::System => ("", Color::Gray),
    };
    let color = paint(color_enabled, color);
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
                Style::default().fg(color).add_modifier(Modifier::BOLD),
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
/// Resolve Enter on the command menu. A unique match, an exact command, or an
/// explicitly highlighted row completes; an ambiguous prefix is sent as typed
/// so Enter can never turn a bare `/` into `/lock`.
fn completion(input: &str, menu_index: usize, chosen: bool) -> Option<String> {
    let matches = crate::dashboard::matching(input);
    let (name, _) = matches.get(menu_index)?;
    if matches.len() == 1 || chosen || *name == input {
        Some((*name).into())
    } else {
        None
    }
}
fn slice_display(text: &str, start_col: usize, width: usize) -> String {
    let mut out = String::new();
    let mut col = 0;
    let mut used = 0;
    for ch in text.chars() {
        let w = UnicodeWidthChar::width(ch).unwrap_or(0);
        // Start on a character boundary rather than half a wide glyph.
        if col < start_col {
            col += w;
            continue;
        }
        if used + w > width {
            break;
        }
        out.push(ch);
        used += w;
        col += w;
    }
    out
}
fn apply_paste(input: &mut String, pasted: &str, secret: bool) {
    // Credentials are single-line. Ordinary messages keep every pasted line,
    // collapsed to spaces, instead of silently dropping all but the first.
    let text = if secret {
        pasted.lines().next().unwrap_or("").trim()
    } else {
        pasted.trim()
    };
    for c in text.chars() {
        let c = if c.is_control() { ' ' } else { c };
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
    if status.contains("ALARM") || status.contains("LOCKED") {
        Color::Red
    } else if status.contains("MICROPHONE UNAVAILABLE") {
        Color::LightRed
    } else if status.contains("CONNECTING") {
        Color::Cyan
    } else if status.contains("SPEAKING") {
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
    fn tone_yields_to_live_phases_and_resumes_its_hold() {
        let mut ui = Ui::new(true).unwrap();
        ui.set_mood(Mood::Idle);
        ui.override_mood(Mood::Proud, Duration::from_secs(10));
        assert_eq!(ui.effective_mood(), Mood::Proud);
        for phase in [
            Mood::Locked,
            Mood::Alarmed,
            Mood::Worried,
            Mood::Cautious,
            Mood::Speaking,
            Mood::Thinking,
            Mood::Working,
            Mood::Listening,
        ] {
            ui.set_mood(phase);
            assert_eq!(ui.effective_mood(), phase);
        }
        // The hold was paused while live activity owned the pose.
        ui.set_mood(Mood::Idle);
        assert_eq!(ui.effective_mood(), Mood::Proud);
        ui.mood_hold.as_mut().unwrap().until = Some(Instant::now() - Duration::from_secs(1));
        assert_eq!(ui.effective_mood(), Mood::Idle);
        ui.override_mood(Mood::Happy, Duration::from_secs(10));
        ui.clear_private();
        assert_eq!(ui.effective_mood(), Mood::Locked);
    }

    #[test]
    fn enter_never_turns_an_ambiguous_prefix_into_a_destructive_command() {
        assert_eq!(completion("/", 0, false), None);
        assert_eq!(completion("/l", 0, false), None);
        assert_eq!(completion("/loc", 0, false), Some("/lock".into()));
        assert_eq!(completion("/", 0, true), Some("/lock".into()));
        assert_eq!(completion("/help", 0, false), Some("/help".into()));
    }

    #[test]
    fn locked_status_is_not_padded_with_an_empty_harness_line() {
        assert_eq!(
            status_lines("LOCKED · local unlock only · /unlock", 80),
            "LOCKED · local unlock only · /unlock"
        );
        assert!(status_lines("", 80).is_empty());
        assert_eq!(
            status_lines("LOCKED · local unlock only · /unlock", 36),
            "Locked · /unlock"
        );
        assert!(
            status_lines("GREEN: conversation open | idle | codex · luna", 80)
                .contains("Main harness: codex · luna")
        );
        assert!(
            status_lines("GREEN: conversation open | idle | codex · luna", 36)
                .contains("codex · luna")
        );
    }

    #[test]
    fn urgent_statuses_are_not_gray() {
        assert_eq!(status_color("ALARM: say 29 stop"), Color::Red);
        assert_eq!(status_color("LOCKED · local unlock only"), Color::Red);
        assert_eq!(
            status_color("MICROPHONE UNAVAILABLE: type a request"),
            Color::LightRed
        );
        assert_eq!(
            status_color("CONNECTING: opening the microphone"),
            Color::Cyan
        );
    }

    #[test]
    fn chat_mode_filters_existing_and_new_entries() {
        let mut ui = Ui::new(true).unwrap();
        ui.lines.push_back(Entry {
            kind: Kind::Agent,
            text: "visible in activity".into(),
        });
        ui.lines.push_back(Entry {
            kind: Kind::Progress,
            text: "hidden in activity".into(),
        });
        assert!(ui.visible(Kind::Agent));
        assert!(!ui.visible(Kind::Progress));
        ui.set_chat("transcript");
        assert!(ui.visible(Kind::Progress));
        ui.set_chat("off");
        assert!(!ui.visible(Kind::Agent));
        assert!(ui.visible(Kind::Tool));
    }

    #[test]
    fn full_size_bear_fits_beside_chat_and_in_tall_terminals() {
        use ratatui::backend::TestBackend;
        for (width, height) in [(76, 24), (80, 24), (120, 40), (44, 33)] {
            let layout = view_layout(Rect::new(0, 0, width, height), false);
            let mascot = layout.mascot.unwrap();
            assert!(mascot.width >= bear::WIDTH + 2 && mascot.height >= bear::HEIGHT + 2);
            assert!(layout.activity.width >= 34 && layout.activity.height >= 5);
            assert_eq!(layout.input.width, width);
            if height >= 28 {
                assert_eq!(layout.status.width, width);
            } else {
                assert!(layout.status.width >= 34);
            }
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            let mut ui = Ui::new(true).unwrap();
            ui.status = "GREEN: conversation open | idle | codex · luna".into();
            ui.lines.push_back(Entry {
                kind: Kind::User,
                text: "Show Bramble at its original size.".into(),
            });
            ui.lines.push_back(Entry { kind: Kind::Agent, text: "Bramble follows my live state and reply tone. You can also choose a pose with /mascot NAME.".into() });
            let frame = bear::Animator::new(Mood::Happy).frame(400.0);
            terminal
                .draw(|f| ui.render(f, Mood::Happy, &frame))
                .unwrap();
            let inner = Block::default().borders(Borders::ALL).inner(mascot);
            let mut expected = ratatui::buffer::Buffer::empty(inner);
            ratatui::widgets::Widget::render(&frame, inner, &mut expected);
            for y in inner.y..inner.bottom() {
                for x in inner.x..inner.right() {
                    assert_eq!(
                        terminal.backend().buffer()[(x, y)],
                        expected[(x, y)],
                        "{width}x{height} at {x},{y}"
                    );
                }
            }
            if let Some(dir) = std::env::var_os("ACC_MASCOT_PREVIEW_DIR") {
                let dir = std::path::PathBuf::from(dir);
                std::fs::create_dir_all(&dir).unwrap();
                let cells: Vec<_> = terminal.backend().buffer().content.iter().map(|cell| {
                    let rgb = |color| match color {
                        Color::Rgb(r,g,b) => format!("#{r:02x}{g:02x}{b:02x}"),
                        Color::Green | Color::LightGreen => "#7fe3bf".into(),
                        Color::Gray | Color::White => "#edf0dc".into(),
                        Color::DarkGray => "#94a8a6".into(),
                        _ => String::new(),
                    };
                    serde_json::json!({"symbol":cell.symbol(), "fg":rgb(cell.fg), "bg":rgb(cell.bg)})
                }).collect();
                let data = serde_json::json!({"width":width, "height":height, "cells":cells});
                std::fs::write(
                    dir.join(format!("bear-{width}x{height}.json")),
                    serde_json::to_vec(&data).unwrap(),
                )
                .unwrap();
            }
        }
        for (width, height) in [(20, 6), (48, 12), (79, 24)] {
            let layout = view_layout(Rect::new(0, 0, width, height), false);
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            let ui = Ui::new(true).unwrap();
            let frame = bear::Animator::new(Mood::Asleep).frame(0.0);
            terminal
                .draw(|f| ui.render(f, Mood::Asleep, &frame))
                .unwrap();
            assert!(layout.input.bottom() <= height);
        }
        let settings = view_layout(Rect::new(0, 0, 80, 24), true);
        assert!(settings.mascot.is_none());
        assert_eq!(settings.activity.width, 80);
    }
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
    fn no_color_disables_styles_and_settings_notices_stay_visible() {
        use ratatui::backend::TestBackend;
        assert_eq!(paint(false, Color::Red), Color::Reset);
        assert_eq!(paint(true, Color::Red), Color::Red);
        let mut ui = Ui::new(true).unwrap();
        ui.settings = Some("SETTINGS\nrow".into());
        ui.notice = "Could not save: bad value".into();
        let frame = bear::Animator::new(Mood::Idle).frame(0.0);
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|f| ui.render(f, Mood::Idle, &frame)).unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(text.contains("Could not save"));
    }
    #[test]
    fn settings_selection_stays_visible_in_short_terminals() {
        assert_eq!(settings_scroll(0, Some(17), 8, 25), 12);
        assert_eq!(settings_scroll(12, Some(2), 8, 25), 2);
        assert!(
            status_lines("GREEN: conversation open | idle | codex · luna", 80)
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
    fn paste_keeps_message_lines_and_limits_credentials_to_one() {
        let mut input = String::new();
        apply_paste(&mut input, "  sk-test-key\nignored\n", true);
        assert_eq!(input, "sk-test-key");
        let mut input = String::new();
        apply_paste(&mut input, "first line\nsecond line\n", false);
        assert_eq!(input, "first line second line");
    }

    #[test]
    fn typing_edits_at_the_cursor() {
        let mut ui = Ui::new(true).unwrap();
        for c in "helo world".chars() {
            ui.insert_char(c);
        }
        assert_eq!(ui.input, "helo world");
        // Insert the missing l in "hello".
        ui.cursor = 3;
        ui.insert_char('l');
        assert_eq!(ui.input, "hello world");
        ui.cursor = 0;
        ui.delete_after();
        assert_eq!(ui.input, "ello world");
        ui.cursor = ui.input.chars().count();
        ui.delete_before();
        assert_eq!(ui.input, "ello worl");
        ui.cursor = ui.input.chars().count();
        ui.delete_word_before();
        assert_eq!(ui.input, "ello ");
        ui.cursor = 0;
        ui.insert_char('W');
        assert_eq!(ui.input, "Wello ");
    }
}
