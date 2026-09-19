use crate::config;
use anyhow::{bail, ensure, Context, Result};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs::{self, File, OpenOptions},
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Directive {
    Sleep,
    Note {
        text: String,
        #[serde(default)]
        title: Option<String>,
    },
    Alarm {
        #[serde(default)]
        label: Option<String>,
        #[serde(default)]
        delay_seconds: Option<u64>,
        #[serde(default)]
        at_unix: Option<u64>,
    },
    Schedule {
        prompt: String,
        #[serde(default)]
        label: Option<String>,
        #[serde(default)]
        delay_seconds: Option<u64>,
        #[serde(default)]
        at_unix: Option<u64>,
        #[serde(default)]
        every_seconds: Option<u64>,
        #[serde(default)]
        harness: Option<String>,
        #[serde(default)]
        model: Option<String>,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    accessor: Directive,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alarm {
    pub id: String,
    pub label: String,
    pub at_unix: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub label: String,
    pub prompt: String,
    pub next_unix: u64,
    pub every_seconds: Option<u64>,
    pub harness: Option<String>,
    pub model: Option<String>,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ScheduleBook {
    alarms: Vec<Alarm>,
    tasks: Vec<Task>,
}

#[derive(Debug)]
pub enum Due {
    Alarm(Alarm),
    Task(Task),
}

pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn schedule_path() -> Result<PathBuf> {
    Ok(config::home()?.join("schedules.json"))
}

fn notes_dir() -> Result<PathBuf> {
    Ok(config::home()?.join("notes"))
}

fn lock() -> Result<File> {
    let home = config::home()?;
    fs::create_dir_all(&home)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&home, fs::Permissions::from_mode(0o700))?;
    }
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(home.join("organizer.lock"))?;
    file.lock_exclusive()?;
    Ok(file)
}

fn load() -> Result<ScheduleBook> {
    let path = schedule_path()?;
    if !path.exists() {
        return Ok(ScheduleBook::default());
    }
    serde_json::from_slice(&fs::read(&path)?)
        .with_context(|| format!("Invalid organizer data: {}", path.display()))
}

fn save(book: &ScheduleBook) -> Result<()> {
    config::save_private(&schedule_path()?, &serde_json::to_vec_pretty(book)?)
}

fn due_time(delay_seconds: Option<u64>, at_unix: Option<u64>) -> Result<u64> {
    ensure!(
        delay_seconds.is_some() ^ at_unix.is_some(),
        "Specify exactly one of delay_seconds or at_unix"
    );
    let at = at_unix.unwrap_or_else(|| now_unix().saturating_add(delay_seconds.unwrap_or(0)));
    ensure!(at > now_unix(), "Time must be in the future");
    Ok(at)
}

pub fn take_directives(text: &str) -> (String, Vec<Directive>) {
    let mut kept = Vec::new();
    let mut directives = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim().trim_matches('`');
        if trimmed == "ACCESSOR_SLEEP" {
            directives.push(Directive::Sleep);
        } else if let Some(directive) = parse_directive(trimmed) {
            directives.push(directive);
        } else {
            kept.push(line);
        }
    }
    (kept.join("\n").trim().to_owned(), directives)
}

fn parse_directive(line: &str) -> Option<Directive> {
    let value: Value = serde_json::from_str(line).ok()?;
    let control = value.get("accessor")?.as_object()?;
    let action = control.get("action")?.as_str()?;
    let allowed: &[&str] = match action {
        "sleep" => &["action"],
        "note" => &["action", "text", "title"],
        "alarm" => &["action", "label", "delay_seconds", "at_unix"],
        "schedule" => &[
            "action",
            "prompt",
            "label",
            "delay_seconds",
            "at_unix",
            "every_seconds",
            "harness",
            "model",
        ],
        _ => return None,
    };
    if control.keys().any(|key| !allowed.contains(&key.as_str())) {
        return None;
    }
    serde_json::from_value::<Envelope>(value)
        .ok()
        .map(|envelope| envelope.accessor)
}

