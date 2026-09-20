//! Conservative circuit breaker. Never automatically replay a possibly applied action.
use crate::{config, organizer::now_unix};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Serialize, Deserialize)]
struct Block {
    until: u64,
    reason: String,
}

#[derive(Default, Serialize, Deserialize)]
pub struct Limits {
    blocks: HashMap<String, Block>,
}

impl Limits {
    pub fn load() -> Self {
        config::home()
            .ok()
            .and_then(|p| std::fs::read(p.join("limits.json")).ok())
            .and_then(|v| serde_json::from_slice(&v).ok())
            .unwrap_or_default()
    }
    pub fn observe(&mut self, harness: &str, message: &str) -> bool {
        let lower = message.to_ascii_lowercase();
        let quota = [
            "usage limit",
            "usage_limit",
            "quota exceeded",
            "exceeded your current quota",
            "hit your limit",
            "resource_exhausted",
            "insufficient_quota",
            "credit balance",
            "out of credits",
            "limit reached",
        ]
        .iter()
        .any(|v| lower.contains(v));
        let rate = ["rate limit", "rate_limit", "too many requests", "http 429"]
            .iter()
            .any(|v| lower.contains(v));
        if !quota && !rate {
            return false;
        }
        self.blocks.insert(
            harness.into(),
            Block {
                until: now_unix() + if quota { 3600 } else { 300 },
                reason: message.chars().take(800).collect(),
            },
        );
        if let Ok(home) = config::home() {
            if let Ok(bytes) = serde_json::to_vec_pretty(self) {
                let _ = config::save_private(&home.join("limits.json"), &bytes);
            }
        }
        true
    }
    pub fn blocked(&self, harness: &str) -> Option<String> {
        self.blocks.get(harness).filter(|b| b.until > now_unix()).map(|b|
            format!("{harness} is cooling down for {}s after a provider limit: {}. This is a local backoff, not a verified provider reset. No action was retried.", b.until - now_unix(), b.reason))
    }
    pub fn report(&self) -> String {
        let lines: Vec<_> = ["codex", "claude", "antigravity"]
            .iter()
            .filter_map(|h| self.blocked(h))
            .collect();
        if lines.is_empty() {
            "No observed provider limit is blocking work. Remaining account quotas are unknown unless the harness reports them.".into()
        } else {
            lines.join("\n")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn expired_blocks_do_not_prevent_work() {
        let mut limits = Limits::default();
        limits.blocks.insert(
            "codex".into(),
            Block {
                until: 0,
                reason: "old".into(),
            },
        );
        assert!(limits.blocked("codex").is_none());
        assert!(limits.blocked("claude").is_none());
    }
}
