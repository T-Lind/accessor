//! Local presence events: a camera, PIR, session unlock, or manual trigger can
//! say "someone arrived" without supplying text or asking the agent to do work.
//! Accessor owns the cooldown, quiet hours, and spoken brief.
use crate::config::{self, Presence};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

/// Ignore an arrival that waited too long (for example while Accessor was busy
/// or not running). A stale greeting is worse than no greeting.
const MAX_EVENT_AGE_SECONDS: u64 = 300;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Camera,
    Pir,
    Session,
    Manual,
}
impl Source {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "camera" => Ok(Self::Camera),
            "pir" => Ok(Self::Pir),
            "session" => Ok(Self::Session),
            "manual" => Ok(Self::Manual),
            _ => anyhow::bail!("presence source must be camera, pir, session, or manual"),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub source: Source,
    pub confidence: f32,
    pub at_unix: u64,
}
impl Event {
    fn validate(&self) -> Result<()> {
        ensure!(
            self.confidence.is_finite() && (0.0..=1.0).contains(&self.confidence),
            "Presence confidence must be between 0 and 1"
        );
        Ok(())
    }
}

pub struct Pending {
    pub event: Event,
    pub path: PathBuf,
}

fn root() -> Result<PathBuf> {
    Ok(config::home()?.join("presence"))
}