pub fn add_note(text: &str, title: Option<&str>) -> Result<PathBuf> {
    let text = text.trim();
    ensure!(
        !text.is_empty() && text.len() <= 32_000,
        "Note must contain 1–32000 characters"
    );
    let dir = notes_dir()?;
    fs::create_dir_all(&dir)?;
    let title: String = title
        .unwrap_or("note")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(120)
        .collect();
    let slug: String = title
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .take(6)
        .collect::<Vec<_>>()
        .join("-");
    let path = dir.join(format!(
        "{}-{}-{}.md",
        now_unix(),
        if slug.is_empty() { "note" } else { &slug },
        &uuid::Uuid::new_v4().to_string()[..8]
    ));
    let body = format!(
        "# {}\n\n{}\n",
        if title.is_empty() { "Note" } else { &title },
        text
    );
    config::save_private(&path, body.as_bytes())?;
    Ok(path)
}

pub fn add_alarm(label: Option<&str>, delay: Option<u64>, at: Option<u64>) -> Result<Alarm> {
    let _lock = lock()?;
    let mut book = load()?;
    let alarm = Alarm {
        id: uuid::Uuid::new_v4().to_string()[..8].into(),
        label: label.unwrap_or("Alarm").trim().chars().take(120).collect(),
        at_unix: due_time(delay, at)?,
    };
    book.alarms.push(alarm.clone());
    save(&book)?;
    Ok(alarm)
}

pub fn add_task(
    prompt: &str,
    label: Option<&str>,
    delay: Option<u64>,
    at: Option<u64>,
    every: Option<u64>,
    harness: Option<&str>,
    model: Option<&str>,
) -> Result<Task> {
    let prompt = prompt.trim();
    ensure!(
        !prompt.is_empty() && prompt.len() <= 32_000,
        "Task prompt must contain 1–32000 characters"
    );
    if let Some(seconds) = every {
        ensure!(
            seconds >= 60,
            "Repeating tasks must be at least 60 seconds apart"
        );
    }
    if let Some(name) = harness {
        ensure!(
            ["codex", "claude", "antigravity", "mock"].contains(&name),
            "Unknown task harness"
        );
    }
    ensure!(
        model.is_none() || harness.is_some(),
        "A task model requires an explicit harness"
    );
    let _lock = lock()?;
    let mut book = load()?;
    let task = Task {
        id: uuid::Uuid::new_v4().to_string()[..8].into(),
        label: label
            .unwrap_or("Scheduled task")
            .trim()
            .chars()
            .take(120)
            .collect(),
        prompt: prompt.into(),
        next_unix: due_time(delay, at)?,
        every_seconds: every,
        harness: harness.map(str::to_owned),
        model: model.map(str::to_owned),
    };
    book.tasks.push(task.clone());
    save(&book)?;
    Ok(task)
}

pub fn claim_due() -> Result<Vec<Due>> {
    let _lock = lock()?;
    let mut book = load()?;
    let now = now_unix();
    let due = take_due(&mut book, now);
    if !due.is_empty() {
        save(&book)?;
    }
    Ok(due)
}

fn take_due(book: &mut ScheduleBook, now: u64) -> Vec<Due> {
    let mut due = Vec::new();
    book.alarms.retain(|alarm| {
        if alarm.at_unix <= now {
            due.push(Due::Alarm(alarm.clone()));
            false
        } else {
            true
        }
    });
    book.tasks.retain_mut(|task| {
        if task.next_unix > now {
            return true;
        }
        due.push(Due::Task(task.clone()));
        if let Some(every) = task.every_seconds {
            while task.next_unix <= now {
                task.next_unix = task.next_unix.saturating_add(every);
            }
            true
        } else {
            false
        }
    });
    due
}

