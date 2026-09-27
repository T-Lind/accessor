//! Provider credentials and tools belong to the agent, never to Accessor.
use crate::{
    config::{self, Settings},
    ui::safe,
};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{ffi::OsString, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::Command,
};

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Model {
    pub id: String,
    pub name: String,
}

/// A connector or plugin as reported by the native harness that owns it. The
/// credentials and definitions always live in the harness, never in Accessor;
/// this only records the name and owner so a prompt can route work correctly.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Connector {
    pub harness: String,
    /// "app", "plugin", or "mcp".
    pub kind: String,
    pub name: String,
}

/// Last successfully probed connector inventory, or empty when never probed.
/// Read on the prompt hot path, so it must stay a cheap file read.
pub fn cached_inventory() -> Vec<Connector> {
    config::home()
        .ok()
        .and_then(|p| std::fs::read(p.join("connectors.json")).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save_inventory(list: &[Connector]) -> Result<()> {
    config::save_private(
        &config::home()?.join("connectors.json"),
        &serde_json::to_vec(list)?,
    )
}

/// One prompt-ready line describing what connectors exist and which harness
/// owns each. Empty when nothing is known.
pub fn provenance_line(list: &[Connector]) -> String {
    if list.is_empty() {
        return String::new();
    }
    let items = list
        .iter()
        .take(20)
        .map(|c| format!("{} -> {} [{}]", c.harness, safe(&c.name), c.kind))
        .collect::<Vec<_>>()
        .join("; ");
    format!(
        "Connected connectors, cached best-effort snapshot (owner -> name [kind]; availability does not prove authorization, and this list can be stale): {items}. Route a connector task to the harness that owns it; the plugin preference below is only a default when no owner is listed.\n"
    )
}

/// Best-effort probe of every discovered harness for the connectors it reports,
/// then cache the result. Harnesses that are absent or unsupported are skipped.
/// `codex_override` follows the same rule as `config::harness_bin`.
pub async fn refresh_inventory(
    settings: &Settings,
    codex_override: Option<&std::path::PathBuf>,
) -> Vec<Connector> {
    let mut out = Vec::new();
    for offer in config::harness_offers(settings, codex_override)
        .into_iter()
        .filter(|h| h.found && h.id != "mock")
    {
        if offer.id == "codex" {
            if let Ok(list) =
                codex_connectors(config::harness_bin("codex", settings, codex_override)).await
            {
                out.extend(list);
            }
        } else {
            out.extend(generic_connectors(offer.id, settings).await);
        }
    }
    if !out.is_empty() {
        let _ = save_inventory(&out);
    }
    out
}

/// Parse the line-oriented output of a harness's native `plugin list` / `mcp
/// list`. Conservative on purpose: metadata rows, headers and stray warnings
/// are dropped so only recognizable connector names reach the prompt.
fn parse_connector_lines(harness: &str, kind: &str, text: &str) -> Vec<Connector> {
    // Field labels that appear next to a real entry but are not connectors.
    const LABELS: &[&str] = &[
        "version",
        "scope",
        "status",
        "name",
        "type",
        "description",
        "command",
        "url",
        "command/url",
        "enabled",
        "disabled",
        "plugin",
        "plugins",
        "mcp",
        "server",
        "servers",
    ];
    let mut out = Vec::new();
    for raw in text.lines() {
        // Claude marks plugin entries with a leading pointer; its Version/Scope/
        // Status lines are separate metadata rows handled by the label filter.
        let line = raw.trim().trim_start_matches(['-', '*', '•', '❯']).trim();
        if line.is_empty() || line.len() > 100 || line.ends_with(':') {
            continue;
        }
        let name = line
            .split([':', '\t'])
            .next()
            .unwrap_or(line)
            .split(" - ")
            .next()
            .unwrap_or(line)
            .trim();
        // Drop the "@marketplace" suffix and any trailing "(state)" marker.
        let name = name.split('@').next().unwrap_or(name).trim();
        let name = name.split('(').next().unwrap_or(name).trim();
        if name.is_empty() || name.len() > 60 || !name.is_ascii() {
            continue;
        }
        if !name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric())
        {
            continue;
        }
        let lower = name.to_ascii_lowercase();
        if lower.starts_with("no ") || lower.ends_with("list") || LABELS.contains(&lower.as_str()) {
            continue;
        }
        // A connector name is one to three words, not a sentence or a path.
        if name.split_whitespace().count() > 3 || name.contains('/') || name.contains('\\') {
            continue;
        }
        if out
            .iter()
            .any(|c: &Connector| c.name.eq_ignore_ascii_case(name))
        {
            continue;
        }
        out.push(Connector {
            harness: harness.into(),
            kind: kind.into(),
            name: name.into(),
        });
        if out.len() >= 40 {
            break;
        }
    }
    out
}

async fn generic_connectors(harness: &str, settings: &Settings) -> Vec<Connector> {
    let bin = config::harness_bin(harness, settings, None);
    let mut out = Vec::new();
    for (kind, args) in [("plugin", ["plugin", "list"]), ("mcp", ["mcp", "list"])] {
        let Ok(output) = Command::new(&bin)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .output()
            .await
        else {
            continue;
        };
        if !output.status.success() {
            continue;
        }
        out.extend(parse_connector_lines(
            harness,
            kind,
            &String::from_utf8_lossy(&output.stdout),
        ));
    }
    out
}

pub fn cached_models() -> Vec<Model> {
    config::home()
        .ok()
        .and_then(|p| std::fs::read(p.join("models.json")).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}
pub async fn models(executable: std::path::PathBuf) -> Result<Vec<Model>> {
    let mut process = Command::new(executable)
        .arg("app-server")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()?;
    let mut stdin = process.stdin.take().context("Missing stdin")?;
    let mut lines = BufReader::new(process.stdout.take().context("Missing stdout")?).lines();
    let result=tokio::time::timeout(Duration::from_secs(40),async {
        stdin.write_all(format!("{}\n",json!({"id":1,"method":"initialize","params":{"clientInfo":{"name":"accessor","version":env!("CARGO_PKG_VERSION")}}})).as_bytes()).await?;
        let mut models=Vec::new();let mut pages=0;
        while let Some(line)=lines.next_line().await? {
            let msg:Value=serde_json::from_str(&line)?;
            if msg.get("method").is_some() {continue;}
            ensure!(msg.get("error").is_none(),"Model discovery failed: {}",msg["error"]);
            if msg["id"]==1 {stdin.write_all(b"{\"method\":\"initialized\"}\n{\"id\":2,\"method\":\"model/list\",\"params\":{\"limit\":100}}\n").await?;}
            if msg["id"]==2 {
                for model in msg["result"]["data"].as_array().context("Unexpected model list")? {
                    if model["hidden"]==true {continue;}
                    let id=model["model"].as_str().or_else(||model["id"].as_str()).context("Missing model ID")?.to_owned();
                    models.push(Model{name:model["displayName"].as_str().unwrap_or(&id).into(),id});
                }
                pages+=1;
                if let Some(cursor)=msg["result"]["nextCursor"].as_str() {
                    ensure!(pages<10,"Too many model catalog pages");
                    stdin.write_all(format!("{}\n",json!({"id":2,"method":"model/list","params":{"limit":100,"cursor":cursor}})).as_bytes()).await?;
                }else{return Ok(models);}
            }
        }
        anyhow::bail!("Codex closed before returning models")
    }).await.context("Model discovery timed out")?;
    let _ = process.kill().await;
    let models = result?;
    config::save_private(
        &config::home()?.join("models.json"),
        &serde_json::to_vec(&models)?,
    )?;
    Ok(models)
}

pub async fn delegate_harness(
    harness: &str,
    settings: &Settings,
    codex_override: Option<&std::path::PathBuf>,
    args: &[OsString],
) -> Result<()> {
    let status = Command::new(config::harness_bin(harness, settings, codex_override))
        .args(args)
        .status()
        .await
        .with_context(|| {
            format!("Could not open {harness}; check that harness's installation and login")
        })?;
    ensure!(status.success(), "{harness} exited with {status}");
    Ok(())
}

pub async fn delegate(args: &[OsString]) -> Result<()> {
    let settings = Settings::load()?;
    delegate_harness("codex", &settings, None, args).await
}

pub async fn delegate_plugin(args: &[OsString]) -> Result<()> {
    let settings = Settings::load()?;
    let harness = settings.plugin_target().0;
    delegate_harness(harness, &settings, None, args).await
}

async fn codex_connectors(executable: std::path::PathBuf) -> Result<Vec<Connector>> {
    let mut process = Command::new(executable)
        .arg("app-server")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()?;
    let mut stdin = process.stdin.take().context("Missing stdin")?;
    let mut lines = BufReader::new(process.stdout.take().context("Missing stdout")?).lines();
    let result=tokio::time::timeout(Duration::from_secs(40),async {
        stdin.write_all(format!("{}\n",json!({"id":1,"method":"initialize","params":{"clientInfo":{"name":"accessor","version":env!("CARGO_PKG_VERSION")},"capabilities":{"experimentalApi":true}}})).as_bytes()).await?;
        while let Some(line)=lines.next_line().await? {
            let msg:Value=serde_json::from_str(&line)?;
            if msg["id"]==1 {
                ensure!(msg.get("error").is_none(),"Codex initialization failed");
                stdin.write_all(b"{\"method\":\"initialized\"}\n").await?;
                stdin.write_all(b"{\"id\":2,\"method\":\"app/list\",\"params\":{\"limit\":100}}\n").await?;
            } else if msg["id"]==2 {
                ensure!(msg.get("error").is_none(),"This Codex version could not list apps. Open acc agent and use /plugins or /mcp.");
                let apps=msg["result"]["data"].as_array().context("Unexpected app list response")?;
                return Ok(apps.iter().filter(|a|a["isAccessible"]==true).map(|app| Connector{
                    harness:"codex".into(), kind:"app".into(),
                    name:safe(app["name"].as_str().unwrap_or("Unnamed")),
                }).collect::<Vec<_>>());
            }
        }
        anyhow::bail!("Codex closed before returning app status")
    }).await.context("Connector status timed out")?;
    let _ = process.kill().await;
    result
}

async fn codex_status(executable: std::path::PathBuf) -> Result<()> {
    let apps = codex_connectors(executable).await?;
    if !apps.is_empty() {
        let _ = save_inventory(&apps);
    }
    println!("Accessible apps reported by Codex (availability is not a completed tool test):");
    if apps.is_empty() {
        println!(
            "No accessible apps in this catalog page. Use acc connectors setup to connect one."
        );
    }
    for app in &apps {
        println!("{}", safe(&app.name));
    }
    println!("\nPlugin setup: acc connectors setup\nCustom MCP servers: acc agent mcp list\nAccessor inherits this Codex installation's configuration. Incoming email requires a configured event source.");
    Ok(())
}

/// Probe every discovered harness and cache the connector inventory that the
/// metaprompt and the `harness_health` MCP tool read. Empty stays empty.
pub async fn refresh() -> Result<()> {
    let settings = Settings::load()?;
    let list = refresh_inventory(&settings, None).await;
    if list.is_empty() {
        println!("No connectors found. Connect plugins or MCP servers in a harness, then retry. The metaprompt is unchanged while the inventory is empty.");
    } else {
        println!("{}", provenance_line(&list).trim_end());
    }
    Ok(())
}

pub async fn status() -> Result<()> {
    let settings = Settings::load()?;
    let harness = settings.plugin_target().0;
    if harness == "codex" {
        return codex_status(config::harness_bin(harness, &settings, None)).await;
    }
    println!("Plugins reported by {harness}:");
    delegate_harness(harness, &settings, None, &["plugin".into(), "list".into()]).await?;
    println!("\nMCP connectors reported by {harness}:");
    delegate_harness(harness, &settings, None, &["mcp".into(), "list".into()]).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connector_lines_are_parsed_conservatively_with_provenance() {
        let generic = "Plugins:\n- gmail: send and read mail\n* github\nNo plugins found\n/usr/bin/foo\nthis is a long sentence that is not a connector name\natlassian\n";
        let found = parse_connector_lines("claude", "mcp", generic);
        let names: Vec<_> = found.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"gmail"));
        assert!(names.contains(&"github"));
        assert!(names.contains(&"atlassian"));
        assert!(!names
            .iter()
            .any(|n| n.contains("sentence") || *n == "Plugins"));
        assert!(found
            .iter()
            .all(|c| c.harness == "claude" && c.kind == "mcp"));
    }

    #[test]
    fn claude_plugin_blocks_yield_only_plugin_names() {
        let text = "Installed plugins:\n\n  \u{276f} feature-dev@claude-plugins-official\n    Version: b81918\n    Scope: user\n    Status: \u{2714} enabled\n\n  \u{276f} superpowers@claude-plugins-official\n    Version: 6.3.0\n";
        let found = parse_connector_lines("claude", "plugin", text);
        let names: Vec<_> = found.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["feature-dev", "superpowers"]);
    }

    #[test]
    fn table_headers_and_metadata_are_not_connectors() {
        let text = "NAME      TYPE   STATUS   COMMAND/URL\nVersion: 1.0\nStatus: enabled\nplugin\n";
        assert!(parse_connector_lines("antigravity", "mcp", text).is_empty());
    }

    #[test]
    fn provenance_line_names_owner_and_kind_or_is_empty() {
        assert!(provenance_line(&[]).is_empty());
        let line = provenance_line(&[Connector {
            harness: "codex".into(),
            kind: "app".into(),
            name: "Gmail".into(),
        }]);
        assert!(line.contains("codex -> Gmail [app]"));
    }
}
