//! Human-only loopback controls, deliberately separate from the MCP credential.
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::{mpsc, oneshot},
};

#[derive(Clone, Serialize)]
pub struct Entry {
    pub key: String,
    pub owner: String,
    pub number: u64,
    pub detail: String,
    pub questions: Option<Value>,
    #[serde(skip)]
    deadline: Instant,
}
#[derive(Clone, Default)]
pub struct Panel(Arc<Mutex<BTreeMap<String, Entry>>>);
impl Panel {
    pub fn add(&self, owner: &str, number: u64, detail: String, questions: Option<Value>) {
        let key = format!("{owner}:{number}");
        self.0.lock().unwrap().insert(
            key.clone(),
            Entry {
                key,
                owner: owner.into(),
                number,
                detail,
                questions,
                deadline: Instant::now() + Duration::from_secs(60),
            },
        );
    }
    pub fn remove(&self, owner: &str, number: u64) {
        self.0
            .lock()
            .unwrap()
            .retain(|_, e| e.owner != owner || e.number != number);
    }
    pub fn clear_owner(&self, owner: &str) {
        self.0.lock().unwrap().retain(|_, e| e.owner != owner);
    }
    pub fn clear(&self) {
        self.0.lock().unwrap().clear();
    }
    pub fn retain_live(&self, live: impl Fn(&str) -> bool) {
        self.0
            .lock()
            .unwrap()
            .retain(|_, e| e.deadline > Instant::now() && live(&e.owner));
    }
    pub fn get(&self, key: &str) -> Option<Entry> {
        self.0
            .lock()
            .unwrap()
            .get(key)
            .filter(|e| e.deadline > Instant::now())
            .cloned()
    }
    fn snapshot(&self) -> Vec<Entry> {
        self.0
            .lock()
            .unwrap()
            .values()
            .filter(|e| e.deadline > Instant::now())
            .cloned()
            .collect()
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Response {
    pub key: String,
    pub allow: Option<bool>,
    pub answers: Option<Value>,
}
pub struct Request {
    pub response: Response,
    pub reply: oneshot::Sender<Value>,
}
pub struct Bridge {
    pub panel: Panel,
    pub url: String,
    path: std::path::PathBuf,
    task: tokio::task::JoinHandle<()>,
}
impl Bridge {
    pub async fn start(input: mpsc::Sender<crate::Input>) -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let token = uuid::Uuid::new_v4().to_string();
        let url = format!("http://{address}/#{token}");
        let directory = crate::config::home()?.join("human-sessions");
        std::fs::create_dir_all(&directory)?;
        let path = directory.join(format!("{}.json", uuid::Uuid::new_v4()));
        crate::config::save_private(&path, &serde_json::to_vec(&json!({"url":url}))?)?;
        let panel = Panel::default();
        let state = panel.clone();
        let task = tokio::spawn(async move {
            let slots = Arc::new(tokio::sync::Semaphore::new(4));
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                let Ok(slot) = slots.clone().try_acquire_owned() else {
                    continue;
                };
                let (token, state, input) = (token.clone(), state.clone(), input.clone());
                tokio::spawn(async move {
                    let _slot = slot;
                    let _=tokio::time::timeout(Duration::from_secs(5),async {
                        let mut bytes=Vec::new();let mut chunk=[0;1024];
                        let header_end=loop {let n=stream.read(&mut chunk).await?;ensure!(n>0,"Empty HTTP request");bytes.extend_from_slice(&chunk[..n]);ensure!(bytes.len()<=24_000,"Request too large");if let Some(i)=bytes.windows(4).position(|w|w==b"\r\n\r\n") {break i+4;}ensure!(bytes.len()<=8192,"Headers too large");};
                        let headers=std::str::from_utf8(&bytes[..header_end])?;
                        let first=headers.lines().next().unwrap_or("").to_owned();
                        let authorized=headers.lines().any(|h|h.strip_prefix("Authorization: Bearer ").is_some_and(|v|crate::control::token_matches(Some(v),&token)));
                        let length=headers.lines().find_map(|h|h.to_ascii_lowercase().strip_prefix("content-length: ").and_then(|v|v.parse::<usize>().ok())).unwrap_or(0);
                        ensure!(length<=16_000,"Body too large");
                        while bytes.len()<header_end+length {let n=stream.read(&mut chunk).await?;ensure!(n>0,"Incomplete body");bytes.extend_from_slice(&chunk[..n]);ensure!(bytes.len()<=24_000,"Body too large");}
                        let (status,kind,body)=if first=="GET / HTTP/1.1" {("200 OK","text/html",PAGE.into())}
                        else if !authorized {("403 Forbidden","application/json",json!({"error":"Authentication required"}).to_string())}
                        else if first=="GET /requests HTTP/1.1" {("200 OK","application/json",serde_json::to_string(&state.snapshot())?)}
                        else if first=="POST /respond HTTP/1.1" {
                            let response:Response=serde_json::from_slice(&bytes[header_end..header_end+length])?;
                            let (reply,receive)=oneshot::channel();
                            input.try_send(crate::Input::Human(Request {response,reply}))?;
                            ("200 OK","application/json",receive.await?.to_string())
                        } else {("404 Not Found","application/json","{}".into())};
                        let response=format!("HTTP/1.1 {status}\r\nContent-Type: {kind}; charset=utf-8\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nContent-Security-Policy: default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",body.len());
                        stream.write_all(response.as_bytes()).await?;
                        Ok::<_,anyhow::Error>(())
                    }).await;
                });
            }
        });
        Ok(Self {
            panel,
            url,
            path,
            task,
        })
    }
}
impl Drop for Bridge {
    fn drop(&mut self) {
        self.task.abort();
        self.panel.clear();
        let _ = std::fs::remove_file(&self.path);
    }
}
pub fn latest_url() -> Result<String> {
    let directory = crate::config::home()?.join("human-sessions");
    let mut files = std::fs::read_dir(directory)?
        .filter_map(|f| f.ok())
        .collect::<Vec<_>>();
    files.sort_by_key(|f| std::cmp::Reverse(f.metadata().and_then(|m| m.modified()).ok()));
    let file = files
        .first()
        .ok_or_else(|| anyhow::anyhow!("Start Accessor first"))?;
    let value: Value = serde_json::from_slice(&std::fs::read(file.path())?)?;
    Ok(value["url"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Invalid human session"))?
        .into())
}
pub async fn open(url: &str) -> Result<String> {
    let mut cmd = if cfg!(target_os = "macos") {
        tokio::process::Command::new("open")
    } else if cfg!(windows) {
        let mut c = tokio::process::Command::new("cmd");
        c.args(["/c", "start", ""]);
        c
    } else {
        tokio::process::Command::new("xdg-open")
    };
    ensure!(
        cmd.arg(url)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .await?
            .success(),
        "Could not open the human controls; use acc approvals --url"
    );
    Ok("Opened human approvals and question controls.".into())
}
const PAGE: &str = r#"<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width"><title>Accessor human controls</title><style>body{font:18px system-ui;max-width:760px;margin:2rem auto;padding:0 1rem}section{border:1px solid #aaa;padding:1rem;margin:1rem 0}pre{white-space:pre-wrap;overflow-wrap:anywhere}button,input,select{font:inherit;margin:.4rem;padding:.6rem}label{display:block}#status{min-height:2rem}</style><h1>Accessor human controls</h1><p>Review each request before responding. Requests expire after one minute. Speech cannot grant permission.</p><p id="status" role="status" aria-live="polite"></p><main id="requests"></main><script>
const token=location.hash.slice(1);history.replaceState(null,'',location.pathname);let previous='';
async function api(path,body){const r=await fetch(path,{method:body?'POST':'GET',headers:{'Authorization':'Bearer '+token,'Content-Type':'application/json'},body:body?JSON.stringify(body):undefined});if(!r.ok)throw Error('Human authentication failed or session ended');return r.json()}
async function send(body){try{const r=await api('/respond',body);document.getElementById('status').textContent=r.error||r.receipt;previous='';await poll()}catch(e){document.getElementById('status').textContent=e.message}}
function el(tag,text){const e=document.createElement(tag);if(text)e.textContent=text;return e}
async function poll(){try{const rows=await api('/requests');const serial=JSON.stringify(rows);if(serial===previous)return;previous=serial;const root=document.getElementById('requests');root.replaceChildren();if(!rows.length){root.append(el('p','No pending requests.'));return}for(const r of rows){const s=el('section');s.append(el('h2',r.questions?'Question':'Permission request'));s.append(el('pre',r.detail));if(r.questions){const form=el('form');for(const q of r.questions){const label=el('label',q.question);let input;if(q.options?.length&&!q.isOther){input=el('select');input.append(new Option('Choose an answer',''));for(const o of q.options)input.append(new Option(o.label+' — '+o.description,o.label))}else {input=el('input');if(q.options?.length)label.append(el('small',' Suggested answers: '+q.options.map(o=>o.label).join(', ')))}input.name=q.id;input.required=true;label.append(input);form.append(label)}const b=el('button','Submit answers');b.type='submit';form.append(b);form.onsubmit=e=>{e.preventDefault();const answers={};for(const [id,value] of new FormData(form))answers[id]={answers:[value]};send({key:r.key,answers})};s.append(form)}else{for(const [label,allow] of [['Allow this request',true],['Decline',false]]){const b=el('button',label);b.onclick=()=>send({key:r.key,allow});s.append(b)}}root.append(s)}}catch(e){document.getElementById('status').textContent=e.message}}
poll();setInterval(poll,1000);
</script></html>"#;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn human_requests_are_scoped_expiring_and_invalidated() {
        let panel = Panel::default();
        panel.add("main:first", 1, "first".into(), None);
        panel.add("worker:second", 1, "second".into(), None);
        panel.clear_owner("main:first");
        assert!(panel.get("main:first:1").is_none());
        assert!(panel.get("worker:second:1").is_some());
        panel
            .0
            .lock()
            .unwrap()
            .get_mut("worker:second:1")
            .unwrap()
            .deadline = Instant::now() - Duration::from_secs(1);
        assert!(panel.get("worker:second:1").is_none());
        assert!(panel.snapshot().is_empty());
        panel.add("main:third", 2, "third".into(), None);
        panel.retain_live(|_| false);
        assert!(panel.get("main:third:2").is_none());
        panel.add("main:fourth", 2, "fourth".into(), None);
        panel.clear();
        assert!(panel.snapshot().is_empty());
    }
}
