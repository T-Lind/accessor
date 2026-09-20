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
    StopAlarm,
    ListSchedules,
    DeleteSchedule {
        id: String,
    },
    UpdateSchedule {
        id: String,
        changes: TaskPatch,
    },
    Delegate {
        prompt: String,
        #[serde(default)]
        role: Option<String>,
        harness: String,
        model: String,
        reasoning: String,
    },
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
        #[serde(default = "low_reasoning")]
        reasoning: String,
    },
}

fn low_reasoning() -> String {
    "low".into()
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct TaskPatch {
    pub prompt: Option<String>,
    pub label: Option<String>,
    pub delay_seconds: Option<u64>,
    pub at_unix: Option<u64>,
    /// Zero removes repetition; omitted preserves it.
    pub every_seconds: Option<u64>,
    pub harness: Option<String>,
    pub model: Option<String>,
    pub reasoning: Option<String>,
    pub paused: Option<bool>,
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
    #[serde(default = "low_reasoning")]
    pub reasoning: String,
    #[serde(default)]
    pub paused: bool,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ScheduleBook {
    alarms: Vec<Alarm>,
    tasks: Vec<Task>,
    runs: Vec<RunRecord>,
}

#[derive(Serialize, Deserialize)]
struct RunRecord {
    id: String,
    at_unix: u64,
    state: String,
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
    let mut book: ScheduleBook = serde_json::from_slice(&fs::read(&path)?)
        .with_context(|| format!("Invalid organizer data: {}", path.display()))?;
    let mut migrated = false;
    for task in &mut book.tasks {
        if task.harness.is_none() || task.model.is_none() {
            let settings = config::Settings::load()?;
            let harness = task
                .harness
                .get_or_insert_with(|| settings.routing.main.clone());
            task.model
                .get_or_insert_with(|| config::light_model(harness).into());
            migrated = true;
        }
        validate_task(task)?;
    }
    if migrated {
        save(&book)?;
    }
    Ok(book)
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
    if matches!(control.get("action")?.as_str()?, "sleep" | "list_schedules") && control.len() != 1
    {
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

#[allow(clippy::too_many_arguments)]
pub fn add_task(
    prompt: &str,
    label: Option<&str>,
    delay: Option<u64>,
    at: Option<u64>,
    every: Option<u64>,
    harness: Option<&str>,
    model: Option<&str>,
    reasoning: &str,
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
        reasoning: reasoning.into(),
        paused: false,
    };
    validate_task(&task)?;
    book.tasks.push(task.clone());
    save(&book)?;
    Ok(task)
}

pub fn validate_execution(harness: &str, model: &str, reasoning: &str) -> Result<()> {
    ensure!(
        ["codex", "claude", "antigravity", "mock"].contains(&harness),
        "Unknown harness"
    );
    ensure!(
        !model.is_empty()
            && model != "default"
            && model.len() <= 128
            && model
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-._/:".contains(c)),
        "Specify an explicit model ID or supported alias"
    );
    ensure!(
        ["low", "medium", "high"].contains(&reasoning),
        "Reasoning must be low, medium, or high"
    );
    Ok(())
}

fn validate_task(task: &Task) -> Result<()> {
    ensure!(
        !task.prompt.trim().is_empty() && task.prompt.len() <= 32_000,
        "Task prompt must contain 1–32000 characters"
    );
    ensure!(
        task.every_seconds.is_none_or(|s| s >= 60),
        "Repeating tasks must be at least 60 seconds apart"
    );
    validate_execution(
        task.harness
            .as_deref()
            .context("A scheduled task requires a harness")?,
        task.model
            .as_deref()
            .context("A scheduled task requires a model")?,
        &task.reasoning,
    )
}

fn apply_patch(task: &mut Task, patch: TaskPatch) -> Result<()> {
    if let Some(v) = patch.prompt {
        task.prompt = v;
    }
    if let Some(v) = patch.label {
        task.label = v.chars().take(120).collect();
    }
    if patch.delay_seconds.is_some() || patch.at_unix.is_some() {
        task.next_unix = due_time(patch.delay_seconds, patch.at_unix)?;
    }
    if let Some(v) = patch.every_seconds {
        task.every_seconds = (v != 0).then_some(v);
    }
    if patch
        .harness
        .as_ref()
        .is_some_and(|h| Some(h) != task.harness.as_ref())
    {
        ensure!(
            patch.model.is_some(),
            "Changing harness also requires its model"
        );
    }
    if let Some(v) = patch.harness {
        task.harness = Some(v);
    }
    if let Some(v) = patch.model {
        task.model = Some(v);
    }
    if let Some(v) = patch.reasoning {
        task.reasoning = v;
    }
    if let Some(v) = patch.paused {
        task.paused = v;
    }
    validate_task(task)
}

pub fn update(id: &str, patch: TaskPatch) -> Result<Task> {
    require_id(id)?;
    let _lock = lock()?;
    let mut book = load()?;
    let task = book
        .tasks
        .iter_mut()
        .find(|t| t.id == id)
        .context("No matching pending task")?;
    apply_patch(task, patch)?;
    let result = task.clone();
    save(&book)?;
    Ok(result)
}

pub fn claim_due(eligible: impl FnMut(&Task) -> bool) -> Result<Vec<Due>> {
    let _lock = lock()?;
    let mut book = load()?;
    let now = now_unix();
    let due = take_due(&mut book, now, eligible);
    if !due.is_empty() {
        save(&book)?;
    }
    Ok(due)
}

fn take_due(
    book: &mut ScheduleBook,
    now: u64,
    mut eligible: impl FnMut(&Task) -> bool,
) -> Vec<Due> {
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
        if task.paused || task.next_unix > now || !eligible(task) {
            return true;
        }
        due.push(Due::Task(task.clone()));
        book.runs.push(RunRecord {
            id: task.id.clone(),
            at_unix: now,
            state: "claimed; outcome unknown until completion".into(),
        });
        if let Some(every) = task.every_seconds {
            let steps = (now - task.next_unix) / every.max(1) + 1;
            task.next_unix = task.next_unix.saturating_add(steps.saturating_mul(every));
            true
        } else {
            false
        }
    });
    if book.runs.len() > 100 {
        book.runs.drain(..book.runs.len() - 100);
    }
    due
}

pub fn finish_run(id: &str, state: &str) -> Result<()> {
    let _lock = lock()?;
    let mut book = load()?;
    if let Some(run) = book.runs.iter_mut().rev().find(|r| r.id == id) {
        run.state = state.into();
        save(&book)?;
    }
    Ok(())
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
            "Task {} · {} · next Unix {}{} · {} / {} · {} · paused={}\n  Instructions: {}",
            task.id,
            task.label,
            task.next_unix,
            task.every_seconds
                .map(|n| format!(" · every {n}s"))
                .unwrap_or_default(),
            task.harness.as_deref().unwrap_or("auto"),
            task.model.as_deref().unwrap_or("default"),
            task.reasoning,
            task.paused,
            task.prompt
        ));
    }
    for run in book.runs.iter().rev().take(10) {
        lines.push(format!(
            "Run {} · Unix {} · {}",
            run.id, run.at_unix, run.state
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
    r#"Accessor local controls use strict one-line JSON in a final reply. When organizer MCP tools are available, prefer them for durable notes/timers/schedules and use their returned receipts; never also emit a duplicate directive. Prefer session_control MCP for sleep and stop_alarm; it reaches the running session and returns a receipt. The JSON forms are compatibility fallbacks when the MCP tool is unavailable. Emit controls only for user-authorized actions. Do not claim success until Accessor returns the actual result. Timers use alarm; stop_alarm silences the currently ringing alarm without cancelling unrelated future timers; notes are private Markdown. Schedules run only while Accessor is running. Every task MUST specify a harness, explicit model, and low/medium/high reasoning. Timing uses exactly one of delay_seconds or at_unix (Unix seconds). Repeats are elapsed seconds, minimum 60, not timezone/calendar recurrence. Ask for a timezone if an absolute clock time is ambiguous. List before editing/deleting when the ID is unknown. Updates preserve omitted fields; every_seconds:0 removes repetition; paused:true/false pauses/resumes. Changing harness also requires a model. Delete supports a specific ID; use all only when explicitly requested. Never repeat a successful control. Sleep ends active listening while keeping the wake detector local.
{"accessor":{"action":"sleep"}}
{"accessor":{"action":"stop_alarm"}}
{"accessor":{"action":"note","title":"workshop","text":"Filter is 20 by 25"}}
{"accessor":{"action":"alarm","label":"tea","delay_seconds":300}}
{"accessor":{"action":"schedule","label":"summary","prompt":"Summarize project status","delay_seconds":3600,"every_seconds":86400,"harness":"codex","model":"gpt-5.6-luna","reasoning":"low"}}
{"accessor":{"action":"list_schedules"}}
{"accessor":{"action":"update_schedule","id":"0123abcd","changes":{"delay_seconds":7200,"prompt":"Updated instructions","harness":"claude","model":"sonnet","reasoning":"medium","paused":false}}}
{"accessor":{"action":"delete_schedule","id":"0123abcd"}}
"#.into()
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
            runs: vec![],
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
                reasoning: "low".into(),
                paused: false,
            }],
        };
        let due = take_due(&mut book, 100, |_| true);
        assert_eq!(due.len(), 2);
        assert!(book.alarms.is_empty());
        assert_eq!(book.tasks[0].next_unix, 110);
    }

    fn sample_task() -> Task {
        Task {
            id: "0123abcd".into(),
            label: "Report".into(),
            prompt: "Original instructions".into(),
            next_unix: 50,
            every_seconds: Some(60),
            harness: Some("codex".into()),
            model: Some("gpt-5.6-luna".into()),
            reasoning: "low".into(),
            paused: false,
        }
    }

    #[test]
    fn task_patch_preserves_fields_and_requires_model_on_harness_change() {
        let mut task = sample_task();
        apply_patch(
            &mut task,
            TaskPatch {
                prompt: Some("New instructions".into()),
                paused: Some(true),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(task.model.as_deref(), Some("gpt-5.6-luna"));
        assert_eq!(task.next_unix, 50);
        assert!(task.paused);
        assert!(apply_patch(
            &mut task,
            TaskPatch {
                harness: Some("claude".into()),
                ..Default::default()
            }
        )
        .is_err());
        apply_patch(
            &mut task,
            TaskPatch {
                harness: Some("claude".into()),
                model: Some("sonnet".into()),
                reasoning: Some("high".into()),
                every_seconds: Some(0),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(task.every_seconds, None);
        assert_eq!(task.reasoning, "high");
        task.model = None;
        assert!(validate_task(&task).is_err());
    }

    #[test]
    fn unavailable_and_paused_tasks_remain_pending() {
        let mut book = ScheduleBook {
            tasks: vec![sample_task()],
            ..Default::default()
        };
        assert!(take_due(&mut book, 100, |_| false).is_empty());
        assert_eq!(book.tasks[0].next_unix, 50);
        book.tasks[0].paused = true;
        assert!(take_due(&mut book, 100, |_| true).is_empty());
        book.tasks[0].paused = false;
        assert_eq!(take_due(&mut book, 100, |_| true).len(), 1);
        assert_eq!(book.runs.len(), 1);
        assert!(book.runs[0].state.contains("unknown"));
    }

    #[test]
    fn malformed_edit_is_not_executed() {
        let (_, controls) = take_directives(
            r#"{"accessor":{"action":"update_schedule","id":"0123abcd","changes":{"modle":"typo"}}}"#,
        );
        assert!(controls.is_empty());
    }
}
