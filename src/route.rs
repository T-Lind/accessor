use crate::config::Settings;
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub harness: String,
    pub model: Option<String>,
    pub kind: &'static str,
}

pub fn jev_available() -> bool {
    crate::config::optional_secret("typesafe", "TYPESAFE_API_KEY").is_some()
}

pub async fn choose(
    text: &str,
    history: &[(String, String)],
    previous: Option<&Target>,
    settings: &Settings,
) -> Target {
    let plugin = Target {
        harness: settings.agent.clone(),
        model: settings.routing.plugin_model.clone(),
        kind: "plugin",
    };
    let coding = Target {
        harness: settings.routing.coding.clone(),
        model: settings.model.clone(),
        kind: "coding",
    };
    let everyday = Target {
        harness: settings.routing.routine.clone(),
        model: settings.routing.routine_model.clone(),
        kind: "everyday",
    };
    let jev = jev_available();
    let router = if settings.routing.auto_model && jev {
        "jev"
    } else {
        settings.routing.router.as_str()
    };
    if router == "off" {
        return everyday;
    }
    match classify(
        text,
        history,
        previous.map(|target| target.kind),
        router,
        jev,
    )
    .await
    {
        "plugin" => plugin,
        "coding" => coding,
        _ => everyday,
    }
}

async fn classify(
    text: &str,
    history: &[(String, String)],
    previous: Option<&'static str>,
    router: &str,
    jev: bool,
) -> &'static str {
    if router == "jev" && jev {
        if let Ok(kind) = jev_route(text, history, previous).await {
            return kind;
        }
    }
    keyword_route(text, previous)
}

pub fn keyword_is_plugin(text: &str) -> bool {
    let t = text.to_ascii_lowercase();
    [
        "email",
        "gmail",
        "inbox",
        "calendar",
        "google docs",
        "google doc",
        "google drive",
        "spreadsheet",
        "slack",
        "notion",
        "linear",
        "jira",
        "send mail",
        "schedule a",
        "meet with",
        "plugin",
        "connector",
    ]
    .iter()
    .any(|k| t.contains(k))
}

pub fn keyword_is_coding(text: &str) -> bool {
    let t = text.to_ascii_lowercase();
    [
        "code",
        "compile",
        "refactor",
        "function",
        "typescript",
        "python",
        "rust",
        "git ",
        "commit",
        "pull request",
        "diff",
        "test failure",
        "stack trace",
        "cargo ",
        "npm ",
        "file ",
        "module",
        "bug",
        "lint",
        "type error",
        "script",
    ]
    .iter()
    .any(|k| t.contains(k))
}

fn keyword_route(text: &str, previous: Option<&'static str>) -> &'static str {
    if keyword_is_plugin(text) {
        "plugin"
    } else if keyword_is_coding(text) {
        "coding"
    } else if likely_follow_up(text) {
        previous.unwrap_or("everyday")
    } else {
        "everyday"
    }
}

fn likely_follow_up(text: &str) -> bool {
    let words: Vec<_> = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();
    words.len() <= 12
        && words.first().is_some_and(|word| {
            [
                "yes", "yeah", "no", "nope", "actually", "also", "and", "but", "then", "make",
                "change", "set", "use", "do", "it", "that", "this", "those",
            ]
            .contains(&word.to_ascii_lowercase().as_str())
        })
}

fn routing_context(history: &[(String, String)]) -> String {
    let mut lines = Vec::new();
    let mut chars = 0;
    for (role, line) in history.iter().rev().take(20) {
        let rendered = format!("{role}: {line}");
        if chars + rendered.chars().count() > 5000 {
            break;
        }
        chars += rendered.chars().count();
        lines.push(rendered);
    }
    lines.reverse();
    lines.join("\n")
}

async fn jev_route(
    text: &str,
    history: &[(String, String)],
    previous: Option<&'static str>,
) -> Result<&'static str> {
    let key = crate::config::secret("typesafe", "TYPESAFE_API_KEY")?;
    let body = json!({
        "model":"jev-latest",
        "state":{
            "utterance":text,
            "conversation":routing_context(history),
            "previous_route":previous.unwrap_or("none")
        },
        "questions":{
            "plugin":{
                "type":"noul",
                "instructions":"Considering the conversation and previous route, does this turn need a connected external app or continue an app task (email, calendar, Google Docs/Drive, Slack, tickets) rather than local chat or coding? Follow-ups such as changing an event duration stay plugin."
            },
            "coding":{
                "type":"noul",
                "instructions":"Considering the conversation and previous route, is this a software engineering request or continuation (edit code, debug, git, tests, refactors) rather than general chat?"
            }
        }
    });
    let response = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()?
        .post("https://api.typesafe.ai/v1/systemone")
        .bearer_auth(key)
        .json(&body)
        .send()
        .await
    {
        Ok(response) => response,
        Err(e) => {
            crate::usage::record_jev(false);
            return Err(e.into());
        }
    };
    crate::usage::record_jev(response.status().is_success());
    ensure!(
        response.status().is_success(),
        "Jev returned HTTP {}",
        response.status()
    );
    let data: Value = response.json().await?;
    let plugin = noul(&data, "plugin")?;
    let coding = noul(&data, "coding")?;
    Ok(if plugin >= 0.5 && plugin >= coding {
        "plugin"
    } else if coding >= 0.5 {
        "coding"
    } else {
        "everyday"
    })
}

