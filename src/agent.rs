use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::{collections::HashMap, path::PathBuf, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, Command},
    sync::mpsc,
    time::Instant,
};

#[derive(Debug)]
pub enum CommandMessage {
    Prompt(String),
    Model(Option<String>),
    Cancel,
    Approval { number: u64, allow: bool },
    Shutdown,
}
#[derive(Debug)]
pub enum Event {
    Ready,
    Started,
    Reply(String),
    Progress(String),
    Done,
    Cancelled,
    Failed(String),
    Approval { number: u64, detail: String },
    ApprovalClosed,
    Error(String),
    Note(String),
    Tool(String),
}

pub struct Options {
    pub control: Option<crate::control::Endpoint>,
    pub shared_memory: bool,
    pub executable: PathBuf,
    pub workspace: PathBuf,
    pub writable: bool,
    pub model: Option<String>,
    pub auto_review: bool,
    pub reasoning: String,
    pub instructions: String,
}

pub fn spawn(
    harness: &str,
    options: Options,
    events: mpsc::Sender<(String, Event)>,
) -> (mpsc::Sender<CommandMessage>, tokio::task::JoinHandle<()>) {
    spawn_tagged(harness, harness, options, events)
}

pub fn spawn_tagged(
    harness: &str,
    tag: &str,
    mut options: Options,
    events: mpsc::Sender<(String, Event)>,
) -> (mpsc::Sender<CommandMessage>, tokio::task::JoinHandle<()>) {
    if options.shared_memory {
        options.instructions.push_str("\n\n");
        options.instructions.push_str(crate::memory::POLICY);
        options.instructions.push_str("\nIf MCP tools are unavailable, use acc memory --help through your normal shell permissions.");
    }
    let (tx, rx) = mpsc::channel(8);
    let (inner_tx, mut inner_rx) = mpsc::channel(32);
    let tag = tag.to_owned();
    let kind = harness.to_owned();
    tokio::spawn(async move {
        while let Some(event) = inner_rx.recv().await {
            if events.send((tag.clone(), event)).await.is_err() {
                break;
            }
        }
    });
    let task = tokio::spawn(async move {
        let result = match kind.as_str() {
            "mock" => mock_agent(rx, inner_tx.clone()).await,
            "claude" => claude_cli(options, rx, inner_tx.clone()).await,
            "antigravity" => antigravity_cli(options, rx, inner_tx.clone()).await,
            _ => codex(options, rx, inner_tx.clone()).await,
        };
        if let Err(e) = result {
            let _ = inner_tx.send(Event::Error(format!("{e:#}"))).await;
        }
    });
    (tx, task)
}

async fn mock_agent(mut rx: mpsc::Receiver<CommandMessage>, tx: mpsc::Sender<Event>) -> Result<()> {
    tx.send(Event::Ready).await?;
    while let Some(cmd) = rx.recv().await {
        match cmd {
            CommandMessage::Model(_) => {}
            CommandMessage::Prompt(text) => {
                tx.send(Event::Started).await?;
                tx.send(Event::Reply(format!("Mock agent received: {text}")))
                    .await?;
                tx.send(Event::Done).await?;
            }
            CommandMessage::Shutdown => break,
            CommandMessage::Cancel => {
                tx.send(Event::Done).await?;
            }
            CommandMessage::Approval { .. } => {
                tx.send(Event::Note("No approval is pending.".into()))
                    .await?;
            }
        }
    }
    Ok(())
}

async fn write(stdin: &mut ChildStdin, message: Value) -> Result<()> {
    let mut bytes = serde_json::to_vec(&message)?;
    bytes.push(b'\n');
    stdin.write_all(&bytes).await?;
    stdin.flush().await?;
    Ok(())
}

fn thread_params(options: &Options) -> Value {
    json!({"cwd": options.workspace, "model":options.model, "sandbox": if options.writable {"workspace-write"} else {"read-only"},
        "approvalPolicy":"on-request",
        "approvalsReviewer": if options.auto_review {"auto_review"} else {"user"},
        "ephemeral":true,
        "developerInstructions": options.instructions})
}

