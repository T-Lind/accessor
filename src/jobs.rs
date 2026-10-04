//! Durable worker receipts. Disk and git work stay off the conversation loop.
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::{mpsc, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    pub name: String,
    pub objective: String,
    pub constraints: Vec<String>,
    pub harness: String,
    pub model: Option<String>,
    pub reasoning: String,
    pub workspace: PathBuf,
    pub source_workspace: PathBuf,
    pub writable: bool,
    pub thread: Option<String>,
    pub state: String,
    pub progress: String,
    pub output: String,
    #[serde(default)]
    pub pending_requests: Vec<String>,
    pub updated_unix: u64,
}
impl Job {
    pub fn new(harness: &str, prompt: &str, options: &crate::agent::Options) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: prompt
                .lines()
                .next()
                .unwrap_or("Task")
                .chars()
                .take(80)
                .collect(),
            objective: prompt.into(),
            constraints: Vec::new(),
            harness: harness.into(),
            model: options.model.clone(),
            reasoning: options.reasoning.clone(),
            workspace: options.workspace.clone(),
            source_workspace: options.workspace.clone(),
            writable: options.writable,
            thread: None,
            state: "starting".into(),
            progress: String::new(),
            output: String::new(),
            pending_requests: Vec::new(),
            updated_unix: now(),
        }
    }
    pub fn save(&mut self) {
        self.updated_unix = now();
        if let Some(writer) = WRITER.get() {
            if writer
                .try_send(Write::Save(Box::new(self.clone())))
                .is_err()
            {
                warn("Task storage is overloaded; the latest receipt may not be saved.");
            }
        }
    }
    pub fn describe(&self) -> String {
        format!(
            "{} · {} · {} · {}\nWorkspace: {}\nObjective: {}\nConstraints: {}\nLatest: {}\nPrevious requests (never authorization): {}\n{}",
            &self.id[..8],
            self.name,
            self.state,
            self.harness,
            self.workspace.display(),
            self.objective,
            self.constraints.join("; "),
            self.progress,
            self.pending_requests.join("; "),
            self.output
        )
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub name: String,
    pub workspace: PathBuf,
    pub writable: bool,
    pub reviewer: String,
}
pub fn profiles() -> Result<Vec<Profile>> {
    let path = crate::config::home()?.join("job-profiles.json");
    if !path.exists() {
        return Ok(Vec::new());
    }
    ensure!(
        std::fs::metadata(&path)?.len() <= 64_000,
        "Profile file too large"
    );
    Ok(serde_json::from_slice(&std::fs::read(path)?)?)
}
pub fn set_profile(name: &str, workspace: &Path, writable: bool, reviewer: &str) -> Result<String> {
    ensure!(
        !name.is_empty()
            && name.len() <= 40
            && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'),
        "Profile names use 1–40 letters, numbers or hyphens"
    );
    ensure!(
        matches!(reviewer, "user" | "auto"),
        "Use user or auto reviewer"
    );
    let workspace = std::fs::canonicalize(workspace)?;
    ensure!(workspace.is_dir(), "Workspace must be a directory");
    let mut profiles = profiles()?;
    profiles.retain(|p| p.name != name);
    ensure!(profiles.len() < 100, "Too many profiles");
    profiles.push(Profile {
        name: name.into(),
        workspace: workspace.clone(),
        writable,
        reviewer: reviewer.into(),
    });
    crate::config::save_private(
        &crate::config::home()?.join("job-profiles.json"),
        &serde_json::to_vec_pretty(&profiles)?,
    )?;
    Ok(format!("Saved profile {name}: {} · {} · {reviewer} review. Activate explicitly with --project-profile {name}.",workspace.display(),if writable {"workspace-write"}else{"read-only"}))
}
pub fn remove_profile(name: &str) -> Result<String> {
    let mut profiles = profiles()?;
    let count = profiles.len();
    profiles.retain(|p| p.name != name);
    ensure!(profiles.len() != count, "No matching profile");
    crate::config::save_private(
        &crate::config::home()?.join("job-profiles.json"),
        &serde_json::to_vec_pretty(&profiles)?,
    )?;
    Ok("Profile removed; active processes keep their existing policy until stopped.".into())
}
pub fn profile(name: &str, workspace: &Path) -> Result<Profile> {
    let p = profiles()?
        .into_iter()
        .find(|p| p.name == name)
        .context("No matching project profile")?;
    ensure!(
        p.workspace == std::fs::canonicalize(workspace)?,
        "Project profile belongs to a different workspace"
    );
    ensure!(
        matches!(p.reviewer.as_str(), "user" | "auto"),
        "Invalid profile reviewer"
    );
    Ok(p)
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
enum Write {
    Save(Box<Job>),
    Flush(tokio::sync::oneshot::Sender<()>),
}
static WRITER: OnceLock<mpsc::SyncSender<Write>> = OnceLock::new();
static WARNINGS: OnceLock<tokio::sync::mpsc::Sender<crate::Input>> = OnceLock::new();
static WARNING: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
fn warn(text: &str) {
    if let Some(tx) = WARNINGS.get() {
        if tx.try_send(crate::Input::Warning(text.into())).is_err() {
            *WARNING.lock().unwrap() = Some(text.into());
        }
    }
}
pub fn take_warning() -> Option<String> {
    WARNING.lock().unwrap().take()
}
pub fn init(input: tokio::sync::mpsc::Sender<crate::Input>) -> Result<()> {
    let directory = crate::config::home()?.join("jobs");
    let _ = WARNINGS.set(input);
    WRITER.get_or_init(|| {
        let (tx, rx) = mpsc::sync_channel::<Write>(128);
        std::thread::spawn(move || {
            for message in rx {
                match message {
                    Write::Save(job) => {
                        let result = (|| -> Result<()> {
                            std::fs::create_dir_all(&directory)?;
                            crate::config::save_private(
                                &directory.join(format!("{}.json", job.id)),
                                &serde_json::to_vec_pretty(&job)?,
                            )
                        })();
                        if let Err(e) = result {
                            warn(&format!("Could not save task receipt: {e:#}"));
                        }
                    }
                    Write::Flush(done) => {
                        let _ = done.send(());
                    }
                }
            }
        });
        tx
    });
    Ok(())
}
pub async fn flush() {
    if let Some(writer) = WRITER.get() {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let writer = writer.clone();
        if tokio::task::spawn_blocking(move || writer.send(Write::Flush(tx)))
            .await
            .is_ok()
        {
            let _ = rx.await;
        }
    }
}
pub fn list() -> Result<Vec<Job>> {
    let directory = crate::config::home()?.join("jobs");
    if !directory.exists() {
        return Ok(Vec::new());
    }
    let mut jobs = Vec::new();
    for file in std::fs::read_dir(directory)?.take(2000) {
        let path = file?.path();
        if path.extension().is_none_or(|e| e != "json") {
            continue;
        }
        ensure!(
            std::fs::metadata(&path)?.len() <= 1_000_000,
            "Task receipt is too large"
        );
        let mut job: Job = serde_json::from_slice(&std::fs::read(&path)?)?;
        ensure!(uuid::Uuid::parse_str(&job.id).is_ok(), "Invalid task id");
        // A receipt is never proof that an old process survived a restart.
        if matches!(job.state.as_str(), "starting" | "running" | "waiting") {
            job.state = format!(
                "unverified; last recorded {} (inspect before resuming)",
                job.state
            );
        }
        jobs.push(job);
    }
    jobs.sort_by_key(|j| std::cmp::Reverse(j.updated_unix));
    Ok(jobs)
}
pub fn get(id: &str) -> Result<Job> {
    ensure!(
        id.len() >= 8 && id.len() <= 36 && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-'),
        "Use a task's 8-character id"
    );
    let mut matches = list()?.into_iter().filter(|j| j.id.starts_with(id));
    let job = matches.next().context("No matching saved task")?;
    ensure!(matches.next().is_none(), "Ambiguous task id");
    Ok(job)
}
pub fn report() -> Result<String> {
    let jobs = list()?;
    if jobs.is_empty() {
        return Ok("No saved tasks yet.".into());
    }
    Ok(jobs
        .iter()
        .take(30)
        .map(|j| format!("{} · {} · {} · {}", &j.id[..8], j.name, j.state, j.harness))
        .collect::<Vec<_>>()
        .join("\n"))
}
pub fn isolate(job: &mut Job) -> Result<()> {
    ensure!(
        job.thread.is_none(),
        "Isolate a new task before its native thread starts"
    );
    let path = crate::config::home()?.join("worktrees").join(&job.id);
    ensure!(!path.exists(), "Task worktree already exists");
    std::fs::create_dir_all(path.parent().unwrap())?;
    let output = std::process::Command::new("git")
        .args(["worktree", "add", "--detach"])
        .arg(&path)
        .arg("HEAD")
        .current_dir(&job.source_workspace)
        .output()?;
    ensure!(
        output.status.success(),
        "Could not prepare worktree: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    job.workspace = std::fs::canonicalize(path)?;
    Ok(())
}
pub fn same_project(job: &Job, workspace: &Path) -> Result<bool> {
    let source = std::fs::canonicalize(&job.source_workspace)?;
    if source != std::fs::canonicalize(workspace)? {
        return Ok(false);
    }
    let execution = std::fs::canonicalize(&job.workspace)?;
    if execution == source {
        return Ok(true);
    }
    ensure!(uuid::Uuid::parse_str(&job.id).is_ok(), "Invalid task id");
    let owned = crate::config::home()?.join("worktrees").join(&job.id);
    if !owned.exists() || execution != std::fs::canonicalize(owned)? {
        return Ok(false);
    }
    fn common(path: &Path) -> Result<PathBuf> {
        let output = std::process::Command::new("git")
            .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
            .current_dir(path)
            .output()?;
        ensure!(
            output.status.success(),
            "Saved worktree no longer belongs to a Git project"
        );
        Ok(std::fs::canonicalize(
            String::from_utf8(output.stdout)?.trim(),
        )?)
    }
    Ok(common(&execution)? == common(&source)?)
}
pub fn patch(id: &str) -> Result<String> {
    let job = get(id)?;
    let output = std::process::Command::new("git")
        .args(["diff", "--no-ext-diff", "--stat", "HEAD"])
        .current_dir(job.workspace)
        .output()?;
    ensure!(output.status.success(), "Could not inspect task diff");
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ids_cannot_traverse_storage() {
        for id in ["../jobs/", "../../etc/passwd", "", "abc"] {
            assert!(get(id).is_err());
        }
    }
}