pub fn list() -> Result<String> {
    let _lock = lock()?;
    let book = load()?;
    let mut lines = vec![format!("Notes folder: {}", notes_dir()?.display())];
    for alarm in book.alarms {
        lines.push(format!(
            "Alarm {} · {} · Unix {}",
            alarm.id, alarm.label, alarm.at_unix
        ));
    }
    for task in book.tasks {
        lines.push(format!(
            "Task {} · {} · next Unix {}{} · {} / {}",
            task.id,
            task.label,
            task.next_unix,
            task.every_seconds
                .map(|n| format!(" · every {n}s"))
                .unwrap_or_default(),
            task.harness.as_deref().unwrap_or("auto"),
            task.model.as_deref().unwrap_or("default")
        ));
    }
    if lines.len() == 1 {
        lines.push("No pending alarms or scheduled tasks.".into());
    }
    Ok(lines.join("\n"))
}

pub fn cancel(id: &str) -> Result<bool> {
    let _lock = lock()?;
    let mut book = load()?;
    let before = book.alarms.len() + book.tasks.len();
    if id == "all" {
        book.alarms.clear();
        book.tasks.clear();
    } else {
        book.alarms.retain(|item| item.id != id);
        book.tasks.retain(|item| item.id != id);
    }
    let changed = before != book.alarms.len() + book.tasks.len();
    if changed {
        save(&book)?;
    }
    Ok(changed)
}

pub fn guide() -> String {
    format!(
        "Accessor local controls are available through one-line JSON directives. Current Unix time is {}. Emit a directive only when the user explicitly asks for that action; include a concise plain-language acknowledgement before it. Accessor validates, applies, and removes the directive from the spoken reply. Sleep only when the user asks to end or sleep the voice session. Notes are private Markdown files in Accessor's notes folder. Alarms beep locally while Accessor is running and stop on ‘29 stop’. Scheduled tasks persist and run while Accessor is running; delay_seconds is relative to now, at_unix is absolute, every_seconds makes a task repeat (minimum 60). Use exactly one of delay_seconds or at_unix. Optional harness values: codex, claude, antigravity, mock. Omit harness/model for normal Jev routing. Formats:\n{{\"accessor\":{{\"action\":\"sleep\"}}}}\n{{\"accessor\":{{\"action\":\"note\",\"title\":\"short title\",\"text\":\"note body\"}}}}\n{{\"accessor\":{{\"action\":\"alarm\",\"label\":\"tea\",\"delay_seconds\":300}}}}\n{{\"accessor\":{{\"action\":\"schedule\",\"label\":\"daily summary\",\"prompt\":\"Summarize...\",\"delay_seconds\":3600,\"every_seconds\":86400,\"harness\":\"codex\",\"model\":\"gpt-5.6-sol\"}}}}",
        now_unix()
    )
}

pub fn require_id(id: &str) -> Result<()> {
    if id == "all" || (id.len() == 8 && id.chars().all(|c| c.is_ascii_hexdigit())) {
        Ok(())
    } else {
        bail!("Use an 8-character organizer ID or 'all'")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_only_strict_control_lines() {
        let (text, controls) = take_directives(
            "Done.\n{\"accessor\":{\"action\":\"note\",\"text\":\"milk\"}}\nnot json",
        );
        assert_eq!(text, "Done.\nnot json");
        assert_eq!(controls.len(), 1);
        assert!(matches!(&controls[0], Directive::Note { text, .. } if text == "milk"));
        let (_, controls) =
            take_directives("{\"accessor\":{\"action\":\"sleep\",\"unexpected\":true}}");
        assert!(controls.is_empty());
    }

    #[test]
    fn due_recurring_task_runs_once_and_advances() {
        let mut book = ScheduleBook {
            alarms: vec![Alarm {
                id: "alarm123".into(),
                label: "Tea".into(),
                at_unix: 90,
            }],
            tasks: vec![Task {
                id: "task1234".into(),
                label: "Status".into(),
                prompt: "Report".into(),
                next_unix: 50,
                every_seconds: Some(20),
                harness: None,
                model: None,
            }],
        };
        let due = take_due(&mut book, 100);
        assert_eq!(due.len(), 2);
        assert!(book.alarms.is_empty());
        assert_eq!(book.tasks[0].next_unix, 110);
    }
}