fn confirmation_only(params: &Value) -> bool {
    // A click-style confirmation can be represented by /approve. Never invent
    // answers for fields, open authentication URLs, or forge device proofs.
    params["mode"] == "form"
        && params["requestedSchema"]["type"] == "object"
        && params["requestedSchema"]["properties"]
            .as_object()
            .is_some_and(|p| p.is_empty())
        && params["requestedSchema"]["required"]
            .as_array()
            .is_none_or(|p| p.is_empty())
}
fn approval_result(id: Value, method: &str, allow: bool) -> Value {
    if method == "mcpServer/elicitation/request" {
        json!({"id":id,"result":{"action":if allow {"accept"}else{"decline"},"content":if allow {json!({})}else{Value::Null}}})
    } else {
        json!({"id":id,"result":{"decision":if allow {"accept"}else{"decline"}}})
    }
}
fn rejected(id: Value, method: &str) -> Value {
    match method {
        "item/commandExecution/requestApproval" | "item/fileChange/requestApproval" => {
            json!({"id":id,"result":{"decision":"decline"}})
        }
        "item/permissions/requestApproval" => {
            json!({"id":id,"result":{"permissions":{},"scope":"turn"}})
        }
        "mcpServer/elicitation/request" => json!({"id":id,"result":{"action":"decline"}}),
        "item/tool/requestUserInput" => json!({"id":id,"result":{"answers":{}}}),
        _ => {
            json!({"id":id,"error":{"code":-32601,"message":"Accessor does not support this request"}})
        }
    }
}

struct Pending {
    id: Value,
    method: String,
    deadline: Instant,
}

