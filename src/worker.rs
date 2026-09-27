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
}

impl Worker {
    pub fn start(
        harness: &str,
        prompt: String,
        mut options: Options,
        events: mpsc::Sender<(String, Event)>,
    ) -> Result<Self> {
        ensure!(
            !prompt.trim().is_empty() && prompt.len() <= 32_000,
            "Worker prompt must contain 1–32000 characters"
        );
        crate::organizer::validate_execution(
            harness,
            options.model.as_deref().unwrap_or(""),
            &options.reasoning,
        )?;
        options.instructions = "You are an isolated Accessor worker executing a bounded user-authorized task. Use your harness tools and existing permissions. Do not create further agents or emit Accessor controls. Return a concise result including outcome, evidence, changed files, unresolved work, and any usage/approval blockers. Never claim success without evidence. Do not retry uncertain external actions. The main conversation will receive and explain your result.".into();
        let id = format!("worker:{}", uuid::Uuid::new_v4());
        let (tx, task) = agent::spawn_tagged(harness, &id, options, events);
        Ok(Self {
            id,
            harness: harness.into(),
            tx,
            task,
            first: Some(prompt),
            output: String::new(),
            started: Instant::now(),
            failed: false,
            schedule_id: None,
        })
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
        if let Some(id) = self.schedule_id.take() {
            let _ = crate::organizer::finish_run(&id, "interrupted; inspect before retrying");
        }
    }
}
