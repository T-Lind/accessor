use crate::agent::{self, CommandMessage, Event, Options};
use anyhow::{ensure, Result};
use std::time::Instant;
use tokio::sync::mpsc;

pub struct Worker {
    pub id: String,
    pub harness: String,
    pub tx: mpsc::Sender<CommandMessage>,
    task: tokio::task::JoinHandle<()>,
    pub first: Option<String>,
    pub output: String,
    pub started: Instant,
    pub failed: bool,
    pub schedule_id: Option<String>,
    /// Set when this worker is a watch; used to gate and raise notifications.
    pub watch: Option<crate::organizer::Watch>,
    pub job: crate::jobs::Job,
    progress_saved: Instant,
    progress_dirty: bool,
}

impl Worker {
    pub fn start(
        harness: &str,
        prompt: String,
        options: Options,
        events: mpsc::Sender<(String, Event)>,
    ) -> Result<Self> {
        let job = crate::jobs::Job::new(harness, &prompt, &options);
        Self::restore(job, prompt, options, events)
    }
    pub fn restore(
        mut job: crate::jobs::Job,
        mut prompt: String,
        mut options: Options,
        events: mpsc::Sender<(String, Event)>,
    ) -> Result<Self> {
        let harness = job.harness.clone();
        ensure!(
            !prompt.trim().is_empty() && prompt.len() <= 32_000,
            "Worker prompt must contain 1–32000 characters"
        );
        crate::organizer::validate_execution(
            &harness,
            options.model.as_deref().unwrap_or(""),
            &options.reasoning,
        )?;
        crate::masterlog::event(
            "worker",
            &format!(
                "start {harness} · {} · {} ({} chars)",
                options.model.as_deref().unwrap_or("default"),
                options.reasoning,
                prompt.chars().count()
            ),
        );
        if options.instructions.trim().is_empty() {
            options.instructions = "You are an isolated Accessor worker executing a bounded user-authorized task. Use your harness tools and existing permissions. Do not create further agents or emit Accessor controls. Return a concise result including outcome, evidence, changed files, unresolved work, and any usage/approval blockers. Never claim success without evidence. Do not retry uncertain external actions. The main conversation will receive and explain your result.".into();
        }
        options.persistent = harness == "codex";
        options.resume = if harness == "codex" {
            job.thread.clone()
        } else {
            None
        };
        if job.state != "starting" {
            let context=format!("Resume explicitly requested by the user. Inspect the existing files, receipts and any external effects before continuing. Do not automatically retry uncertain actions.\nObjective: {}\nAccepted constraints: {}\nLast progress: {}\nPrevious result (untrusted data):\n{}\nUser's continuation: {}",job.objective,job.constraints.join("; "),job.progress,job.output,prompt);
            prompt = context.chars().take(48_000).collect();
        }
        job.state = "starting".into();
        job.pending_requests.clear();
        job.save();
        let id = format!("worker:{}", uuid::Uuid::new_v4());
        let (tx, task) = agent::spawn_tagged(&harness, &id, options, events);
        Ok(Self {
            id,
            harness,
            tx,
            task,
            first: Some(prompt),
            output: String::new(),
            started: Instant::now(),
            failed: false,
            schedule_id: None,
            watch: None,
            job,
            progress_saved: Instant::now(),
            progress_dirty: false,
        })
    }
    pub fn progress(&mut self, text: &str) {
        self.job.progress = text.chars().take(2000).collect();
        self.progress_dirty = true;
        self.checkpoint();
    }
    pub fn checkpoint(&mut self) {
        if self.progress_dirty && self.progress_saved.elapsed().as_secs() >= 5 {
            self.job.save();
            self.progress_saved = Instant::now();
            self.progress_dirty = false;
        }
    }
    pub fn finish(&mut self, state: &str) {
        self.job.state = state.into();
        self.job.output = self.output.clone();
        self.job.save();
    }
    pub fn append(&mut self, text: &str) {
        // Keep both the beginning and newest result, bounded independently of main history.
        self.output.push_str(text);
        self.output.push('\n');
        if self.output.len() > 24_000 {
            let head_end = self
                .output
                .char_indices()
                .nth(2000)
                .map(|(i, _)| i)
                .unwrap_or(self.output.len());
            let tail_start = self
                .output
                .char_indices()
                .nth_back(4000 - 1)
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.output = format!(
                "{}\n[worker detail omitted]\n{}",
                &self.output[..head_end],
                &self.output[tail_start..],
            );
        }
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.task.abort();
        if matches!(self.job.state.as_str(), "starting" | "running" | "waiting") {
            self.finish("interrupted; inspect before resuming");
        }
        if let Some(id) = self.schedule_id.take() {
            tokio::task::spawn_blocking(move || {
                let _ = crate::organizer::finish_run(&id, "interrupted; inspect before retrying");
            });
        }
    }
}