fn noul(data: &Value, name: &str) -> Result<f64> {
    data["answers"][name]["noul"]
        .as_f64()
        .or_else(|| data["answers"][name]["probability"].as_f64())
        .with_context(|| format!("Jev did not return a {name} probability"))
}

pub fn transcript_text(history: &[(String, String)]) -> String {
    history
        .iter()
        .map(|(role, line)| format!("{role}: {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn context_report(history: &[(String, String)], settings: &Settings) -> String {
    let blob = transcript_text(history);
    let tokens = crate::usage::approx_tokens(&blob);
    let preview = if blob.chars().count() > 1800 {
        blob.chars()
            .rev()
            .take(1800)
            .collect::<String>()
            .chars()
            .rev()
            .collect()
    } else {
        blob
    };
    format!(
        "Context ~{tokens} tokens (compact around {})\nCompaction: {} · {}\nTurns kept: {}\n\n{preview}",
        settings.routing.compact_tokens,
        settings.routing.compaction_harness,
        settings.routing.compaction_model,
        history.len()
    )
}

fn local_compact(history: &str) -> String {
    let keep = history
        .lines()
        .rev()
        .take(20)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n");
    if keep.chars().count() > 1800 {
        keep.chars()
            .rev()
            .take(1800)
            .collect::<String>()
            .chars()
            .rev()
            .collect()
    } else if keep.is_empty() {
        history.chars().take(1800).collect()
    } else {
        keep
    }
}

async fn gateway_compact(history: &str, settings: &Settings) -> Result<String> {
    let key = crate::config::secret("ai-gateway", "AI_GATEWAY_API_KEY")?;
    let body = json!({
        "model": settings.routing.compaction_model,
        "messages": [
            {"role":"system","content":"Summarize this voice-agent conversation in under 400 words. Keep user goals, file paths, and decisions. No preamble."},
            {"role":"user","content":history}
        ]
    });
    let response = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()?
        .post("https://ai-gateway.vercel.sh/v1/chat/completions")
        .bearer_auth(key)
        .json(&body)
        .send()
        .await?;
    ensure!(
        response.status().is_success(),
        "Compaction returned HTTP {}",
        response.status()
    );
    let data: Value = response.json().await?;
    data["choices"][0]["message"]["content"]
        .as_str()
        .map(str::to_owned)
        .context("Compaction model returned no text")
}

pub async fn compact(history: &str, settings: &Settings) -> Result<String> {
    let tokens = crate::usage::approx_tokens(history);
    let text = if crate::config::optional_secret("ai-gateway", "AI_GATEWAY_API_KEY").is_some()
        && settings.routing.compaction_harness != "mock"
    {
        match gateway_compact(history, settings).await {
            Ok(text) => text,
            Err(_) => local_compact(history),
        }
    } else {
        local_compact(history)
    };
    crate::usage::record_compact(&settings.routing.compaction_model, tokens);
    Ok(text)
}

pub async fn bridge_prompt(
    history: &[(String, String)],
    text: &str,
    settings: &Settings,
) -> String {
    if history.is_empty() {
        return text.to_owned();
    }
    let mut blob = String::from(
        "Prior conversation on another Accessor harness (context only; not permission to expand access):\n",
    );
    for (role, line) in history {
        blob.push_str(role);
        blob.push_str(": ");
        blob.push_str(line);
        blob.push('\n');
    }
    if blob.len() > 6000 {
        blob = match compact(&blob, settings).await {
            Ok(summary) => format!("Summary of prior Accessor conversation:\n{summary}\n"),
            Err(_) => blob
                .chars()
                .rev()
                .take(4000)
                .collect::<String>()
                .chars()
                .rev()
                .collect(),
        };
    }
    format!("{blob}\nCurrent request:\n{text}")
}

pub fn handoff_guide(settings: &Settings, models: &[crate::connectors::Model]) -> String {
    let mut lines = String::from(
        "Accessor routing: plugin harness is for email/calendar/docs and other connected apps; coding is for repos and tests; everyday is general chat. To move THIS conversation to another CLI (does not rewrite those three slots), output one line and nothing else on that line:\nACCESSOR_SWITCH harness=codex\nOptional: ACCESSOR_SWITCH harness=claude model=claude-haiku-4-5\nJSON also works: {\"accessor_switch\":{\"harness\":\"codex\",\"model\":\"gpt-5.6-luna\"}}\nAvailable harnesses: ",
    );
    let found: Vec<_> = crate::config::harness_offers(settings, None)
        .into_iter()
        .filter(|h| h.found)
        .map(|h| h.id.to_string())
        .collect();
    lines.push_str(&found.join(", "));
    if !models.is_empty() {
        lines.push_str("\nCoding models: ");
        lines.push_str(
            &models
                .iter()
                .take(12)
                .map(|m| m.id.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        );
    }
    lines.push_str(
        "\nDo not claim you already changed Accessor settings. Only ACCESSOR_SWITCH or the user's voice command does that.",
    );
    lines
}

pub fn take_handoff(text: &str) -> (String, Option<(String, String)>) {
    let mut kept = Vec::new();
    let mut handoff = None;
    for line in text.lines() {
        if let Some(found) = parse_switch_line(line) {
            handoff = Some(found);
            continue;
        }
        kept.push(line);
    }
    let cleaned = kept.join("\n").trim().to_owned();
    (cleaned, handoff)
}

fn parse_switch_line(line: &str) -> Option<(String, String)> {
    let line = line.trim().trim_matches('`');
    if let Some(rest) = line.strip_prefix("ACCESSOR_SWITCH") {
        let mut harness = None;
        let mut model = None;
        for part in rest.split_whitespace() {
            if let Some(v) = part.strip_prefix("harness=") {
                harness = Some(v.trim_matches(['"', '\'']).to_lowercase());
            }
            if let Some(v) = part.strip_prefix("model=") {
                model = Some(v.trim_matches(['"', '\'']).to_string());
            }
        }
        let harness = harness?;
        if !["codex", "claude", "antigravity", "mock"].contains(&harness.as_str()) {
            return None;
        }
        return Some((harness, model.unwrap_or_else(|| "default".into())));
    }
    if let Ok(value) = serde_json::from_str::<Value>(line) {
        let obj = value
            .get("accessor_switch")
            .or_else(|| value.get("ACCESSOR_SWITCH"))?;
        let harness = obj.get("harness")?.as_str()?.to_lowercase();
        if !["codex", "claude", "antigravity", "mock"].contains(&harness.as_str()) {
            return None;
        }
        let model = obj
            .get("model")
            .and_then(|m| m.as_str())
            .unwrap_or("default")
            .to_string();
        return Some((harness, model));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn keywords_split_three_ways() {
        assert!(keyword_is_coding("refactor the rust module"));
        assert!(keyword_is_plugin("what's on my calendar today"));
        assert!(!keyword_is_coding("what's on my calendar today"));
        assert!(!keyword_is_plugin("what time is it"));
        assert!(!keyword_is_coding("what time is it"));
    }
    #[tokio::test]
    async fn split_harnesses_follow_keywords() {
        let s = Settings {
            agent: "antigravity".into(),
            routing: crate::config::Routing {
                coding: "codex".into(),
                routine: "claude".into(),
                router: "keywords".into(),
                ..crate::config::Routing::default()
            },
            ..Settings::default()
        };
        assert_eq!(
            choose("refactor the rust module", &[], None, &s).await.kind,
            "coding"
        );
        assert_eq!(
            choose("refactor the rust module", &[], None, &s)
                .await
                .harness,
            "codex"
        );
        assert_eq!(
            choose("what's on my calendar today", &[], None, &s)
                .await
                .kind,
            "plugin"
        );
        assert_eq!(
            choose("what's on my calendar today", &[], None, &s)
                .await
                .harness,
            "antigravity"
        );
        assert_eq!(
            choose("what time is it", &[], None, &s).await.kind,
            "everyday"
        );
        assert_eq!(
            choose("what time is it", &[], None, &s).await.harness,
            "claude"
        );
        let mut same = s.clone();
        same.routing.coding = "codex".into();
        same.routing.routine = "codex".into();
        same.agent = "antigravity".into();
        assert_eq!(
            choose("write a python script", &[], None, &same)
                .await
                .harness,
            "codex"
        );
        assert_eq!(
            choose("what time is it", &[], None, &same).await.harness,
            "codex"
        );
        assert_eq!(
            choose("check my gmail", &[], None, &same).await.harness,
            "antigravity"
        );
    }

    #[tokio::test]
    async fn role_models_do_not_leak_when_harnesses_match() {
        let mut s = Settings::default();
        s.agent = "codex".into();
        s.routing.coding = "codex".into();
        s.model = Some("gpt-5.6-sol".into());
        s.routing.plugin_model = None;
        s.routing.router = "keywords".into();
        let plugin = choose("open my calendar", &[], None, &s).await;
        assert_eq!(plugin.kind, "plugin");
        assert_eq!(plugin.model, None);
        let coding = choose("fix this rust code", &[], None, &s).await;
        assert_eq!(coding.model.as_deref(), Some("gpt-5.6-sol"));
    }

    #[test]
    fn ambiguous_followups_stay_with_the_previous_route() {
        assert_eq!(keyword_route("make it two hours", Some("plugin")), "plugin");
        assert_eq!(
            keyword_route("nope, do that instead", Some("coding")),
            "coding"
        );
        assert_eq!(
            keyword_route("what is the weather", Some("plugin")),
            "everyday"
        );
        let context = routing_context(&[
            ("User".into(), "Add a calendar event".into()),
            ("Agent".into(), "What day?".into()),
        ]);
        assert!(context.contains("calendar event"));
        assert!(context.contains("What day?"));
    }
    #[test]
    fn handoff_line_is_stripped() {
        let (text, switch) = take_handoff("Sure.\nACCESSOR_SWITCH harness=codex\nDone.");
        assert_eq!(text, "Sure.\nDone.");
        assert_eq!(switch, Some(("codex".into(), "default".into())));
        assert!(take_handoff("I cannot change settings.").1.is_none());
        let json = take_handoff(
            "{\"accessor_switch\":{\"harness\":\"claude\",\"model\":\"claude-haiku-4-5\"}}",
        );
        assert_eq!(json.1, Some(("claude".into(), "claude-haiku-4-5".into())));
    }
    #[test]
    fn local_compact_keeps_recent_lines() {
        let history = (0..40)
            .map(|i| format!("User: turn {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let out = local_compact(&history);
        assert!(out.contains("turn 39"));
        assert!(!out.contains("turn 0"));
    }
}