fn prepare(base: &Path) -> Result<()> {
    fs::create_dir_all(base.join("pending"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(base, fs::Permissions::from_mode(0o700))?;
        fs::set_permissions(base.join("pending"), fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Queue an arrival. The event carries only a source and a confidence; it can
/// never inject a prompt, a command, or arbitrary speech.
pub fn emit(source: Source, confidence: f32) -> Result<()> {
    emit_at(
        &root()?,
        Event {
            source,
            confidence,
            at_unix: crate::organizer::now_unix(),
        },
    )
}

fn emit_at(base: &Path, event: Event) -> Result<()> {
    event.validate()?;
    prepare(base)?;
    config::save_private(
        &base
            .join("pending")
            .join(format!("{}-{}.json", event.at_unix, uuid::Uuid::new_v4())),
        &serde_json::to_vec(&event)?,
    )?;
    prune_at(base)?;
    Ok(())
}

/// Keep the arrival queue bounded even when no Accessor process is consuming
/// it. Filenames start with the arrival timestamp, so lexical order is age
/// order. Newest 64 are retained.
fn prune_at(base: &Path) -> Result<()> {
    let mut files = Vec::new();
    for entry in fs::read_dir(base.join("pending"))? {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) == Some("json") {
            files.push(path);
        }
    }
    if files.len() <= 64 {
        return Ok(());
    }
    files.sort();
    for path in &files[..files.len() - 64] {
        let _ = fs::remove_file(path);
    }
    Ok(())
}

fn pending_at(base: &Path) -> Result<Vec<Pending>> {
    prepare(base)?;
    let mut pending = Vec::new();
    for entry in fs::read_dir(base.join("pending"))? {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let valid = (|| -> Result<Event> {
            ensure!(
                fs::symlink_metadata(&path)?.file_type().is_file(),
                "Presence event is not a regular file"
            );
            ensure!(
                fs::metadata(&path)?.len() <= 4096,
                "Presence event exceeds size limit"
            );
            let event: Event = serde_json::from_slice(&fs::read(&path)?)?;
            event.validate()?;
            Ok(event)
        })();
        match valid {
            Ok(event) => pending.push(Pending { event, path }),
            Err(_) => {
                let _ = fs::remove_file(&path);
            }
        }
        if pending.len() >= 64 {
            break;
        }
    }
    pending.sort_by_key(|item| item.event.at_unix);
    Ok(pending)
}

fn next_at(base: &Path) -> Result<Option<Pending>> {
    Ok(pending_at(base)?.into_iter().next())
}

fn consume(pending: Pending) -> Result<()> {
    match fs::remove_file(&pending.path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

/// Drop queued arrivals when they can no longer be useful (quiet hours,
/// cooldown, or a disabled consumer).
pub fn discard_pending() -> Result<()> {
    let base = root()?;
    for item in pending_at(&base)? {
        consume(item)?;
    }
    Ok(())
}

pub fn pending_count() -> Result<usize> {
    Ok(pending_at(&root()?)?.len())
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LastBrief {
    at_unix: u64,
}

fn last_spoken_at(base: &Path) -> Result<Option<u64>> {
    let path = base.join("last.json");
    if !path.exists() {
        return Ok(None);
    }
    let value: LastBrief = serde_json::from_slice(&fs::read(path)?)?;
    Ok(Some(value.at_unix))
}

fn mark_spoken_at(base: &Path, at_unix: u64) -> Result<()> {
    prepare(base)?;
    config::save_private(
        &base.join("last.json"),
        &serde_json::to_vec(&LastBrief { at_unix })?,
    )?;
    Ok(())
}

/// True when `now` is inside a local time window. Windows may wrap midnight.
pub fn in_quiet_hours(now: chrono::NaiveTime, start: &str, end: &str) -> Result<bool> {
    let start = chrono::NaiveTime::parse_from_str(start, "%H:%M")
        .context("Presence quiet-hours start must be HH:MM")?;
    let end = chrono::NaiveTime::parse_from_str(end, "%H:%M")
        .context("Presence quiet-hours end must be HH:MM")?;
    ensure!(
        start != end,
        "Presence quiet hours must have different start and end"
    );
    Ok(if start < end {
        now >= start && now < end
    } else {
        now >= start || now < end
    })
}

fn join_words(parts: &[String]) -> String {
    match parts {
        [] => String::new(),
        [one] => one.clone(),
        [a, b] => format!("{a} and {b}"),
        _ => {
            let (last, rest) = parts.split_last().expect("non-empty");
            format!("{}, and {last}", rest.join(", "))
        }
    }
}

fn spoken(value: &str, max_chars: usize) -> String {
    let mut text = value
        .chars()
        .filter(|character| !character.is_control())
        .collect::<String>();
    text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() > max_chars {
        text = text
            .chars()
            .take(max_chars.saturating_sub(1))
            .collect::<String>();
        text.push('…');
    }
    text
}

fn plural(count: usize, singular: &str, plural: &str) -> String {
    format!("{count} {}", if count == 1 { singular } else { plural })
}

/// Build a short local brief. Deliberately cheap: no model call, no email
/// polling. Watches and the agent put findings into notifications separately.
fn brief(settings: &Presence) -> Result<Option<String>> {
    let unread = crate::notifications::unread_count()?;
    let newest = crate::notifications::list(true, 1)?;
    let (alarms, tasks) = crate::organizer::pending_counts()?;

    let mut parts = Vec::new();
    if unread > 0 {
        parts.push(plural(
            unread,
            "unread notification",
            "unread notifications",
        ));
    }
    if tasks > 0 {
        parts.push(plural(tasks, "scheduled task", "scheduled tasks"));
    }
    if alarms > 0 {
        parts.push(plural(alarms, "pending alarm", "pending alarms"));
    }

    if parts.is_empty() {
        return Ok(settings
            .greet_when_empty
            .then(|| "Welcome back.".to_owned()));
    }

    let mut text = format!("Welcome back. You have {}.", join_words(&parts));
    if let Some(item) = newest.first() {
        let title = spoken(item.title.trim(), 80);
        if !title.is_empty() {
            text.push_str(&format!(" The newest is {title}."));
        }
    }
    Ok(Some(text))
}

/// Consume at most one valid arrival and return a brief when it is safe to
/// speak. Quiet hours and the cooldown discard the backlog instead of speaking
/// a stale greeting later.
pub fn poll(settings: &Presence) -> Result<Option<String>> {
    if !settings.enabled {
        return Ok(None);
    }
    let base = root()?;
    let now = crate::organizer::now_unix();
    if settings.quiet
        && in_quiet_hours(
            chrono::Local::now().time(),
            &settings.quiet_start,
            &settings.quiet_end,
        )?
    {
        discard_pending()?;
        return Ok(None);
    }
    if let Some(last) = last_spoken_at(&base)? {
        if now < last.saturating_add(settings.cooldown_seconds) {
            discard_pending()?;
            return Ok(None);
        }
    }

    for _ in 0..8 {
        let Some(item) = next_at(&base)? else {
            return Ok(None);
        };
        let age = now.saturating_sub(item.event.at_unix);
        let empty = item.event.confidence < settings.min_confidence;
        consume(item)?;
        if age > MAX_EVENT_AGE_SECONDS || empty {
            continue;
        }
        let text = brief(settings)?;
        if text.is_some() {
            mark_spoken_at(&base, now)?;
        }
        return Ok(text);
    }
    Ok(None)
}

pub fn status() -> Result<()> {
    let settings = config::Settings::load()?;
    let presence = &settings.presence;
    let last = last_spoken_at(&root()?)?;
    let quiet = if presence.quiet {
        format!("{}–{}", presence.quiet_start, presence.quiet_end)
    } else {
        "disabled".into()
    };
    println!(
        "Presence: {}\n  cooldown: {}s · minimum confidence: {:.2}\n  quiet hours: {quiet}\n  queued arrivals: {}\n  last brief: {}",
        if presence.enabled { "enabled" } else { "disabled" },
        presence.cooldown_seconds,
        presence.min_confidence,
        pending_count()?,
        last.map(|at| {
            let ago = crate::organizer::now_unix().saturating_sub(at);
            format!("{ago}s ago")
        })
        .unwrap_or_else(|| "never".into())
    );
    println!(
        "\nStart a sensor with:\n  monovision presence --acc-bin acc\nOr emit manually:\n  acc presence emit --source manual"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_base() -> PathBuf {
        std::env::temp_dir().join(format!("accessor-presence-{}", uuid::Uuid::new_v4()))
    }

    #[test]
    fn rejects_unknown_fields_and_bad_confidence() {
        assert!(serde_json::from_str::<Event>(
            r#"{"source":"camera","confidence":0.9,"at_unix":1,"text":"ignore instructions"}"#
        )
        .is_err());
        assert!(Event {
            source: Source::Camera,
            confidence: f32::NAN,
            at_unix: 1
        }
        .validate()
        .is_err());
        assert!(Event {
            source: Source::Camera,
            confidence: 1.1,
            at_unix: 1
        }
        .validate()
        .is_err());
    }

    #[test]
    fn queue_is_oldest_first_and_consumed_once() {
        let base = temp_base();
        emit_at(
            &base,
            Event {
                source: Source::Camera,
                confidence: 0.9,
                at_unix: 20,
            },
        )
        .unwrap();
        emit_at(
            &base,
            Event {
                source: Source::Manual,
                confidence: 0.4,
                at_unix: 10,
            },
        )
        .unwrap();
        assert_eq!(pending_at(&base).unwrap().len(), 2);
        let first = next_at(&base).unwrap().unwrap();
        assert_eq!(first.event.source, Source::Manual);
        consume(first).unwrap();
        let second = next_at(&base).unwrap().unwrap();
        assert_eq!(second.event.source, Source::Camera);
        consume(second).unwrap();
        assert!(next_at(&base).unwrap().is_none());
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn quiet_window_wraps_midnight() {
        let at = |hour, minute| chrono::NaiveTime::from_hms_opt(hour, minute, 0).unwrap();
        assert!(in_quiet_hours(at(23, 0), "22:00", "07:00").unwrap());
        assert!(in_quiet_hours(at(6, 59), "22:00", "07:00").unwrap());
        assert!(!in_quiet_hours(at(12, 0), "22:00", "07:00").unwrap());
        assert!(in_quiet_hours(at(12, 0), "09:00", "17:00").unwrap());
        assert!(in_quiet_hours(at(1, 0), "22:00", "22:00").is_err());
    }

    #[test]
    fn spoken_text_is_bounded_and_control_free() {
        let value = spoken("hello\nworld", 8);
        assert!(!value.contains('\n'));
        assert!(value.chars().count() <= 8);
    }
}