async fn codex(
    options: Options,
    mut commands: mpsc::Receiver<CommandMessage>,
    events: mpsc::Sender<Event>,
) -> Result<()> {
    let mut cmd = Command::new(&options.executable);
    cmd.arg("app-server");
    let _mcp_config = if options.shared_memory {
        crate::mcp::configure(
            &mut cmd,
            "codex",
            &options.workspace,
            options.control.as_ref(),
        )?
    } else {
        None
    };
    let mut process: Child = crate::process_tree::spawn(cmd
        .current_dir(&options.workspace).stdin(Stdio::piped()).stdout(Stdio::piped())
        .stderr(Stdio::null()).kill_on_drop(true))
        .context("Could not start Codex. Install/login to Codex CLI, or pass --codex-bin with its executable path")?;
    let _tree = crate::process_tree::ProcessTree::attach(&process)?;
    let mut stdin = process.stdin.take().context("Codex stdin missing")?;
    let mut lines = BufReader::new(process.stdout.take().context("Codex stdout missing")?).lines();
    write(&mut stdin, json!({"id":1,"method":"initialize","params":{"clientInfo":{"name":"accessor","title":"Accessor","version":env!("CARGO_PKG_VERSION")}}})).await?;
    let mut thread: Option<String> = None;
    let mut model = options.model.clone();
    let mut turn: Option<String> = None;
    let mut turn_starting = false;
    let mut replies = Vec::new();
    let mut cancel_requested = false;
    let mut next_id = 10_u64;
    let mut approval_number = 0_u64;
    let mut pending: HashMap<u64, Pending> = HashMap::new();
    let mut requests: HashMap<u64, &'static str> = HashMap::new();
    let mut tick = tokio::time::interval(Duration::from_millis(250));
    let handshake_deadline = Instant::now() + Duration::from_secs(30);
    loop {
        tokio::select! {
            line = lines.next_line() => {
                let Some(line) = line? else { bail!("Codex connection closed"); };
                let msg: Value = serde_json::from_str(&line).context("Invalid Codex protocol message")?;
                if let Some(method) = msg["method"].as_str() {
                    if let Some(id) = msg.get("id") {
                        if (matches!(method, "item/commandExecution/requestApproval" | "item/fileChange/requestApproval") || (method=="mcpServer/elicitation/request" && confirmation_only(&msg["params"]))) && turn.is_some() {
                            approval_number += 1;
                            events.send(Event::Approval { number: approval_number, detail: serde_json::to_string_pretty(&msg["params"])? }).await?;
                            pending.insert(approval_number, Pending { id: id.clone(), method: method.into(), deadline: Instant::now()+Duration::from_secs(60) });
                        } else {
                            write(&mut stdin, rejected(id.clone(), method)).await?;
                            events.send(Event::Note(format!("Declined unsupported request: {method}"))).await?;
                        }
                        continue;
                    }
                    match method {
                        "turn/started" => {
                            turn = msg["params"]["turn"]["id"].as_str().map(str::to_owned);
                            turn_starting = false;
                        }
                        "item/started" => {
                            if let Some(text) = summarize_item(&msg["params"]["item"], false) {
                                events.send(Event::Tool(text)).await?;
                            }
                        }
                        "item/completed" => {
                            let item = &msg["params"]["item"];
                            if item["type"] == "agentMessage" {
                                if let Some(text) = item["text"].as_str() {
                                    if item["phase"]=="commentary" {events.send(Event::Progress(text.into())).await?;}
                                    else {replies.push(text.to_owned());}
                                }
                            } else if let Some(text) = summarize_item(item, true) {
                                events.send(Event::Tool(text)).await?;
                            }
                        }
                        "turn/completed" => {
                            turn = None; turn_starting = false; cancel_requested = false;
                            for (_, p) in pending.drain() { write(&mut stdin, rejected(p.id, &p.method)).await?; }
                            let completed = &msg["params"]["turn"];
                            if completed["status"] == "failed" { events.send(Event::Failed(format!("Task failed: {}", completed["error"]))).await?; }
                            else if completed["status"]=="completed" {
                                for text in replies.drain(..) {events.send(Event::Reply(text)).await?;}
                            }
                            replies.clear();
                            events.send(Event::Done).await?;
                        }
                        "serverRequest/resolved" => {
                            pending.retain(|_,p| p.id != msg["params"]["requestId"]);
                            if pending.is_empty() { events.send(Event::ApprovalClosed).await?; }
                        }
                        "error" => { events.send(Event::Note(format!("Codex: {}", msg["params"]["error"]))).await?; }
                        _ => {}
                    }
                } else if let Some(id) = msg["id"].as_u64() {
                    if let Some(error) = msg.get("error") {
                        if id <= 2 { bail!("Codex setup failed: {error}"); }
                        if requests.remove(&id) == Some("turn/start") { turn_starting = false; events.send(Event::Done).await?; }
                        events.send(Event::Failed(format!("Codex request failed: {error}"))).await?;
                        continue;
                    }
                    if id == 1 {
                        write(&mut stdin, json!({"method":"initialized"})).await?;
                        write(&mut stdin, json!({"id":2,"method":"thread/start","params":thread_params(&options)})).await?;
                    } else if id == 2 {
                        thread = msg["result"]["thread"]["id"].as_str().map(str::to_owned);
                        if thread.is_none() { bail!("Codex did not return a thread id"); }
                        events.send(Event::Ready).await?;
                    } else if requests.remove(&id) == Some("turn/start") {
                        turn = msg["result"]["turn"]["id"].as_str().map(str::to_owned);
                        turn_starting = false;
                    }
                }
            }
            cmd = commands.recv() => {
                match cmd {
                    Some(CommandMessage::Model(value)) => {model=value;}
                    None | Some(CommandMessage::Shutdown) => {
                        // Interrupt first. Dropping the connection also invalidates pending approvals.
                        if let (Some(t), Some(r)) = (&thread, &turn) { write(&mut stdin, json!({"id":next_id,"method":"turn/interrupt","params":{"threadId":t,"turnId":r}})).await?; }
                        drop(stdin);
                        if tokio::time::timeout(Duration::from_secs(3), process.wait()).await.is_err() { process.kill().await?; }
                        return Ok(());
                    }
                    Some(CommandMessage::Prompt(text)) => {
                        if turn.is_some() || turn_starting { events.send(Event::Note("Agent is working. Cancel or wait before sending another request.".into())).await?; continue; }
                        cancel_requested=false;
                        replies.clear();
                        let Some(t) = &thread else { events.send(Event::Note("Agent is still connecting; repeat your request when ready.".into())).await?; events.send(Event::Done).await?; continue; };
                        next_id += 1;
                        requests.insert(next_id, "turn/start");
                        let mut params = json!({"threadId":t,"model":model,"input":[{"type":"text","text":text}]});
                        if options.reasoning != "default" {
                            params["effort"] = json!(options.reasoning);
                        }
                        write(&mut stdin, json!({"id":next_id,"method":"turn/start","params":params})).await?;
                        turn_starting = true;
                        events.send(Event::Started).await?;
                    }
                    Some(CommandMessage::Cancel) => { cancel_requested = true; }
                    Some(CommandMessage::Approval { number, allow }) => {
                        if let Some(p) = pending.remove(&number) {
                            let allow = allow && p.deadline > Instant::now();
                            write(&mut stdin, approval_result(p.id,&p.method,allow)).await?;
                            if pending.is_empty() { events.send(Event::ApprovalClosed).await?; }
                        } else { events.send(Event::Note("Approval is absent, expired, or already resolved.".into())).await?; }
                    }
                }
            }
            _ = tick.tick() => {
                if thread.is_none() && Instant::now() > handshake_deadline { bail!("Codex connection timed out"); }
                let expired: Vec<_> = pending.iter().filter(|(_,p)| p.deadline <= Instant::now()).map(|(n,_)| *n).collect();
                let had_expired = !expired.is_empty();
                for n in expired {
                    if let Some(p) = pending.remove(&n) { write(&mut stdin, rejected(p.id, &p.method)).await?; events.send(Event::Note(format!("Approval {n} expired and was declined."))).await?; }
                }
                if had_expired && pending.is_empty() { events.send(Event::ApprovalClosed).await?; }
                if cancel_requested && !turn_starting {
                    cancel_requested = false;
                    for (_,p) in pending.drain() { write(&mut stdin, rejected(p.id, &p.method)).await?; }
                    if let (Some(t), Some(r)) = (&thread, &turn) {
                        next_id += 1;
                        write(&mut stdin, json!({"id":next_id,"method":"turn/interrupt","params":{"threadId":t,"turnId":r}})).await?;
                    } else { events.send(Event::Done).await?; }
                    events.send(Event::ApprovalClosed).await?;
                }
            }
        }
    }
}

