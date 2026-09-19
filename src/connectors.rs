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

pub async fn delegate(args: &[OsString]) -> Result<()> {
    let status = Command::new(config::codex(&Settings::load()?))
        .args(args)
        .status()
        .await
        .context("Could not open Codex; use acc config set codex-bin PATH")?;
    ensure!(status.success(), "Codex exited with {status}");
    Ok(())
}
pub async fn status() -> Result<()> {
    let mut process = Command::new(config::codex(&Settings::load()?))
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
                println!("Accessible apps reported by Codex (availability is not a completed tool test):");
                let mut found=false;
                for app in apps.iter().filter(|a|a["isAccessible"]==true) {
                    found=true;
                    println!("{}  ·  {}",safe(app["name"].as_str().unwrap_or("Unnamed")),if app["isEnabled"]==false{"disabled"}else{"enabled"});
                }
                if !found {println!("No accessible apps in this catalog page. Use acc connectors setup to connect one.");}
                if msg["result"]["nextCursor"].is_string() {println!("Showing accessible apps from the first 100 catalog entries. Browse the full catalog in /plugins.");}
                println!("\nPlugin setup: acc connectors setup\nCustom MCP servers: acc agent mcp list\nAccessor inherits this Codex installation's configuration. Incoming email requires a configured event source.");
                return Ok(());
            }
        }
        anyhow::bail!("Codex closed before returning app status")
    }).await.context("Connector status timed out")?;
    let _ = process.kill().await;
    result
}
