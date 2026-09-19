use crate::config::{self, Settings};
use anyhow::{Context, Result};
use std::{path::Path, time::Duration};
use tokio::process::Command;

pub fn update_args(id: &str) -> Option<&'static [&'static str]> {
    match id {
        "codex" | "claude" | "antigravity" => Some(&["update"]),
        _ => None,
    }
}

pub async fn report(settings: &Settings, apply: bool) -> Result<String> {
    let mut lines = vec![if apply {
        "Harness updates (check, then apply if the CLI supports it).".to_string()
    } else {
        "Harness versions (no install).".to_string()
    }];
    for offer in config::harness_offers(settings, None) {
        if offer.id == "mock" {
            continue;
        }
        if !offer.found {
            lines.push(format!("{}: not on PATH ({})", offer.name, offer.detail));
            continue;
        }
        let bin = config::harness_bin(offer.id, settings, None);
        let before = version(&bin).await;
        lines.push(format!("{} ({})", offer.name, bin.display()));
        lines.push(format!("  now: {}", compact(&before)));
        let Some(args) = update_args(offer.id) else {
            continue;
        };
        if !apply {
            continue;
        }
        match run(&bin, args, Duration::from_secs(240)).await {
            Ok(text) => lines.push(format!("  update: {}", compact(&text))),
            Err(e) => lines.push(format!("  update failed: {e:#}")),
        }
        let after = version(&bin).await;
        if after != before {
            lines.push(format!("  after: {}", compact(&after)));
        }
    }
    if lines.len() == 1 {
        lines.push("No Codex, Claude Code, or Antigravity CLI found.".into());
    }
    lines.push(
        "Restart Accessor after a successful update so the next session uses the new binary."
            .into(),
    );
    Ok(lines.join("\n"))
}

async fn version(bin: &Path) -> String {
    match run(bin, &["--version"], Duration::from_secs(12)).await {
        Ok(text) => text,
        Err(e) => format!("could not read version ({e:#})"),
    }
}

async fn run(bin: &Path, args: &[&str], timeout: Duration) -> Result<String> {
    let mut command = Command::new(bin);
    command.args(args);
    #[cfg(windows)]
    {
        command.creation_flags(0x0800_0000);
    }
    let output = tokio::time::timeout(timeout, command.output())
        .await
        .with_context(|| format!("{} timed out", bin.display()))?
        .with_context(|| format!("could not start {}", bin.display()))?;
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    let err = String::from_utf8_lossy(&output.stderr);
    if !err.trim().is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&err);
    }
    let text = text.trim();
    if text.is_empty() && !output.status.success() {
        anyhow::bail!("{} exited with {}", bin.display(), output.status);
    }
    Ok(text.to_owned())
}

fn compact(text: &str) -> String {
    let kept: Vec<_> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let start = kept.len().saturating_sub(8);
    kept[start..].join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn known_clis_have_update_subcommand() {
        assert_eq!(update_args("codex"), Some(&["update"][..]));
        assert_eq!(update_args("claude"), Some(&["update"][..]));
        assert_eq!(update_args("antigravity"), Some(&["update"][..]));
        assert!(update_args("mock").is_none());
    }
    #[test]
    fn compact_keeps_tail() {
        let text = (0..20)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let out = compact(&text);
        assert!(out.contains("line 19"));
        assert!(!out.contains("line 0"));
    }
}