fn summarize_item(item: &Value, done: bool) -> Option<String> {
    let ty = item["type"].as_str()?;
    if matches!(ty, "agentMessage" | "reasoning") {
        return None;
    }
    let mark = if done { "✓" } else { "▸" };
    let detail = match ty {
        "commandExecution" => truncate(
            item["command"]
                .as_str()
                .or_else(|| item["commandLine"].as_str())
                .unwrap_or("command"),
        ),
        "mcpToolCall" => {
            let server = item["server"]
                .as_str()
                .or_else(|| item["serverName"].as_str())
                .unwrap_or("mcp");
            let tool = item["tool"]
                .as_str()
                .or_else(|| item["toolName"].as_str())
                .or_else(|| item["name"].as_str())
                .unwrap_or("tool");
            return Some(format!("{mark} {server}/{tool}"));
        }
        "webSearch" => truncate(
            item["query"]
                .as_str()
                .or_else(|| item["searchTerm"].as_str())
                .unwrap_or("web search"),
        ),
        "fileChange" => truncate(
            item["path"]
                .as_str()
                .or_else(|| item["changes"][0]["path"].as_str())
                .unwrap_or("file"),
        ),
        "todoList" => "todo list".into(),
        other => other.into(),
    };
    Some(format!("{mark} {detail}"))
}
fn truncate(text: &str) -> String {
    let text: String = text.chars().take(80).collect();
    text.replace('\n', " ")
}

