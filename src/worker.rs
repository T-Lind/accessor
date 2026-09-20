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
            self.output = format!(
                "{}\n[worker detail omitted]\n{}",
                self.output.chars().take(2000).collect::<String>(),
                self.output
                    .chars()
                    .rev()
                    .take(4000)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect::<String>()
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
