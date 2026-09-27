//! Persistent, reviewable notifications raised by watches and the agent.
//!
//! A notification is user-facing data, never an instruction: it is stored
//! privately, capped, and can be listed, read, or dismissed. The file is small
//! and rewritten atomically under an exclusive lock, like the organizer.
use crate::config;
use anyhow::{ensure, Context, Result};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs::{self, File, OpenOptions},
    path::PathBuf,
};

/// Keep the newest notifications; older ones are dropped.
pub const LIMIT: usize = 100;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    pub id: String,
    pub title: String,
    pub text: String,
    pub at_unix: u64,
    /// Where it came from, e.g. `watch:<task id>` or `agent`.
    pub source: String,
    #[serde(default)]
    pub unread: bool,
    /// Optional Jev relevance of a watch finding, 0–1.
    #[serde(default)]
    pub relevance: Option<f32>,
    /// The raise requested a spoken summary.
    #[serde(default)]
    pub speak: bool,
    /// The live app has already dinged/spoken this one.
    #[serde(default)]
    pub announced: bool,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Book {
    items: Vec<Notification>,
}

fn path() -> Result<PathBuf> {
    Ok(config::home()?.join("notifications.json"))
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
        .open(home.join("notifications.lock"))?;
    file.lock_exclusive()?;
    Ok(file)
}

fn load() -> Result<Book> {
    let path = path()?;
    if !path.exists() {
        return Ok(Book::default());
    }
    serde_json::from_slice(&fs::read(&path)?)
        .with_context(|| format!("Invalid notification data: {}", path.display()))
}

fn save(book: &Book) -> Result<()> {
    config::save_private(&path()?, &serde_json::to_vec_pretty(book)?)
}

/// Store a new unread notification and return it.
pub fn add(
    title: &str,
    text: &str,
    source: &str,
    relevance: Option<f32>,
    speak: bool,
) -> Result<Notification> {
    let text = text.trim();
    ensure!(!text.is_empty(), "Notification text is empty");
    let _lock = lock()?;
    let mut book = load()?;
    let item = Notification {
        id: uuid::Uuid::new_v4().to_string()[..8].into(),
        title: {
            let title = title.trim();
            if title.is_empty() {
                "Notification".into()
            } else {
                title.chars().take(120).collect()
            }
        },
        text: text.chars().take(4000).collect(),
        at_unix: crate::organizer::now_unix(),
        source: source.trim().chars().take(120).collect(),
        unread: true,
        relevance,
        speak,
        announced: false,
    };
    book.items.push(item.clone());
    if book.items.len() > LIMIT {
        let excess = book.items.len() - LIMIT;
        book.items.drain(0..excess);
    }
    save(&book)?;
    Ok(item)
}

/// Notifications the live app has not announced yet, oldest first.
pub fn pending() -> Result<Vec<Notification>> {
    let mut items: Vec<Notification> = load()?
        .items
        .into_iter()
        .filter(|item| !item.announced)
        .collect();
    items.sort_by_key(|item| item.at_unix);
    Ok(items)
}

pub fn mark_announced(id: &str) -> Result<()> {
    let _lock = lock()?;
    let mut book = load()?;
    let mut changed = false;
    for item in &mut book.items {
        if item.id == id && !item.announced {
            item.announced = true;
            changed = true;
        }
    }
    if changed {
        save(&book)?;
    }
    Ok(())
}

/// Newest first.
pub fn list(unread_only: bool, limit: usize) -> Result<Vec<Notification>> {
    let book = load()?;
    let mut items: Vec<Notification> = book
        .items
        .into_iter()
        .filter(|item| !unread_only || item.unread)
        .collect();
    items.reverse();
    items.truncate(limit.clamp(1, LIMIT));
    Ok(items)
}

pub fn unread_count() -> Result<usize> {
    Ok(load()?.items.iter().filter(|item| item.unread).count())
}

/// Mark one notification read and return it.
pub fn read(id: &str) -> Result<Option<Notification>> {
    let _lock = lock()?;
    let mut book = load()?;
    let found = book.items.iter_mut().find(|item| item.id == id);
    match found {
        Some(item) => {
            item.unread = false;
            let item = item.clone();
            save(&book)?;
            Ok(Some(item))
        }
        None => Ok(None),
    }
}

/// Mark every notification read; returns how many changed.
pub fn read_all() -> Result<usize> {
    let _lock = lock()?;
    let mut book = load()?;
    let mut changed = 0;
    for item in &mut book.items {
        if item.unread {
            item.unread = false;
            changed += 1;
        }
    }
    if changed > 0 {
        save(&book)?;
    }
    Ok(changed)
}

/// Permanently remove one notification.
pub fn dismiss(id: &str) -> Result<bool> {
    let _lock = lock()?;
    let mut book = load()?;
    let before = book.items.len();
    book.items.retain(|item| item.id != id);
    let removed = book.items.len() != before;
    if removed {
        save(&book)?;
    }
    Ok(removed)
}

pub fn json_item(item: &Notification) -> Value {
    json!({
        "id": item.id,
        "title": item.title,
        "text": item.text,
        "at_unix": item.at_unix,
        "source": item.source,
        "unread": item.unread,
        "relevance": item.relevance,
        "speak": item.speak,
    })
}

pub fn json_list(unread_only: bool, limit: usize) -> Result<Value> {
    let items = list(unread_only, limit)?;
    Ok(json!({
        "unread": unread_count()?,
        "items": items.iter().map(json_item).collect::<Vec<_>>(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_item_carries_the_review_fields() {
        let item = Notification {
            id: "abc12345".into(),
            title: "Build failed".into(),
            text: "The nightly build failed on main.".into(),
            at_unix: 1,
            source: "watch:deadbeef".into(),
            unread: true,
            relevance: Some(0.5),
            speak: true,
            announced: false,
        };
        let value = json_item(&item);
        assert_eq!(value["id"], "abc12345");
        assert_eq!(value["unread"], true);
        assert_eq!(value["relevance"], 0.5);
    }
}