async fn claude_cli(
    options: Options,
    commands: mpsc::Receiver<CommandMessage>,
    events: mpsc::Sender<Event>,
) -> Result<()> {
    let mut cmd = Command::new(&options.executable);
    let _mcp_config = if options.shared_memory {
        crate::mcp::configure(
            &mut cmd,
            "claude",
            &options.workspace,
            options.control.as_ref(),
        )?
    } else {
        None
    };
    cmd.args([
        "--output-format",
        "stream-json",
        "--input-format",
        "stream-json",
        "--verbose",
        "--permission-mode",
        if options.auto_review {
            "acceptEdits"
        } else {
            "default"
        },
    ])
    .current_dir(&options.workspace)
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .kill_on_drop(true);
    if let Some(model) = &options.model {
        cmd.arg("--model").arg(model);
    }
    // A file avoids Windows batch-wrapper newline/length limits and keeps the
    // full metaprompt out of the command line. Keep it alive for this process.
    let home = crate::config::home()?;
    std::fs::create_dir_all(&home)?;
    let mut instruction_file = tempfile::NamedTempFile::new_in(home)?;
    std::io::Write::write_all(&mut instruction_file, options.instructions.as_bytes())?;
    cmd.arg("--append-system-prompt-file")
        .arg(instruction_file.path());
    if options.reasoning != "default" {
        cmd.arg("--effort").arg(&options.reasoning);
    }
    stdio_agent(
        crate::process_tree::spawn(&mut cmd)
            .context("Could not start Claude Code. Install the `claude` CLI and log in.")?,
        commands,
        events,
        json_user,
        "Claude Code",
        String::new(),
    )
    .await
}

async fn antigravity_cli(
    options: Options,
    commands: mpsc::Receiver<CommandMessage>,
    events: mpsc::Sender<Event>,
) -> Result<()> {
    let mut cmd = Command::new(&options.executable);
    cmd.env("ACC_MEMORY_WORKSPACE", &options.workspace);
    if let Some(endpoint) = &options.control {
        cmd.env("ACC_CONTROL_ENDPOINT", serde_json::to_string(endpoint)?);
    } else {
        cmd.env_remove("ACC_CONTROL_ENDPOINT");
    }
    cmd.args([
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--print-timeout",
        "15m",
    ])
    .current_dir(&options.workspace)
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .kill_on_drop(true);
    if let Some(model) = &options.model {
        cmd.arg("--model").arg(model);
    } else if let Some(model) = crate::config::harness_default_model("antigravity") {
        cmd.arg("--model").arg(model);
    }
    let effort = match options.reasoning.as_str() {
        "low" | "medium" | "high" => options.reasoning.as_str(),
        _ => "low",
    };
    cmd.arg("--effort").arg(effort);
    cmd.arg("--mode").arg(if options.writable {
        "accept-edits"
    } else {
        "plan"
    });
    cmd.arg("--sandbox");
    stdio_agent(
        crate::process_tree::spawn(&mut cmd)
            .context("Could not start Antigravity. Install the `agy` CLI and log in.")?,
        commands,
        events,
        agy_user,
        "Antigravity",
        options.instructions,
    )
    .await
}

fn json_user(text: &str) -> Value {
    json!({"type":"user","message":{"role":"user","content":[{"type":"text","text":text}]}})
}
fn agy_user(text: &str) -> Value {
    json!({"event":"user","message":{"content":text}})
}

#[derive(Debug, PartialEq)]
enum StreamLine {
    Skip,
    Tool(String),
    Reply(String),
    Finished {
        reply: Option<String>,
        error: Option<String>,
    },
}

fn parse_stream_line(msg: &Value) -> StreamLine {
    match msg["event"].as_str() {
        Some("result") => {
            let err = nonempty_str(&msg["result"]["error"]);
            let reply = nonempty_str(&msg["result"]["response"]);
            let failed = err.is_some()
                || msg["result"]["status"].as_str().is_some_and(|s| {
                    s.eq_ignore_ascii_case("ERROR") || s.eq_ignore_ascii_case("FAILED")
                });
            StreamLine::Finished {
                reply: if failed { None } else { reply },
                error: err.or_else(|| failed.then(|| "Harness reported an error".into())),
            }
        }
        Some("step_update") => {
            let step = &msg["step_update"];
            if step["step_type"] == "tool" {
                let name = step["tool_name"]
                    .as_str()
                    .or_else(|| step["tool_info"]["name"].as_str())
                    .unwrap_or("tool");
                return StreamLine::Tool(format!("▸ {name}"));
            }
            StreamLine::Skip
        }
        Some(_) => StreamLine::Skip,
        None => claude_stream_line(msg),
    }
}

fn claude_stream_line(msg: &Value) -> StreamLine {
    if msg["type"] == "result" && msg["is_error"] == true {
        return StreamLine::Finished {
            reply: None,
            error: Some(
                nonempty_str(&msg["result"])
                    .unwrap_or_else(|| format!("Claude task failed: {}", msg["errors"])),
            ),
        };
    }
    let kind = msg["type"].as_str().or_else(|| msg["event"].as_str());
    match kind {
        Some("result" | "end" | "done" | "turn_complete") => StreamLine::Finished {
            reply: nonempty_str(&msg["result"]).or_else(|| claude_text(msg)),
            error: nonempty_str(&msg["error"]).or_else(|| nonempty_str(&msg["result"]["error"])),
        },
        Some("error") => StreamLine::Finished {
            reply: None,
            error: nonempty_str(&msg["error"]["message"])
                .or_else(|| nonempty_str(&msg["error"]))
                .or_else(|| Some("Harness reported an error".into())),
        },
        Some("assistant" | "content_block_delta") => claude_text(msg)
            .map(StreamLine::Reply)
            .unwrap_or(StreamLine::Skip),
        Some("tool_use" | "tool_call") => {
            let name = msg["name"]
                .as_str()
                .or_else(|| msg["tool_name"].as_str())
                .unwrap_or("tool");
            StreamLine::Tool(format!("▸ {name}"))
        }
        _ => StreamLine::Skip,
    }
}

fn claude_text(msg: &Value) -> Option<String> {
    nonempty_str(&msg["result"])
        .or_else(|| nonempty_str(&msg["text"]))
        .or_else(|| nonempty_str(&msg["delta"]["text"]))
        .or_else(|| nonempty_str(&msg["message"]["content"][0]["text"]))
        .or_else(|| nonempty_str(&msg["message"]["content"]))
}

fn nonempty_str(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn noteworthy_stderr(line: &str) -> bool {
    let line = line.to_ascii_lowercase();
    [
        "error", "fail", "auth", "denied", "warning", "fatal", "login",
    ]
    .iter()
    .any(|k| line.contains(k))
}

fn harness_exit_message(
    name: &str,
    busy: bool,
    status: std::process::ExitStatus,
    stderr: &str,
) -> Option<String> {
    if !busy && status.success() && stderr.trim().is_empty() {
        return None;
    }
    let code = status
        .code()
        .map(|c| c.to_string())
        .unwrap_or_else(|| "signal".into());
    let detail = stderr.trim();
    Some(if detail.is_empty() {
        format!(
            "{name} exited (status {code}) while {}.",
            if busy { "working" } else { "idle" }
        )
    } else {
        format!("{name} exited (status {code}): {detail}")
    })
}

async fn stdio_agent(
    mut process: Child,
    mut commands: mpsc::Receiver<CommandMessage>,
    events: mpsc::Sender<Event>,
    encode: fn(&str) -> Value,
    name: &'static str,
    instructions: String,
) -> Result<()> {
    let mut tree = crate::process_tree::ProcessTree::attach(&process)?;
    let mut stdin = process.stdin.take().context("Agent stdin missing")?;
    let mut lines = BufReader::new(process.stdout.take().context("Agent stdout missing")?).lines();
    let mut err_lines =
        BufReader::new(process.stderr.take().context("Agent stderr missing")?).lines();
    events.send(Event::Ready).await?;
    let mut busy = false;
    let mut err_log = String::new();
    let mut err_open = true;
    let mut primed = instructions.is_empty();
    let mut reply_buffer = String::new();
    loop {
        tokio::select! {
            line = lines.next_line() => {
                let Some(line) = line? else {
                    let status = process.wait().await?;
                    if let Some(msg) = harness_exit_message(name, busy, status, &err_log) {
                        bail!(msg);
                    }
                    return Ok(());
                };
                if line.trim().is_empty() { continue; }
                let Ok(msg) = serde_json::from_str::<Value>(&line) else {
                    if err_log.len() < 4000 {
                        err_log.push_str(line.trim());
                        err_log.push('\n');
                    }
                    if noteworthy_stderr(&line) {
                        events.send(Event::Note(format!("{name}: {}", line.trim()))).await?;
                    }
                    continue;
                };
                match parse_stream_line(&msg) {
                    StreamLine::Skip => {}
                    StreamLine::Tool(text) => events.send(Event::Tool(text)).await?,
                    StreamLine::Reply(text) => {
                        if !reply_buffer.ends_with(&text) { reply_buffer.push_str(&text); reply_buffer.push('\n'); }
                    },
                    StreamLine::Finished { reply, error } => {
                        let failed=error.is_some();
                        if let Some(error) = error {
                            events.send(Event::Failed(format!("{name}: {error}"))).await?;
                        }
                        if !failed {if let Some(reply) = reply.or_else(|| (!reply_buffer.is_empty()).then(|| std::mem::take(&mut reply_buffer))) {
                            events.send(Event::Reply(reply)).await?;
                        }}
                        busy = false;
                        events.send(Event::Done).await?;
                    }
                }
            }
            line = err_lines.next_line(), if err_open => {
                match line? {
                    Some(line) => {
                        if err_log.len() < 4000 {
                            err_log.push_str(line.trim());
                            err_log.push('\n');
                        }
                        if noteworthy_stderr(&line) {
                            events.send(Event::Note(format!("{name}: {}", line.trim()))).await?;
                        }
                    }
                    None => err_open = false,
                }
            }
            cmd = commands.recv() => {
                match cmd {
                    Some(CommandMessage::Prompt(text)) => {
                        if busy {
                            events.send(Event::Note("Agent is working. Cancel or wait before sending another request.".into())).await?;
                            continue;
                        }
                        busy = true;
                        reply_buffer.clear();
                        events.send(Event::Started).await?;
                        let text = if primed {
                            text
                        } else {
                            primed = true;
                            format!("{instructions}\n\n{text}")
                        };
                        write(&mut stdin, encode(&text)).await?;
                    }
                    Some(CommandMessage::Cancel) => {
                        // These CLIs have no shared turn-interrupt protocol.
                        // Stop the actual process before acknowledging cancellation.
                        tree.stop();
                        let _=process.start_kill();
                        process.wait().await?;
                        events.send(Event::Note(format!("{name}: cancelled; reconnecting starts a fresh session with a handoff summary."))).await?;
                        events.send(Event::Cancelled).await?;
                        return Ok(());
                    }
                    None | Some(CommandMessage::Shutdown) => {
                        drop(stdin);
                        if tokio::time::timeout(Duration::from_secs(3), process.wait()).await.is_err() {
                            process.kill().await?;
                        }
                        return Ok(());
                    }
                    Some(CommandMessage::Model(_) | CommandMessage::Approval { .. }) => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_empty_mcp_confirmations_can_be_approved() {
        assert!(confirmation_only(
            &json!({"mode":"form","requestedSchema":{"type":"object","properties":{}}})
        ));
        assert!(!confirmation_only(
            &json!({"mode":"form","requestedSchema":{"type":"object","properties":{"recipient":{"type":"string"}}}})
        ));
        assert!(!confirmation_only(
            &json!({"mode":"openai/userVerification"})
        ));
        assert!(!confirmation_only(&json!({"mode":"url"})));
        assert_eq!(
            approval_result(json!(4), "mcpServer/elicitation/request", false)["result"]["action"],
            "decline"
        );
        assert_eq!(
            approval_result(json!(4), "mcpServer/elicitation/request", true)["result"]["content"],
            json!({})
        );
    }
    #[test]
    fn defaults_and_unknown_requests_fail_closed() {
        let p = thread_params(&Options {
            control: None,
            shared_memory: false,
            executable: "codex".into(),
            workspace: "/tmp/work".into(),
            writable: false,
            model: None,
            auto_review: false,
            reasoning: "default".into(),
            instructions: String::new(),
        });
        assert_eq!(p["sandbox"], "read-only");
        assert_eq!(p["approvalsReviewer"], "user");
        assert_eq!(
            thread_params(&Options {
                control: None,
                shared_memory: false,
                executable: "codex".into(),
                workspace: "/tmp/work".into(),
                writable: true,
                model: None,
                auto_review: true,
                reasoning: "high".into(),
                instructions: String::new(),
            })["approvalsReviewer"],
            "auto_review"
        );
        assert_eq!(
            rejected(json!(3), "item/permissions/requestApproval")["result"]["permissions"],
            json!({})
        );
        assert!(rejected(json!(3), "future/request").get("error").is_some());
    }
    #[test]
    fn tool_items_are_summarized() {
        assert_eq!(
            summarize_item(
                &json!({"type":"mcpToolCall","server":"gmail","tool":"search"}),
                false
            ),
            Some("▸ gmail/search".into())
        );
        assert_eq!(
            summarize_item(&json!({"type":"commandExecution","command":"ls"}), true),
            Some("✓ ls".into())
        );
        assert!(summarize_item(&json!({"type":"agentMessage","text":"hi"}), true).is_none());
    }
    #[test]
    fn antigravity_user_line_uses_event_not_type() {
        let msg = agy_user("check the time");
        assert_eq!(msg["event"], "user");
        assert_eq!(msg["message"]["content"], "check the time");
        assert!(msg.get("type").is_none());
    }
    #[test]
    fn antigravity_result_response_is_the_reply() {
        let msg =
            json!({"event":"result","result":{"status":"SUCCESS","response":"It is 2:24.\n"}});
        assert_eq!(
            parse_stream_line(&msg),
            StreamLine::Finished {
                reply: Some("It is 2:24.".into()),
                error: None
            }
        );
    }
    #[test]
    fn antigravity_error_result_is_not_silent_success() {
        let msg = json!({"event":"result","result":{"status":"ERROR","response":"","error":"missing event field"}});
        assert_eq!(
            parse_stream_line(&msg),
            StreamLine::Finished {
                reply: None,
                error: Some("missing event field".into())
            }
        );
    }
    #[test]
    fn antigravity_tool_and_delta_are_visible() {
        assert_eq!(
            parse_stream_line(
                &json!({"event":"step_update","step_update":{"step_type":"tool","tool_name":"run_command"}})
            ),
            StreamLine::Tool("▸ run_command".into())
        );
        assert_eq!(
            parse_stream_line(
                &json!({"event":"step_update","step_update":{"step_type":"agent_response","text_delta":"apple"}})
            ),
            StreamLine::Skip
        );
    }
    #[test]
    fn claude_result_string_still_completes() {
        assert_eq!(
            parse_stream_line(&json!({"type":"result","result":"hello"})),
            StreamLine::Finished {
                reply: Some("hello".into()),
                error: None
            }
        );
    }
    #[test]
    fn harness_exit_while_busy_is_an_error() {
        let failed = std::process::Command::new(if cfg!(windows) { "cmd" } else { "false" })
            .args(if cfg!(windows) {
                vec!["/C", "exit 2"]
            } else {
                vec![]
            })
            .status()
            .unwrap();
        let msg =
            harness_exit_message("Antigravity", true, failed, "authentication required").unwrap();
        assert!(msg.contains("authentication required"));
        assert!(msg.contains("2") || msg.contains("status"));
    }
}
